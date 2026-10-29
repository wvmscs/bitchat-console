// SPDX-FileCopyrightText: 2026 William VITAL, FR (wvmscs)
// SPDX-License-Identifier: GPL-3.0-or-later

//! `--republish` (A4): an iOS relay and an Android relay of the area are
//! connected by the console.

mod common;

use std::sync::Arc;
use std::time::Duration;

use bitchat_console::connector::Loopback;
use bitchat_console::nostr::{Event, unix_now};
use common::*;
use geohash_relays::{Platform, RelayDirectory};

fn list(url: &str, platform: Platform) -> RelayDirectory {
    RelayDirectory::parse(
        &format!("Relay URL,Latitude,Longitude\n{url},48.8566,2.3522\n"),
        platform,
    )
}

#[tokio::test]
async fn the_console_connects_the_ios_and_android_relays() {
    let (ios, android) = (
        TestRelay::spawn("127.0.0.1:0").await.unwrap(),
        TestRelay::spawn("127.0.0.1:0").await.unwrap(),
    );
    let c = Arc::new(console(&[ios.url()], Arc::new(Loopback)));
    c.set_area("u09tv", false).await.unwrap();
    bitchat_console::republish::spawn(
        c.clone(),
        list(&ios.url(), Platform::Ios),
        Some(list(&android.url(), Platform::Android)),
        Arc::new(Loopback),
    );
    tokio::time::sleep(Duration::from_millis(800)).await;
    let key = k256::schnorr::SigningKey::from_bytes(&[5u8; 32]).unwrap();
    let e = Event::signed(
        &key,
        unix_now(),
        20000,
        vec![vec!["g".into(), "u09tw".into()]],
        "hi".into(),
    );
    ios.publish(e.clone());
    for _ in 0..80 {
        if android.received(&e.id) > 0 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert_eq!(
        android.received(&e.id),
        1,
        "republished once to the Android relay"
    );
}
