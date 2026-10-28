//! The message history (lots 14 to 17, D31): every channel, private and
//! group message, received and sent, muted keys included, in
//! `history.sqlite`, a file apart from `console.sqlite` (no group keys beside
//! it), readable by the user only.
//!
//! Other processes read and change it too (the daily cron, a local AI, D33):
//! WAL, a long busy timeout, and the console's writes on a thread of their
//! own, so that it never waits for their locks. Deleted, the file comes back
//! empty within seconds. A message older than the start of the base (`since`
//! in `meta`) is not written by the console: relays sending it again after a
//! purge or a rotation cannot bring it back.
//!
//! A change of content made after writing (`content`, `deleted_at`: by the
//! page, the API, `bitchat-history` or any SQLite client) is noted by a
//! trigger in `change`; the console takes it into its feed within seconds
//! (lot 16). Rows removed outright (purge, rotation) are archive work: the
//! feed keeps them.
//!
//! [`HistoryFile`] is the file itself, for the console's thread and for the
//! `bitchat-history` tool (lot 17); [`History`] the console's handle.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::Duration;

use rusqlite::{Connection, OptionalExtension, ToSql, params};
use serde::{Deserialize, Serialize};
use tokio::sync::oneshot;

use crate::console::{DirectMessage, DmStatus, GroupChat};
use crate::error::{Error, Result};
use crate::nostr::unix_now;
use crate::store::Message;

/// File of the history, in the data directory.
pub const HISTORY_FILE: &str = "history.sqlite";

const MIGRATIONS: &[(&str, &str)] = &[
    (
        "0001_history",
        include_str!("migrations/history/0001_history.sql"),
    ),
    (
        "0002_changes",
        include_str!("migrations/history/0002_changes.sql"),
    ),
];
/// Schema version this code writes.
pub const SCHEMA_VERSION: u32 = MIGRATIONS.len() as u32;

/// How long a write waits for another process's lock (a `VACUUM` of the
/// daily cron): on the history thread, so the console does not wait.
const BUSY_TIMEOUT: Duration = Duration::from_secs(60);
/// How soon a deleted or replaced file, or a change made by another
/// process, is noticed.
const CHECK_EVERY: Duration = Duration::from_secs(2);
/// Rows a query returns at most.
pub const MAX_ROWS: usize = 5000;

/// One row of the history, as the API and `bitchat-history` show it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Record {
    /// Row number; ignored when inserting.
    #[serde(default)]
    pub id: i64,
    /// `channel`, `dm` or `group`.
    pub kind: String,
    /// channel: Nostr event ID; dm: BitChat message ID; group: message ID.
    pub msg_id: String,
    #[serde(default)]
    pub own: bool,
    /// channel: the cell; dm: the cell whose key the console uses.
    pub geohash: Option<String>,
    pub group_id: Option<String>,
    /// Hex key (Nostr; Ed25519 in groups).
    pub sender: String,
    /// dm: the other key.
    pub recipient: Option<String>,
    pub nickname: Option<String>,
    #[serde(default)]
    pub teleport: bool,
    /// None once deleted.
    pub content: Option<String>,
    pub created_at: i64,
    #[serde(default)]
    pub received_at: i64,
    pub status: Option<String>,
    pub deleted_at: Option<i64>,
}

/// What to look for; everything is optional.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct Query {
    /// `channel`, `dm` or `group`.
    pub kind: Option<String>,
    /// A cell or a prefix of cells (`u09t`: `u09t` and every cell inside).
    pub cell: Option<String>,
    pub group: Option<String>,
    /// Sender or recipient.
    pub key: Option<String>,
    /// In the content or the nickname, any case.
    pub text: Option<String>,
    /// `created_at` from, to (Unix time, inclusive).
    pub from: Option<i64>,
    pub to: Option<i64>,
    /// Deleted rows too.
    pub deleted: bool,
    /// `new` (default), `old`, `sender`.
    pub order: Option<String>,
    /// At most [`MAX_ROWS`]; 200 by default.
    pub limit: Option<usize>,
    pub offset: Option<usize>,
}

/// A message whose content changed after it was written; `content` None:
/// deleted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Changed {
    pub kind: String,
    pub msg_id: String,
    pub content: Option<String>,
}

/// What a rotation did (D33).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Rotation {
    /// The copy of the whole base, before.
    pub archive: PathBuf,
    /// Rows older than the new start, removed.
    pub removed: usize,
    /// The new start of the base.
    pub since: i64,
}

type Call = Box<dyn FnOnce(&HistoryFile) + Send>;

enum Op {
    Channel(Message),
    Dm {
        dm: DirectMessage,
        /// The console's key in the conversation's cell.
        me: String,
    },
    DmStatus(String, DmStatus),
    DmRead(Vec<String>),
    DmDeleted(Vec<String>),
    Group(GroupChat),
    /// Anything else, on the thread; `true`: it may change contents.
    Call(bool, Call),
}

/// The console's handle on the history thread; cheap to clone. The thread
/// ends with the last handle.
#[derive(Clone)]
pub struct History {
    tx: mpsc::Sender<Op>,
}

impl History {
    /// Opens (or creates) the history and starts its thread.
    pub fn open(path: &Path) -> Result<History> {
        Self::open_notifying(path, None)
    }

    /// The same, telling `changes` what changed after writing (lot 16).
    pub fn open_notifying(
        path: &Path,
        changes: Option<tokio::sync::mpsc::UnboundedSender<Vec<Changed>>>,
    ) -> Result<History> {
        let file = HistoryFile::open(path)?;
        let (tx, rx) = mpsc::channel();
        let path = path.to_owned();
        std::thread::Builder::new()
            .name("history".into())
            .spawn(move || run(path, Some(file), rx, changes))?;
        Ok(History { tx })
    }

    fn send(&self, op: Op) {
        let _ = self.tx.send(op);
    }

    async fn call<T: Send + 'static>(
        &self,
        modifies: bool,
        f: impl FnOnce(&HistoryFile) -> Result<T> + Send + 'static,
    ) -> Result<T> {
        let (reply, rx) = oneshot::channel();
        self.send(Op::Call(
            modifies,
            Box::new(move |h| {
                let _ = reply.send(f(h));
            }),
        ));
        rx.await
            .map_err(|_| Error::Db("history unavailable".into()))?
    }

    /// A channel message, received or sent.
    pub fn channel(&self, m: &Message) {
        self.send(Op::Channel(m.clone()));
    }

    /// A private message; `me`: the console's key in its cell.
    pub fn dm(&self, dm: &DirectMessage, me: &str) {
        self.send(Op::Dm {
            dm: dm.clone(),
            me: me.to_owned(),
        });
    }

    /// A sent private message moved forward.
    pub fn dm_status(&self, id: &str, status: DmStatus) {
        self.send(Op::DmStatus(id.to_owned(), status));
    }

    /// Received private messages the user read.
    pub fn dm_read(&self, ids: &[String]) {
        if !ids.is_empty() {
            self.send(Op::DmRead(ids.to_vec()));
        }
    }

    /// Private messages the user deleted: their content goes, the row stays.
    pub fn dm_deleted(&self, ids: &[String]) {
        if !ids.is_empty() {
            self.send(Op::DmDeleted(ids.to_vec()));
        }
    }

    pub fn group(&self, m: &GroupChat) {
        self.send(Op::Group(m.clone()));
    }

    /// The last `limit` channel messages of `cell` since `since`, oldest
    /// first, and the IDs of those deleted in that time.
    pub async fn recent(
        &self,
        cell: &str,
        since: i64,
        limit: usize,
    ) -> (Vec<Message>, Vec<String>) {
        let cell = cell.to_owned();
        self.call(false, move |h| h.recent(&cell, since, limit))
            .await
            .unwrap_or_default()
    }

    pub async fn query(&self, q: Query) -> Result<Vec<Record>> {
        self.call(false, move |h| h.query(&q)).await
    }

    pub async fn get(&self, id: i64) -> Result<Option<Record>> {
        self.call(false, move |h| h.get(id)).await
    }

    /// Deletes rows: their content goes, the row stays (the console hides
    /// them). Returns how many.
    pub async fn delete(&self, ids: Vec<i64>) -> Result<usize> {
        self.call(true, move |h| h.delete(&ids)).await
    }

    /// Changes the content of a row (moderation); the console shows it.
    pub async fn update(&self, id: i64, content: String) -> Result<bool> {
        self.call(true, move |h| h.update(id, &content)).await
    }

    /// Adds a row (an archive put back); `None` if already there.
    pub async fn insert(&self, r: Record) -> Result<Option<i64>> {
        self.call(true, move |h| h.insert(&r)).await
    }

    /// Waits until everything sent before is written.
    pub async fn flush(&self) {
        let _ = self.call(false, |_| Ok(())).await;
    }
}

fn run(
    path: PathBuf,
    mut file: Option<HistoryFile>,
    rx: mpsc::Receiver<Op>,
    changes: Option<tokio::sync::mpsc::UnboundedSender<Vec<Changed>>>,
) {
    // `PRAGMA data_version` when changes were last taken: it moves when
    // another connection commits.
    let mut version = None;
    loop {
        let op = match rx.recv_timeout(CHECK_EVERY) {
            Ok(op) => Some(op),
            Err(mpsc::RecvTimeoutError::Timeout) => None,
            Err(mpsc::RecvTimeoutError::Disconnected) => return,
        };
        if !file.as_ref().is_some_and(HistoryFile::still_there) {
            // Deleted (a full wipe) or replaced: opened again, empty if gone.
            file = None;
            version = None;
            match HistoryFile::open(&path) {
                Ok(f) => {
                    tracing::info!("history opened again");
                    file = Some(f);
                }
                Err(e) => tracing::warn!(error = %e, "history unusable for now"),
            }
        }
        // Without a file, a call's reply is dropped: the caller gets an error.
        let Some(f) = &file else { continue };
        let mut modified = false;
        if let Some(op) = op {
            match f.apply(op) {
                Ok(m) => modified = m,
                Err(e) => tracing::warn!(error = %e, "history: write failed"),
            }
        }
        let Some(tx) = &changes else { continue };
        let v = f.data_version().ok();
        if modified || v != version {
            version = v;
            match f.take_changes() {
                Ok(c) if !c.is_empty() => {
                    let _ = tx.send(c);
                }
                Ok(_) => {}
                Err(e) => tracing::warn!(error = %e, "history: changes unreadable"),
            }
        }
    }
}

/// The history file: opened, created or migrated as needed.
pub struct HistoryFile {
    path: PathBuf,
    conn: Connection,
    /// Device and inode of the file opened.
    file: (u64, u64),
}

const COLUMNS: &str = "id, kind, msg_id, own, geohash, group_id, sender, recipient, nickname, \
                       teleport, content, created_at, received_at, status, deleted_at";

impl HistoryFile {
    /// Opens the history at `path`; creates it (`0600`) if it is not there.
    pub fn open(path: &Path) -> Result<HistoryFile> {
        if !path.exists() {
            // Leftovers of a deleted base would be replayed into the new one.
            for p in side_files(path) {
                let _ = std::fs::remove_file(p);
            }
            create_private(path)?;
        }
        let mut conn = Connection::open(path).map_err(sql)?;
        // The WAL and shared-memory files take the mode of the database.
        set_private(path)?;
        conn.busy_timeout(BUSY_TIMEOUT).map_err(sql)?;
        conn.pragma_update(None, "journal_mode", "WAL")
            .map_err(sql)?;
        conn.pragma_update(None, "synchronous", "NORMAL")
            .map_err(sql)?;
        migrate(&mut conn)?;
        add_functions(&conn)?;
        // A new base starts now.
        conn.execute(
            "INSERT OR IGNORE INTO meta (key, value) VALUES ('since', ?1)",
            params![unix_now().to_string()],
        )
        .map_err(sql)?;
        Ok(HistoryFile {
            path: path.to_owned(),
            conn,
            file: file_id(path)?,
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    fn still_there(&self) -> bool {
        file_id(&self.path).is_ok_and(|id| id == self.file)
    }

    /// The start of the base (Unix time).
    pub fn since(&self) -> Result<i64> {
        let v: String = self
            .conn
            .query_row("SELECT value FROM meta WHERE key = 'since'", [], |r| {
                r.get(0)
            })
            .map_err(sql)?;
        v.parse().map_err(|_| Error::Db(format!("bad since {v:?}")))
    }

    fn data_version(&self) -> Result<i64> {
        self.conn
            .query_row("PRAGMA data_version", [], |r| r.get(0))
            .map_err(sql)
    }

    /// Applies one of the console's writes; true if contents may have
    /// changed (see `take_changes`).
    fn apply(&self, op: Op) -> Result<bool> {
        let now = unix_now();
        match op {
            Op::Channel(m) => self.write(&Record {
                id: 0,
                kind: "channel".into(),
                msg_id: m.id,
                own: m.own,
                geohash: Some(m.cell),
                group_id: None,
                sender: m.pubkey,
                recipient: None,
                nickname: m.nickname,
                teleport: m.teleport,
                content: Some(m.content),
                created_at: m.created_at,
                received_at: now,
                status: None,
                deleted_at: None,
            }),
            Op::Dm { dm, me } => {
                let (sender, recipient) = if dm.own { (me, dm.peer) } else { (dm.peer, me) };
                let status = match (dm.own, dm.status) {
                    (true, Some(s)) => Some(status_name(s).to_owned()),
                    (false, _) if dm.read => Some("read".to_owned()),
                    _ => None,
                };
                self.write(&Record {
                    id: 0,
                    kind: "dm".into(),
                    msg_id: dm.id,
                    own: dm.own,
                    geohash: Some(dm.cell),
                    group_id: None,
                    sender,
                    recipient: Some(recipient),
                    nickname: None,
                    teleport: false,
                    content: Some(dm.content),
                    created_at: dm.created_at,
                    received_at: now,
                    status,
                    deleted_at: None,
                })
            }
            Op::DmStatus(id, status) => {
                self.conn
                    .execute(
                        "UPDATE message SET status = ?2 WHERE kind = 'dm' AND own = 1 AND msg_id = ?1",
                        params![id, status_name(status)],
                    )
                    .map_err(sql)?;
                Ok(false)
            }
            Op::DmRead(ids) => {
                for id in ids {
                    self.conn
                        .execute(
                            "UPDATE message SET status = 'read' WHERE kind = 'dm' AND own = 0 AND msg_id = ?1",
                            params![id],
                        )
                        .map_err(sql)?;
                }
                Ok(false)
            }
            Op::DmDeleted(ids) => {
                for id in ids {
                    self.conn
                        .execute(
                            "UPDATE message SET content = NULL, deleted_at = ?2 \
                             WHERE kind = 'dm' AND msg_id = ?1 AND deleted_at IS NULL",
                            params![id, now],
                        )
                        .map_err(sql)?;
                }
                Ok(true)
            }
            Op::Group(m) => self.write(&Record {
                id: 0,
                kind: "group".into(),
                msg_id: m.id,
                own: m.own,
                geohash: None,
                group_id: Some(m.group),
                sender: m.sender,
                recipient: None,
                nickname: Some(m.nickname).filter(|n| !n.is_empty()),
                teleport: false,
                content: Some(m.content),
                created_at: m.created_at,
                received_at: now,
                status: None,
                deleted_at: None,
            }),
            Op::Call(modifies, f) => {
                f(self);
                Ok(modifies)
            }
        }
    }

    /// The console's write: once per message (a relay sending it again
    /// changes nothing), and not if older than the start of the base.
    fn write(&self, r: &Record) -> Result<bool> {
        if r.created_at < self.since()? {
            return Ok(false);
        }
        self.insert(r)?;
        Ok(false)
    }

    /// Adds a row whatever its date (an archive put back); `None` if a row
    /// for that message is already there.
    pub fn insert(&self, r: &Record) -> Result<Option<i64>> {
        if !matches!(r.kind.as_str(), "channel" | "dm" | "group") {
            return Err(Error::Db(format!("unknown kind {:?}", r.kind)));
        }
        let n = self
            .conn
            .execute(
                "INSERT OR IGNORE INTO message (kind, msg_id, own, geohash, group_id, sender, \
                 recipient, nickname, teleport, content, created_at, received_at, status, deleted_at) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
                params![
                    r.kind,
                    r.msg_id,
                    r.own,
                    r.geohash,
                    r.group_id,
                    r.sender,
                    r.recipient,
                    r.nickname,
                    r.teleport,
                    r.content,
                    r.created_at,
                    if r.received_at == 0 {
                        unix_now()
                    } else {
                        r.received_at
                    },
                    r.status,
                    r.deleted_at,
                ],
            )
            .map_err(sql)?;
        Ok((n > 0).then(|| self.conn.last_insert_rowid()))
    }

    fn recent(&self, cell: &str, since: i64, limit: usize) -> Result<(Vec<Message>, Vec<String>)> {
        let mut st = self
            .conn
            .prepare(
                "SELECT msg_id, geohash, sender, nickname, content, created_at, teleport, own \
                 FROM message WHERE kind = 'channel' AND geohash = ?1 AND created_at >= ?2 \
                 AND deleted_at IS NULL AND content IS NOT NULL \
                 ORDER BY created_at DESC, id DESC LIMIT ?3",
            )
            .map_err(sql)?;
        let rows = st
            .query_map(params![cell, since, limit as i64], |r| {
                Ok(Message {
                    id: r.get(0)?,
                    cell: r.get(1)?,
                    pubkey: r.get(2)?,
                    nickname: r.get(3)?,
                    content: r.get(4)?,
                    created_at: r.get(5)?,
                    teleport: r.get(6)?,
                    own: r.get(7)?,
                    relays: Vec::new(),
                    received: 0,
                })
            })
            .map_err(sql)?;
        let mut kept = rows
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(sql)?;
        kept.reverse();
        let mut st = self
            .conn
            .prepare(
                "SELECT msg_id FROM message WHERE kind = 'channel' AND geohash = ?1 \
                 AND created_at >= ?2 AND (deleted_at IS NOT NULL OR content IS NULL)",
            )
            .map_err(sql)?;
        let deleted = st
            .query_map(params![cell, since], |r| r.get(0))
            .map_err(sql)?
            .collect::<std::result::Result<Vec<String>, _>>()
            .map_err(sql)?;
        Ok((kept, deleted))
    }

    pub fn query(&self, q: &Query) -> Result<Vec<Record>> {
        let mut wh = Vec::<String>::new();
        let mut args = Vec::<Box<dyn ToSql>>::new();
        // The placeholder of a new argument.
        let mut arg = |v: Box<dyn ToSql>| {
            args.push(v);
            format!("?{}", args.len())
        };
        if let Some(k) = &q.kind {
            wh.push(format!("kind = {}", arg(Box::new(k.clone()))));
        }
        if let Some(c) = &q.cell {
            let p = arg(Box::new(format!("{}%", like_escape(&c.to_lowercase()))));
            wh.push(format!("geohash LIKE {p} ESCAPE '\\'"));
        }
        if let Some(g) = &q.group {
            wh.push(format!("group_id = {}", arg(Box::new(g.clone()))));
        }
        if let Some(k) = &q.key {
            let p = arg(Box::new(k.to_lowercase()));
            wh.push(format!("(sender = {p} OR recipient = {p})"));
        }
        if let Some(t) = q.text.as_deref().filter(|t| !t.is_empty()) {
            // Any case, beyond ASCII (`LIKE` folds ASCII only).
            let p = arg(Box::new(t.to_lowercase()));
            wh.push(format!(
                "(contains_folded(content, {p}) OR contains_folded(nickname, {p}))"
            ));
        }
        if let Some(f) = q.from {
            wh.push(format!("created_at >= {}", arg(Box::new(f))));
        }
        if let Some(t) = q.to {
            wh.push(format!("created_at <= {}", arg(Box::new(t))));
        }
        if !q.deleted {
            wh.push("deleted_at IS NULL".into());
        }
        let order = match q.order.as_deref() {
            Some("old") => "created_at, id",
            Some("sender") => "sender, created_at DESC, id DESC",
            _ => "created_at DESC, id DESC",
        };
        let limit = q.limit.unwrap_or(200).min(MAX_ROWS);
        let sql_text = format!(
            "SELECT {COLUMNS} FROM message {} ORDER BY {order} LIMIT {limit} OFFSET {}",
            if wh.is_empty() {
                String::new()
            } else {
                format!("WHERE {}", wh.join(" AND "))
            },
            q.offset.unwrap_or(0)
        );
        let mut st = self.conn.prepare(&sql_text).map_err(sql)?;
        let refs: Vec<&dyn ToSql> = args.iter().map(|b| b.as_ref()).collect();
        let rows = st.query_map(refs.as_slice(), record).map_err(sql)?;
        rows.collect::<std::result::Result<_, _>>().map_err(sql)
    }

    pub fn get(&self, id: i64) -> Result<Option<Record>> {
        self.conn
            .query_row(
                &format!("SELECT {COLUMNS} FROM message WHERE id = ?1"),
                params![id],
                record,
            )
            .optional()
            .map_err(sql)
    }

    /// Deletes rows: content removed, row kept with `deleted_at`, so that
    /// the console hides them and relays cannot bring them back.
    pub fn delete(&self, ids: &[i64]) -> Result<usize> {
        let now = unix_now();
        let mut n = 0;
        for id in ids {
            n += self
                .conn
                .execute(
                    "UPDATE message SET content = NULL, deleted_at = ?2 \
                     WHERE id = ?1 AND deleted_at IS NULL",
                    params![id, now],
                )
                .map_err(sql)?;
        }
        Ok(n)
    }

    /// Changes the content of a row (moderation); false if no such row, or
    /// deleted.
    pub fn update(&self, id: i64, content: &str) -> Result<bool> {
        Ok(self
            .conn
            .execute(
                "UPDATE message SET content = ?2 WHERE id = ?1 AND deleted_at IS NULL",
                params![id, content],
            )
            .map_err(sql)?
            > 0)
    }

    /// Removes rows outright (archive work: the console's feed keeps them).
    pub fn remove(&self, ids: &[i64]) -> Result<usize> {
        let mut n = 0;
        for id in ids {
            n += self
                .conn
                .execute("DELETE FROM message WHERE id = ?1", params![id])
                .map_err(sql)?;
        }
        Ok(n)
    }

    /// Removes the rows older than `before` (Unix time) outright.
    pub fn purge(&self, before: i64) -> Result<usize> {
        self.conn
            .execute("DELETE FROM message WHERE created_at < ?1", params![before])
            .map_err(sql)
    }

    /// A consistent copy of the whole base in `to` (`VACUUM INTO`, while the
    /// console writes), readable by the user only.
    pub fn backup(&self, to: &Path) -> Result<()> {
        if to.exists() {
            return Err(Error::Db(format!("{} already exists", to.display())));
        }
        if let Some(dir) = to.parent().filter(|d| !d.as_os_str().is_empty()) {
            create_private_dir(dir)?;
        }
        self.conn
            .execute("VACUUM INTO ?1", params![to.to_string_lossy()])
            .map_err(sql)?;
        set_private(to)
    }

    /// The daily rotation (D33): the whole base copied into `archive_dir`,
    /// then only the last `keep_secs` kept, the start of the base moved
    /// there (never back: relays cannot bring older messages back), and the
    /// file compacted.
    pub fn rotate(&self, keep_secs: i64, archive_dir: &Path) -> Result<Rotation> {
        let now = unix_now();
        let archive = archive_path(archive_dir, now);
        self.backup(&archive)?;
        let cut = now - keep_secs.max(0);
        // The start never goes back: what a wipe or a purge removed stays out.
        let since = cut.max(self.since()?);
        let tx = self.conn.unchecked_transaction().map_err(sql)?;
        let removed = tx
            .execute("DELETE FROM message WHERE created_at < ?1", params![cut])
            .map_err(sql)?;
        tx.execute(
            "UPDATE meta SET value = ?1 WHERE key = 'since'",
            params![since.to_string()],
        )
        .map_err(sql)?;
        tx.execute("DELETE FROM change", []).map_err(sql)?;
        tx.commit().map_err(sql)?;
        self.conn.execute_batch("VACUUM").map_err(sql)?;
        Ok(Rotation {
            archive,
            removed,
            since,
        })
    }

    /// The changes noted since the last call (and forgets them): the
    /// current content, None if deleted.
    fn take_changes(&self) -> Result<Vec<Changed>> {
        let mut st = self
            .conn
            .prepare(
                "SELECT c.id, c.kind, c.msg_id, m.content, m.deleted_at FROM change c \
                 LEFT JOIN message m ON m.kind = c.kind AND m.msg_id = c.msg_id ORDER BY c.id",
            )
            .map_err(sql)?;
        let rows = st
            .query_map([], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, Option<String>>(3)?,
                    r.get::<_, Option<i64>>(4)?,
                ))
            })
            .map_err(sql)?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(sql)?;
        let Some(last) = rows.last().map(|r| r.0) else {
            return Ok(Vec::new());
        };
        // The last state of each message, in order.
        let mut by: BTreeMap<(String, String), (i64, Option<String>)> = BTreeMap::new();
        for (id, kind, msg_id, content, deleted) in rows {
            let content = if deleted.is_some() { None } else { content };
            by.insert((kind, msg_id), (id, content));
        }
        self.conn
            .execute("DELETE FROM change WHERE id <= ?1", params![last])
            .map_err(sql)?;
        let mut v: Vec<_> = by.into_iter().collect();
        v.sort_by_key(|(_, (id, _))| *id);
        Ok(v.into_iter()
            .map(|((kind, msg_id), (_, content))| Changed {
                kind,
                msg_id,
                content,
            })
            .collect())
    }
}

/// A free name for a copy made at `t` in `dir`:
/// `history-20261028-214700.sqlite`, then `-2`, `-3`… within the same second.
pub fn archive_path(dir: &Path, t: i64) -> PathBuf {
    let stamp = utc_stamp(t);
    let mut p = dir.join(format!("history-{stamp}.sqlite"));
    let mut n = 1;
    while p.exists() {
        n += 1;
        p = dir.join(format!("history-{stamp}-{n}.sqlite"));
    }
    p
}

/// Deletes the history (the console makes a new one within seconds).
pub fn wipe(path: &Path) -> Result<()> {
    std::fs::remove_file(path)?;
    for p in side_files(path) {
        let _ = std::fs::remove_file(p);
    }
    Ok(())
}

fn side_files(path: &Path) -> [PathBuf; 2] {
    ["-wal", "-shm"].map(|ext| {
        let mut p = path.as_os_str().to_owned();
        p.push(ext);
        PathBuf::from(p)
    })
}

fn record(r: &rusqlite::Row) -> rusqlite::Result<Record> {
    Ok(Record {
        id: r.get(0)?,
        kind: r.get(1)?,
        msg_id: r.get(2)?,
        own: r.get(3)?,
        geohash: r.get(4)?,
        group_id: r.get(5)?,
        sender: r.get(6)?,
        recipient: r.get(7)?,
        nickname: r.get(8)?,
        teleport: r.get(9)?,
        content: r.get(10)?,
        created_at: r.get(11)?,
        received_at: r.get(12)?,
        status: r.get(13)?,
        deleted_at: r.get(14)?,
    })
}

/// `contains_folded(text, needle)`: `needle` (already lower case) in `text`,
/// any case.
fn add_functions(conn: &Connection) -> Result<()> {
    use rusqlite::functions::FunctionFlags;
    conn.create_scalar_function(
        "contains_folded",
        2,
        FunctionFlags::SQLITE_UTF8 | FunctionFlags::SQLITE_DETERMINISTIC,
        |ctx| {
            let text: Option<String> = ctx.get(0)?;
            let needle: String = ctx.get(1)?;
            Ok(text.is_some_and(|t| t.to_lowercase().contains(&needle)))
        },
    )
    .map_err(sql)
}

fn like_escape(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
}

/// `20261028-214700` (UTC).
pub fn utc_stamp(t: i64) -> String {
    let (days, secs) = (t.div_euclid(86_400), t.rem_euclid(86_400));
    // Civil date from days since 1970-01-01 (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!(
        "{y:04}{m:02}{d:02}-{:02}{:02}{:02}",
        secs / 3600,
        secs / 60 % 60,
        secs % 60
    )
}

fn status_name(s: DmStatus) -> &'static str {
    match s {
        DmStatus::Sent => "sent",
        DmStatus::Relayed => "relayed",
        DmStatus::Delivered => "delivered",
        DmStatus::Read => "read",
    }
}

fn migrate(conn: &mut Connection) -> Result<()> {
    let version: u32 = conn
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .map_err(sql)?;
    if version > SCHEMA_VERSION {
        return Err(Error::Db(format!(
            "history schema version {version} is newer than this code ({SCHEMA_VERSION})"
        )));
    }
    for (i, (name, sql_text)) in MIGRATIONS.iter().enumerate().skip(version as usize) {
        let tx = conn.transaction().map_err(sql)?;
        tx.execute_batch(sql_text).map_err(sql)?;
        tx.pragma_update(None, "user_version", i as u32 + 1)
            .map_err(sql)?;
        tx.commit().map_err(sql)?;
        tracing::info!(migration = name, "history schema migrated");
    }
    Ok(())
}

fn create_private(path: &Path) -> Result<()> {
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        create_private_dir(dir)?;
    }
    let mut o = std::fs::OpenOptions::new();
    o.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        o.mode(0o600);
    }
    match o.open(path) {
        Ok(_) => Ok(()),
        // Created by another process in between.
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Ok(()),
        Err(e) => Err(e.into()),
    }
}

fn create_private_dir(dir: &Path) -> Result<()> {
    if dir.exists() {
        return Ok(());
    }
    std::fs::create_dir_all(dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

fn set_private(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    }
    let _ = path;
    Ok(())
}

fn file_id(path: &Path) -> Result<(u64, u64)> {
    let m = std::fs::metadata(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        Ok((m.dev(), m.ino()))
    }
    #[cfg(not(unix))]
    {
        let _ = m;
        Ok((0, 0))
    }
}

fn sql(e: rusqlite::Error) -> Error {
    Error::Db(e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utc_stamps() {
        assert_eq!(utc_stamp(0), "19700101-000000");
        assert_eq!(utc_stamp(1_793_220_448), "20261028-204728");
        assert_eq!(utc_stamp(951_782_400), "20000229-000000");
    }

    #[test]
    fn query_filters() {
        let dir = std::env::temp_dir().join(format!("bitchat-history-q-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let f = HistoryFile::open(&dir.join(HISTORY_FILE)).unwrap();
        let row = |id: &str, cell: &str, who: &str, text: &str, at: i64| Record {
            id: 0,
            kind: "channel".into(),
            msg_id: id.into(),
            own: false,
            geohash: Some(cell.into()),
            group_id: None,
            sender: who.into(),
            recipient: None,
            nickname: Some("Zoé".into()),
            teleport: false,
            content: Some(text.into()),
            created_at: at,
            received_at: at,
            status: None,
            deleted_at: None,
        };
        let a = f
            .insert(&row("a", "u09tv", "k1", "Bonjour 50%", 100))
            .unwrap()
            .unwrap();
        f.insert(&row("b", "u09ty", "k2", "salut", 200)).unwrap();
        f.insert(&row("c", "u0bcd", "k1", "ailleurs", 300)).unwrap();
        assert_eq!(
            f.insert(&row("a", "u09tv", "k1", "x", 1)).unwrap(),
            None,
            "once"
        );
        let ids = |q: Query| -> Vec<String> {
            f.query(&q).unwrap().into_iter().map(|r| r.msg_id).collect()
        };
        assert_eq!(ids(Query::default()), ["c", "b", "a"]);
        let cell = |c: &str| Query {
            cell: Some(c.into()),
            order: Some("old".into()),
            ..Default::default()
        };
        assert_eq!(ids(cell("u09t")), ["a", "b"]);
        assert_eq!(
            ids(Query {
                text: Some("bonjour".into()),
                ..Default::default()
            }),
            ["a"]
        );
        assert_eq!(
            ids(Query {
                text: Some("%".into()),
                ..Default::default()
            }),
            ["a"],
            "escaped"
        );
        assert_eq!(
            ids(Query {
                text: Some("ZOÉ".into()),
                ..Default::default()
            })
            .len(),
            3,
            "any case, beyond ASCII"
        );
        assert_eq!(
            ids(Query {
                text: Some("zoé".into()),
                ..Default::default()
            })
            .len(),
            3,
            "nickname"
        );
        assert_eq!(
            ids(Query {
                key: Some("K1".into()),
                ..Default::default()
            }),
            ["c", "a"]
        );
        assert_eq!(
            ids(Query {
                from: Some(150),
                to: Some(250),
                ..Default::default()
            }),
            ["b"]
        );
        assert_eq!(
            ids(Query {
                limit: Some(1),
                offset: Some(1),
                ..Default::default()
            }),
            ["b"]
        );

        // Deleted: out unless asked; noted as a change, with no content.
        assert_eq!(f.delete(&[a]).unwrap(), 1);
        assert_eq!(f.delete(&[a]).unwrap(), 0);
        assert_eq!(ids(Query::default()), ["c", "b"]);
        let all = Query {
            deleted: true,
            ..Default::default()
        };
        assert_eq!(ids(all).len(), 3);
        let b = f
            .query(&Query {
                text: Some("salut".into()),
                ..Default::default()
            })
            .unwrap()[0]
            .id;
        assert!(f.update(b, "[modéré]").unwrap());
        assert_eq!(
            f.take_changes().unwrap(),
            [
                Changed {
                    kind: "channel".into(),
                    msg_id: "a".into(),
                    content: None
                },
                Changed {
                    kind: "channel".into(),
                    msg_id: "b".into(),
                    content: Some("[modéré]".into())
                },
            ]
        );
        assert!(f.take_changes().unwrap().is_empty(), "taken once");
        std::fs::remove_dir_all(dir).unwrap();
    }
}
