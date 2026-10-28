//! The message history (lot 14, D31): every message kept in
//! `history.sqlite`, private to the user, back when deleted, back in the
//! feed when a cell is followed again.

mod common;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use bitchat_console::connector::Loopback;
use bitchat_console::console::{Console, DirectMessage, DmStatus, KIND_MESSAGE};
use bitchat_console::history::{HISTORY_FILE, History};
use bitchat_console::identity::Seed;
use bitchat_console::nostr::unix_now;
use common::*;
use rusqlite::Connection;

fn scratch(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!(
        "bitchat-console-history-{name}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn console_with(urls: &[String], seed: [u8; 32], history: Option<&Path>) -> Console {
    let mut c = config(urls, Arc::new(Loopback));
    c.seed = Arc::new(Seed::from_bytes(seed));
    c.history_path = history.map(Path::to_owned);
    Console::start(c)
}

async fn until<T>(what: &str, mut get: impl AsyncFnMut() -> Option<T>) -> T {
    for _ in 0..250 {
        if let Some(v) = get().await {
            return v;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("never {what}");
}

async fn dm_where(c: &Console, check: impl Fn(&[DirectMessage]) -> bool) {
    until("the private messages", async || {
        let d = c.dms().await.unwrap();
        check(&d).then_some(())
    })
    .await;
}

/// (kind, own, geohash, sender, recipient, nickname, content, status) by id.
type Row = (
    String,
    bool,
    Option<String>,
    String,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
);

fn rows(path: &Path) -> Vec<Row> {
    let conn = Connection::open(path).unwrap();
    let mut st = conn
        .prepare(
            "SELECT kind, own, geohash, sender, recipient, nickname, content, status \
             FROM message ORDER BY id",
        )
        .unwrap();
    st.query_map([], |r| {
        Ok((
            r.get(0)?,
            r.get(1)?,
            r.get(2)?,
            r.get(3)?,
            r.get(4)?,
            r.get(5)?,
            r.get(6)?,
            r.get(7)?,
        ))
    })
    .unwrap()
    .collect::<Result<_, _>>()
    .unwrap()
}

#[tokio::test]
async fn every_message_is_kept() {
    let relay = TestRelay::spawn("127.0.0.1:0").await.unwrap();
    let dir = scratch("all");
    let path = dir.join(HISTORY_FILE);
    let alice = console_with(&[relay.url()], [1; 32], Some(&path));
    let bob = console_with(&[relay.url()], [2; 32], None);
    for c in [&alice, &bob] {
        c.set_area("u09tv", true).await.unwrap();
        status_until(c, "connected", all_connected).await;
    }
    let key = |seed: u8| {
        Seed::from_bytes([seed; 32])
            .cell("u09ty")
            .unwrap()
            .pubkey_hex()
    };
    let (alice_key, bob_key) = (key(1), key(2));

    // Channel: received (muted ones too) and sent.
    let carol = Identity::generate();
    relay.publish(channel_event(
        &carol,
        KIND_MESSAGE,
        "u09ty",
        &[["n", "carol"]],
        "bonjour",
    ));
    until("carol's message", async || {
        (!alice.messages("u09ty").await.unwrap().is_empty()).then_some(())
    })
    .await;
    alice
        .set_muted(&carol.public_key_hex(), true)
        .await
        .unwrap();
    relay.publish(channel_event(&carol, KIND_MESSAGE, "u09ty", &[], "encore"));
    alice.send("salut", None, None).await.unwrap();

    // Private: both ways, with receipts.
    let sent = alice.send_dm(&bob_key, "u09ty", "pour bob").await.unwrap();
    dm_where(&bob, |d| !d.is_empty()).await;
    bob.mark_read(&alice_key).await.unwrap();
    dm_where(&alice, |d| d[0].status == Some(DmStatus::Read)).await;
    bob.send_dm(&alice_key, "u09ty", "pour alice")
        .await
        .unwrap();
    dm_where(&alice, |d| d.len() == 2).await;
    alice.mark_read(&bob_key).await.unwrap();

    until("everything written", async || {
        alice.history().unwrap().flush().await;
        (rows(&path).len() == 5).then_some(())
    })
    .await;
    let r = rows(&path);
    let carol_key = carol.public_key_hex();
    let channel: Vec<_> = r.iter().filter(|x| x.0 == "channel").collect();
    assert_eq!(channel.len(), 3, "{r:#?}");
    let find = |content: &str| {
        r.iter()
            .find(|x| x.6.as_deref() == Some(content))
            .unwrap_or_else(|| panic!("{content} not kept: {r:#?}"))
            .clone()
    };
    let b = find("bonjour");
    assert_eq!(
        (b.1, b.2.as_deref(), b.3.as_str(), b.5.as_deref()),
        (false, Some("u09ty"), carol_key.as_str(), Some("carol"))
    );
    assert_eq!(find("encore").3, carol_key, "a muted key is kept");
    let s = find("salut");
    assert_eq!((s.1, s.2.as_deref()), (true, Some("u09tv")));
    let out = find("pour bob");
    assert_eq!(
        (
            out.0.as_str(),
            out.1,
            out.3.as_str(),
            out.4.as_deref(),
            out.7.as_deref()
        ),
        (
            "dm",
            true,
            alice_key.as_str(),
            Some(bob_key.as_str()),
            Some("read")
        )
    );
    let back = find("pour alice");
    assert_eq!(
        (
            back.1,
            back.3.as_str(),
            back.4.as_deref(),
            back.7.as_deref()
        ),
        (
            false,
            bob_key.as_str(),
            Some(alice_key.as_str()),
            Some("read")
        )
    );
    assert_eq!(sent.content, "pour bob");

    // Readable by the user only, its WAL and shared memory too.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        for ext in ["", "-wal", "-shm"] {
            let p = PathBuf::from(format!("{}{ext}", path.display()));
            let mode = std::fs::metadata(&p).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600, "{}", p.display());
        }
    }
    alice.shutdown().await;
    bob.shutdown().await;
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn a_deleted_history_comes_back_empty() {
    let relay = TestRelay::spawn("127.0.0.1:0").await.unwrap();
    let dir = scratch("wipe");
    let path = dir.join(HISTORY_FILE);
    let c = console_with(&[relay.url()], [3; 32], Some(&path));
    c.set_area("u09tv", true).await.unwrap();
    status_until(&c, "connected", all_connected).await;
    c.send("avant", None, None).await.unwrap();
    c.history().unwrap().flush().await;
    assert_eq!(rows(&path).len(), 1);

    // A full wipe by the cron, while the console runs.
    for ext in ["", "-wal", "-shm"] {
        let _ = std::fs::remove_file(format!("{}{ext}", path.display()));
    }
    let wiped = unix_now();
    until("the file again", async || path.exists().then_some(())).await;
    c.send("après", None, None).await.unwrap();
    c.history().unwrap().flush().await;
    let r = rows(&path);
    assert_eq!(r.len(), 1);
    assert_eq!(r[0].6.as_deref(), Some("après"));
    let since: i64 = Connection::open(&path)
        .unwrap()
        .query_row("SELECT value FROM meta WHERE key = 'since'", [], |r| {
            r.get::<_, String>(0)
        })
        .unwrap()
        .parse()
        .unwrap();
    assert!(since >= wiped, "a new base starts when it is made");
    c.shutdown().await;
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn kept_messages_come_back_in_the_feed() {
    let relay = TestRelay::spawn("127.0.0.1:0").await.unwrap();
    let dir = scratch("back");
    let path = dir.join(HISTORY_FILE);
    // A base whose start is an hour ago, with three messages written by a
    // console that is gone: one an hour old, one deleted, one elsewhere.
    History::open(&path).unwrap().flush().await;
    let conn = Connection::open(&path).unwrap();
    let now = unix_now();
    conn.execute(
        "UPDATE meta SET value = ?1 WHERE key = 'since'",
        [(now - 7200).to_string()],
    )
    .unwrap();
    let insert = |id: &str, cell: &str, content: Option<&str>, at: i64| {
        conn.execute(
            "INSERT INTO message (kind, msg_id, own, geohash, sender, nickname, content, \
             created_at, received_at, deleted_at) VALUES ('channel', ?1, 0, ?2, ?3, 'dan', ?4, \
             ?5, ?5, CASE WHEN ?4 IS NULL THEN ?5 END)",
            rusqlite::params![id, cell, "ab".repeat(32), content, at],
        )
        .unwrap();
    };
    insert(
        &"1".repeat(64),
        "u09tv",
        Some("il y a une heure"),
        now - 3600,
    );
    insert(&"2".repeat(64), "u09tv", None, now - 3000);
    insert(&"3".repeat(64), "u0bcd", Some("ailleurs"), now - 3000);
    drop(conn);

    let c = console_with(&[relay.url()], [4; 32], Some(&path));
    c.set_area("u09tv", true).await.unwrap();
    let m = until("the kept message", async || {
        let m = c.messages("u09tv").await.unwrap();
        (!m.is_empty()).then_some(m)
    })
    .await;
    assert_eq!(m.len(), 1, "deleted ones stay out: {m:#?}");
    assert_eq!(
        (m[0].content.as_str(), m[0].nickname.as_deref(), m[0].own),
        ("il y a une heure", Some("dan"), false)
    );

    // Older than the start of the base: not written (relays sending it
    // again after a rotation cannot bring it back).
    let mut old = m[0].clone();
    old.id = "4".repeat(64);
    old.created_at = now - 3 * 3600;
    let h = c.history().unwrap();
    h.channel(&old);
    // A deleted one sent again stays deleted.
    let mut again = old.clone();
    again.id = "2".repeat(64);
    again.created_at = now;
    h.channel(&again);
    h.flush().await;
    let r = rows(&path);
    assert_eq!(r.len(), 3, "the old one is not written: {r:#?}");
    let conn = Connection::open(&path).unwrap();
    let gone: Option<String> = conn
        .query_row(
            "SELECT content FROM message WHERE msg_id = ?1",
            [&"2".repeat(64)],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(gone, None);
    c.shutdown().await;
    let _ = std::fs::remove_dir_all(dir);
}
