//! Local storage (lot 8, D16): SQLite in the data directory, for what the
//! user decides only: muted keys, saved messages, schedules. Never received
//! contents or nicknames (D11, D25). Append-only numbered migrations, as in
//! bitchat-geohash.

use std::path::Path;

use rusqlite::{Connection, OptionalExtension, params};
use serde::Serialize;

use crate::error::{Error, Result};

const MIGRATIONS: &[(&str, &str)] = &[("0001_init", include_str!("migrations/0001_init.sql"))];
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

fn sql(e: rusqlite::Error) -> Error {
    Error::Db(e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn a_newer_schema_is_refused() {
        let conn = Connection::open_in_memory().unwrap();
        conn.pragma_update(None, "user_version", SCHEMA_VERSION + 1)
            .unwrap();
        assert!(matches!(Db::init(conn), Err(Error::Db(_))));
    }
}
