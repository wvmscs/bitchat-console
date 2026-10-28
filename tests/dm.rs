//! Private messages between two consoles: the app's envelope and packets,
//! automatic delivery receipt, read receipt.

mod common;

use std::sync::Arc;
use std::time::Duration;

use bitchat_console::Error;
use bitchat_console::connector::Loopback;
use bitchat_console::console::{Console, DirectMessage, DmStatus};
use bitchat_console::identity::Seed;
use common::*;

fn console_with_seed(urls: &[String], seed: [u8; 32]) -> Console {
    let mut c = config(urls, Arc::new(Loopback));
    c.seed = Arc::new(Seed::from_bytes(seed));
    Console::start(c)
}

async fn dm_until(
    c: &Console,
    what: &str,
    check: impl Fn(&[DirectMessage]) -> bool,
) -> Vec<DirectMessage> {
    for _ in 0..250 {
        let d = c.dms().await.unwrap();
        if check(&d) {
            return d;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("never {what}: {:#?}", c.dms().await.unwrap());
}

#[tokio::test]
async fn a_conversation_with_receipts() {
    let relay = TestRelay::spawn("127.0.0.1:0").await.unwrap();
    let (alice, bob) = (
        console_with_seed(&[relay.url()], [1; 32]),
        console_with_seed(&[relay.url()], [2; 32]),
    );
    for c in [&alice, &bob] {
        c.set_area("u09tv", true).await.unwrap();
        status_until(c, "connected", all_connected).await;
    }
    let bob_key = Seed::from_bytes([2; 32])
        .cell("u09ty")
        .unwrap()
        .pubkey_hex();
    let alice_key = Seed::from_bytes([1; 32])
        .cell("u09ty")
        .unwrap()
        .pubkey_hex();

    let sent = alice
        .send_dm(&bob_key, "u09ty", "salut bob 👋")
        .await
        .unwrap();
    assert_eq!(sent.status, Some(DmStatus::Sent));
    assert!(sent.id.len() == 36 && sent.own);

    let got = dm_until(&bob, "a message", |d| !d.is_empty()).await;
    assert_eq!(got.len(), 1);
    assert_eq!(
        (got[0].id.as_str(), got[0].content.as_str()),
        (sent.id.as_str(), "salut bob 👋")
    );
    assert_eq!(
        (got[0].peer.as_str(), got[0].cell.as_str()),
        (alice_key.as_str(), "u09ty")
    );
    assert!(!got[0].own && !got[0].read);

    // Bob's console acknowledged it at once; then reads it.
    dm_until(&alice, "delivered", |d| {
        d[0].status == Some(DmStatus::Delivered)
    })
    .await;
    assert_eq!(bob.mark_read(&alice_key).await.unwrap(), 1);
    assert_eq!(bob.mark_read(&alice_key).await.unwrap(), 0, "once");
    assert!(bob.dms().await.unwrap()[0].read);
    dm_until(&alice, "read", |d| d[0].status == Some(DmStatus::Read)).await;

    // And back, from bob's key in that cell.
    bob.send_dm(&alice_key, "u09ty", "salut alice")
        .await
        .unwrap();
    let a = dm_until(&alice, "the answer", |d| d.len() == 2).await;
    assert_eq!(
        (a[1].content.as_str(), a[1].peer.as_str()),
        ("salut alice", bob_key.as_str())
    );
    alice.shutdown().await;
    bob.shutdown().await;
}

#[tokio::test]
async fn refused_private_messages() {
    let relay = TestRelay::spawn("127.0.0.1:0").await.unwrap();
    let c = console_with_seed(&[relay.url()], [3; 32]);
    c.set_area("u09tv", true).await.unwrap();
    let peer = Seed::from_bytes([4; 32])
        .cell("u09tv")
        .unwrap()
        .pubkey_hex();
    assert!(matches!(
        c.send_dm(&peer, "u09tv", &"x".repeat(256)).await,
        Err(Error::DmTooLong)
    ));
    assert!(matches!(
        c.send_dm(&peer, "u0bcd", "x").await,
        Err(Error::NotInArea(_))
    ));
    assert!(matches!(
        c.send_dm("abc", "u09tv", "x").await,
        Err(Error::InvalidKey(_))
    ));
    assert!(matches!(
        c.send_dm(&peer, "u09tv", " ").await,
        Err(Error::EmptyMessage)
    ));
    c.shutdown().await;
}

#[tokio::test]
async fn read_stays_read_after_a_restart() {
    // The relay keeps envelopes: after a restart they come back (D29).
    let relay = TestRelay::spawn("127.0.0.1:0").await.unwrap();
    let dir = std::env::temp_dir().join(format!("bitchat-console-read-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let bob_at = |db: &std::path::Path| {
        let mut c = config(&[relay.url()], Arc::new(Loopback));
        c.seed = Arc::new(Seed::from_bytes([2; 32]));
        c.db_path = Some(db.to_owned());
        Console::start(c)
    };
    let db = dir.join("console.sqlite");
    let alice = console_with_seed(&[relay.url()], [1; 32]);
    let bob = bob_at(&db);
    for c in [&alice, &bob] {
        c.set_area("u09tv", true).await.unwrap();
        status_until(c, "connected", all_connected).await;
    }
    let bob_key = Seed::from_bytes([2; 32])
        .cell("u09ty")
        .unwrap()
        .pubkey_hex();
    let alice_key = Seed::from_bytes([1; 32])
        .cell("u09ty")
        .unwrap()
        .pubkey_hex();
    alice.send_dm(&bob_key, "u09ty", "one").await.unwrap();
    alice.send_dm(&bob_key, "u09ty", "two").await.unwrap();
    dm_until(&bob, "two messages", |d| d.len() == 2).await;
    assert_eq!(bob.mark_read(&alice_key).await.unwrap(), 2);
    bob.shutdown().await;
    // A third, unread, while bob is away.
    alice.send_dm(&bob_key, "u09ty", "three").await.unwrap();

    let bob = bob_at(&db);
    bob.set_area("u09tv", true).await.unwrap();
    let d = dm_until(&bob, "three messages", |d| d.len() == 3).await;
    let mut read: Vec<(String, bool)> = d.into_iter().map(|m| (m.content, m.read)).collect();
    read.sort();
    assert_eq!(
        read,
        [
            ("one".into(), true),
            ("three".into(), false),
            ("two".into(), true)
        ]
    );
    // Only the new one gets a read receipt.
    assert_eq!(bob.mark_read(&alice_key).await.unwrap(), 1);
    bob.shutdown().await;
    alice.shutdown().await;
    std::fs::remove_dir_all(&dir).unwrap();
}

#[tokio::test]
async fn deleted_stay_deleted_after_a_restart() {
    // Lot 15: deleted here only; the relay keeps the envelopes and sends
    // them again after a restart, they stay deleted.
    let relay = TestRelay::spawn("127.0.0.1:0").await.unwrap();
    let dir = std::env::temp_dir().join(format!("bitchat-console-deleted-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let bob_at = |dir: &std::path::Path| {
        let mut c = config(&[relay.url()], Arc::new(Loopback));
        c.seed = Arc::new(Seed::from_bytes([2; 32]));
        c.db_path = Some(dir.join("console.sqlite"));
        c.history_path = Some(dir.join("history.sqlite"));
        Console::start(c)
    };
    let alice = console_with_seed(&[relay.url()], [1; 32]);
    let bob = bob_at(&dir);
    for c in [&alice, &bob] {
        c.set_area("u09tv", true).await.unwrap();
        status_until(c, "connected", all_connected).await;
    }
    let key = |n: u8| {
        Seed::from_bytes([n; 32])
            .cell("u09ty")
            .unwrap()
            .pubkey_hex()
    };
    let (alice_key, bob_key) = (key(1), key(2));
    let one = alice.send_dm(&bob_key, "u09ty", "one").await.unwrap();
    alice.send_dm(&bob_key, "u09ty", "two").await.unwrap();
    dm_until(&bob, "two messages", |d| d.len() == 2).await;
    bob.send_dm(&alice_key, "u09ty", "mine").await.unwrap();

    // One message, then the rest of the conversation.
    assert_eq!(
        bob.delete_dms(&alice_key, "u09ty", Some(vec![one.id.clone()]))
            .await
            .unwrap(),
        1
    );
    let left: Vec<String> = bob
        .dms()
        .await
        .unwrap()
        .into_iter()
        .map(|d| d.content)
        .collect();
    assert_eq!(left, ["two", "mine"]);
    assert_eq!(bob.delete_dms(&alice_key, "u09ty", None).await.unwrap(), 2);
    assert!(bob.dms().await.unwrap().is_empty());
    bob.history().unwrap().flush().await;
    let contents: Vec<Option<String>> = rusqlite::Connection::open(dir.join("history.sqlite"))
        .unwrap()
        .prepare("SELECT content FROM message WHERE kind = 'dm' AND deleted_at IS NOT NULL")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(contents, [None, None, None], "their content is gone");
    bob.shutdown().await;

    // After a restart: the relay sends them again, they stay out; a new
    // one comes in.
    let bob = bob_at(&dir);
    bob.set_area("u09tv", true).await.unwrap();
    status_until(&bob, "connected", all_connected).await;
    alice.send_dm(&bob_key, "u09ty", "new").await.unwrap();
    let d = dm_until(&bob, "the new message", |d| !d.is_empty()).await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    let d2 = bob.dms().await.unwrap();
    assert_eq!(d.len(), 1);
    assert_eq!(
        d2.iter().map(|d| d.content.as_str()).collect::<Vec<_>>(),
        ["new"]
    );
    bob.shutdown().await;
    alice.shutdown().await;
    std::fs::remove_dir_all(&dir).unwrap();
}
