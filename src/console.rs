//! The console: the followed area (a cell and its 8 neighbours), one
//! connection per (relay, cell) through the connector, the feed.
//!
//! A task owns the state; [`Console`] is the handle to it. Changing the
//! area moves only the connections that change.
//!
//! Writing (lot 5): messages signed with the key of their cell, mined like
//! iOS, sent to the cell's relays; presences every 40 to 80 s to the levels
//! 2, 4 and 5 of the centre, unless the console says it is teleported (D5,
//! D22).

use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::sync::Arc;
use std::time::Duration;

use rand_core::{OsRng, RngCore};

use geohash_relays::Strategy;
use serde::Serialize;
use tokio::sync::{broadcast, mpsc, oneshot};
use tokio::task::JoinHandle;

use crate::connector::Connector;
use crate::error::{Error, Result};
use crate::identity::Seed;
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
    /// Root of the cell keys.
    pub seed: Arc<Seed>,
    /// NIP-13 target of messages (iOS: 8; Android users may require 12).
    pub pow_bits: u32,
    /// Send presences (D5).
    pub presence: bool,
    /// Delay between presences, drawn in this range (iOS: 40 to 80 s).
    pub presence_every: (Duration, Duration),
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
    pub fn new(
        strategy: Arc<dyn Strategy>,
        connector: Arc<dyn Connector>,
        seed: Arc<Seed>,
    ) -> Self {
        ConsoleConfig {
            strategy,
            connector,
            seed,
            pow_bits: crate::pow::TARGET_BITS,
            presence: true,
            presence_every: (Duration::from_secs(40), Duration::from_secs(80)),
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
    /// The console says it is not in the area (`t=teleport`, no presence).
    pub teleport: bool,
    pub connector: String,
    /// NIP-13 target of the messages it sends.
    pub pow_bits: u32,
}

/// Levels the app sends presences to (`GeohashPresenceService`).
pub const PRESENCE_LEVELS: [usize; 3] = [2, 4, 5];

/// The cells a presence goes to for a centre: its prefixes of 2, 4 and 5
/// characters (as long as the centre is).
pub fn presence_cells(center: &str) -> Vec<String> {
    PRESENCE_LEVELS
        .iter()
        .filter(|&&l| l <= center.len())
        .map(|&l| center[..l].to_owned())
        .collect()
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
    SetArea(String, bool, oneshot::Sender<Result<Vec<String>>>),
    Publish(Box<Event>, oneshot::Sender<Result<Message>>),
    Messages(String, oneshot::Sender<Vec<Message>>),
    Status(oneshot::Sender<AreaStatus>),
    Shutdown,
}

/// Handle on the running console.
pub struct Console {
    tx: mpsc::UnboundedSender<Command>,
    events: broadcast::Sender<ConsoleEvent>,
    task: JoinHandle<()>,
    seed: Arc<Seed>,
}

impl Console {
    pub fn start(config: ConsoleConfig) -> Console {
        let seed = config.seed.clone();
        let (tx, rx) = mpsc::unbounded_channel();
        let (relay_tx, relay_rx) = mpsc::unbounded_channel();
        let (events, _) = broadcast::channel(1024);
        let state = State {
            store: Box::new(MemoryStore::new(config.per_cell)),
            config,
            center: None,
            teleport: false,
            next_presence: None,
            cells: Vec::new(),
            conns: HashMap::new(),
            states: HashMap::new(),
            seen: Seen::default(),
            participants: HashMap::new(),
            relay_tx,
            events: events.clone(),
        };
        let task = tokio::spawn(state.run(rx, relay_rx));
        Console {
            tx,
            events,
            task,
            seed,
        }
    }

    async fn ask<T>(&self, make: impl FnOnce(oneshot::Sender<T>) -> Command) -> Result<T> {
        let (reply, answer) = oneshot::channel();
        self.tx
            .send(make(reply))
            .map_err(|_| Error::ConsoleStopped)?;
        answer.await.map_err(|_| Error::ConsoleStopped)
    }

    /// Follows `geohash` (a channel level: 2, 4, 5, 6 or 7 characters) and
    /// its neighbours. `teleport`: the console says it is not there (its
    /// messages carry `t=teleport` and it sends no presence, like a
    /// teleported user of the app); chosen at each change of area (D5).
    /// Returns the followed cells, centre first.
    pub async fn set_area(&self, geohash: &str, teleport: bool) -> Result<Vec<String>> {
        let g = geohash.trim().to_lowercase();
        self.ask(|r| Command::SetArea(g, teleport, r)).await?
    }

    /// Sends a message to a followed cell (the centre by default), signed
    /// with the key of that cell, mined like iOS. Returns it as the feed
    /// shows it; the relays that accept it are added as they answer.
    pub async fn send(
        &self,
        text: &str,
        cell: Option<&str>,
        nickname: Option<&str>,
    ) -> Result<Message> {
        if text.trim().is_empty() {
            return Err(Error::EmptyMessage);
        }
        let status = self.status().await?;
        let center = status.center.clone().ok_or(Error::NoArea)?;
        let cell = cell.map(str::to_lowercase).unwrap_or(center);
        if !status.cells.iter().any(|c| c.geohash == cell) {
            return Err(Error::NotInArea(cell));
        }
        let key = self.seed.cell(&cell)?;
        let mut tags = vec![vec!["g".to_owned(), cell.clone()]];
        if let Some(n) = nickname.map(str::trim).filter(|n| !n.is_empty()) {
            tags.push(vec!["n".to_owned(), n.to_owned()]);
        }
        if status.teleport {
            tags.push(vec!["t".to_owned(), "teleport".to_owned()]);
        }
        let (text, bits) = (text.to_owned(), status.pow_bits);
        let event = tokio::task::spawn_blocking(move || {
            let pubkey = key.pubkey_hex();
            let at = unix_now();
            let tags = crate::pow::mine(&pubkey, at, KIND_MESSAGE, &tags, &text, bits);
            Event::signed(key.signing_key(), at, KIND_MESSAGE, tags, text)
        })
        .await
        .map_err(|_| Error::ConsoleStopped)?;
        self.ask(|r| Command::Publish(Box::new(event), r)).await?
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
    teleport: bool,
    next_presence: Option<tokio::time::Instant>,
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
            let presence = self.next_presence;
            tokio::select! {
                cmd = commands.recv() => match cmd {
                    None | Some(Command::Shutdown) => break,
                    Some(c) => self.on_command(c),
                },
                Some(e) = relays.recv() => self.on_relay(e),
                _ = tick.tick() => self.on_tick(),
                _ = async {
                    match presence {
                        Some(at) => tokio::time::sleep_until(at).await,
                        None => std::future::pending().await,
                    }
                } => self.send_presences(),
            }
        }
        self.conns.clear();
    }

    fn emit(&self, e: ConsoleEvent) {
        let _ = self.events.send(e);
    }

    fn on_command(&mut self, c: Command) {
        match c {
            Command::SetArea(g, teleport, reply) => {
                let _ = reply.send(self.set_area(&g, teleport));
            }
            Command::Publish(event, reply) => {
                let _ = reply.send(self.publish_message(*event));
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

    fn set_area(&mut self, g: &str, teleport: bool) -> Result<Vec<String>> {
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
        // Presences go to the prefixes of the centre, through connections
        // that only write (no subscription) when the cell is not followed.
        let presence = self.config.presence && !teleport;
        let mut write_only: Vec<(String, String)> = Vec::new();
        if presence {
            for cell in presence_cells(g) {
                if cells.contains(&cell) {
                    continue;
                }
                for s in self
                    .config
                    .strategy
                    .write(&cell)
                    .map_err(|_| Error::InvalidGeohash(cell.clone()))?
                {
                    write_only.push((s.relay.url, cell.clone()));
                }
            }
        }
        for k in &write_only {
            wanted.insert(k.clone(), ());
        }
        self.conns.retain(|k, _| wanted.contains_key(k));
        self.states.retain(|k, _| wanted.contains_key(k));
        let since = unix_now() - self.config.lookback_secs;
        for (relay, cell) in wanted.into_keys() {
            if self.conns.contains_key(&(relay.clone(), cell.clone())) {
                continue;
            }
            if write_only.contains(&(relay.clone(), cell.clone())) {
                self.connect(relay, cell, Vec::new());
                continue;
            }
            let filter = Filter {
                kinds: vec![KIND_MESSAGE, KIND_PRESENCE],
                since: Some(since),
                limit: Some(self.config.limit as usize),
                tags: BTreeMap::from([('g', vec![cell.clone()])]),
                ..Default::default()
            };
            self.connect(relay, cell, vec![filter]);
        }
        tracing::info!(center = %g, cells = cells.len(), connections = self.conns.len(), teleport, "area set");
        self.teleport = teleport;
        self.next_presence = presence.then(|| {
            // The first one soon, once the connections are up.
            tokio::time::Instant::now() + self.config.presence_every.0.min(Duration::from_secs(3))
        });
        self.center = Some(g.to_owned());
        self.cells = cells.clone();
        self.emit(ConsoleEvent::Area);
        Ok(cells)
    }

    fn connect(&mut self, relay: String, cell: String, filters: Vec<Filter>) {
        let handle = relay::spawn(
            relay.clone(),
            cell.clone(),
            filters,
            self.config.connector.clone(),
            self.relay_tx.clone(),
        );
        self.conns.insert((relay, cell), handle);
    }

    /// Sends an event of `cell` to the relays the strategy writes to.
    fn publish(&mut self, cell: &str, event: &Event) -> Result<()> {
        let relays = self
            .config
            .strategy
            .write(cell)
            .map_err(|_| Error::InvalidGeohash(cell.to_owned()))?;
        for s in relays {
            let key = (s.relay.url.clone(), cell.to_owned());
            if !self.conns.contains_key(&key) {
                self.connect(key.0.clone(), key.1.clone(), Vec::new());
            }
            if let Some(conn) = self.conns.get(&key) {
                conn.publish(event.clone());
            }
        }
        Ok(())
    }

    /// A message built by the handle: checked, sent, added to the feed.
    fn publish_message(&mut self, event: Event) -> Result<Message> {
        let cell = event.tag_value("g").unwrap_or_default().to_owned();
        if !self.cells.contains(&cell) {
            return Err(Error::NotInArea(cell));
        }
        self.publish(&cell, &event)?;
        self.seen.insert(&event.id);
        let message = Message {
            nickname: event.tag_value("n").map(str::to_owned),
            teleport: self.teleport,
            id: event.id,
            cell,
            pubkey: event.pubkey,
            content: event.content,
            created_at: event.created_at,
            own: true,
            relays: Vec::new(),
        };
        self.store.add(message.clone());
        self.emit(ConsoleEvent::Message(message.clone()));
        Ok(message)
    }

    /// One presence per presence cell, each signed with that cell's key,
    /// then the next one in 40 to 80 s.
    fn send_presences(&mut self) {
        let Some(center) = self.center.clone() else {
            self.next_presence = None;
            return;
        };
        for cell in presence_cells(&center) {
            let Ok(key) = self.config.seed.cell(&cell) else {
                continue;
            };
            // Exactly the app's presence: `g` only, empty content.
            let e = Event::signed(
                key.signing_key(),
                unix_now(),
                KIND_PRESENCE,
                vec![vec!["g".to_owned(), cell.clone()]],
                String::new(),
            );
            self.seen.insert(&e.id);
            let _ = self.publish(&cell, &e);
        }
        let (lo, hi) = self.config.presence_every;
        let span = hi.saturating_sub(lo).as_millis() as u64;
        let jitter = if span == 0 {
            0
        } else {
            OsRng.next_u64() % span
        };
        self.next_presence = Some(tokio::time::Instant::now() + lo + Duration::from_millis(jitter));
        tracing::debug!(%center, "presence sent");
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
            teleport: self.teleport,
            connector: self.config.connector.describe(),
            pow_bits: self.config.pow_bits,
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
            RelayEvent::Ok {
                relay,
                cell,
                id,
                accepted,
                message,
            } => {
                if accepted {
                    self.store.add_relay(&id, &relay);
                    self.emit(ConsoleEvent::Delivered { id, relay });
                } else {
                    tracing::info!(%relay, %cell, reason = %message, "event refused by a relay");
                    self.emit(ConsoleEvent::RelayText {
                        relay,
                        cell,
                        text: format!("refused: {message}"),
                    });
                }
            }
            RelayEvent::Eose { .. } | RelayEvent::Auth { .. } => {}
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
