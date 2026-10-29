// SPDX-FileCopyrightText: 2026 William VITAL, FR (wvmscs)
// SPDX-License-Identifier: GPL-3.0-or-later

//! `--monitors` (A2): two trusted monitors on a local relay; the cache when
//! they cannot be read.

mod common;

use std::collections::BTreeSet;
use std::sync::Arc;

use bitchat_console::connector::Loopback;
use bitchat_console::monitors;
use bitchat_console::nostr::{Event, hex_encode, unix_now};
use common::*;

fn key(b: u8) -> k256::schnorr::SigningKey {
    k256::schnorr::SigningKey::from_bytes(&[b; 32]).unwrap()
}

#[tokio::test]
async fn reads_trusted_monitors_and_keeps_a_cache() {
    let relay = TestRelay::spawn("127.0.0.1:0").await.unwrap();
    let (m1, m2) = (key(1), key(2));
    for m in [&m1, &m2] {
        for (r, extra) in [
            ("wss://good.example", None),
            ("wss://auth.example", Some("auth")),
        ] {
            let mut tags = vec![vec!["d".into(), r.into()], vec!["g".into(), "u09tv".into()]];
            if let Some(x) = extra {
                tags.push(vec!["R".into(), x.into()]);
            }
            relay.publish(Event::signed(
                m,
                unix_now() - 60,
                30166,
                tags,
                String::new(),
            ));
        }
    }
    let trusted: BTreeSet<String> = [&m1, &m2]
        .iter()
        .map(|k| hex_encode(&k.verifying_key().to_bytes()))
        .collect();
    let dir = std::env::temp_dir().join(format!("bitchat-console-monitors-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let (d, list) = monitors::load(trusted.clone(), vec![relay.url()], Arc::new(Loopback), &dir)
        .await
        .expect("a directory");
    assert_eq!(
        list.relays()
            .iter()
            .map(|r| r.url.as_str())
            .collect::<Vec<_>>(),
        ["wss://good.example"]
    );
    assert!(!d.qualification.allows("wss://auth.example"));
    assert!(dir.join(monitors::CACHE_FILE).exists());
    // The monitors' relay is gone: the cached directory serves.
    relay.shutdown().await;
    let (_, cached) = monitors::load(
        trusted,
        vec!["ws://127.0.0.1:9".into()],
        Arc::new(Loopback),
        &dir,
    )
    .await
    .expect("the cache");
    assert_eq!(cached.len(), 1);
    std::fs::remove_dir_all(&dir).unwrap();
}
