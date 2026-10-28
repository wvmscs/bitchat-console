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

mod groups;

pub use groups::{GroupChat, GroupOp, GroupView, GroupsView, Invitation, MemberView};

use crate::connector::Connector;
use crate::db::{Db, Preset, Schedule};
use crate::envelope;
use crate::error::{Error, Result};
use crate::identity::{CellKey, NoiseIdentity, Seed};
use crate::nostr::{Event, Filter, unix_now};
use crate::packet::{self, Packet};
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
    /// Relays of private messages: the default relays of both apps.
    pub dm_relays: Vec<String>,
    /// Local storage (mutes, saved and scheduled messages); `None`: in
    /// memory, lost at the end.
    pub db_path: Option<std::path::PathBuf>,
    /// The console as a BitChat peer, for groups (lot 10).
    pub noise: Option<Arc<NoiseIdentity>>,
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
            dm_relays: default_dm_relays(),
            db_path: None,
            noise: None,
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

/// Someone seen in a cell within the window (messages and presences), in
/// memory only.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Participant {
    pub pubkey: String,
    /// Last `n` tag seen from them.
    pub nickname: Option<String>,
    /// Last seen (`created_at`, never in the future).
    pub last_seen: i64,
    /// Their last event was tagged `t=teleport`.
    pub teleport: bool,
    /// Channel messages seen from them in the window.
    pub messages: u32,
}

/// What is kept of a key in a cell.
#[derive(Debug, Clone, Default)]
struct Known {
    last: i64,
    nickname: Option<String>,
    teleport: bool,
    messages: u32,
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
    /// The console's public key in each cell it uses (public).
    pub keys: BTreeMap<String, String>,
}

/// Delay before a connection no longer wanted is closed (it may still have
/// something to send); the actual close happens at the next control tick.
const CLOSE_DELAY: Duration = Duration::from_secs(10);

/// How far back the private-message inbox asks (iOS: 24 h).
pub const DM_LOOKBACK_SECS: i64 = 24 * 3600;

/// The default relays of iOS and Android together: a message there reaches
/// users of both apps.
pub fn default_dm_relays() -> Vec<String> {
    let mut v: Vec<String> = geohash_relays::Platform::Ios
        .default_relays()
        .iter()
        .chain(geohash_relays::Platform::Android.default_relays())
        .map(|s| s.to_string())
        .collect();
    v.sort();
    v.dedup();
    v
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

/// Where a private message the console sent stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DmStatus {
    Sent,
    /// A relay accepted it.
    Relayed,
    /// The recipient's app acknowledged it.
    Delivered,
    Read,
}

/// A private message, sent or received.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DirectMessage {
    /// BitChat message ID (a UUID string).
    pub id: String,
    /// The other side's key (of the cell where it was met).
    pub peer: String,
    /// The cell whose key the console uses in this conversation.
    pub cell: String,
    pub content: String,
    pub created_at: i64,
    pub own: bool,
    /// For messages the console sent.
    pub status: Option<DmStatus>,
}

/// What the user keeps locally: muted keys, saved and scheduled messages.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Local {
    pub mutes: Vec<String>,
    pub presets: Vec<Preset>,
    pub schedules: Vec<Schedule>,
}

/// Private messages kept in memory.
const MAX_DMS: usize = 2000;

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
    Dm(DirectMessage),
    DmStatus {
        id: String,
        status: DmStatus,
    },
    /// Mutes, saved or scheduled messages changed.
    Local,
    /// Groups or invitations changed.
    Groups,
    GroupMessage(GroupChat),
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
    SendDm {
        peer: String,
        cell: String,
        text: String,
        reply: oneshot::Sender<Result<DirectMessage>>,
    },
    Dms(oneshot::Sender<Vec<DirectMessage>>),
    MarkRead(String, oneshot::Sender<usize>),
    Mute(String, bool, oneshot::Sender<Result<()>>),
    Local(oneshot::Sender<Result<Local>>),
    AddPreset(Preset, oneshot::Sender<Result<i64>>),
    DeletePreset(i64, oneshot::Sender<Result<bool>>),
    AddSchedule(Schedule, oneshot::Sender<Result<i64>>),
    DeleteSchedule(i64, oneshot::Sender<Result<bool>>),
    Group(groups::GroupOp, oneshot::Sender<Result<()>>),
    Groups(oneshot::Sender<GroupsView>),
    Messages(String, oneshot::Sender<Vec<Message>>),
    Participants(String, oneshot::Sender<Vec<Participant>>),
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
        let db = match &config.db_path {
            Some(p) => Db::open(p).unwrap_or_else(|e| {
                tracing::error!(error = %e, "local storage unusable, kept in memory for this run");
                Db::open_in_memory().expect("in-memory SQLite")
            }),
            None => Db::open_in_memory().expect("in-memory SQLite"),
        };
        let (tx, rx) = mpsc::unbounded_channel();
        let (relay_tx, relay_rx) = mpsc::unbounded_channel();
        let (events, _) = broadcast::channel(1024);
        let loaded_groups = groups::load(&db);
        let state = State {
            groups: loaded_groups,
            group_msgs: VecDeque::new(),
            invitations: Vec::new(),
            awaiting: HashSet::new(),
            invited: Vec::new(),
            closing: Vec::new(),
            db,
            store: Box::new(MemoryStore::new(config.per_cell)),
            config,
            center: None,
            teleport: false,
            next_presence: None,
            cells: Vec::new(),
            identities: Vec::new(),
            conns: HashMap::new(),
            subs: HashMap::new(),
            dms: VecDeque::new(),
            dm_wraps: HashMap::new(),
            read_sent: HashSet::new(),
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
        let (seed, text, nickname) = (
            self.seed.clone(),
            text.to_owned(),
            nickname.map(str::to_owned),
        );
        let (teleport, bits) = (status.teleport, status.pow_bits);
        let event = tokio::task::spawn_blocking(move || {
            build_message(&seed, &cell, &text, nickname.as_deref(), teleport, bits)
        })
        .await
        .map_err(|_| Error::ConsoleStopped)??;
        self.ask(|r| Command::Publish(Box::new(event), r)).await?
    }

    /// Sends a private message to `peer` (hex key) from the key of `cell`
    /// (one of the cells in use: where the peer was met), in the app's
    /// envelope, on the default relays. At most 255 bytes of UTF-8, the
    /// app's limit.
    pub async fn send_dm(&self, peer: &str, cell: &str, text: &str) -> Result<DirectMessage> {
        if text.trim().is_empty() {
            return Err(Error::EmptyMessage);
        }
        let (peer, cell, text) = (peer.to_lowercase(), cell.to_lowercase(), text.to_owned());
        self.ask(|reply| Command::SendDm {
            peer,
            cell,
            text,
            reply,
        })
        .await?
    }

    /// Private messages, oldest first.
    pub async fn dms(&self) -> Result<Vec<DirectMessage>> {
        self.ask(Command::Dms).await
    }

    /// Sends read receipts for the messages received from `peer`, as the app
    /// does when the conversation is shown. Returns how many.
    pub async fn mark_read(&self, peer: &str) -> Result<usize> {
        let p = peer.to_lowercase();
        self.ask(|r| Command::MarkRead(p, r)).await
    }

    /// Hides (or shows again) everything from a key, locally (D25).
    pub async fn set_muted(&self, pubkey: &str, muted: bool) -> Result<()> {
        let p = pubkey.to_lowercase();
        if p.len() != 64 || !p.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(Error::InvalidKey(p));
        }
        self.ask(|r| Command::Mute(p, muted, r)).await?
    }

    /// Muted keys, saved and scheduled messages.
    pub async fn local(&self) -> Result<Local> {
        self.ask(Command::Local).await?
    }

    /// Saves a message to send later on demand. Returns its id.
    pub async fn add_preset(
        &self,
        name: &str,
        text: &str,
        cell: Option<&str>,
        nickname: Option<&str>,
    ) -> Result<i64> {
        if text.trim().is_empty() {
            return Err(Error::EmptyMessage);
        }
        let p = Preset {
            id: 0,
            name: name.trim().to_owned(),
            text: text.to_owned(),
            cell: cell.map(str::to_lowercase),
            nickname: nickname.map(str::to_owned),
        };
        self.ask(|r| Command::AddPreset(p, r)).await?
    }

    pub async fn delete_preset(&self, id: i64) -> Result<bool> {
        self.ask(|r| Command::DeletePreset(id, r)).await?
    }

    /// Sends a saved message, in `cell` if given, else in its own cell, else
    /// in the centre.
    pub async fn send_preset(&self, id: i64, cell: Option<&str>) -> Result<Message> {
        let p = self
            .local()
            .await?
            .presets
            .into_iter()
            .find(|p| p.id == id)
            .ok_or(Error::NotFound(id))?;
        let cell = cell.map(str::to_owned).or(p.cell);
        self.send(&p.text, cell.as_deref(), p.nickname.as_deref())
            .await
    }

    /// Sends `text` at `at` (Unix time), then every `repeat_secs` if given,
    /// while the console runs. Returns its id.
    pub async fn add_schedule(
        &self,
        text: &str,
        cell: Option<&str>,
        nickname: Option<&str>,
        at: i64,
        repeat_secs: Option<i64>,
    ) -> Result<i64> {
        if text.trim().is_empty() {
            return Err(Error::EmptyMessage);
        }
        if repeat_secs.is_some_and(|r| r < 60) {
            return Err(Error::Schedule("repeat at most once a minute".into()));
        }
        let s = Schedule {
            id: 0,
            text: text.to_owned(),
            cell: cell.map(str::to_lowercase),
            nickname: nickname.map(str::to_owned),
            next_at: Some(at),
            repeat_secs,
            last_result: None,
        };
        self.ask(|r| Command::AddSchedule(s, r)).await?
    }

    pub async fn delete_schedule(&self, id: i64) -> Result<bool> {
        self.ask(|r| Command::DeleteSchedule(id, r)).await?
    }

    /// Groups, their messages and the invitations waiting.
    pub async fn groups(&self) -> Result<GroupsView> {
        self.ask(Command::Groups).await
    }

    /// Runs a group operation (create, invite, accept, send…).
    pub async fn group(&self, op: groups::GroupOp) -> Result<()> {
        self.ask(|r| Command::Group(op, r)).await?
    }

    /// The messages of a cell, oldest first.
    pub async fn messages(&self, cell: &str) -> Result<Vec<Message>> {
        let c = cell.to_owned();
        self.ask(|r| Command::Messages(c, r)).await
    }

    pub async fn status(&self) -> Result<AreaStatus> {
        self.ask(Command::Status).await
    }

    /// Who was seen in `cell` within the window, the most recent first.
    pub async fn participants(&self, cell: &str) -> Result<Vec<Participant>> {
        let c = cell.to_ascii_lowercase();
        self.ask(|r| Command::Participants(c, r)).await
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

/// A channel message as the app writes it: tags `g`, `n`, `t=teleport`,
/// then the mined nonce; signed with the key of the cell.
fn build_message(
    seed: &Seed,
    cell: &str,
    text: &str,
    nickname: Option<&str>,
    teleport: bool,
    pow_bits: u32,
) -> Result<Event> {
    let key = seed.cell(cell)?;
    let mut tags = vec![vec!["g".to_owned(), cell.to_owned()]];
    if let Some(n) = nickname.map(str::trim).filter(|n| !n.is_empty()) {
        tags.push(vec!["n".to_owned(), n.to_owned()]);
    }
    if teleport {
        tags.push(vec!["t".to_owned(), "teleport".to_owned()]);
    }
    let pubkey = key.pubkey_hex();
    let at = unix_now();
    let tags = crate::pow::mine(&pubkey, at, KIND_MESSAGE, &tags, text, pow_bits);
    Ok(Event::signed(
        key.signing_key(),
        at,
        KIND_MESSAGE,
        tags,
        text.to_owned(),
    ))
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
    db: Db,
    /// Group ID (hex) -> state and the pinned Nostr address of its creator.
    groups: BTreeMap<String, (crate::groups::GroupState, String)>,
    group_msgs: VecDeque<GroupChat>,
    /// Invitations received, waiting for the user.
    invitations: Vec<Invitation>,
    /// Groups accepted, whose first state has not come yet.
    awaiting: HashSet<String>,
    /// Join requests sent: (group, peer key, cell).
    invited: Vec<(String, String, String)>,
    /// Connections no longer wanted, closed a little later so that what
    /// they still have to send (a message just before an area change, the
    /// last envelopes of a group) goes out. Their events are ignored.
    closing: Vec<(tokio::time::Instant, RelayHandle)>,
    store: Box<dyn MessageStore>,
    center: Option<String>,
    teleport: bool,
    next_presence: Option<tokio::time::Instant>,
    cells: Vec<String>,
    /// Cells whose key the console uses: the 9 cells, plus the presence
    /// cells.
    identities: Vec<String>,
    conns: HashMap<(String, String), RelayHandle>,
    /// Filters sent on each connection.
    subs: HashMap<(String, String), Vec<Filter>>,
    dms: VecDeque<DirectMessage>,
    /// Outer event id of a DM the console sent -> its message ID.
    dm_wraps: HashMap<String, String>,
    /// Received message IDs already acknowledged as read.
    read_sent: HashSet<String>,
    states: HashMap<(String, String), ConnState>,
    seen: Seen,
    /// cell -> key -> last seen.
    participants: HashMap<String, HashMap<String, Known>>,
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
        let mut schedules = tokio::time::interval(Duration::from_secs(1));
        schedules.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            let presence = self.next_presence;
            tokio::select! {
                cmd = commands.recv() => match cmd {
                    None | Some(Command::Shutdown) => break,
                    Some(c) => self.on_command(c),
                },
                Some(e) = relays.recv() => self.on_relay(e),
                _ = tick.tick() => self.on_tick(),
                _ = schedules.tick() => self.send_due(),
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
            Command::SendDm {
                peer,
                cell,
                text,
                reply,
            } => {
                let _ = reply.send(self.send_dm(&peer, &cell, &text));
            }
            Command::Dms(reply) => {
                let _ = reply.send(self.dms.iter().cloned().collect());
            }
            Command::MarkRead(peer, reply) => {
                let _ = reply.send(self.mark_read(&peer));
            }
            Command::Mute(pubkey, muted, reply) => {
                let r = self.db.set_muted(&pubkey, muted, unix_now());
                self.local_changed();
                let _ = reply.send(r);
            }
            Command::Local(reply) => {
                let _ = reply.send(self.local());
            }
            Command::AddPreset(p, reply) => {
                let r =
                    self.db
                        .add_preset(&p.name, &p.text, p.cell.as_deref(), p.nickname.as_deref());
                self.local_changed();
                let _ = reply.send(r);
            }
            Command::DeletePreset(id, reply) => {
                let r = self.db.delete_preset(id);
                self.local_changed();
                let _ = reply.send(r);
            }
            Command::AddSchedule(s, reply) => {
                let r = self.db.add_schedule(
                    &s.text,
                    s.cell.as_deref(),
                    s.nickname.as_deref(),
                    s.next_at.unwrap_or_else(unix_now),
                    s.repeat_secs,
                );
                self.local_changed();
                let _ = reply.send(r);
            }
            Command::Group(op, reply) => {
                let r = self.group_op(op);
                let _ = reply.send(r);
            }
            Command::Groups(reply) => {
                let _ = reply.send(self.groups_view());
            }
            Command::DeleteSchedule(id, reply) => {
                let r = self.db.delete_schedule(id);
                self.local_changed();
                let _ = reply.send(r);
            }
            Command::Messages(cell, reply) => {
                let _ = reply.send(self.store.cell(&cell));
            }
            Command::Participants(cell, reply) => {
                let _ = reply.send(self.participants_of(&cell, unix_now()));
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
        let bad = |c: &str| Error::InvalidGeohash(c.to_owned());
        let now = unix_now();
        let mut wanted: BTreeMap<(String, String), Vec<Filter>> = BTreeMap::new();
        // The channels of the 9 cells.
        for cell in &cells {
            for s in self.config.strategy.read(cell).map_err(|_| bad(cell))? {
                wanted
                    .entry((s.relay.url, cell.clone()))
                    .or_default()
                    .push(Filter {
                        kinds: vec![KIND_MESSAGE, KIND_PRESENCE],
                        since: Some(now - self.config.lookback_secs),
                        limit: Some(self.config.limit as usize),
                        tags: BTreeMap::from([('g', vec![cell.clone()])]),
                        ..Default::default()
                    });
            }
        }
        // Presences go to the prefixes of the centre, through connections
        // that only write when the cell is not followed.
        let presence = self.config.presence && !teleport;
        let mut identities = cells.clone();
        if presence {
            for cell in presence_cells(g) {
                if cells.contains(&cell) {
                    continue;
                }
                for s in self.config.strategy.write(&cell).map_err(|_| bad(&cell))? {
                    wanted.entry((s.relay.url, cell.clone())).or_default();
                }
                identities.push(cell);
            }
        }
        // The private-message inbox of each identity in use, on the default
        // relays, through the circuit of its cell (D7, D20).
        let mut inboxes = identities.clone();
        inboxes.extend(
            self.groups
                .keys()
                .chain(&self.awaiting)
                .map(|g| groups::label(g)),
        );
        for cell in &inboxes {
            let key = self.label_key(cell)?;
            for relay in &self.config.dm_relays {
                wanted
                    .entry((relay.clone(), cell.clone()))
                    .or_default()
                    .push(Filter {
                        kinds: vec![envelope::KIND_WRAP],
                        since: Some(now - DM_LOOKBACK_SECS),
                        limit: Some(100),
                        tags: BTreeMap::from([('p', vec![key.pubkey_hex()])]),
                        ..Default::default()
                    });
            }
        }
        let gone: Vec<(String, String)> = self
            .conns
            .keys()
            .filter(|k| !wanted.contains_key(*k))
            .cloned()
            .collect();
        let later = tokio::time::Instant::now() + CLOSE_DELAY;
        for k in gone {
            if let Some(h) = self.conns.remove(&k) {
                self.closing.push((later, h));
            }
        }
        self.states.retain(|k, _| wanted.contains_key(k));
        self.subs.retain(|k, _| wanted.contains_key(k));
        for ((relay, cell), filters) in wanted {
            let key = (relay.clone(), cell.clone());
            // `since` moves with time: compare the rest.
            let shape = |f: &[Filter]| -> Vec<Filter> {
                f.iter()
                    .map(|f| Filter {
                        since: None,
                        ..f.clone()
                    })
                    .collect()
            };
            match self.conns.get(&key) {
                Some(conn) => {
                    if self.subs.get(&key).map(|f| shape(f)) != Some(shape(&filters)) {
                        conn.set_filters(filters.clone());
                        self.subs.insert(key, filters);
                    }
                }
                None => {
                    self.subs.insert(key, filters.clone());
                    self.connect(relay, cell, filters);
                }
            }
        }
        tracing::info!(center = %g, cells = cells.len(), connections = self.conns.len(), teleport, "area set");
        self.teleport = teleport;
        self.next_presence = presence.then(|| {
            // The first one soon, once the connections are up.
            tokio::time::Instant::now() + self.config.presence_every.0.min(Duration::from_secs(3))
        });
        self.center = Some(g.to_owned());
        self.cells = cells.clone();
        self.identities = identities;
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

    /// Publishes a private envelope from the key of `cell` on the DM relays.
    fn publish_dm(&mut self, cell: &str, wrap: &Event) {
        for relay in self.config.dm_relays.clone() {
            let key = (relay.clone(), cell.to_owned());
            if !self.conns.contains_key(&key) {
                self.connect(relay, cell.to_owned(), Vec::new());
            }
            if let Some(conn) = self.conns.get(&key) {
                conn.publish(wrap.clone());
            }
        }
        self.seen.insert(&wrap.id);
    }

    /// A `bitchat1:` packet with a `noiseEncrypted` payload, in an envelope
    /// to `peer` from the key of `cell`.
    /// The key behind an inbox label: a cell, or `group:<id>` (D27).
    fn label_key(&self, label: &str) -> Result<CellKey> {
        match label.strip_prefix("group:") {
            Some(g) => self.config.seed.labelled(&format!("group|{g}")),
            None => self.config.seed.cell(label),
        }
    }

    fn envelope_to(&self, peer: &str, cell: &str, payload: Vec<u8>) -> Result<Event> {
        self.packet_to(
            peer,
            cell,
            Packet::embedded(payload, (unix_now() as u64) * 1000),
        )
    }

    /// A packet in an envelope to `peer` from the key of `label`.
    fn packet_to(&self, peer: &str, label: &str, packet: Packet) -> Result<Event> {
        let key = self.label_key(label)?;
        let packet = packet.encode();
        let content = format!("bitchat1:{}", envelope::b64url_encode(&packet));
        envelope::create(&content, peer, key.signing_key())
            .ok_or(Error::InvalidKey(peer.to_owned()))
    }

    fn send_dm(&mut self, peer: &str, cell: &str, text: &str) -> Result<DirectMessage> {
        if !self.identities.iter().any(|c| c == cell) {
            return Err(Error::NotInArea(cell.to_owned()));
        }
        if peer.len() != 64 || !peer.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(Error::InvalidKey(peer.to_owned()));
        }
        let id = packet::new_message_id();
        let payload = packet::encode_private_message(&id, text).ok_or(Error::DmTooLong)?;
        let wrap = self.envelope_to(peer, cell, payload)?;
        self.publish_dm(cell, &wrap);
        self.dm_wraps.insert(wrap.id.clone(), id.clone());
        let dm = DirectMessage {
            id,
            peer: peer.to_owned(),
            cell: cell.to_owned(),
            content: text.to_owned(),
            created_at: unix_now(),
            own: true,
            status: Some(DmStatus::Sent),
        };
        self.keep_dm(dm.clone());
        Ok(dm)
    }

    fn keep_dm(&mut self, dm: DirectMessage) {
        self.dms.push_back(dm.clone());
        while self.dms.len() > MAX_DMS {
            self.dms.pop_front();
        }
        self.emit(ConsoleEvent::Dm(dm));
    }

    /// Moves a sent message forward (never back: a late OK does not undo a
    /// read receipt).
    fn set_dm_status(&mut self, id: &str, status: DmStatus) {
        let mut changed = false;
        for dm in self.dms.iter_mut().filter(|d| d.own && d.id == id) {
            if dm.status.is_none_or(|s| s < status) {
                dm.status = Some(status);
                changed = true;
            }
        }
        if changed {
            self.emit(ConsoleEvent::DmStatus {
                id: id.to_owned(),
                status,
            });
        }
    }

    fn mark_read(&mut self, peer: &str) -> usize {
        let unread: Vec<(String, String)> = self
            .dms
            .iter()
            .filter(|d| !d.own && d.peer == peer && !self.read_sent.contains(&d.id))
            .map(|d| (d.id.clone(), d.cell.clone()))
            .collect();
        for (id, cell) in &unread {
            let mut payload = vec![packet::payload::READ_RECEIPT];
            payload.extend_from_slice(id.as_bytes());
            if let Ok(wrap) = self.envelope_to(peer, cell, payload) {
                self.publish_dm(cell, &wrap);
            }
            self.read_sent.insert(id.clone());
        }
        unread.len()
    }

    /// A private envelope for the key of `cell`.
    fn on_wrap(&mut self, cell: String, wrap: Event) {
        // A group's inbox, or that of a group accepted and not yet joined.
        let group_inbox = cell
            .strip_prefix("group:")
            .is_some_and(|g| self.groups.contains_key(g) || self.awaiting.contains(g));
        if !(self.identities.contains(&cell) || group_inbox) || !self.seen.insert(&wrap.id) {
            return;
        }
        let Ok(key) = self.label_key(&cell) else {
            return;
        };
        let opened = match envelope::open(&wrap, key.signing_key()) {
            Ok(o) => o,
            Err(why) => {
                tracing::debug!(%cell, ?why, "private envelope dropped");
                return;
            }
        };
        let Some(b64) = opened.content.strip_prefix("bitchat1:") else {
            tracing::debug!(%cell, "private message without a BitChat packet, ignored");
            return;
        };
        let Some(p) = envelope::b64url_decode(b64).and_then(|b| Packet::decode(&b)) else {
            return;
        };
        if p.kind == crate::groups::TYPE_GROUP_MESSAGE {
            self.on_group_message(&cell, &opened.sender, &p.payload);
            return;
        }
        if p.kind != packet::TYPE_NOISE_ENCRYPTED || p.payload.is_empty() {
            return;
        }
        let body = &p.payload[1..];
        match p.payload[0] {
            packet::payload::GROUP_INVITE | packet::payload::GROUP_KEY_UPDATE => {
                self.on_group_state(&cell, &opened.sender, body);
            }
            crate::groups::join::REQUEST => self.on_join_request(&cell, &opened.sender, body),
            crate::groups::join::ACCEPT => self.on_join_accept(&cell, &opened.sender, body),
            packet::payload::PRIVATE_MESSAGE => {
                let Some((id, content)) = packet::decode_private_message(body) else {
                    return;
                };
                if self
                    .dms
                    .iter()
                    .any(|d| !d.own && d.id == id && d.peer == opened.sender)
                {
                    return;
                }
                // Acknowledged at once, like the app.
                let mut ack = vec![packet::payload::DELIVERED];
                ack.extend_from_slice(id.as_bytes());
                if let Ok(w) = self.envelope_to(&opened.sender, &cell, ack) {
                    self.publish_dm(&cell, &w);
                }
                self.keep_dm(DirectMessage {
                    id,
                    peer: opened.sender,
                    cell,
                    content,
                    created_at: opened.created_at.min(unix_now()),
                    own: false,
                    status: None,
                });
            }
            t @ (packet::payload::DELIVERED | packet::payload::READ_RECEIPT) => {
                let Ok(id) = String::from_utf8(body.to_vec()) else {
                    return;
                };
                let status = if t == packet::payload::DELIVERED {
                    DmStatus::Delivered
                } else {
                    DmStatus::Read
                };
                // Only from the peer the message went to.
                if self
                    .dms
                    .iter()
                    .any(|d| d.own && d.id == id && d.peer == opened.sender)
                {
                    self.set_dm_status(&id, status);
                }
            }
            other => tracing::debug!(%cell, payload = other, "private payload not handled yet"),
        }
    }

    fn local(&self) -> Result<Local> {
        Ok(Local {
            mutes: self.db.mutes()?,
            presets: self.db.presets()?,
            schedules: self.db.schedules()?,
        })
    }

    fn local_changed(&self) {
        self.emit(ConsoleEvent::Local);
    }

    /// Sends the scheduled messages that are due, in the area as it is now:
    /// a schedule whose cell is not followed is skipped, and says so.
    fn send_due(&mut self) {
        let now = unix_now();
        let due = match self.db.due(now) {
            Ok(d) => d,
            Err(e) => {
                tracing::warn!(error = %e, "schedules unreadable");
                return;
            }
        };
        if due.is_empty() {
            return;
        }
        for s in due {
            let result = match self.scheduled_event(&s) {
                Ok(event) => match self.publish_message(event) {
                    Ok(_) => "sent".to_owned(),
                    Err(e) => format!("skipped: {e}"),
                },
                Err(e) => format!("skipped: {e}"),
            };
            tracing::info!(schedule = s.id, %result, "scheduled message");
            if let Err(e) = self.db.advance(s.id, now, &result) {
                tracing::warn!(error = %e, "schedule not updated");
            }
        }
        self.local_changed();
    }

    /// The message of a schedule, built as `Console::send` builds one.
    fn scheduled_event(&self, s: &Schedule) -> Result<Event> {
        let center = self.center.clone().ok_or(Error::NoArea)?;
        let cell = s.cell.clone().unwrap_or(center);
        if !self.cells.contains(&cell) {
            return Err(Error::NotInArea(cell));
        }
        build_message(
            &self.config.seed,
            &cell,
            &s.text,
            s.nickname.as_deref(),
            self.teleport,
            self.config.pow_bits,
        )
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
            keys: self
                .identities
                .iter()
                .filter_map(|c| Some((c.clone(), self.config.seed.cell(c).ok()?.pubkey_hex())))
                .collect(),
        }
    }

    fn participant_count(&self, cell: &str, now: i64) -> usize {
        let from = now - self.config.participant_window_secs;
        self.participants
            .get(cell)
            .map_or(0, |m| m.values().filter(|s| s.last >= from).count())
    }

    fn participants_of(&self, cell: &str, now: i64) -> Vec<Participant> {
        let from = now - self.config.participant_window_secs;
        let mut v: Vec<Participant> = self
            .participants
            .get(cell)
            .into_iter()
            .flatten()
            .filter(|(_, s)| s.last >= from)
            .map(|(k, s)| Participant {
                pubkey: k.clone(),
                nickname: s.nickname.clone(),
                last_seen: s.last,
                teleport: s.teleport,
                messages: s.messages,
            })
            .collect();
        v.sort_by(|a, b| b.last_seen.cmp(&a.last_seen).then(a.pubkey.cmp(&b.pubkey)));
        v
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
                    if let Some(mid) = self.dm_wraps.get(&id).cloned() {
                        self.set_dm_status(&mid, DmStatus::Relayed);
                        return;
                    }
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
        if event.kind == envelope::KIND_WRAP {
            self.on_wrap(cell, event);
            return;
        }
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
        let teleport = event.tags.iter().any(|t| {
            t.first().map(String::as_str) == Some("t")
                && t.get(1).map(String::as_str) == Some("teleport")
        });
        let people = self.participants.entry(cell.clone()).or_default();
        let before = people.len();
        let seen = people.entry(event.pubkey.clone()).or_default();
        if at >= seen.last {
            seen.last = at;
            seen.teleport = teleport;
            if let Some(n) = event.tag_value("n").filter(|n| !n.is_empty()) {
                seen.nickname = Some(n.to_owned());
            }
        }
        if event.kind == KIND_MESSAGE {
            seen.messages += 1;
        }
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
        let t = tokio::time::Instant::now();
        self.closing.retain(|(until, _)| *until > t);
        let now = unix_now();
        let from = now - self.config.participant_window_secs;
        for people in self.participants.values_mut() {
            people.retain(|_, s| s.last >= from);
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
