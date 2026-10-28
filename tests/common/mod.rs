//! Test helpers: a local NIP-01 relay and signing keys (from
//! bitchat-geohash), a SOCKS5 proxy that records what it is asked, an echo
//! server.
#![allow(dead_code)]

pub mod relay;
pub mod socks;

#[allow(unused_imports)]
pub use relay::{Identity, Sign, TestRelay};

use std::sync::Arc;
use std::time::Duration;

use bitchat_console::connector::Connector;
use bitchat_console::console::{AreaStatus, Console, ConsoleConfig};
use bitchat_console::nostr::Event;
use futures_util::{SinkExt, StreamExt};
use geohash_relays::{AppStrategy, Apps, Platform, RelayDirectory};
use serde_json::Value;
use tokio::net::TcpListener;
use tokio_tungstenite::tungstenite::Message;

/// An iOS-rule strategy over these relays, all at the centre of Paris: with
/// fewer than 5 relays, every cell gets all of them.
pub fn strategy_of(urls: &[String]) -> AppStrategy {
    let mut csv = String::from("Relay URL,Latitude,Longitude\n");
    for url in urls {
        csv.push_str(&format!("{url},48.8566,2.3522\n"));
    }
    AppStrategy::new(
        Some(RelayDirectory::parse(&csv, Platform::Ios)),
        None,
        Apps::Both,
    )
}

/// A fixed seed: keys are the same from one run to the next.
pub fn test_seed() -> Arc<bitchat_console::identity::Seed> {
    Arc::new(bitchat_console::identity::Seed::from_bytes([7; 32]))
}

/// The test configuration: no presence unless a test turns it on.
pub fn config(urls: &[String], connector: Arc<dyn Connector>) -> ConsoleConfig {
    let mut c = ConsoleConfig::new(Arc::new(strategy_of(urls)), connector, test_seed());
    c.presence = false;
    // Never the real default relays from a test.
    c.dm_relays = urls.to_vec();
    c
}

pub fn console(urls: &[String], connector: Arc<dyn Connector>) -> Console {
    Console::start(config(urls, connector))
}

/// Waits until `check` holds on the console status.
pub async fn status_until(
    c: &Console,
    what: &str,
    check: impl Fn(&AreaStatus) -> bool,
) -> AreaStatus {
    for _ in 0..300 {
        let s = c.status().await.unwrap();
        if check(&s) {
            return s;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("never {what}: {:#?}", c.status().await.unwrap());
}

pub fn all_connected(s: &AreaStatus) -> bool {
    !s.cells.is_empty()
        && s.cells
            .iter()
            .all(|c| c.relays.iter().all(|r| r.state == "connected"))
}

/// A channel event of `kind` in `cell`, with extra tags.
pub fn channel_event(
    who: &Identity,
    kind: u32,
    cell: &str,
    extra: &[[&str; 2]],
    content: &str,
) -> Event {
    let mut tags = vec![vec!["g".to_owned(), cell.to_owned()]];
    tags.extend(
        extra
            .iter()
            .map(|[k, v]| vec![k.to_string(), v.to_string()]),
    );
    Event::new_unsigned(
        &who.public_key_hex(),
        bitchat_console::nostr::unix_now(),
        kind,
        tags,
        content.into(),
    )
    .sign(who)
    .unwrap()
}

/// A WebSocket server that answers each REQ with `replies`, where `{sub}`
/// stands for the subscription id (from bitchat-geohash).
pub async fn scripted_relay(replies: Vec<String>) -> String {
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("ws://{}", l.local_addr().unwrap());
    tokio::spawn(async move {
        while let Ok((stream, _)) = l.accept().await {
            let replies = replies.clone();
            tokio::spawn(async move {
                let Ok(mut ws) = tokio_tungstenite::accept_async(stream).await else {
                    return;
                };
                while let Some(Ok(msg)) = ws.next().await {
                    let Message::Text(text) = msg else { continue };
                    let Ok(Value::Array(v)) = serde_json::from_str::<Value>(text.as_str()) else {
                        continue;
                    };
                    if v.first().and_then(Value::as_str) != Some("REQ") {
                        continue;
                    }
                    let sub = v.get(1).and_then(Value::as_str).unwrap_or("");
                    for r in &replies {
                        let _ = ws.send(Message::text(r.replace("{sub}", sub))).await;
                    }
                }
            });
        }
    });
    url
}

/// A plain client subscribed to `relay` with `filter` (JSON); returns the
/// events it receives.
pub async fn reader(relay: &str, filter: Value) -> tokio::sync::mpsc::UnboundedReceiver<Event> {
    let (mut ws, _) = tokio_tungstenite::connect_async(relay).await.unwrap();
    let req = serde_json::json!(["REQ", "reader", filter]).to_string();
    ws.send(Message::text(req)).await.unwrap();
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    tokio::spawn(async move {
        while let Some(Ok(msg)) = ws.next().await {
            let Message::Text(text) = msg else { continue };
            let Ok(Value::Array(v)) = serde_json::from_str::<Value>(text.as_str()) else {
                continue;
            };
            if v.first().and_then(Value::as_str) == Some("EVENT")
                && let Some(Ok(e)) = v.get(2).cloned().map(serde_json::from_value::<Event>)
            {
                let _ = tx.send(e);
            }
        }
    });
    // Let the REQ reach the relay before anything is published.
    tokio::time::sleep(Duration::from_millis(50)).await;
    rx
}

pub async fn next(rx: &mut tokio::sync::mpsc::UnboundedReceiver<Event>) -> Event {
    tokio::time::timeout(Duration::from_secs(5), rx.recv())
        .await
        .expect("no event within 5 s")
        .unwrap()
}
