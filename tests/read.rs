//! Reading the area: 9 cells, one connection per (relay, cell), the feed.

mod common;

use std::sync::Arc;
use std::time::Duration;

use bitchat_console::Error;
use bitchat_console::connector::{Loopback, Socks5};
use bitchat_console::console::{ConsoleEvent, KIND_MESSAGE, KIND_PRESENCE};
use common::socks::TestSocks;
use common::*;
use serde_json::json;

#[tokio::test]
async fn the_area_is_a_cell_and_its_neighbours() {
    let (a, b) = (
        TestRelay::spawn("127.0.0.1:0").await.unwrap(),
        TestRelay::spawn("127.0.0.1:0").await.unwrap(),
    );
    let c = console(&[a.url(), b.url()], Arc::new(Loopback));
    let cells = c.set_area("U09TV").await.unwrap();
    assert_eq!(
        cells,
        [
            "u09tv", "u09wj", "u09wn", "u09ty", "u09tw", "u09tt", "u09ts", "u09tu", "u09wh"
        ]
    );
    let s = status_until(&c, "connected", all_connected).await;
    assert_eq!(s.center.as_deref(), Some("u09tv"));
    assert_eq!(s.cells.len(), 9);
    assert!(s.cells.iter().all(|c| c.relays.len() == 2));
    assert_eq!(s.connector, "loopback (tests)");
    let mut events = c.subscribe();

    // A message in a neighbour, published on both relays: once in the feed,
    // delivered by both.
    let who = Identity::generate();
    let e = channel_event(
        &who,
        KIND_MESSAGE,
        "u09ty",
        &[["n", "alice"], ["t", "teleport"]],
        "bonjour",
    );
    a.publish(e.clone());
    b.publish(e.clone());
    let mut feed = Vec::new();
    for _ in 0..150 {
        feed = c.messages("u09ty").await.unwrap();
        if feed.first().is_some_and(|m| m.relays.len() == 2) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(feed.len(), 1);
    let m = &feed[0];
    assert_eq!(
        (m.id.as_str(), m.content.as_str()),
        (e.id.as_str(), "bonjour")
    );
    assert_eq!(m.nickname.as_deref(), Some("alice"));
    assert!(m.teleport && !m.own);
    assert_eq!(m.relays.len(), 2);
    // A presence counts as a participant, not as a message.
    let other = Identity::generate();
    a.publish(channel_event(&other, KIND_PRESENCE, "u09ty", &[], ""));
    let s = status_until(&c, "2 participants", |s| s.cells[3].participants == 2).await;
    assert_eq!(s.cells[3].geohash, "u09ty");
    assert_eq!(c.messages("u09ty").await.unwrap().len(), 1);
    // The page hears about it.
    let mut got_message = false;
    while let Ok(e) = events.try_recv() {
        got_message |= matches!(e, ConsoleEvent::Message(ref m) if m.content == "bonjour");
    }
    assert!(got_message);
    c.shutdown().await;
}

#[tokio::test]
async fn bad_events_are_dropped() {
    let good = Identity::generate();
    let ok = channel_event(&good, KIND_MESSAGE, "u09tv", &[], "good");
    let mut forged = channel_event(&good, KIND_MESSAGE, "u09tv", &[], "forged");
    forged.content = "changed after signing".into();
    let elsewhere = channel_event(&good, KIND_MESSAGE, "u0bcd", &[], "other cell");
    let note = channel_event(&good, 1, "u09tv", &[], "kind 1");
    let mut replies: Vec<String> = [&ok, &forged, &elsewhere, &note]
        .iter()
        .map(|e| json!(["EVENT", "{sub}", e]).to_string())
        .collect();
    replies.push(json!(["EOSE", "{sub}"]).to_string());
    let relay = scripted_relay(replies).await;
    let c = console(&[relay], Arc::new(Loopback));
    c.set_area("u09tv").await.unwrap();
    status_until(&c, "connected", all_connected).await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    let feed = c.messages("u09tv").await.unwrap();
    assert_eq!(feed.len(), 1, "{feed:#?}");
    assert_eq!(feed[0].content, "good");
    c.shutdown().await;
}

#[tokio::test]
async fn moving_the_area_moves_the_connections() {
    let relay = TestRelay::spawn("127.0.0.1:0").await.unwrap();
    let c = console(&[relay.url()], Arc::new(Loopback));
    c.set_area("u09tv").await.unwrap();
    status_until(&c, "connected", all_connected).await;
    let cells = c.set_area("u09ty").await.unwrap();
    assert_eq!(cells[0], "u09ty");
    let s = status_until(&c, "connected", all_connected).await;
    assert_eq!(s.center.as_deref(), Some("u09ty"));
    // A message in a cell no longer followed is not shown.
    let gone = cells.iter().all(|c| c != "u09ts");
    assert!(gone);
    relay.publish(channel_event(
        &Identity::generate(),
        KIND_MESSAGE,
        "u09ts",
        &[],
        "x",
    ));
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(c.messages("u09ts").await.unwrap().is_empty());
    // Not channels.
    assert!(matches!(
        c.set_area("u09").await,
        Err(Error::NotAChannel(_))
    ));
    assert!(matches!(
        c.set_area("u0a00").await,
        Err(Error::InvalidGeohash(_))
    ));
    c.shutdown().await;
}

#[tokio::test]
async fn each_cell_gets_its_own_circuit() {
    let relay = TestRelay::spawn("127.0.0.1:0").await.unwrap();
    let proxy = TestSocks::spawn().await;
    let c = console(&[relay.url()], Arc::new(Socks5::new(proxy.addr)));
    c.set_area("u09tv").await.unwrap();
    status_until(&c, "connected", all_connected).await;
    let mut users: Vec<String> = proxy.requests().into_iter().map(|r| r.0).collect();
    users.sort();
    users.dedup();
    assert_eq!(users.len(), 9, "{users:?}");
    assert!(users.contains(&"bitchat-console/u09tv".to_owned()));
    c.shutdown().await;
}
