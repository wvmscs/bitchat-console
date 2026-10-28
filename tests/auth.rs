//! Relays that require NIP-42 authentication (D28): the console answers
//! with the connection's key, subscribes again and sends again what was
//! refused, once.

mod common;

use std::sync::Arc;
use std::time::Duration;

use bitchat_console::connector::Loopback;
use bitchat_console::console::{ConsoleEvent, KIND_MESSAGE};
use common::relay::AuthMode;
use common::*;

async fn until(what: &str, mut check: impl AsyncFnMut() -> bool) {
    for _ in 0..300 {
        if check().await {
            return;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("never {what}");
}

#[tokio::test]
async fn reading_and_writing_after_authentication() {
    // The challenge comes with the first refusal of a message, which goes
    // again after the authentication.
    let relay = TestRelay::spawn_auth("127.0.0.1:0", AuthMode::WriteLater)
        .await
        .unwrap();
    let c = console(&[relay.url()], Arc::new(Loopback));
    let mut events = c.subscribe();
    c.set_area("u09tv", true).await.unwrap();
    let sent = c.send("before AUTH", None, None).await.unwrap();
    until("delivered", async || {
        c.messages("u09tv")
            .await
            .unwrap()
            .iter()
            .any(|m| m.id == sent.id && !m.relays.is_empty())
    })
    .await;
    assert_eq!(relay.received(&sent.id), 2);
    assert_eq!(relay.auths(), 1);
    status_until(&c, "connected", all_connected).await;

    // Reading goes on.
    let who = Identity::generate();
    let e = channel_event(&who, KIND_MESSAGE, "u09tv", &[], "after AUTH");
    relay.publish(e.clone());
    until("received", async || {
        c.messages("u09tv")
            .await
            .unwrap()
            .iter()
            .any(|m| m.id == e.id)
    })
    .await;

    // The journal says so; the refusal waiting for it does not show.
    let mut texts = Vec::new();
    while let Ok(ev) = events.try_recv() {
        if let ConsoleEvent::RelayText { text, .. } = ev {
            texts.push(text);
        }
    }
    assert!(
        texts.iter().any(|t| t == "authenticated (NIP-42)"),
        "{texts:?}"
    );
    assert!(!texts.iter().any(|t| t.starts_with("refused")), "{texts:?}");
    c.shutdown().await;
}

#[tokio::test]
async fn a_challenge_at_connection() {
    // The subscription, sent before the challenge is answered, is refused
    // and sent again after: one authentication per connection.
    let relay = TestRelay::spawn_auth("127.0.0.1:0", AuthMode::Required)
        .await
        .unwrap();
    let c = console(&[relay.url()], Arc::new(Loopback));
    c.set_area("u09tv", true).await.unwrap();
    until("9 authentications", async || relay.auths() >= 9).await;
    status_until(&c, "connected", all_connected).await;
    let who = Identity::generate();
    let e = channel_event(&who, KIND_MESSAGE, "u09ty", &[], "hello");
    until("received", async || {
        relay.publish(e.clone());
        c.messages("u09ty")
            .await
            .unwrap()
            .iter()
            .any(|m| m.id == e.id)
    })
    .await;
    let sent = c.send("hi", None, None).await.unwrap();
    until("delivered", async || {
        c.messages("u09tv")
            .await
            .unwrap()
            .iter()
            .any(|m| m.id == sent.id && !m.relays.is_empty())
    })
    .await;
    c.shutdown().await;
}

#[tokio::test]
async fn a_refusal_after_authentication_is_final() {
    // Like relay.mmwaves.de: authenticated, still not allowed to write.
    let relay = TestRelay::spawn_auth("127.0.0.1:0", AuthMode::Whitelist(Vec::new()))
        .await
        .unwrap();
    let c = console(&[relay.url()], Arc::new(Loopback));
    let mut events = c.subscribe();
    c.set_area("u09tv", true).await.unwrap();
    let sent = c.send("not allowed", None, None).await.unwrap();
    let mut refused = None;
    for _ in 0..300 {
        while let Ok(ev) = events.try_recv() {
            if let ConsoleEvent::RelayText { text, .. } = ev
                && text.starts_with("refused")
            {
                refused = Some(text);
            }
        }
        if refused.is_some() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(
        refused.as_deref(),
        Some("refused: auth-required: event author pubkey not in whitelist")
    );
    // Sent at most twice (if it went before the authentication), never
    // more; reading goes on.
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!((1..=2).contains(&relay.received(&sent.id)));
    status_until(&c, "connected", all_connected).await;
    c.shutdown().await;
}
