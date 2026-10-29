// SPDX-FileCopyrightText: 2026 William VITAL, FR (wvmscs)
// SPDX-License-Identifier: GPL-3.0-or-later

//! `--qualify-probe` (A6) against local relays.

mod common;

use std::sync::Arc;

use bitchat_console::connector::Loopback;
use bitchat_console::qualify;
use common::*;
use geohash_relays::{Platform, RelayDirectory};

#[tokio::test]
async fn qualifies_the_relays_of_the_lists() {
    let (a, b) = (
        TestRelay::spawn("127.0.0.1:0").await.unwrap(),
        TestRelay::spawn("127.0.0.1:0").await.unwrap(),
    );
    let ios = RelayDirectory::parse(
        &format!("{},1,1\n{},1,1\n", a.url(), b.url()),
        Platform::Ios,
    );
    let android = RelayDirectory::parse(&format!("{},1,1\n", a.url()), Platform::Android);
    let relays = qualify::relays_of(&[&ios, &android]);
    assert_eq!(relays.len(), 2);
    let (p, complete) = qualify::run(relays, Arc::new(Loopback), true).await;
    assert!(complete);
    let q = p.qualification();
    assert!(q.verdicts.values().all(|v| v.usable() && v.probed), "{q:?}");
    let text = qualify::report(&p, true, complete);
    assert!(
        text.contains("2 relays, 0 with a NIP-11 document, 0 refused, probed by writing"),
        "{text}"
    );
    assert!(text.contains("R11"), "{text}");
}
