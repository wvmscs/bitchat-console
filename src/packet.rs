//! BitChat binary packets, as the app carries them inside Nostr private
//! messages (`bitchat1:` + base64url): `BinaryProtocol.swift` and
//! `NostrEmbeddedBitChat.swift` of BitFoundation / the iOS app.
//!
//! Encoding writes version 1 like the app, uncompressed (the app reads both;
//! inside the envelope nobody else sees the difference), PKCS#7-padded to
//! 256/512/1024/2048 bytes like `MessagePadding`. Decoding reads versions 1
//! and 2, routes, and raw-DEFLATE compressed payloads.

use rand_core::{OsRng, RngCore};

/// `MessageType.noiseEncrypted`: every private payload.
pub const TYPE_NOISE_ENCRYPTED: u8 = 0x11;

/// First byte of a `noiseEncrypted` payload (`NoisePayloadType`).
pub mod payload {
    pub const PRIVATE_MESSAGE: u8 = 0x01;
    pub const READ_RECEIPT: u8 = 0x02;
    pub const DELIVERED: u8 = 0x03;
    pub const GROUP_INVITE: u8 = 0x06;
    pub const GROUP_KEY_UPDATE: u8 = 0x07;
}

const FLAG_RECIPIENT: u8 = 0x01;
const FLAG_SIGNATURE: u8 = 0x02;
const FLAG_COMPRESSED: u8 = 0x04;
const FLAG_ROUTE: u8 = 0x08;
const ID_SIZE: usize = 8;
const SIGNATURE_SIZE: usize = 64;
/// Largest payload accepted, compressed or not.
pub const MAX_PAYLOAD: usize = 64 * 1024;
/// Largest DEFLATE ratio accepted (decompression bombs).
const MAX_DEFLATE_RATIO: usize = 50;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Packet {
    pub version: u8,
    pub kind: u8,
    pub ttl: u8,
    /// Milliseconds since the Unix epoch.
    pub timestamp: u64,
    pub sender: [u8; ID_SIZE],
    pub recipient: Option<[u8; ID_SIZE]>,
    pub payload: Vec<u8>,
    pub signature: Option<Vec<u8>>,
}

impl Packet {
    /// A `noiseEncrypted` packet as the app embeds it in a geohash DM: no
    /// recipient, a random sender ID per envelope (`unlinkableSenderID`),
    /// TTL 7, no signature.
    pub fn embedded(payload: Vec<u8>, timestamp_ms: u64) -> Packet {
        let mut sender = [0u8; ID_SIZE];
        OsRng.fill_bytes(&mut sender);
        Packet {
            version: 1,
            kind: TYPE_NOISE_ENCRYPTED,
            ttl: 7,
            timestamp: timestamp_ms,
            sender,
            recipient: None,
            payload,
            signature: None,
        }
    }

    /// Version 1 encoding, uncompressed, padded.
    pub fn encode(&self) -> Vec<u8> {
        let mut d = vec![1, self.kind, self.ttl];
        d.extend_from_slice(&self.timestamp.to_be_bytes());
        let mut flags = 0;
        if self.recipient.is_some() {
            flags |= FLAG_RECIPIENT;
        }
        if self.signature.is_some() {
            flags |= FLAG_SIGNATURE;
        }
        d.push(flags);
        d.extend_from_slice(&(self.payload.len().min(u16::MAX as usize) as u16).to_be_bytes());
        d.extend_from_slice(&self.sender);
        if let Some(r) = self.recipient {
            d.extend_from_slice(&r);
        }
        d.extend_from_slice(&self.payload);
        if let Some(sig) = &self.signature {
            let mut s = sig.clone();
            s.resize(SIGNATURE_SIZE, 0);
            d.extend_from_slice(&s);
        }
        pad(d)
    }

    /// Decodes a packet, padded or not.
    pub fn decode(data: &[u8]) -> Option<Packet> {
        decode_core(data).or_else(|| {
            let unpadded = unpad(data)?;
            decode_core(unpadded)
        })
    }
}

/// `MessagePadding`: up to the smallest block of 256, 512, 1024 or 2048
/// that fits the data plus 16 bytes, if at most 255 bytes are needed.
fn pad(mut d: Vec<u8>) -> Vec<u8> {
    let total = d.len() + 16;
    if let Some(&block) = [256usize, 512, 1024, 2048].iter().find(|&&b| total <= b) {
        let n = block - d.len();
        if (1..=255).contains(&n) {
            d.extend(std::iter::repeat_n(n as u8, n));
        }
    }
    d
}

fn unpad(d: &[u8]) -> Option<&[u8]> {
    let n = *d.last()? as usize;
    if n == 0 || n > d.len() || !d[d.len() - n..].iter().all(|&b| b as usize == n) {
        return None;
    }
    Some(&d[..d.len() - n])
}

fn decode_core(raw: &[u8]) -> Option<Packet> {
    let mut r = Reader { d: raw, at: 0 };
    let version = r.u8()?;
    if version != 1 && version != 2 {
        return None;
    }
    let kind = r.u8()?;
    let ttl = r.u8()?;
    let timestamp = u64::from_be_bytes(r.take(8)?.try_into().ok()?);
    let flags = r.u8()?;
    let len = if version == 2 {
        r.u32()? as usize
    } else {
        r.u16()? as usize
    };
    if len > MAX_PAYLOAD * 2 {
        return None;
    }
    let sender: [u8; ID_SIZE] = r.take(ID_SIZE)?.try_into().ok()?;
    let recipient = if flags & FLAG_RECIPIENT != 0 {
        Some(r.take(ID_SIZE)?.try_into().ok()?)
    } else {
        None
    };
    if version >= 2 && flags & FLAG_ROUTE != 0 {
        let hops = r.u8()? as usize;
        r.take(hops * ID_SIZE)?;
    }
    let payload = if flags & FLAG_COMPRESSED != 0 {
        let size_bytes = if version == 2 { 4 } else { 2 };
        if len < size_bytes {
            return None;
        }
        let original = if version == 2 {
            r.u32()? as usize
        } else {
            r.u16()? as usize
        };
        let compressed = r.take(len - size_bytes)?;
        if compressed.is_empty()
            || original > MAX_PAYLOAD
            || original > compressed.len() * MAX_DEFLATE_RATIO
        {
            return None;
        }
        let out = miniz_oxide::inflate::decompress_to_vec_with_limit(compressed, original).ok()?;
        (out.len() == original).then_some(out)?
    } else {
        if len > MAX_PAYLOAD {
            return None;
        }
        r.take(len)?.to_vec()
    };
    let signature = if flags & FLAG_SIGNATURE != 0 {
        Some(r.take(SIGNATURE_SIZE)?.to_vec())
    } else {
        None
    };
    Some(Packet {
        version,
        kind,
        ttl,
        timestamp,
        sender,
        recipient,
        payload,
        signature,
    })
}

struct Reader<'a> {
    d: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        let s = self.d.get(self.at..self.at.checked_add(n)?)?;
        self.at += n;
        Some(s)
    }
    fn u8(&mut self) -> Option<u8> {
        Some(self.take(1)?[0])
    }
    fn u16(&mut self) -> Option<u16> {
        Some(u16::from_be_bytes(self.take(2)?.try_into().ok()?))
    }
    fn u32(&mut self) -> Option<u32> {
        Some(u32::from_be_bytes(self.take(4)?.try_into().ok()?))
    }
}

/// `PrivateMessagePacket`: TLV `0x00` message ID, `0x01` content, each at
/// most 255 bytes of UTF-8.
pub fn encode_private_message(message_id: &str, content: &str) -> Option<Vec<u8>> {
    let (id, text) = (message_id.as_bytes(), content.as_bytes());
    if id.len() > 255 || text.len() > 255 {
        return None;
    }
    let mut d = vec![payload::PRIVATE_MESSAGE, 0x00, id.len() as u8];
    d.extend_from_slice(id);
    d.push(0x01);
    d.push(text.len() as u8);
    d.extend_from_slice(text);
    Some(d)
}

/// The TLV of a private message (after the payload type byte): (message ID,
/// content). Unknown TLV types make the whole packet invalid, as in the app.
pub fn decode_private_message(tlv: &[u8]) -> Option<(String, String)> {
    let (mut id, mut content) = (None, None);
    let mut r = Reader { d: tlv, at: 0 };
    while r.at + 2 <= tlv.len() {
        let t = r.u8()?;
        let n = r.u8()? as usize;
        let v = String::from_utf8(r.take(n)?.to_vec()).ok()?;
        match t {
            0x00 => id = Some(v),
            0x01 => content = Some(v),
            _ => return None,
        }
    }
    Some((id?, content?))
}

/// A UUID string like Swift's `UUID().uuidString` (uppercase, version 4):
/// the app's message IDs.
pub fn new_message_id() -> String {
    let mut b = [0u8; 16];
    OsRng.fill_bytes(&mut b);
    b[6] = (b[6] & 0x0f) | 0x40;
    b[8] = (b[8] & 0x3f) | 0x80;
    let h: String = b.iter().map(|x| format!("{x:02X}")).collect();
    format!(
        "{}-{}-{}-{}-{}",
        &h[..8],
        &h[8..12],
        &h[12..16],
        &h[16..20],
        &h[20..]
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_and_padding() {
        let pm = encode_private_message("ID-1", "bonjour").unwrap();
        let p = Packet::embedded(pm.clone(), 1_800_000_000_000);
        let bytes = p.encode();
        // Header 14 + sender 8 + payload, padded to 256.
        assert_eq!(bytes.len(), 256);
        assert_eq!(&bytes[..3], &[1, 0x11, 7]);
        assert_eq!(bytes[11], 0, "no flag");
        assert_eq!(
            u16::from_be_bytes([bytes[12], bytes[13]]) as usize,
            pm.len()
        );
        let back = Packet::decode(&bytes).unwrap();
        assert_eq!(back, p);
        assert_eq!(
            decode_private_message(&back.payload[1..]).unwrap(),
            ("ID-1".into(), "bonjour".into())
        );
        assert!(
            encode_private_message("x", &"é".repeat(128)).is_none(),
            "over 255 bytes"
        );
    }

    #[test]
    fn version_2_route_and_compression() {
        // Version 2, a route of one hop, compressed payload (raw DEFLATE).
        let text = vec![b'a'; 300];
        let compressed = miniz_oxide::deflate::compress_to_vec(&text, 6);
        let mut d = vec![2, 0x11, 7];
        d.extend_from_slice(&5u64.to_be_bytes());
        d.push(FLAG_COMPRESSED | FLAG_ROUTE | FLAG_RECIPIENT);
        d.extend_from_slice(&((compressed.len() + 4) as u32).to_be_bytes());
        d.extend_from_slice(&[1; 8]);
        d.extend_from_slice(&[2; 8]);
        d.push(1);
        d.extend_from_slice(&[3; 8]);
        d.extend_from_slice(&(text.len() as u32).to_be_bytes());
        d.extend_from_slice(&compressed);
        let p = Packet::decode(&d).unwrap();
        assert_eq!((p.version, p.recipient), (2, Some([2; 8])));
        assert_eq!(p.payload, text);
        // A claimed size the data cannot hold: refused.
        let n = d.len() - compressed.len() - 4;
        d[n..n + 4].copy_from_slice(&(10_000_000u32).to_be_bytes());
        assert!(Packet::decode(&d).is_none());
    }

    #[test]
    fn message_ids_look_like_swift_uuids() {
        let id = new_message_id();
        assert_eq!(id.len(), 36);
        assert_eq!(&id[14..15], "4");
        assert!(
            id.chars()
                .all(|c| c == '-' || c.is_ascii_digit() || c.is_ascii_uppercase())
        );
    }
}
