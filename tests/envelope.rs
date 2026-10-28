// SPDX-FileCopyrightText: 2026 William VITAL, FR (wvmscs)
// SPDX-License-Identifier: GPL-3.0-or-later

//! The BitChat private envelope: vectors from the app's own XChaCha code
//! (`tests/vectors/envelope.swift`), round trips, and the app's refusals.

use bitchat_console::envelope::*;
use bitchat_console::identity::Seed;
use bitchat_console::nostr::{Event, unix_now};

fn keys() -> (k256::schnorr::SigningKey, k256::schnorr::SigningKey) {
    let seed = Seed::from_bytes([9; 32]);
    (
        seed.cell("u09tv").unwrap().signing_key().clone(),
        seed.cell("u09ty").unwrap().signing_key().clone(),
    )
}

fn hex(k: &k256::schnorr::SigningKey) -> String {
    k.verifying_key()
        .to_bytes()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

#[test]
fn same_bytes_as_the_app() {
    let mut shared = vec![0x02];
    shared.extend([0x11; 32]);
    let key = envelope_key(&shared);
    let key_hex: String = key.iter().map(|b| format!("{b:02x}")).collect();
    assert_eq!(
        key_hex,
        "7bdd72117159a311a017f9ceb68623c9fa320427e2fb71c0cbd04d326a78aaa7"
    );
    let nonce: [u8; 24] = std::array::from_fn(|i| i as u8);
    assert_eq!(
        seal_with(&key, &nonce, b"bitchat1:hello"),
        "v2:AAECAwQFBgcICQoLDA0ODxAREhMUFRYXBmW1gf9WnO9huw5DeW5AbKgoCovAoJ1J2k1yIq3C"
    );
}

#[test]
fn base64url_without_padding() {
    for n in 0..20 {
        let data: Vec<u8> = (0..n).map(|i| (i * 37) as u8).collect();
        let s = b64url_encode(&data);
        assert!(!s.contains('=') && !s.contains('+') && !s.contains('/'));
        assert_eq!(b64url_decode(&s).unwrap(), data);
    }
}

#[test]
fn round_trip_between_two_cells() {
    let (alice, bob) = keys();
    let wrap = create("bitchat1:salut", &hex(&bob), &alice).unwrap();
    assert_eq!(wrap.kind, KIND_WRAP);
    assert_eq!(wrap.tags, [vec!["p".to_owned(), hex(&bob)]]);
    assert_ne!(wrap.pubkey, hex(&alice), "the outer key is a one-time key");
    assert!((wrap.created_at - unix_now()).abs() <= 901);
    assert!(wrap.content.starts_with("v2:"));
    let opened = open(&wrap, &bob).unwrap();
    assert_eq!(opened.content, "bitchat1:salut");
    assert_eq!(opened.sender, hex(&alice));
    assert!((opened.created_at - unix_now()).abs() <= 2);
    // Not for alice.
    assert_eq!(open(&wrap, &alice), Err(EnvelopeError::Malformed));
}

#[test]
fn tampering_and_forgery_are_refused() {
    let (alice, bob) = keys();
    let mut wrap = create("bitchat1:x", &hex(&bob), &alice).unwrap();
    wrap.content.push('A');
    assert_eq!(
        open(&wrap, &bob),
        Err(EnvelopeError::Malformed),
        "signature broken"
    );

    // A seal signed by alice around a rumor that claims to be from bob.
    let now = unix_now();
    let rumor = Event {
        id: String::new(),
        pubkey: hex(&bob),
        created_at: now,
        kind: KIND_RUMOR,
        tags: vec![],
        content: "bitchat1:forged".into(),
        sig: None,
    };
    let seal = Event::signed(
        &alice,
        now,
        KIND_SEAL,
        vec![],
        encrypt(&rumor.to_json(), &hex(&bob), &alice).unwrap(),
    );
    let one_time = k256::schnorr::SigningKey::from_bytes(&[5; 32]).unwrap();
    let wrap = Event::signed(
        &one_time,
        now,
        KIND_WRAP,
        vec![vec!["p".into(), hex(&bob)]],
        encrypt(&seal.to_json(), &hex(&bob), &one_time).unwrap(),
    );
    assert_eq!(open(&wrap, &bob), Err(EnvelopeError::Forged));
}

#[test]
fn the_android_shape_is_accepted() {
    // Android puts the recipient's `p` tag on the rumor.
    let (alice, bob) = keys();
    let now = unix_now();
    let rumor = Event {
        id: String::new(),
        pubkey: hex(&alice),
        created_at: now,
        kind: KIND_RUMOR,
        tags: vec![vec!["p".into(), hex(&bob)]],
        content: "bitchat1:android".into(),
        sig: None,
    };
    let seal = Event::signed(
        &alice,
        now,
        KIND_SEAL,
        vec![],
        encrypt(&rumor.to_json(), &hex(&bob), &alice).unwrap(),
    );
    let one_time = k256::schnorr::SigningKey::from_bytes(&[6; 32]).unwrap();
    let wrap = Event::signed(
        &one_time,
        now,
        KIND_WRAP,
        vec![vec!["p".into(), hex(&bob)]],
        encrypt(&seal.to_json(), &hex(&bob), &one_time).unwrap(),
    );
    assert_eq!(open(&wrap, &bob).unwrap().content, "bitchat1:android");
}
