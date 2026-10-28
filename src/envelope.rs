// SPDX-FileCopyrightText: 2026 William VITAL, FR (wvmscs)
// SPDX-License-Identifier: GPL-3.0-or-later

//! The BitChat private envelope (geohash DMs), exactly as the app writes
//! and checks it (`NostrProtocol.swift`): a kind 14 rumor (unsigned, no
//! tag) sealed in a kind 13 event signed by the sender, wrapped in a kind
//! 1059 event signed by a one-time key and tagged with the recipient.
//!
//! Not NIP-44 nor NIP-17 although it reuses their kind numbers: ECDH on
//! secp256k1 (the shared point, compressed, 33 bytes), HKDF-SHA256 with an
//! empty salt and the info `"nip44-v2"`, XChaCha20-Poly1305 with a random
//! 24-byte nonce, `v2:` + base64url (no padding) of nonce ‖ ciphertext ‖
//! tag. Seal and wrap dates are randomised by ±15 min.

use chacha20poly1305::aead::{Aead, KeyInit};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use hkdf::Hkdf;
use k256::elliptic_curve::sec1::ToEncodedPoint;
use k256::schnorr::SigningKey;
use k256::{ProjectivePoint, PublicKey};
use rand_core::{OsRng, RngCore};
use sha2::Sha256;

use crate::nostr::{Event, hex_decode, hex_encode, unix_now};

pub const KIND_RUMOR: u32 = 14;
pub const KIND_SEAL: u32 = 13;
pub const KIND_WRAP: u32 = 1059;
/// Largest outer ciphertext accepted (`maximumPrivateEnvelopeCiphertextBytes`).
pub const MAX_ENVELOPE_BYTES: usize = 64 * 1024;

/// Why an envelope does not open.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnvelopeError {
    /// Wrong outer shape: kind, recipient tag, signature, size.
    Malformed,
    /// Does not decrypt with this key.
    Undecryptable,
    /// Seal or rumor not as BitChat writes them, or sender mismatch.
    Forged,
}

/// An opened private message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Opened {
    pub content: String,
    /// The authenticated sender (the seal's signer).
    pub sender: String,
    /// Date of the rumor (the real one; the outer dates are randomised).
    pub created_at: i64,
}

pub fn b64url_encode(data: &[u8]) -> String {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = (b[0] as u32) << 16 | (b[1] as u32) << 8 | b[2] as u32;
        for i in 0..=chunk.len() {
            out.push(A[(n >> (18 - 6 * i)) as usize & 63] as char);
        }
    }
    out
}

pub fn b64url_decode(s: &str) -> Option<Vec<u8>> {
    let value = |c: u8| -> Option<u32> {
        Some(match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'-' | b'+' => 62,
            b'_' | b'/' => 63,
            _ => return None,
        } as u32)
    };
    let s = s.trim_end_matches('=').as_bytes();
    if s.len() % 4 == 1 {
        return None;
    }
    let mut out = Vec::with_capacity(s.len() * 3 / 4);
    for chunk in s.chunks(4) {
        let mut n = 0u32;
        for (i, &c) in chunk.iter().enumerate() {
            n |= value(c)? << (18 - 6 * i);
        }
        for i in 0..chunk.len() - 1 {
            out.push((n >> (16 - 8 * i)) as u8);
        }
    }
    Some(out)
}

/// The compressed shared point of `secret` and the x-only key `x` lifted
/// with `prefix` (2: even Y, 3: odd Y).
fn shared_point(secret: &SigningKey, x: &[u8], prefix: u8) -> Option<[u8; 33]> {
    let mut sec1 = vec![prefix];
    sec1.extend_from_slice(x);
    let public = PublicKey::from_sec1_bytes(&sec1).ok()?;
    let scalar = *secret.as_nonzero_scalar().as_ref();
    let point = (ProjectivePoint::from(*public.as_affine()) * scalar).to_affine();
    point.to_encoded_point(true).as_bytes().try_into().ok()
}

/// `derivePrivateEnvelopeKey`: HKDF-SHA256, empty salt, info "nip44-v2".
pub fn envelope_key(shared: &[u8]) -> [u8; 32] {
    let mut key = [0u8; 32];
    Hkdf::<Sha256>::new(Some(&[]), shared)
        .expand(b"nip44-v2", &mut key)
        .expect("32 bytes is a valid HKDF length");
    key
}

/// `v2:` + base64url(nonce ‖ ciphertext ‖ tag) with a given key and nonce.
pub fn seal_with(key: &[u8; 32], nonce: &[u8; 24], plaintext: &[u8]) -> String {
    let cipher = XChaCha20Poly1305::new(key.into());
    let ct = cipher
        .encrypt(&XNonce::from(*nonce), plaintext)
        .expect("in-memory encryption cannot fail");
    let mut combined = nonce.to_vec();
    combined.extend_from_slice(&ct);
    format!("v2:{}", b64url_encode(&combined))
}

/// Encrypts to `recipient` (x-only hex), the recipient key lifted with an
/// even Y, as the app does.
pub fn encrypt(plaintext: &str, recipient: &str, sender: &SigningKey) -> Option<String> {
    let x = hex_decode(recipient).filter(|x| x.len() == 32)?;
    let key = envelope_key(&shared_point(sender, &x, 2)?);
    let mut nonce = [0u8; 24];
    OsRng.fill_bytes(&mut nonce);
    Some(seal_with(&key, &nonce, plaintext.as_bytes()))
}

/// Decrypts from `sender` (x-only hex), trying an even then an odd Y.
pub fn decrypt(ciphertext: &str, sender: &str, recipient: &SigningKey) -> Option<String> {
    if ciphertext.len() > MAX_ENVELOPE_BYTES {
        return None;
    }
    let data = b64url_decode(ciphertext.strip_prefix("v2:")?)?;
    if data.len() <= 24 + 16 {
        return None;
    }
    let x = hex_decode(sender).filter(|x| x.len() == 32)?;
    let (nonce, ct) = data.split_at(24);
    let nonce = XNonce::from(<[u8; 24]>::try_from(nonce).ok()?);
    for prefix in [2u8, 3] {
        let Some(shared) = shared_point(recipient, &x, prefix) else {
            continue;
        };
        let cipher = XChaCha20Poly1305::new((&envelope_key(&shared)).into());
        if let Ok(pt) = cipher.decrypt(&nonce, ct) {
            return String::from_utf8(pt).ok();
        }
    }
    None
}

fn randomized(now: i64) -> i64 {
    now + (OsRng.next_u64() % 1801) as i64 - 900
}

/// A private message to `recipient` (x-only hex) from `sender`: the outer
/// kind 1059 event to publish.
pub fn create(content: &str, recipient: &str, sender: &SigningKey) -> Option<Event> {
    let now = unix_now();
    let sender_hex = hex_encode(&sender.verifying_key().to_bytes());
    // The app's rumor: empty id, no signature, no tag.
    let rumor = Event {
        id: String::new(),
        pubkey: sender_hex,
        created_at: now,
        kind: KIND_RUMOR,
        tags: Vec::new(),
        content: content.to_owned(),
        sig: None,
    };
    let sealed = encrypt(&rumor.to_json(), recipient, sender)?;
    let seal = Event::signed(sender, randomized(now), KIND_SEAL, Vec::new(), sealed);
    let mut wrap_secret = [0u8; 32];
    let wrap_key = loop {
        OsRng.fill_bytes(&mut wrap_secret);
        if let Ok(k) = SigningKey::from_bytes(&wrap_secret) {
            break k;
        }
    };
    let wrapped = encrypt(&seal.to_json(), recipient, &wrap_key)?;
    Some(Event::signed(
        &wrap_key,
        randomized(now),
        KIND_WRAP,
        vec![vec!["p".to_owned(), recipient.to_owned()]],
        wrapped,
    ))
}

/// Opens a kind 1059 event addressed to `me`, with every check of the app
/// (`decryptPrivateMessage`).
pub fn open(wrap: &Event, me: &SigningKey) -> Result<Opened, EnvelopeError> {
    let my_hex = hex_encode(&me.verifying_key().to_bytes());
    if wrap.content.len() > MAX_ENVELOPE_BYTES
        || wrap.kind != KIND_WRAP
        || wrap.tags != [vec!["p".to_owned(), my_hex.clone()]]
        || wrap.verify().is_err()
    {
        return Err(EnvelopeError::Malformed);
    }
    let seal_json = decrypt(&wrap.content, &wrap.pubkey, me).ok_or(EnvelopeError::Undecryptable)?;
    let seal: Event = serde_json::from_str(&seal_json).map_err(|_| EnvelopeError::Forged)?;
    if seal.kind != KIND_SEAL || !seal.tags.is_empty() || seal.verify().is_err() {
        return Err(EnvelopeError::Forged);
    }
    let rumor_json =
        decrypt(&seal.content, &seal.pubkey, me).ok_or(EnvelopeError::Undecryptable)?;
    let rumor: Event = serde_json::from_str(&rumor_json).map_err(|_| EnvelopeError::Forged)?;
    // Released iOS: no inner tag; current Android: the recipient's `p` only.
    let tags_ok = rumor.tags.is_empty() || rumor.tags == [vec!["p".to_owned(), my_hex]];
    if rumor.kind != KIND_RUMOR || !tags_ok || rumor.sig.is_some() || rumor.pubkey != seal.pubkey {
        return Err(EnvelopeError::Forged);
    }
    Ok(Opened {
        content: rumor.content,
        sender: seal.pubkey,
        created_at: rumor.created_at,
    })
}
