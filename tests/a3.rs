// SPDX-FileCopyrightText: 2026 William VITAL, FR (wvmscs)
// SPDX-License-Identifier: GPL-3.0-or-later

//! `--announce-relays` (A3): two consoles announce their relay in their
//! presences, on a relay they share with a third; the third reads it too.

mod common;

use std::sync::Arc;
use std::time::Duration;

use bitchat_console::connector::Loopback;
use bitchat_console::console::Console;
use bitchat_console::identity::Seed;
use common::*;

fn announcing(urls: &[String], seed: u8) -> Console {
    let mut c = config(urls, Arc::new(Loopback));
    c.seed = Arc::new(Seed::from_bytes([seed; 32]));
    c.announce = true;
    c.presence = true;
    c.presence_every = (Duration::from_millis(200), Duration::from_millis(300));
    Console::start(c)
}

#[tokio::test]
async fn announced_relays_are_read() {
    let (shared, a, b) = (
        TestRelay::spawn("127.0.0.1:0").await.unwrap(),
        TestRelay::spawn("127.0.0.1:0").await.unwrap(),
        TestRelay::spawn("127.0.0.1:0").await.unwrap(),
    );
    let (c1, c2) = (
        announcing(&[shared.url(), a.url()], 1),
        announcing(&[shared.url(), a.url()], 2),
    );
    let c3 = announcing(&[shared.url(), b.url()], 3);
    for c in [&c1, &c2, &c3] {
        c.set_area("u09tv", false).await.unwrap();
    }
    let s = status_until(&c3, "the announced relay read", |s| {
        s.cells.first().is_some_and(|c| {
            c.relays
                .iter()
                .any(|r| r.url == a.url() && r.state == "connected")
        })
    })
    .await;
    assert!(s.cells[0].relays.iter().any(|r| r.url == b.url()));
}
