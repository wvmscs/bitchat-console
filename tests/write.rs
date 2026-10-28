//! Writing: messages exactly as the app writes them, presences, answers of
//! the relays.

mod common;

use std::sync::Arc;
use std::time::Duration;

use bitchat_console::Error;
use bitchat_console::connector::Loopback;
use bitchat_console::console::{Console, KIND_MESSAGE, KIND_PRESENCE};
use bitchat_console::pow::leading_zero_bits;
use common::*;
use serde_json::json;

#[tokio::test]
async fn messages_are_written_like_the_app() {
    let relay = TestRelay::spawn("127.0.0.1:0").await.unwrap();
    let c = console(&[relay.url()], Arc::new(Loopback));
    c.set_area("u09tv", false).await.unwrap();
    status_until(&c, "connected", all_connected).await;
    let mut seen = reader(&relay.url(), json!({"kinds": [20000]})).await;

    let sent = c.send("bonjour", None, Some(" bob ")).await.unwrap();
    let e = next(&mut seen).await;
    assert_eq!(e.id, sent.id);
    assert!(e.verify().is_ok());
    assert_eq!(e.kind, KIND_MESSAGE);
    assert_eq!(e.content, "bonjour");
    // The key of the cell, the app's tags in the app's order, mined.
    assert_eq!(e.pubkey, test_seed().cell("u09tv").unwrap().pubkey_hex());
    assert_eq!(
        e.tags[..2],
        [vec!["g", "u09tv"], vec!["n", "bob"]]
            .map(|t| t.iter().map(|s| s.to_string()).collect::<Vec<_>>())
    );
    assert_eq!(e.tags.len(), 3);
    assert_eq!(
        (e.tags[2][0].as_str(), e.tags[2][2].as_str()),
        ("nonce", "8")
    );
    assert!(leading_zero_bits(&e.id) >= 8);
    assert!(sent.own && !sent.teleport);

    // The relay's OK adds it to the message; the feed has it once.
    for _ in 0..150 {
        if c.messages("u09tv").await.unwrap()[0].relays == [relay.url()] {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let feed = c.messages("u09tv").await.unwrap();
    assert_eq!(feed.len(), 1);
    assert_eq!(feed[0].relays, [relay.url()]);

    // A neighbour: its own key.
    let sent = c.send("à côté", Some("u09ty"), None).await.unwrap();
    let e = next(&mut seen).await;
    assert_eq!(e.id, sent.id);
    assert_eq!(e.pubkey, test_seed().cell("u09ty").unwrap().pubkey_hex());
    assert_eq!(e.tags[0], ["g", "u09ty"]);
    assert_eq!(e.tags.len(), 2);

    // Refusals.
    assert!(matches!(
        c.send("x", Some("u0bcd"), None).await,
        Err(Error::NotInArea(_))
    ));
    assert!(matches!(
        c.send("  ", None, None).await,
        Err(Error::EmptyMessage)
    ));
    c.shutdown().await;
}

#[tokio::test]
async fn teleported_messages_say_so() {
    let relay = TestRelay::spawn("127.0.0.1:0").await.unwrap();
    let c = console(&[relay.url()], Arc::new(Loopback));
    c.set_area("u09tv", true).await.unwrap();
    status_until(&c, "connected", all_connected).await;
    let mut seen = reader(&relay.url(), json!({"kinds": [20000]})).await;
    let sent = c.send("de loin", None, Some("bob")).await.unwrap();
    let e = next(&mut seen).await;
    let names: Vec<&str> = e.tags.iter().map(|t| t[0].as_str()).collect();
    assert_eq!(names, ["g", "n", "t", "nonce"]);
    assert_eq!(e.tags[2], ["t", "teleport"]);
    assert!(sent.teleport);
    assert!(c.status().await.unwrap().teleport);
    c.shutdown().await;
}

async fn presence_console(relay: &TestRelay) -> Console {
    let mut cfg = config(&[relay.url()], Arc::new(Loopback));
    cfg.presence = true;
    cfg.presence_every = (Duration::from_millis(300), Duration::from_millis(400));
    Console::start(cfg)
}

#[tokio::test]
async fn presences_go_to_levels_2_4_5_unless_teleported() {
    let relay = TestRelay::spawn("127.0.0.1:0").await.unwrap();
    let mut seen = reader(&relay.url(), json!({"kinds": [20001]})).await;
    let c = presence_console(&relay).await;
    c.set_area("u09tvw", false).await.unwrap();
    let mut cells = Vec::new();
    for _ in 0..3 {
        let e = next(&mut seen).await;
        assert_eq!(e.kind, KIND_PRESENCE);
        assert_eq!(e.content, "");
        // Exactly the app's presence: `g` only, with that cell's key.
        assert_eq!(e.tags.len(), 1);
        let cell = e.tags[0][1].clone();
        assert_eq!(e.pubkey, test_seed().cell(&cell).unwrap().pubkey_hex());
        assert!(e.verify().is_ok());
        cells.push(cell);
    }
    cells.sort();
    assert_eq!(cells, ["u0", "u09t", "u09tv"]);
    // And again, 300 to 400 ms later.
    let again = next(&mut seen).await;
    assert_eq!(again.kind, KIND_PRESENCE);

    // Teleported: no presence any more.
    c.set_area("u09tvw", true).await.unwrap();
    tokio::time::sleep(Duration::from_millis(300)).await;
    while seen.try_recv().is_ok() {}
    tokio::time::sleep(Duration::from_millis(900)).await;
    assert!(
        seen.try_recv().is_err(),
        "a teleported console sent a presence"
    );
    c.shutdown().await;
}
