// SPDX-FileCopyrightText: 2026 William VITAL, FR (wvmscs)
// SPDX-License-Identifier: GPL-3.0-or-later

//! `--federation-check` (A5): a member that lacks a note is found.

mod common;

use std::sync::Arc;

use bitchat_console::connector::Loopback;
use bitchat_console::federation;
use bitchat_console::nostr::{Event, unix_now};
use common::*;

#[tokio::test]
async fn finds_what_a_member_lacks() {
    let (a, b, down) = (
        TestRelay::spawn("127.0.0.1:0").await.unwrap(),
        TestRelay::spawn("127.0.0.1:0").await.unwrap(),
        "ws://127.0.0.1:9".to_owned(),
    );
    let key = k256::schnorr::SigningKey::from_bytes(&[4u8; 32]).unwrap();
    let note = |text: &str| {
        Event::signed(
            &key,
            unix_now() - 30,
            1,
            vec![vec!["g".into(), "u09tvw".into()]],
            text.into(),
        )
    };
    let (n1, n2) = (note("both"), note("only a"));
    a.publish(n1.clone());
    b.publish(n1);
    a.publish(n2.clone());
    let members = vec![a.url(), b.url(), down.clone()];
    let (omissions, failed) =
        federation::check(&members, &["u09tvw".into()], Arc::new(Loopback)).await;
    assert!(omissions[&a.url()].is_empty());
    assert_eq!(omissions[&b.url()].iter().collect::<Vec<_>>(), [&n2.id]);
    assert!(failed.contains(&down));
    let text = federation::report(&omissions, &failed);
    assert!(text.contains("not reachable"), "{text}");
}
