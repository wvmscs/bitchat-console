// SPDX-FileCopyrightText: 2026 William VITAL, FR (wvmscs)
// SPDX-License-Identifier: GPL-3.0-or-later

//! NIP-13 proof of work for channel messages, as iOS mines it
//! (`bitchat/Nostr/NostrPoW.swift`): a `["nonce", <16 hex>, <target>]` tag
//! appended last, from a random start; after 2 s the committed target is
//! halved (down to 0), so a message is never blocked.

use std::time::{Duration, Instant};

use rand_core::{OsRng, RngCore};

use crate::nostr::Event;

/// iOS target (`NostrPoW.targetBits`).
pub const TARGET_BITS: u32 = 8;
/// Time allowed at the first target (`miningTimeCap`).
pub const TIME_CAP: Duration = Duration::from_secs(2);

/// Leading zero bits of a hex id.
pub fn leading_zero_bits(id_hex: &str) -> u32 {
    let mut bits = 0;
    for c in id_hex.chars() {
        let v = c.to_digit(16).unwrap_or(0);
        if v == 0 {
            bits += 4;
        } else {
            return bits + v.leading_zeros() - 28;
        }
    }
    bits
}

/// The tags with a nonce tag that makes the event id reach `target` bits
/// (or a lower committed target if time runs out).
pub fn mine(
    pubkey: &str,
    created_at: i64,
    kind: u32,
    tags: &[Vec<String>],
    content: &str,
    target: u32,
) -> Vec<Vec<String>> {
    let mut target = target.min(256);
    let mut budget = TIME_CAP;
    loop {
        let deadline = Instant::now() + budget;
        let mut nonce = OsRng.next_u64();
        let mut attempts = 0u32;
        loop {
            let mut t = tags.to_vec();
            t.push(vec![
                "nonce".into(),
                format!("{nonce:016x}"),
                target.to_string(),
            ]);
            let e = Event::new_unsigned(pubkey, created_at, kind, t, content.to_owned());
            if leading_zero_bits(&e.id) >= target {
                return e.tags;
            }
            nonce = nonce.wrapping_add(1);
            attempts += 1;
            if attempts.is_multiple_of(1024) && Instant::now() >= deadline {
                break;
            }
        }
        target /= 2;
        budget = Duration::from_millis(500);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_bits() {
        assert_eq!(leading_zero_bits("ff"), 0);
        assert_eq!(leading_zero_bits("0f"), 4);
        assert_eq!(leading_zero_bits("00f"), 8);
        assert_eq!(leading_zero_bits("001"), 11);
        assert_eq!(leading_zero_bits("7"), 1);
    }

    #[test]
    fn mined_tag_is_last_and_reaches_the_target() {
        let base = vec![vec!["g".to_owned(), "u09tv".to_owned()]];
        let tags = mine(
            &"a".repeat(64),
            1_800_000_000,
            20000,
            &base,
            "hi",
            TARGET_BITS,
        );
        assert_eq!(tags[0], base[0]);
        let nonce = tags.last().unwrap();
        assert_eq!((nonce[0].as_str(), nonce[2].as_str()), ("nonce", "8"));
        assert_eq!(nonce[1].len(), 16);
        let e = Event::new_unsigned(&"a".repeat(64), 1_800_000_000, 20000, tags, "hi".into());
        assert!(leading_zero_bits(&e.id) >= 8);
    }
}
