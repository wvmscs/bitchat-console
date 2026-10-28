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
