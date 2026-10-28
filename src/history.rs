//! The message history (lot 14, D31): every channel, private and group
//! message, received and sent, muted keys included, in `history.sqlite`, a
//! file apart from `console.sqlite` (no group keys beside it), readable by
//! the user only.
//!
//! Other processes read and change it too (the daily cron, a local AI, D33):
//! WAL, a long busy timeout, and writes on a thread of their own, so that the
//! console never waits for their locks. Deleted, the file comes back empty
//! within seconds. A message older than the start of the base (`since` in
//! `meta`) is not written: relays sending it again after a purge or a
//! rotation cannot bring it back.

use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::Duration;

use rusqlite::{Connection, params};
use tokio::sync::oneshot;

use crate::console::{DirectMessage, DmStatus, GroupChat};
use crate::error::{Error, Result};
use crate::nostr::unix_now;
use crate::store::Message;

/// File of the history, in the data directory.
pub const HISTORY_FILE: &str = "history.sqlite";

const MIGRATIONS: &[(&str, &str)] = &[(
    "0001_history",
    include_str!("migrations/history/0001_history.sql"),
)];
/// Schema version this code writes.
pub const SCHEMA_VERSION: u32 = MIGRATIONS.len() as u32;

/// How long a write waits for another process's lock (a `VACUUM` of the
/// daily cron): on the history thread, so nothing else waits.
const BUSY_TIMEOUT: Duration = Duration::from_secs(60);
/// How soon a deleted or replaced file is noticed without any write.
const CHECK_EVERY: Duration = Duration::from_secs(2);

enum Op {
    Channel(Message),
    Dm {
        dm: DirectMessage,
        /// The console's key in the conversation's cell.
        me: String,
    },
    DmStatus(String, DmStatus),
    DmRead(Vec<String>),
    Group(GroupChat),
    Recent {
        cell: String,
        since: i64,
        limit: usize,
        reply: oneshot::Sender<Vec<Message>>,
    },
    Flush(oneshot::Sender<()>),
}

/// Handle on the history thread; cheap to clone. The thread ends with the
/// last handle.
#[derive(Clone)]
pub struct History {
    tx: mpsc::Sender<Op>,
}

impl History {
    /// Opens (or creates) the history and starts its thread.
    pub fn open(path: &Path) -> Result<History> {
        let base = Base::open(path)?;
        let (tx, rx) = mpsc::channel();
        let path = path.to_owned();
        std::thread::Builder::new()
            .name("history".into())
            .spawn(move || run(path, Some(base), rx))?;
        Ok(History { tx })
    }

    fn send(&self, op: Op) {
        let _ = self.tx.send(op);
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

    pub fn group(&self, m: &GroupChat) {
        self.send(Op::Group(m.clone()));
    }

    /// The last `limit` channel messages of `cell` since `since`, oldest
    /// first, deleted ones left out.
    pub async fn recent(&self, cell: &str, since: i64, limit: usize) -> Vec<Message> {
        let (reply, rx) = oneshot::channel();
        self.send(Op::Recent {
            cell: cell.to_owned(),
            since,
            limit,
            reply,
        });
        rx.await.unwrap_or_default()
    }

    /// Waits until everything sent before is written.
    pub async fn flush(&self) {
        let (reply, rx) = oneshot::channel();
        self.send(Op::Flush(reply));
        let _ = rx.await;
    }
}

fn run(path: PathBuf, mut base: Option<Base>, rx: mpsc::Receiver<Op>) {
    loop {
        let op = match rx.recv_timeout(CHECK_EVERY) {
            Ok(op) => Some(op),
            Err(mpsc::RecvTimeoutError::Timeout) => None,
            Err(mpsc::RecvTimeoutError::Disconnected) => return,
        };
        if !base.as_ref().is_some_and(Base::still_there) {
            // Deleted (a full wipe) or replaced: opened again, empty if gone.
            base = None;
            match Base::open(&path) {
                Ok(b) => {
                    tracing::info!("history opened again");
                    base = Some(b);
                }
                Err(e) => tracing::warn!(error = %e, "history unusable for now"),
            }
        }
        let Some(op) = op else { continue };
        let Some(b) = &base else {
            // Replies are dropped: the callers get nothing.
            continue;
        };
        if let Err(e) = b.apply(op) {
            tracing::warn!(error = %e, "history: write failed");
        }
    }
}

struct Base {
    path: PathBuf,
    conn: Connection,
    /// Device and inode of the file opened.
    file: (u64, u64),
}

impl Base {
    fn open(path: &Path) -> Result<Base> {
        if !path.exists() {
            // Leftovers of a deleted base would be replayed into the new one.
            for ext in ["-wal", "-shm"] {
                let mut p = path.as_os_str().to_owned();
                p.push(ext);
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
        // A new base starts now.
        conn.execute(
            "INSERT OR IGNORE INTO meta (key, value) VALUES ('since', ?1)",
            params![unix_now().to_string()],
        )
        .map_err(sql)?;
        Ok(Base {
            path: path.to_owned(),
            conn,
            file: file_id(path)?,
        })
    }

    fn still_there(&self) -> bool {
        file_id(&self.path).is_ok_and(|id| id == self.file)
    }

    fn apply(&self, op: Op) -> Result<()> {
        let now = unix_now();
        match op {
            Op::Channel(m) => self.insert(&Row {
                kind: "channel",
                msg_id: &m.id,
                own: m.own,
                geohash: Some(&m.cell),
                group_id: None,
                sender: &m.pubkey,
                recipient: None,
                nickname: m.nickname.as_deref(),
                teleport: m.teleport,
                content: &m.content,
                created_at: m.created_at,
                received_at: now,
                status: None,
            }),
            Op::Dm { dm, me } => {
                let (sender, recipient) = if dm.own {
                    (me.as_str(), dm.peer.as_str())
                } else {
                    (dm.peer.as_str(), me.as_str())
                };
                let status = match (dm.own, dm.status) {
                    (true, Some(s)) => Some(status_name(s)),
                    (false, _) if dm.read => Some("read"),
                    _ => None,
                };
                self.insert(&Row {
                    kind: "dm",
                    msg_id: &dm.id,
                    own: dm.own,
                    geohash: Some(&dm.cell),
                    group_id: None,
                    sender,
                    recipient: Some(recipient),
                    nickname: None,
                    teleport: false,
                    content: &dm.content,
                    created_at: dm.created_at,
                    received_at: now,
                    status,
                })
            }
            Op::DmStatus(id, status) => {
                self.conn
                    .execute(
                        "UPDATE message SET status = ?2 WHERE kind = 'dm' AND own = 1 AND msg_id = ?1",
                        params![id, status_name(status)],
                    )
                    .map_err(sql)?;
                Ok(())
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
                Ok(())
            }
            Op::Group(m) => self.insert(&Row {
                kind: "group",
                msg_id: &m.id,
                own: m.own,
                geohash: None,
                group_id: Some(&m.group),
                sender: &m.sender,
                recipient: None,
                nickname: Some(&m.nickname)
                    .filter(|n| !n.is_empty())
                    .map(|n| n.as_str()),
                teleport: false,
                content: &m.content,
                created_at: m.created_at,
                received_at: now,
                status: None,
            }),
            Op::Recent {
                cell,
                since,
                limit,
                reply,
            } => {
                let _ = reply.send(self.recent(&cell, since, limit)?);
                Ok(())
            }
            Op::Flush(reply) => {
                let _ = reply.send(());
                Ok(())
            }
        }
    }

    /// Once per message (a relay sending it again changes nothing), and not
    /// if older than the start of the base.
    fn insert(&self, r: &Row) -> Result<()> {
        self.conn
            .execute(
                "INSERT OR IGNORE INTO message (kind, msg_id, own, geohash, group_id, sender, \
                 recipient, nickname, teleport, content, created_at, received_at, status) \
                 SELECT ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13 \
                 WHERE ?11 >= (SELECT COALESCE(MAX(CAST(value AS INTEGER)), 0) FROM meta WHERE key = 'since')",
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
                    r.received_at,
                    r.status,
                ],
            )
            .map_err(sql)?;
        Ok(())
    }

    fn recent(&self, cell: &str, since: i64, limit: usize) -> Result<Vec<Message>> {
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
        let mut v = rows
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(sql)?;
        v.reverse();
        Ok(v)
    }
}

struct Row<'a> {
    kind: &'static str,
    msg_id: &'a str,
    own: bool,
    geohash: Option<&'a str>,
    group_id: Option<&'a str>,
    sender: &'a str,
    recipient: Option<&'a str>,
    nickname: Option<&'a str>,
    teleport: bool,
    content: &'a str,
    created_at: i64,
    received_at: i64,
    status: Option<&'static str>,
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
