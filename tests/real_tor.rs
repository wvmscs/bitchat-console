//! Real network, through Tor, on the user's relay only
//! (`wss://relay.famillevital.club`, authorised for tests) with throwaway
//! seeds. Not run by default:
//!
//!   cargo test --features arti --test real_tor -- --ignored --nocapture
//!   BITCHAT_TOR_SOCKS=127.0.0.1:9050 cargo test --test real_tor -- --ignored --nocapture
//!
//! With `BITCHAT_TOR_SOCKS`, a Tor daemon's SOCKS port is used instead of Arti.

mod common;

use std::sync::Arc;
use std::time::{Duration, Instant};

use bitchat_console::connector::{Connector, Socks5};
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
    let started = Instant::now();
    let tor = tor().await;
    println!("tor ready in {:.1} s", started.elapsed().as_secs_f64());

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
        // Private messages on the user's relay too, never on the defaults.
        c.dm_relays = vec![OWN_RELAY.to_owned()];
        let mut n = [0u8; 32];
        rand_core::RngCore::fill_bytes(&mut rand_core::OsRng, &mut n);
        c.noise = Some(Arc::new(
            bitchat_console::identity::NoiseIdentity::from_bytes(n, n),
        ));
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

    // A private message from the writer to the key the reader used in that
    // cell, with its delivery receipt.
    let reader_cell_key = reader_seed_key(&reader).await;
    let t = Instant::now();
    let dm = writer
        .send_dm(
            &reader_cell_key,
            "u09tvw",
            "bitchat-console lot 6: DM through Tor",
        )
        .await
        .unwrap();
    let mut received = false;
    for _ in 0..300 {
        if reader.dms().await.unwrap().iter().any(|d| d.id == dm.id) {
            received = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(received, "the DM never arrived");
    println!("DM received in {:.2} s", t.elapsed().as_secs_f64());
    let mut status = None;
    for _ in 0..300 {
        status = writer
            .dms()
            .await
            .unwrap()
            .iter()
            .find(|d| d.id == dm.id)
            .and_then(|d| d.status);
        if status == Some(bitchat_console::console::DmStatus::Delivered) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    println!(
        "DM status after {:.2} s: {status:?}",
        t.elapsed().as_secs_f64()
    );
    assert_eq!(status, Some(bitchat_console::console::DmStatus::Delivered));

    // A group between the two consoles, through Tor.
    use bitchat_console::console::GroupOp;
    let t = Instant::now();
    writer
        .group(GroupOp::Create {
            name: "lot 10".into(),
            nickname: "w".into(),
        })
        .await
        .unwrap();
    let gid = writer.groups().await.unwrap().groups[0].id.clone();
    writer
        .group(GroupOp::Invite {
            group: gid.clone(),
            peer: reader_cell_key.clone(),
            cell: "u09tvw".into(),
        })
        .await
        .unwrap();
    let mut invited = false;
    for _ in 0..300 {
        if !reader.groups().await.unwrap().invitations.is_empty() {
            invited = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(invited, "no invitation");
    reader
        .group(GroupOp::Accept {
            group: gid.clone(),
            nickname: "r".into(),
        })
        .await
        .unwrap();
    let mut joined = false;
    for _ in 0..600 {
        if reader.groups().await.unwrap().groups.len() == 1 {
            joined = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(joined, "never joined");
    println!(
        "group joined through Tor in {:.2} s",
        t.elapsed().as_secs_f64()
    );
    let t = Instant::now();
    writer
        .group(GroupOp::Send {
            group: gid.clone(),
            text: "bitchat-console lot 10: group through Tor".into(),
            nickname: "w".into(),
        })
        .await
        .unwrap();
    let mut got_group = false;
    for _ in 0..600 {
        if reader
            .groups()
            .await
            .unwrap()
            .messages
            .iter()
            .any(|m| m.content.contains("lot 10"))
        {
            got_group = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(got_group, "group message never arrived");
    println!(
        "group message received in {:.2} s",
        t.elapsed().as_secs_f64()
    );
    writer.group(GroupOp::Destroy { group: gid }).await.unwrap();
    writer.shutdown().await;
    reader.shutdown().await;
}

/// A Tor daemon if `BITCHAT_TOR_SOCKS` names its SOCKS port, else Arti.
async fn tor() -> Arc<dyn Connector> {
    if let Ok(addr) = std::env::var("BITCHAT_TOR_SOCKS") {
        let s = Socks5::new(addr.parse().expect("BITCHAT_TOR_SOCKS: address:port"));
        s.check()
            .await
            .expect("no SOCKS proxy at BITCHAT_TOR_SOCKS");
        println!("tor daemon at {addr}");
        return Arc::new(s);
    }
    arti().await
}

#[cfg(feature = "arti")]
async fn arti() -> Arc<dyn Connector> {
    let dir = std::env::temp_dir().join("bitchat-console-arti-test");
    println!("arti");
    Arc::new(bitchat_console::arti::Arti::bootstrap(&dir).await.unwrap())
}

#[cfg(not(feature = "arti"))]
async fn arti() -> Arc<dyn Connector> {
    panic!("set BITCHAT_TOR_SOCKS, or build with --features arti")
}

/// The reader's key in `u09tvw`: it sends a message there, whose author is it.
async fn reader_seed_key(reader: &Console) -> String {
    reader
        .send("bitchat-console lot 6: reader here", Some("u09tvw"), None)
        .await
        .unwrap()
        .pubkey
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
