//! Local storage (lot 8, D16): SQLite in the data directory, for what the
//! user decides only: muted keys, saved messages, schedules, groups, the IDs
//! of the private messages read (D29) or deleted, the user's own nickname and the cells
//! where it is not sent (D32). Never received contents, nicknames or the keys
//! of others, mutes aside (D11, D25). Append-only numbered migrations, as in
//! bitchat-geohash.

use std::path::Path;

use rusqlite::{Connection, OptionalExtension, params};
use serde::Serialize;

use crate::error::{Error, Result};

const MIGRATIONS: &[(&str, &str)] = &[
    ("0001_init", include_str!("migrations/0001_init.sql")),
    ("0002_groups", include_str!("migrations/0002_groups.sql")),
    ("0003_read", include_str!("migrations/0003_read.sql")),
    (
        "0004_nickname",
        include_str!("migrations/0004_nickname.sql"),
    ),
    (
        "0005_dm_deleted",
        include_str!("migrations/0005_dm_deleted.sql"),
    ),
];

/// How long a read private message is remembered (relays rarely keep
/// envelopes longer).
pub const READ_KEPT_SECS: i64 = 30 * 24 * 3600;
/// Schema version this code writes.
pub const SCHEMA_VERSION: u32 = MIGRATIONS.len() as u32;
/// File of the database, in the data directory.
pub const DB_FILE: &str = "console.sqlite";

/// A saved message, sent on demand.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Preset {
    pub id: i64,
    pub name: String,
    pub text: String,
    pub cell: Option<String>,
    pub nickname: Option<String>,
}

/// A message to send at a given time, once or repeatedly.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Schedule {
    pub id: i64,
    pub text: String,
    pub cell: Option<String>,
    pub nickname: Option<String>,
    pub next_at: Option<i64>,
    pub repeat_secs: Option<i64>,
    pub last_result: Option<String>,
}

pub struct Db {
    conn: Connection,
}

impl Db {
    /// Opens (or creates) the database; the file is made readable by the
    /// user only, like the keys.
    pub fn open(path: &Path) -> Result<Db> {
        let db = Self::init(Connection::open(path).map_err(sql)?)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
        }
        Ok(db)
    }

    pub fn open_in_memory() -> Result<Db> {
        Self::init(Connection::open_in_memory().map_err(sql)?)
    }

    fn init(mut conn: Connection) -> Result<Db> {
        let version: u32 = conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .map_err(sql)?;
        if version > SCHEMA_VERSION {
            return Err(Error::Db(format!(
                "database schema version {version} is newer than this code ({SCHEMA_VERSION})"
            )));
        }
        for (i, (name, sql_text)) in MIGRATIONS.iter().enumerate().skip(version as usize) {
            let tx = conn.transaction().map_err(sql)?;
            tx.execute_batch(sql_text).map_err(sql)?;
            tx.pragma_update(None, "user_version", i as u32 + 1)
                .map_err(sql)?;
            tx.commit().map_err(sql)?;
            tracing::info!(migration = name, "schema migrated");
        }
        Ok(Db { conn })
    }

    pub fn mutes(&self) -> Result<Vec<String>> {
        let mut st = self
            .conn
            .prepare("SELECT pubkey FROM mute ORDER BY muted_at")
            .map_err(sql)?;
        let rows = st.query_map([], |r| r.get(0)).map_err(sql)?;
        rows.collect::<std::result::Result<_, _>>().map_err(sql)
    }

    pub fn set_muted(&self, pubkey: &str, muted: bool, at: i64) -> Result<()> {
        if muted {
            self.conn
                .execute(
                    "INSERT OR IGNORE INTO mute (pubkey, muted_at) VALUES (?1, ?2)",
                    params![pubkey, at],
                )
                .map_err(sql)?;
        } else {
            self.conn
                .execute("DELETE FROM mute WHERE pubkey = ?1", params![pubkey])
                .map_err(sql)?;
        }
        Ok(())
    }

    pub fn presets(&self) -> Result<Vec<Preset>> {
        let mut st = self
            .conn
            .prepare("SELECT id, name, text, cell, nickname FROM preset ORDER BY name, id")
            .map_err(sql)?;
        let rows = st
            .query_map([], |r| {
                Ok(Preset {
                    id: r.get(0)?,
                    name: r.get(1)?,
                    text: r.get(2)?,
                    cell: r.get(3)?,
                    nickname: r.get(4)?,
                })
            })
            .map_err(sql)?;
        rows.collect::<std::result::Result<_, _>>().map_err(sql)
    }

    pub fn preset(&self, id: i64) -> Result<Option<Preset>> {
        Ok(self.presets()?.into_iter().find(|p| p.id == id))
    }

    pub fn add_preset(
        &self,
        name: &str,
        text: &str,
        cell: Option<&str>,
        nickname: Option<&str>,
    ) -> Result<i64> {
        self.conn
            .execute(
                "INSERT INTO preset (name, text, cell, nickname) VALUES (?1, ?2, ?3, ?4)",
                params![name, text, cell, nickname],
            )
            .map_err(sql)?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn delete_preset(&self, id: i64) -> Result<bool> {
        Ok(self
            .conn
            .execute("DELETE FROM preset WHERE id = ?1", params![id])
            .map_err(sql)?
            > 0)
    }

    pub fn schedules(&self) -> Result<Vec<Schedule>> {
        let mut st = self
            .conn
            .prepare(
                "SELECT id, text, cell, nickname, next_at, repeat_secs, last_result FROM schedule \
                 ORDER BY next_at IS NULL, next_at, id",
            )
            .map_err(sql)?;
        let rows = st
            .query_map([], |r| {
                Ok(Schedule {
                    id: r.get(0)?,
                    text: r.get(1)?,
                    cell: r.get(2)?,
                    nickname: r.get(3)?,
                    next_at: r.get(4)?,
                    repeat_secs: r.get(5)?,
                    last_result: r.get(6)?,
                })
            })
            .map_err(sql)?;
        rows.collect::<std::result::Result<_, _>>().map_err(sql)
    }

    pub fn add_schedule(
        &self,
        text: &str,
        cell: Option<&str>,
        nickname: Option<&str>,
        at: i64,
        repeat_secs: Option<i64>,
    ) -> Result<i64> {
        self.conn
            .execute(
                "INSERT INTO schedule (text, cell, nickname, next_at, repeat_secs) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![text, cell, nickname, at, repeat_secs],
            )
            .map_err(sql)?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn delete_schedule(&self, id: i64) -> Result<bool> {
        Ok(self
            .conn
            .execute("DELETE FROM schedule WHERE id = ?1", params![id])
            .map_err(sql)?
            > 0)
    }

    /// Schedules due at `now`.
    pub fn due(&self, now: i64) -> Result<Vec<Schedule>> {
        Ok(self
            .schedules()?
            .into_iter()
            .filter(|s| s.next_at.is_some_and(|t| t <= now))
            .collect())
    }

    /// After a send (or a skip): the next time for a repeating schedule,
    /// the first one still in the future (missed ones are not made up),
    /// none for a one-shot one.
    pub fn advance(&self, id: i64, now: i64, result: &str) -> Result<()> {
        let s: Option<(Option<i64>, Option<i64>)> = self
            .conn
            .query_row(
                "SELECT next_at, repeat_secs FROM schedule WHERE id = ?1",
                params![id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
            .map_err(sql)?;
        let Some((next, repeat)) = s else {
            return Ok(());
        };
        let next = match (next, repeat) {
            (Some(mut t), Some(every)) if every > 0 => {
                while t <= now {
                    t += every;
                }
                Some(t)
            }
            _ => None,
        };
        self.conn
            .execute(
                "UPDATE schedule SET next_at = ?1, last_result = ?2 WHERE id = ?3",
                params![next, result, id],
            )
            .map_err(sql)?;
        Ok(())
    }
}

impl Db {
    /// Groups: (encoded state, pinned creator address).
    pub fn groups(&self) -> Result<Vec<(Vec<u8>, String)>> {
        let mut st = self
            .conn
            .prepare("SELECT state, creator_addr FROM grp")
            .map_err(sql)?;
        let rows = st
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .map_err(sql)?;
        rows.collect::<std::result::Result<_, _>>().map_err(sql)
    }

    pub fn save_group(&self, group_id: &str, state: &[u8], creator_addr: &str) -> Result<()> {
        self.conn
            .execute(
                "INSERT INTO grp (group_id, state, creator_addr) VALUES (?1, ?2, ?3) \
                 ON CONFLICT(group_id) DO UPDATE SET state = excluded.state",
                params![group_id, state, creator_addr],
            )
            .map_err(sql)?;
        Ok(())
    }

    pub fn delete_group(&self, group_id: &str) -> Result<()> {
        self.conn
            .execute("DELETE FROM grp WHERE group_id = ?1", params![group_id])
            .map_err(sql)?;
        Ok(())
    }
}

impl Db {
    /// Remembers private messages as read (D29).
    pub fn set_read(&self, ids: &[String], at: i64) -> Result<()> {
        for id in ids {
            self.conn
                .execute(
                    "INSERT OR IGNORE INTO dm_read (id, at) VALUES (?1, ?2)",
                    params![id, at],
                )
                .map_err(sql)?;
        }
        Ok(())
    }

    pub fn is_read(&self, id: &str) -> Result<bool> {
        self.conn
            .query_row("SELECT 1 FROM dm_read WHERE id = ?1", params![id], |_| {
                Ok(())
            })
            .optional()
            .map(|r| r.is_some())
            .map_err(sql)
    }

    /// Forgets what was read before `before`, and what was deleted.
    pub fn purge_read(&self, before: i64) -> Result<usize> {
        let n = self
            .conn
            .execute("DELETE FROM dm_read WHERE at < ?1", params![before])
            .map_err(sql)?;
        self.conn
            .execute("DELETE FROM dm_deleted WHERE at < ?1", params![before])
            .map_err(sql)?;
        Ok(n)
    }

    /// Remembers private messages the user deleted (lot 15).
    pub fn set_deleted(&self, ids: &[String], at: i64) -> Result<()> {
        for id in ids {
            self.conn
                .execute(
                    "INSERT OR IGNORE INTO dm_deleted (id, at) VALUES (?1, ?2)",
                    params![id, at],
                )
                .map_err(sql)?;
        }
        Ok(())
    }

    pub fn is_deleted(&self, id: &str) -> Result<bool> {
        self.conn
            .query_row(
                "SELECT 1 FROM dm_deleted WHERE id = ?1",
                params![id],
                |_| Ok(()),
            )
            .optional()
            .map(|r| r.is_some())
            .map_err(sql)
    }
}

impl Db {
    /// The user's nickname (D32), if set.
    pub fn nickname(&self) -> Result<Option<String>> {
        self.conn
            .query_row(
                "SELECT value FROM setting WHERE key = 'nickname'",
                [],
                |r| r.get(0),
            )
            .optional()
            .map_err(sql)
    }

    /// Sets the nickname; empty or `None` removes it.
    pub fn set_nickname(&self, nickname: Option<&str>) -> Result<()> {
        match nickname.map(str::trim).filter(|n| !n.is_empty()) {
            Some(n) => self.conn.execute(
                "INSERT INTO setting (key, value) VALUES ('nickname', ?1) \
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                params![n],
            ),
            None => self
                .conn
                .execute("DELETE FROM setting WHERE key = 'nickname'", []),
        }
        .map_err(sql)?;
        Ok(())
    }

    /// Cells where the nickname is not sent.
    pub fn anon_cells(&self) -> Result<Vec<String>> {
        let mut st = self
            .conn
            .prepare("SELECT geohash FROM anon_cell ORDER BY geohash")
            .map_err(sql)?;
        let rows = st.query_map([], |r| r.get(0)).map_err(sql)?;
        rows.collect::<std::result::Result<_, _>>().map_err(sql)
    }

    pub fn is_anon(&self, cell: &str) -> Result<bool> {
        self.conn
            .query_row(
                "SELECT 1 FROM anon_cell WHERE geohash = ?1",
                params![cell],
                |_| Ok(()),
            )
            .optional()
            .map(|r| r.is_some())
            .map_err(sql)
    }

    pub fn set_anon(&self, cell: &str, anon: bool, at: i64) -> Result<()> {
        if anon {
            self.conn.execute(
                "INSERT OR IGNORE INTO anon_cell (geohash, at) VALUES (?1, ?2)",
                params![cell, at],
            )
        } else {
            self.conn
                .execute("DELETE FROM anon_cell WHERE geohash = ?1", params![cell])
        }
        .map_err(sql)?;
        Ok(())
    }
}

fn sql(e: rusqlite::Error) -> Error {
    Error::Db(e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_private_messages() {
        let db = Db::open_in_memory().unwrap();
        assert!(!db.is_read("a").unwrap());
        db.set_read(&["a".into(), "b".into()], 100).unwrap();
        db.set_read(&["a".into()], 500).unwrap();
        assert!(db.is_read("a").unwrap() && db.is_read("b").unwrap());
        // Kept from the first time.
        assert_eq!(db.purge_read(101).unwrap(), 2);
        assert!(!db.is_read("a").unwrap());
        db.set_deleted(&["c".into()], 100).unwrap();
        assert!(db.is_deleted("c").unwrap() && !db.is_deleted("a").unwrap());
        db.purge_read(101).unwrap();
        assert!(!db.is_deleted("c").unwrap());
    }

    #[test]
    fn mutes_presets_schedules() {
        let db = Db::open_in_memory().unwrap();
        db.set_muted("aa", true, 1).unwrap();
        db.set_muted("aa", true, 2).unwrap();
        db.set_muted("bb", true, 3).unwrap();
        assert_eq!(db.mutes().unwrap(), ["aa", "bb"]);
        db.set_muted("aa", false, 4).unwrap();
        assert_eq!(db.mutes().unwrap(), ["bb"]);

        let id = db
            .add_preset("hello", "bonjour à tous", None, Some("bob"))
            .unwrap();
        assert_eq!(db.preset(id).unwrap().unwrap().text, "bonjour à tous");
        assert!(db.delete_preset(id).unwrap());
        assert!(db.presets().unwrap().is_empty());

        let once = db
            .add_schedule("once", Some("u09tv"), None, 100, None)
            .unwrap();
        let every = db.add_schedule("every", None, None, 100, Some(60)).unwrap();
        assert_eq!(db.due(99).unwrap().len(), 0);
        assert_eq!(db.due(100).unwrap().len(), 2);
        db.advance(once, 100, "sent").unwrap();
        // Missed occurrences are not made up: next one after now.
        db.advance(every, 250, "sent").unwrap();
        let all = db.schedules().unwrap();
        let get = |id| all.iter().find(|s| s.id == id).unwrap().clone();
        assert_eq!(
            (get(once).next_at, get(once).last_result.as_deref()),
            (None, Some("sent"))
        );
        assert_eq!(get(every).next_at, Some(280));
        assert!(db.due(279).unwrap().is_empty());
    }

    #[test]
    fn nickname_and_anonymous_cells() {
        let db = Db::open_in_memory().unwrap();
        assert_eq!(db.nickname().unwrap(), None);
        db.set_nickname(Some(" bob ")).unwrap();
        db.set_nickname(Some("alice")).unwrap();
        assert_eq!(db.nickname().unwrap().as_deref(), Some("alice"));
        db.set_nickname(Some("  ")).unwrap();
        assert_eq!(db.nickname().unwrap(), None);

        db.set_anon("u09tv", true, 1).unwrap();
        db.set_anon("u09tv", true, 2).unwrap();
        db.set_anon("u09ty", true, 3).unwrap();
        assert_eq!(db.anon_cells().unwrap(), ["u09tv", "u09ty"]);
        db.set_anon("u09tv", false, 4).unwrap();
        assert!(!db.is_anon("u09tv").unwrap() && db.is_anon("u09ty").unwrap());
    }

    #[test]
    fn a_newer_schema_is_refused() {
        let conn = Connection::open_in_memory().unwrap();
        conn.pragma_update(None, "user_version", SCHEMA_VERSION + 1)
            .unwrap();
        assert!(matches!(Db::init(conn), Err(Error::Db(_))));
    }
}
