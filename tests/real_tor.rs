//! Real network, through Tor (Arti), on the user's relay only
//! (`wss://relay.famillevital.club`, authorised for tests) with throwaway
//! seeds. Not run by default:
//!
//!   cargo test --features arti --test real_tor -- --ignored --nocapture
#![cfg(feature = "arti")]

mod common;

use std::sync::Arc;
use std::time::{Duration, Instant};

use bitchat_console::arti::Arti;
use bitchat_console::console::{Console, ConsoleConfig};
use bitchat_console::identity::Seed;
use bitchat_console::lists::fetch;
use common::*;
use geohash_relays::{Platform, RelayDirectory};

const OWN_RELAY: &str = "wss://relay.famillevital.club";

fn throwaway_seed() -> Arc<Seed> {
    let mut b = [0u8; 32];
    rand_core::RngCore::fill_bytes(&mut rand_core::OsRng, &mut b);
    Arc::new(Seed::from_bytes(b))
}

#[tokio::test]
#[ignore]
async fn closed_loop_through_tor() {
    bitchat_console::install_crypto_provider();
    let _ = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .with_test_writer()
        .try_init();
    let dir = std::env::temp_dir().join("bitchat-console-arti-test");
    let started = Instant::now();
    let tor = Arc::new(Arti::bootstrap(&dir).await.unwrap());
    println!(
        "tor bootstrapped in {:.1} s",
        started.elapsed().as_secs_f64()
    );

    // The official iOS list, through Tor.
    let t = Instant::now();
    let csv = fetch(Platform::Ios.list_url(), tor.as_ref()).await.unwrap();
    let ios = RelayDirectory::parse(&csv, Platform::Ios);
    println!(
        "iOS list through Tor: {} relays in {:.1} s",
        ios.len(),
        t.elapsed().as_secs_f64()
    );
    assert!(ios.len() > 100);

    // Two consoles on the user's relay only: one writes, one reads.
    let strategy = Arc::new(strategy_of(&[OWN_RELAY.to_owned()]));
    let mk = || {
        let mut c = ConsoleConfig::new(strategy.clone(), tor.clone(), throwaway_seed());
        c.presence = false;
        Console::start(c)
    };
    let (writer, reader) = (mk(), mk());
    for c in [&writer, &reader] {
        c.set_area("u09tvw", true).await.unwrap();
    }
    let t = Instant::now();
    status_until_long(&writer, &reader).await;
    println!("18 connections up in {:.1} s", t.elapsed().as_secs_f64());

    let t = Instant::now();
    let sent = writer
        .send(
            "bitchat-console lot 11: closed loop through Tor",
            Some("u09tvw"),
            Some("test"),
        )
        .await
        .unwrap();
    let mut got = None;
    for _ in 0..300 {
        let feed = reader.messages("u09tvw").await.unwrap();
        if let Some(m) = feed.into_iter().find(|m| m.id == sent.id) {
            got = Some(m);
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let got = got.expect("the reader never got the message");
    println!(
        "message seen by the other console in {:.2} s",
        t.elapsed().as_secs_f64()
    );
    assert!(got.teleport && !got.own);
    assert_eq!(got.nickname.as_deref(), Some("test"));
    // The writer's relay accepted it.
    for _ in 0..100 {
        if writer
            .messages("u09tvw")
            .await
            .unwrap()
            .iter()
            .any(|m| m.id == sent.id && !m.relays.is_empty())
        {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let mine = writer.messages("u09tvw").await.unwrap();
    assert!(
        mine.iter()
            .any(|m| m.id == sent.id && m.relays == [OWN_RELAY]),
        "{mine:#?}"
    );
    writer.shutdown().await;
    reader.shutdown().await;
}

async fn status_until_long(a: &Console, b: &Console) {
    for _ in 0..600 {
        if all_connected(&a.status().await.unwrap()) && all_connected(&b.status().await.unwrap()) {
            return;
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    panic!(
        "not connected after 2 min: {:#?}",
        a.status().await.unwrap()
    );
}
