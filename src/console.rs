//! The console: the followed area (a cell and its 8 neighbours), one
//! connection per (relay, cell) through the connector, the feed.
//!
//! A task owns the state; [`Console`] is the handle to it. Changing the
//! area moves only the connections that change.

use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::sync::Arc;
use std::time::Duration;

use geohash_relays::Strategy;
use serde::Serialize;
use tokio::sync::{broadcast, mpsc, oneshot};
use tokio::task::JoinHandle;

use crate::connector::Connector;
use crate::error::{Error, Result};
use crate::nostr::{Event, Filter, unix_now};
use crate::relay::{self, ConnState, RelayEvent, RelayHandle};
use crate::store::{MemoryStore, Message, MessageStore};

/// Location-channel message.
pub const KIND_MESSAGE: u32 = 20000;
/// Location-channel presence.
pub const KIND_PRESENCE: u32 = 20001;
/// The app's channel levels (geohash lengths); 3 is not a channel.
pub const CHANNEL_LEVELS: [usize; 5] = [2, 4, 5, 6, 7];

#[derive(Clone)]
pub struct ConsoleConfig {
    pub strategy: Arc<dyn Strategy>,
    pub connector: Arc<dyn Connector>,
    /// How far back a subscription asks (the apps: 1 h).
    pub lookback_secs: i64,
    /// `limit` of the subscription (the apps: 200).
    pub limit: u32,
    /// Messages kept per cell.
    pub per_cell: usize,
    /// A participant counts while seen within this window.
    pub participant_window_secs: i64,
}

impl ConsoleConfig {
    pub fn new(strategy: Arc<dyn Strategy>, connector: Arc<dyn Connector>) -> Self {
        ConsoleConfig {
            strategy,
            connector,
            lookback_secs: 3600,
            limit: 200,
            per_cell: 500,
            participant_window_secs: 15 * 60,
        }
    }
}

/// A relay of a cell and how its connection is doing.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RelayStatus {
    pub url: String,
    /// Rank in the iOS choice, if iOS picks it.
    pub ios: Option<usize>,
    /// Ranks in the Android choice.
    pub android: Vec<usize>,
    pub state: String,
}

/// One followed cell.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CellStatus {
    pub geohash: String,
    pub relays: Vec<RelayStatus>,
    /// Distinct keys seen in the window (messages and presences).
    pub participants: usize,
}

/// The followed area.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct AreaStatus {
    /// The chosen cell; `None` before the first [`Console::set_area`].
    pub center: Option<String>,
    /// The centre first, then its neighbours.
    pub cells: Vec<CellStatus>,
    pub connector: String,
}

/// What changed, for the page.
#[derive(Debug, Clone, PartialEq)]
pub enum ConsoleEvent {
    /// The followed cells changed.
    Area,
    Message(Message),
    /// A known message was delivered (or accepted) by one more relay.
    Delivered {
        id: String,
        relay: String,
    },
    /// Participants of a cell changed.
    Participants {
        cell: String,
        count: usize,
    },
    /// A connection changed state.
    Relays,
    /// Text from a relay (NOTICE, CLOSED reason): show, never trust.
    RelayText {
        relay: String,
        cell: String,
        text: String,
    },
}

enum Command {
    SetArea(String, oneshot::Sender<Result<Vec<String>>>),
    Messages(String, oneshot::Sender<Vec<Message>>),
    Status(oneshot::Sender<AreaStatus>),
    Shutdown,
}

/// Handle on the running console.
pub struct Console {
    tx: mpsc::UnboundedSender<Command>,
    events: broadcast::Sender<ConsoleEvent>,
    task: JoinHandle<()>,
}

impl Console {
    pub fn start(config: ConsoleConfig) -> Console {
        let (tx, rx) = mpsc::unbounded_channel();
        let (relay_tx, relay_rx) = mpsc::unbounded_channel();
        let (events, _) = broadcast::channel(1024);
        let state = State {
            store: Box::new(MemoryStore::new(config.per_cell)),
            config,
            center: None,
            cells: Vec::new(),
            conns: HashMap::new(),
            states: HashMap::new(),
            seen: Seen::default(),
            participants: HashMap::new(),
            relay_tx,
            events: events.clone(),
        };
        let task = tokio::spawn(state.run(rx, relay_rx));
        Console { tx, events, task }
    }

    async fn ask<T>(&self, make: impl FnOnce(oneshot::Sender<T>) -> Command) -> Result<T> {
        let (reply, answer) = oneshot::channel();
        self.tx
            .send(make(reply))
            .map_err(|_| Error::ConsoleStopped)?;
        answer.await.map_err(|_| Error::ConsoleStopped)
    }

    /// Follows `geohash` (a channel level: 2, 4, 5, 6 or 7 characters) and
    /// its neighbours. Returns the followed cells, centre first.
    pub async fn set_area(&self, geohash: &str) -> Result<Vec<String>> {
        let g = geohash.trim().to_lowercase();
        self.ask(|r| Command::SetArea(g, r)).await?
    }

    /// The messages of a cell, oldest first.
    pub async fn messages(&self, cell: &str) -> Result<Vec<Message>> {
        let c = cell.to_owned();
        self.ask(|r| Command::Messages(c, r)).await
    }

    pub async fn status(&self) -> Result<AreaStatus> {
        self.ask(Command::Status).await
    }

    /// Changes from now on.
    pub fn subscribe(&self) -> broadcast::Receiver<ConsoleEvent> {
        self.events.subscribe()
    }

    /// Closes every connection.
    pub async fn shutdown(self) {
        let _ = self.tx.send(Command::Shutdown);
        let _ = self.task.await;
    }
}

/// Event ids already handled, bounded.
#[derive(Default)]
struct Seen {
    ids: HashSet<String>,
    order: VecDeque<String>,
}

impl Seen {
    const MAX: usize = 20_000;

    fn contains(&self, id: &str) -> bool {
        self.ids.contains(id)
    }

    fn insert(&mut self, id: &str) -> bool {
        if !self.ids.insert(id.to_owned()) {
            return false;
        }
        self.order.push_back(id.to_owned());
        if self.order.len() > Self::MAX
            && let Some(old) = self.order.pop_front()
        {
            self.ids.remove(&old);
        }
        true
    }
}

struct State {
    config: ConsoleConfig,
    store: Box<dyn MessageStore>,
    center: Option<String>,
    cells: Vec<String>,
    conns: HashMap<(String, String), RelayHandle>,
    states: HashMap<(String, String), ConnState>,
    seen: Seen,
    /// cell -> key -> last seen.
    participants: HashMap<String, HashMap<String, i64>>,
    relay_tx: mpsc::UnboundedSender<RelayEvent>,
    events: broadcast::Sender<ConsoleEvent>,
}

impl State {
    async fn run(
        mut self,
        mut commands: mpsc::UnboundedReceiver<Command>,
        mut relays: mpsc::UnboundedReceiver<RelayEvent>,
    ) {
        let mut tick = tokio::time::interval(Duration::from_secs(30));
        loop {
            tokio::select! {
                cmd = commands.recv() => match cmd {
                    None | Some(Command::Shutdown) => break,
                    Some(c) => self.on_command(c),
                },
                Some(e) = relays.recv() => self.on_relay(e),
                _ = tick.tick() => self.on_tick(),
            }
        }
        self.conns.clear();
    }

    fn emit(&self, e: ConsoleEvent) {
        let _ = self.events.send(e);
    }

    fn on_command(&mut self, c: Command) {
        match c {
            Command::SetArea(g, reply) => {
                let _ = reply.send(self.set_area(&g));
            }
            Command::Messages(cell, reply) => {
                let _ = reply.send(self.store.cell(&cell));
            }
            Command::Status(reply) => {
                let _ = reply.send(self.status());
            }
            Command::Shutdown => {}
        }
    }

    fn set_area(&mut self, g: &str) -> Result<Vec<String>> {
        if !CHANNEL_LEVELS.contains(&g.len()) {
            return Err(Error::NotAChannel(g.to_owned()));
        }
        let cells = geohash_relays::geohash::with_neighbours(g)
            .map_err(|_| Error::InvalidGeohash(g.to_owned()))?;
        let mut wanted: BTreeMap<(String, String), ()> = BTreeMap::new();
        for cell in &cells {
            for s in self
                .config
                .strategy
                .read(cell)
                .map_err(|_| Error::InvalidGeohash(cell.clone()))?
            {
                wanted.insert((s.relay.url, cell.clone()), ());
            }
        }
        self.conns.retain(|k, _| wanted.contains_key(k));
        self.states.retain(|k, _| wanted.contains_key(k));
        let since = unix_now() - self.config.lookback_secs;
        for (relay, cell) in wanted.into_keys() {
            if self.conns.contains_key(&(relay.clone(), cell.clone())) {
                continue;
            }
            let filter = Filter {
                kinds: vec![KIND_MESSAGE, KIND_PRESENCE],
                since: Some(since),
                limit: Some(self.config.limit as usize),
                tags: BTreeMap::from([('g', vec![cell.clone()])]),
                ..Default::default()
            };
            let handle = relay::spawn(
                relay.clone(),
                cell.clone(),
                vec![filter],
                self.config.connector.clone(),
                self.relay_tx.clone(),
            );
            self.conns.insert((relay, cell), handle);
        }
        tracing::info!(center = %g, cells = cells.len(), connections = self.conns.len(), "area set");
        self.center = Some(g.to_owned());
        self.cells = cells.clone();
        self.emit(ConsoleEvent::Area);
        Ok(cells)
    }

    fn status(&self) -> AreaStatus {
        let now = unix_now();
        let cells = self
            .cells
            .iter()
            .map(|cell| {
                let relays = self
                    .config
                    .strategy
                    .read(cell)
                    .unwrap_or_default()
                    .into_iter()
                    .map(|s| {
                        let key = (s.relay.url.clone(), cell.clone());
                        RelayStatus {
                            state: state_name(self.states.get(&key)),
                            url: s.relay.url,
                            ios: s.ios,
                            android: s.android,
                        }
                    })
                    .collect();
                CellStatus {
                    geohash: cell.clone(),
                    relays,
                    participants: self.participant_count(cell, now),
                }
            })
            .collect();
        AreaStatus {
            center: self.center.clone(),
            cells,
            connector: self.config.connector.describe(),
        }
    }

    fn participant_count(&self, cell: &str, now: i64) -> usize {
        let from = now - self.config.participant_window_secs;
        self.participants
            .get(cell)
            .map_or(0, |m| m.values().filter(|&&t| t >= from).count())
    }

    fn on_relay(&mut self, e: RelayEvent) {
        match e {
            RelayEvent::State { relay, cell, state } => {
                let key = (relay, cell);
                if self.conns.contains_key(&key) {
                    if let ConnState::Abandoned { reason } = &state {
                        tracing::warn!(relay = %key.0, cell = %key.1, %reason, "relay given up");
                    }
                    self.states.insert(key, state);
                    self.emit(ConsoleEvent::Relays);
                }
            }
            RelayEvent::Event { relay, cell, event } => self.on_event(relay, cell, event),
            RelayEvent::Notice { relay, cell, text }
            | RelayEvent::Closed {
                relay,
                cell,
                reason: text,
            } => self.emit(ConsoleEvent::RelayText { relay, cell, text }),
            RelayEvent::Ok { .. } | RelayEvent::Eose { .. } | RelayEvent::Auth { .. } => {}
        }
    }

    /// Checks an event delivered for `cell` and adds it to the feed.
    fn on_event(&mut self, relay: String, cell: String, event: Event) {
        if !self.cells.contains(&cell) {
            return;
        }
        // Cheap checks first; an event that fails them is not marked as
        // seen, so that a relay sending it on the wrong connection cannot
        // hide it when it comes on the right one.
        let fits = (event.kind == KIND_MESSAGE || event.kind == KIND_PRESENCE)
            && event
                .tag_value("g")
                .is_some_and(|g| g.eq_ignore_ascii_case(&cell))
            && event.within_tag_limits();
        if !fits {
            tracing::debug!(%relay, %cell, "event dropped (kind, cell or limits)");
            return;
        }
        if self.seen.contains(&event.id) {
            self.store.add_relay(&event.id, &relay);
            self.emit(ConsoleEvent::Delivered {
                id: event.id,
                relay,
            });
            return;
        }
        if event.verify().is_err() {
            tracing::debug!(%relay, %cell, "event dropped (id or signature)");
            return;
        }
        self.seen.insert(&event.id);
        let now = unix_now();
        // A date in the future is taken as now, as iOS does.
        let at = event.created_at.min(now);
        let people = self.participants.entry(cell.clone()).or_default();
        let before = people.len();
        let last = people.entry(event.pubkey.clone()).or_insert(at);
        *last = (*last).max(at);
        if people.len() != before {
            let count = self.participant_count(&cell, now);
            self.emit(ConsoleEvent::Participants {
                cell: cell.clone(),
                count,
            });
        }
        if event.kind != KIND_MESSAGE {
            return;
        }
        let teleport = event.tags.iter().any(|t| {
            t.first().map(String::as_str) == Some("t")
                && t.get(1).map(String::as_str) == Some("teleport")
        });
        let message = Message {
            nickname: event.tag_value("n").map(str::to_owned),
            id: event.id,
            cell,
            pubkey: event.pubkey,
            content: event.content,
            created_at: at,
            teleport,
            own: false,
            relays: vec![relay],
        };
        if self.store.add(message.clone()) {
            self.emit(ConsoleEvent::Message(message));
        }
    }

    fn on_tick(&mut self) {
        let now = unix_now();
        let from = now - self.config.participant_window_secs;
        for people in self.participants.values_mut() {
            people.retain(|_, t| *t >= from);
        }
        self.store.purge(now - 24 * 3600);
    }
}

fn state_name(s: Option<&ConnState>) -> String {
    match s {
        None | Some(ConnState::Connecting) => "connecting".into(),
        Some(ConnState::Connected) => "connected".into(),
        Some(ConnState::Waiting { retry_in_secs }) => format!("waiting ({retry_in_secs} s)"),
        Some(ConnState::Abandoned { reason }) => format!("abandoned: {reason}"),
    }
}
