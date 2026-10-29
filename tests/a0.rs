// SPDX-FileCopyrightText: 2026 William VITAL, FR (wvmscs)
// SPDX-License-Identifier: GPL-3.0-or-later

//! `--strategy A0`: a dead relay is replaced by the next one in the shared
//! order.

mod common;

use std::sync::Arc;

use bitchat_console::connector::Loopback;
use bitchat_console::console::{Console, Rebuild};
use common::*;
use geohash_relays::a0::SharedStrategy;
use geohash_relays::{RelayHealth, Strategy};

#[tokio::test]
async fn a_dead_relay_is_replaced() {
    // The nearest relay is down: nothing listens on its port.
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let dead = format!("ws://127.0.0.1:{port}");
    let open = TestRelay::spawn("127.0.0.1:0").await.unwrap();
    let (lat, lon) = geohash_relays::geohash::center("u09tv").unwrap();
    let csv = format!(
        "Relay URL,Latitude,Longitude\n{},{lat},{lon}\n{},{},{}\n",
        dead,
        open.url(),
        lat + 1.0,
        lon
    );
    // Read 1, write 2: only the nearest is read at first.
    let base = SharedStrategy::new(&csv).with_widths(2, 1);
    assert_eq!(base.read("u09tv").unwrap()[0].relay.url, dead);
    let rebuild: Rebuild = Arc::new(move |h: &RelayHealth| {
        Arc::new(base.clone().with_health(h.clone())) as Arc<dyn Strategy>
    });
    let mut config = config(&[open.url()], Arc::new(Loopback));
    config.strategy = rebuild(&RelayHealth::new());
    config.rebuild = Some(rebuild);
    let c = Console::start(config);
    c.set_area("u09tv", false).await.unwrap();
    let s = status_until(&c, "the dead relay replaced", |s| {
        let centre = &s.cells[0];
        centre
            .relays
            .iter()
            .any(|r| r.url == open.url() && r.state == "connected")
            && centre.relays.iter().all(|r| r.url != dead)
    })
    .await;
    assert_eq!(s.cells[0].geohash, "u09tv");
}
