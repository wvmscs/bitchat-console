// SPDX-FileCopyrightText: 2026 William VITAL, FR (wvmscs)
// SPDX-License-Identifier: GPL-3.0-or-later

//! Keys: same derivation as the app (vectors from CryptoKit, see
//! `tests/vectors/identity.swift`), files only the user can read.

use bitchat_console::Error;
use bitchat_console::identity::{Keys, NOISE_FILE, NoiseIdentity, SEED_FILE, Seed};
use k256::schnorr::SigningKey;

fn seed() -> Seed {
    Seed::from_bytes(std::array::from_fn(|i| i as u8))
}

fn bytes(hex: &str) -> [u8; 32] {
    std::array::from_fn(|i| u8::from_str_radix(&hex[2 * i..2 * i + 2], 16).unwrap())
}

#[test]
fn cell_keys_are_the_apps() {
    // HMAC-SHA256(seed, geohash ‖ 0) from CryptoKit: the first candidate is
    // valid, so it is the key.
    for (geohash, hmac0) in [
        (
            "u09tv",
            "4e66e69b1007c993f4222a2e2769a7171745e985a0d6d0fbe15d354fa1259bf1",
        ),
        (
            "u09tvw",
            "77d45c48b76c5f963d0fe364016ed3884052210546de43a48ca85a8717700ddc",
        ),
        (
            "9q8yy",
            "acc10d6d715537e9b172fa3528d1bd855819c870f61012e194d221d4cb81cc9d",
        ),
    ] {
        let key = seed().cell(geohash).unwrap();
        let expected = SigningKey::from_bytes(&bytes(hmac0)).unwrap();
        assert_eq!(
            key.signing_key().to_bytes(),
            expected.to_bytes(),
            "{geohash}"
        );
        assert_eq!(key.pubkey_hex().len(), 64);
        assert_eq!(key.suffix(), key.pubkey_hex()[60..]);
    }
    // One key per cell, the same every time.
    let s = seed();
    assert_ne!(
        s.cell("u09tv").unwrap().pubkey_hex(),
        s.cell("u09tw").unwrap().pubkey_hex()
    );
    assert_eq!(
        s.cell("u09tv").unwrap().pubkey_hex(),
        s.cell("u09tv").unwrap().pubkey_hex()
    );
    assert!(matches!(s.cell("u0a"), Err(Error::InvalidGeohash(_))));
    // The secret never shows in a debug print.
    let shown = format!("{:?}", s.cell("u09tv").unwrap());
    assert!(!shown.contains("4e66e69b"), "{shown}");
}

#[test]
fn noise_identity_is_the_apps() {
    let raw: [u8; 32] = std::array::from_fn(|i| 32 + i as u8);
    let id = NoiseIdentity::from_bytes(raw, raw);
    assert_eq!(
        id.static_public(),
        bytes("358072d6365880d1aeea329adf9121383851ed21a28e3b75e965d0d2cd166254")
    );
    assert_eq!(
        id.fingerprint(),
        "a0289810032eb417274eebc4a38e6899b935321d53554e32ec6322f0fa1e724e"
    );
    assert_eq!(id.peer_id(), "a0289810032eb417");
    assert_eq!(
        id.signing_public(),
        bytes("29acbae141bccaf0b22e1a94d34d0bc7361e526d0bfe12c89794bc9322966dd7")
    );
}

#[cfg(unix)]
#[test]
fn key_files_are_private_and_stable() {
    use std::os::unix::fs::PermissionsExt;
    let dir = std::env::temp_dir().join(format!("bitchat-console-keys-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let first = Keys::load_or_create(&dir).unwrap();
    let mode = |f: &str| std::fs::metadata(dir.join(f)).unwrap().permissions().mode() & 0o777;
    assert_eq!((mode(SEED_FILE), mode(NOISE_FILE)), (0o600, 0o600));
    assert_eq!(
        std::fs::metadata(&dir).unwrap().permissions().mode() & 0o777,
        0o700
    );
    let again = Keys::load_or_create(&dir).unwrap();
    assert_eq!(
        first.seed.cell("u09tv").unwrap().pubkey_hex(),
        again.seed.cell("u09tv").unwrap().pubkey_hex()
    );
    assert_eq!(first.noise.fingerprint(), again.noise.fingerprint());

    // Readable by others: refused.
    std::fs::set_permissions(dir.join(SEED_FILE), std::fs::Permissions::from_mode(0o644)).unwrap();
    assert!(matches!(
        Keys::load_or_create(&dir),
        Err(Error::InsecureKeyFile(_))
    ));
    // Wrong size: refused.
    std::fs::write(dir.join(SEED_FILE), b"short").unwrap();
    std::fs::set_permissions(dir.join(SEED_FILE), std::fs::Permissions::from_mode(0o600)).unwrap();
    assert!(matches!(
        Keys::load_or_create(&dir),
        Err(Error::BadKeyFile(_))
    ));
    std::fs::remove_dir_all(&dir).unwrap();
}
