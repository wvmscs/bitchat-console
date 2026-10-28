// SPDX-FileCopyrightText: 2026 William VITAL, FR (wvmscs)
// SPDX-License-Identifier: GPL-3.0-or-later

//! Prints a group state and a group message made by the console, hex, one
//! per line, for `tests/vectors/groups.swift` to check with the app's code.

use bitchat_console::groups::{GroupState, Member, seal_message};
use bitchat_console::identity::hex;
use ed25519_dalek::SigningKey;

fn main() {
    let ask = SigningKey::from_bytes(&[1; 32]);
    let bsk = SigningKey::from_bytes(&[2; 32]);
    let m = |n: u8, sk: &SigningKey, nick: &str| Member {
        fingerprint: hex(&[n; 32]),
        signing_key: hex(&sk.verifying_key().to_bytes()),
        nickname: nick.into(),
    };
    let (alice, bob) = (m(0xa1, &ask, "alice"), m(0xb2, &bsk, "bob 😀"));
    let state = GroupState::signed(
        [7; 16],
        "les amis",
        [9; 32],
        3,
        vec![alice.clone(), bob.clone()],
        &alice.fingerprint,
        &ask,
        vec![(bob.fingerprint.clone(), "ab".repeat(32))],
    )
    .unwrap();
    println!("{}", hex(&state.encode().unwrap()));
    let env = seal_message(
        &state,
        "8F0B7E1C-0000-4000-8000-000000000001",
        "bob 😀",
        &bsk,
        1_800_000_000_123,
        "salut à tous",
    )
    .unwrap();
    println!("{}", hex(&env));
    println!("{}", hex(&state.key));
}
