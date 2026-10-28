//! Private groups in the format of BitChat 1.8 (`GroupProtocol.swift`):
//! creator-signed group state (invites `0x06`, key updates `0x07`) and
//! ChaCha20-Poly1305 group messages (packet `0x25`), byte for byte.
//!
//! The app carries them over Bluetooth only. The console carries the same
//! bytes over Nostr (D6, D27): `bitchat1:` packets in the private envelope,
//! one per member, between per-group Nostr addresses. The only addition is
//! a TLV the app ignores (`0x10`, unknown TLVs are skipped): which Nostr
//! address each member's fingerprint uses.

use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{ChaCha20Poly1305, Nonce};
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use rand_core::{OsRng, RngCore};
use sha2::{Digest, Sha256};

use crate::identity::hex;

/// `BitchatGroup.maxMembers`.
pub const MAX_MEMBERS: usize = 16;
pub const GROUP_ID_LEN: usize = 16;
pub const KEY_LEN: usize = 32;
/// Packet type of a group message (`MessageType.groupMessage`).
pub const TYPE_GROUP_MESSAGE: u8 = 0x25;
const STATE_DOMAIN: &[u8] = b"bitchat-group-v1";
const MESSAGE_DOMAIN: &[u8] = b"bitchat-group-msg-v1";
const MAX_NICKNAME_BYTES: usize = 64;

/// A member, as pinned in the creator-signed roster.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Member {
    /// SHA-256 of the member's Noise static key, 64 hex.
    pub fingerprint: String,
    /// Ed25519 signing key, 32 bytes, hex.
    pub signing_key: String,
    pub nickname: String,
}

/// TLV with a 16-bit length (`GroupTLV`).
fn put(t: u8, v: &[u8], out: &mut Vec<u8>) -> Option<()> {
    let n = u16::try_from(v.len()).ok()?;
    out.push(t);
    out.extend_from_slice(&n.to_be_bytes());
    out.extend_from_slice(v);
    Some(())
}

fn parse(d: &[u8]) -> Option<Vec<(u8, &[u8])>> {
    let mut out = Vec::new();
    let mut at = 0;
    while at < d.len() {
        if d.len() - at < 3 {
            return None;
        }
        let t = d[at];
        let n = u16::from_be_bytes([d[at + 1], d[at + 2]]) as usize;
        let v = d.get(at + 3..at + 3 + n)?;
        out.push((t, v));
        at += 3 + n;
    }
    Some(out)
}

fn unhex(s: &str) -> Option<Vec<u8>> {
    if !s.len().is_multiple_of(2) {
        return None;
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(s.get(i..i + 2)?, 16).ok())
        .collect()
}

fn sha(d: &[u8]) -> [u8; 32] {
    Sha256::digest(d).into()
}

/// `GroupRosterCoding.encode`: count, then per member the fingerprint and
/// signing key (32 bytes each) and the nickname (length byte, at most 64
/// bytes, cut on a character boundary).
pub fn encode_roster(members: &[Member]) -> Option<Vec<u8>> {
    if members.len() > MAX_MEMBERS {
        return None;
    }
    let mut out = vec![members.len() as u8];
    for m in members {
        let fp = unhex(&m.fingerprint).filter(|f| f.len() == 32)?;
        let key = unhex(&m.signing_key).filter(|k| k.len() == 32)?;
        out.extend_from_slice(&fp);
        out.extend_from_slice(&key);
        let mut nick = m.nickname.as_str();
        while nick.len() > MAX_NICKNAME_BYTES {
            let mut cut = nick.len() - 1;
            while !nick.is_char_boundary(cut) {
                cut -= 1;
            }
            nick = &nick[..cut];
        }
        out.push(nick.len() as u8);
        out.extend_from_slice(nick.as_bytes());
    }
    Some(out)
}

pub fn decode_roster(d: &[u8]) -> Option<Vec<Member>> {
    let count = *d.first()? as usize;
    if count > MAX_MEMBERS {
        return None;
    }
    let mut at = 1;
    let mut out = Vec::with_capacity(count);
    for _ in 0..count {
        let fixed = d.get(at..at + 65)?;
        let n = fixed[64] as usize;
        let nick = std::str::from_utf8(d.get(at + 65..at + 65 + n)?).ok()?;
        out.push(Member {
            fingerprint: hex(&fixed[..32]),
            signing_key: hex(&fixed[32..64]),
            nickname: nick.to_owned(),
        });
        at += 65 + n;
    }
    (at == d.len()).then_some(out)
}

/// Creator-signed group state (`GroupStatePayload`), plus the console's
/// address book (TLV `0x10`, not signed: it travels in an envelope signed by
/// the creator's group address, which the members pin).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupState {
    pub group_id: [u8; GROUP_ID_LEN],
    pub name: String,
    pub key: [u8; KEY_LEN],
    pub epoch: u32,
    pub members: Vec<Member>,
    pub creator: String,
    pub signature: [u8; 64],
    /// Fingerprint -> Nostr address (x-only key, hex).
    pub addresses: Vec<(String, String)>,
}

/// The bytes the creator signs: `"bitchat-group-v1" | groupID | epoch |
/// SHA256(key) | SHA256(roster) | SHA256(name)`.
pub fn state_signing_content(
    group_id: &[u8],
    epoch: u32,
    key: &[u8],
    roster: &[u8],
    name: &str,
) -> Vec<u8> {
    let mut c = STATE_DOMAIN.to_vec();
    c.extend_from_slice(group_id);
    c.extend_from_slice(&epoch.to_be_bytes());
    c.extend_from_slice(&sha(key));
    c.extend_from_slice(&sha(roster));
    c.extend_from_slice(&sha(name.as_bytes()));
    c
}

impl GroupState {
    /// A state signed by `creator` (whose fingerprint is `creator_fp`).
    #[allow(clippy::too_many_arguments)]
    pub fn signed(
        group_id: [u8; GROUP_ID_LEN],
        name: &str,
        key: [u8; KEY_LEN],
        epoch: u32,
        members: Vec<Member>,
        creator_fp: &str,
        creator: &SigningKey,
        addresses: Vec<(String, String)>,
    ) -> Option<GroupState> {
        let roster = encode_roster(&members)?;
        let sig = creator.sign(&state_signing_content(
            &group_id, epoch, &key, &roster, name,
        ));
        Some(GroupState {
            group_id,
            name: name.to_owned(),
            key,
            epoch,
            members,
            creator: creator_fp.to_owned(),
            signature: sig.to_bytes(),
            addresses,
        })
    }

    pub fn encode(&self) -> Option<Vec<u8>> {
        let roster = encode_roster(&self.members)?;
        let fp = unhex(&self.creator).filter(|f| f.len() == 32)?;
        let mut out = Vec::new();
        put(0x01, &self.group_id, &mut out)?;
        put(0x02, self.name.as_bytes(), &mut out)?;
        put(0x03, &self.key, &mut out)?;
        put(0x04, &self.epoch.to_be_bytes(), &mut out)?;
        put(0x05, &roster, &mut out)?;
        put(0x06, &fp, &mut out)?;
        put(0x07, &self.signature, &mut out)?;
        if !self.addresses.is_empty() {
            let mut book = Vec::new();
            for (fp, addr) in &self.addresses {
                book.extend(unhex(fp).filter(|f| f.len() == 32)?);
                book.extend(unhex(addr).filter(|a| a.len() == 32)?);
            }
            put(0x10, &book, &mut out)?;
        }
        Some(out)
    }

    pub fn decode(d: &[u8]) -> Option<GroupState> {
        let (mut id, mut name, mut key, mut epoch, mut members, mut creator, mut sig) =
            (None, None, None, None, None, None, None);
        let mut addresses = Vec::new();
        for (t, v) in parse(d)? {
            match t {
                0x01 if v.len() == GROUP_ID_LEN => id = v.try_into().ok(),
                0x02 => name = std::str::from_utf8(v).ok().map(str::to_owned),
                0x03 if v.len() == KEY_LEN => key = v.try_into().ok(),
                0x04 if v.len() == 4 => epoch = Some(u32::from_be_bytes(v.try_into().ok()?)),
                0x05 => members = decode_roster(v),
                0x06 if v.len() == 32 => creator = Some(hex(v)),
                0x07 if v.len() == 64 => sig = v.try_into().ok(),
                0x10 if v.len().is_multiple_of(64) => {
                    addresses = v
                        .chunks(64)
                        .map(|c| (hex(&c[..32]), hex(&c[32..])))
                        .collect()
                }
                // Forward compatible: unknown TLVs are skipped, as in the app.
                _ => {}
            }
        }
        let members = members.filter(|m| !m.is_empty())?;
        Some(GroupState {
            group_id: id?,
            name: name?,
            key: key?,
            epoch: epoch?,
            members,
            creator: creator?,
            signature: sig?,
            addresses,
        })
    }

    /// `verifyCreatorSignature`: the creator is in the roster and signed.
    pub fn verify(&self) -> bool {
        let Some(creator) = self.members.iter().find(|m| m.fingerprint == self.creator) else {
            return false;
        };
        let (Some(roster), Some(pk)) = (encode_roster(&self.members), unhex(&creator.signing_key))
        else {
            return false;
        };
        let Ok(pk) = VerifyingKey::try_from(pk.as_slice()) else {
            return false;
        };
        let content =
            state_signing_content(&self.group_id, self.epoch, &self.key, &roster, &self.name);
        pk.verify(&content, &Signature::from_bytes(&self.signature))
            .is_ok()
    }

    pub fn member(&self, fingerprint: &str) -> Option<&Member> {
        self.members.iter().find(|m| m.fingerprint == fingerprint)
    }

    pub fn address(&self, fingerprint: &str) -> Option<&str> {
        self.addresses
            .iter()
            .find(|(f, _)| f == fingerprint)
            .map(|(_, a)| a.as_str())
    }
}

/// A decrypted, signature-checked group message (`GroupMessagePlaintext`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupMessage {
    pub group_id: [u8; GROUP_ID_LEN],
    pub epoch: u32,
    pub message_id: String,
    /// Ed25519 key of the author, hex.
    pub sender: String,
    pub nickname: String,
    pub timestamp_ms: u64,
    pub content: String,
}

/// `messageSigningContent`: domain | groupID | epoch | messageID |
/// timestamp | content.
fn message_signing_content(
    group_id: &[u8],
    epoch: u32,
    id: &str,
    ts: u64,
    content: &str,
) -> Vec<u8> {
    let mut d = MESSAGE_DOMAIN.to_vec();
    d.extend_from_slice(group_id);
    d.extend_from_slice(&epoch.to_be_bytes());
    d.extend_from_slice(id.as_bytes());
    d.extend_from_slice(&ts.to_be_bytes());
    d.extend_from_slice(content.as_bytes());
    d
}

/// `GroupCrypto.sealMessage`: the encoded envelope (`0x25` payload).
pub fn seal_message(
    state: &GroupState,
    message_id: &str,
    nickname: &str,
    sender: &SigningKey,
    timestamp_ms: u64,
    content: &str,
) -> Option<Vec<u8>> {
    let sig = sender.sign(&message_signing_content(
        &state.group_id,
        state.epoch,
        message_id,
        timestamp_ms,
        content,
    ));
    let mut inner = Vec::new();
    put(0x01, message_id.as_bytes(), &mut inner)?;
    put(0x02, &sender.verifying_key().to_bytes(), &mut inner)?;
    put(0x03, nickname.as_bytes(), &mut inner)?;
    put(0x04, &timestamp_ms.to_be_bytes(), &mut inner)?;
    put(0x05, content.as_bytes(), &mut inner)?;
    put(0x06, &sig.to_bytes(), &mut inner)?;
    let mut aad = state.group_id.to_vec();
    aad.extend_from_slice(&state.epoch.to_be_bytes());
    let mut nonce = [0u8; 12];
    OsRng.fill_bytes(&mut nonce);
    let ct = ChaCha20Poly1305::new((&state.key).into())
        .encrypt(
            &Nonce::from(nonce),
            Payload {
                msg: &inner,
                aad: &aad,
            },
        )
        .ok()?;
    let mut out = Vec::new();
    put(0x01, &state.group_id, &mut out)?;
    put(0x02, &state.epoch.to_be_bytes(), &mut out)?;
    put(0x03, &nonce, &mut out)?;
    put(0x04, &ct, &mut out)?;
    Some(out)
}

/// The group ID and epoch of an envelope, readable without the key.
pub fn envelope_header(d: &[u8]) -> Option<([u8; GROUP_ID_LEN], u32)> {
    let (mut id, mut epoch) = (None, None);
    for (t, v) in parse(d)? {
        match t {
            0x01 if v.len() == GROUP_ID_LEN => id = v.try_into().ok(),
            0x02 if v.len() == 4 => epoch = Some(u32::from_be_bytes(v.try_into().ok()?)),
            _ => {}
        }
    }
    Some((id?, epoch?))
}

/// `GroupCrypto.openMessage`: decrypts with the epoch key and checks the
/// author's signature. Membership of the author is the caller's check.
pub fn open_message(d: &[u8], key: &[u8; KEY_LEN]) -> Option<GroupMessage> {
    let (mut id, mut epoch, mut nonce, mut ct) = (None, None, None, None);
    for (t, v) in parse(d)? {
        match t {
            0x01 if v.len() == GROUP_ID_LEN => id = <[u8; GROUP_ID_LEN]>::try_from(v).ok(),
            0x02 if v.len() == 4 => epoch = Some(u32::from_be_bytes(v.try_into().ok()?)),
            0x03 if v.len() == 12 => nonce = <[u8; 12]>::try_from(v).ok(),
            0x04 if v.len() > 16 => ct = Some(v),
            _ => {}
        }
    }
    let (id, epoch, nonce, ct) = (id?, epoch?, nonce?, ct?);
    let mut aad = id.to_vec();
    aad.extend_from_slice(&epoch.to_be_bytes());
    let inner = ChaCha20Poly1305::new(key.into())
        .decrypt(&Nonce::from(nonce), Payload { msg: ct, aad: &aad })
        .ok()?;
    let (mut mid, mut sender, mut nick, mut ts, mut content, mut sig) =
        (None, None, None, None, None, None);
    for (t, v) in parse(&inner)? {
        match t {
            0x01 => mid = std::str::from_utf8(v).ok().map(str::to_owned),
            0x02 if v.len() == 32 => sender = <[u8; 32]>::try_from(v).ok(),
            0x03 => nick = std::str::from_utf8(v).ok().map(str::to_owned),
            0x04 if v.len() == 8 => ts = Some(u64::from_be_bytes(v.try_into().ok()?)),
            0x05 => content = std::str::from_utf8(v).ok().map(str::to_owned),
            0x06 if v.len() == 64 => sig = <[u8; 64]>::try_from(v).ok(),
            _ => {}
        }
    }
    let (mid, sender, nick, ts, content, sig) = (
        mid.filter(|m| !m.is_empty())?,
        sender?,
        nick?,
        ts?,
        content?,
        sig?,
    );
    let pk = VerifyingKey::from_bytes(&sender).ok()?;
    pk.verify(
        &message_signing_content(&id, epoch, &mid, ts, &content),
        &Signature::from_bytes(&sig),
    )
    .ok()?;
    Some(GroupMessage {
        group_id: id,
        epoch,
        message_id: mid,
        sender: hex(&sender),
        nickname: nick,
        timestamp_ms: ts,
        content,
    })
}

/// Console extension payloads (inside `noiseEncrypted`), outside the app's
/// range: the consent handshake before an invite (D27).
pub mod join {
    /// Creator -> invitee: "would you join this group?"
    pub const REQUEST: u8 = 0xC0;
    /// Invitee -> creator: yes, with its Noise identity and group address.
    pub const ACCEPT: u8 = 0xC1;
}

/// `REQUEST`: group ID, name, creator nickname.
pub fn encode_join_request(
    group_id: &[u8; GROUP_ID_LEN],
    name: &str,
    nickname: &str,
) -> Option<Vec<u8>> {
    let mut out = vec![join::REQUEST];
    put(0x01, group_id, &mut out)?;
    put(0x02, name.as_bytes(), &mut out)?;
    put(0x03, nickname.as_bytes(), &mut out)?;
    Some(out)
}

pub fn decode_join_request(d: &[u8]) -> Option<([u8; GROUP_ID_LEN], String, String)> {
    let (mut id, mut name, mut nick) = (None, None, None);
    for (t, v) in parse(d)? {
        match t {
            0x01 if v.len() == GROUP_ID_LEN => id = v.try_into().ok(),
            0x02 => name = std::str::from_utf8(v).ok().map(str::to_owned),
            0x03 => nick = std::str::from_utf8(v).ok().map(str::to_owned),
            _ => {}
        }
    }
    Some((id?, name?, nick?))
}

/// `ACCEPT`: group ID, the member as it will be pinned, its group address.
pub fn encode_join_accept(
    group_id: &[u8; GROUP_ID_LEN],
    me: &Member,
    address: &str,
) -> Option<Vec<u8>> {
    let mut out = vec![join::ACCEPT];
    put(0x01, group_id, &mut out)?;
    put(0x02, &encode_roster(std::slice::from_ref(me))?, &mut out)?;
    put(0x03, &unhex(address).filter(|a| a.len() == 32)?, &mut out)?;
    Some(out)
}

pub fn decode_join_accept(d: &[u8]) -> Option<([u8; GROUP_ID_LEN], Member, String)> {
    let (mut id, mut member, mut addr) = (None, None, None);
    for (t, v) in parse(d)? {
        match t {
            0x01 if v.len() == GROUP_ID_LEN => id = v.try_into().ok(),
            0x02 => member = decode_roster(v).and_then(|mut m| (m.len() == 1).then(|| m.remove(0))),
            0x03 if v.len() == 32 => addr = Some(hex(v)),
            _ => {}
        }
    }
    Some((id?, member?, addr?))
}

pub fn new_group_id() -> [u8; GROUP_ID_LEN] {
    let mut id = [0u8; GROUP_ID_LEN];
    OsRng.fill_bytes(&mut id);
    id
}

pub fn new_key() -> [u8; KEY_LEN] {
    let mut k = [0u8; KEY_LEN];
    OsRng.fill_bytes(&mut k);
    k
}

#[cfg(test)]
mod tests {
    use super::*;

    fn member(n: u8, nick: &str) -> (Member, SigningKey) {
        let sk = SigningKey::from_bytes(&[n; 32]);
        (
            Member {
                fingerprint: hex(&[n; 32]),
                signing_key: hex(&sk.verifying_key().to_bytes()),
                nickname: nick.into(),
            },
            sk,
        )
    }

    #[test]
    fn roster_round_trip_and_nickname_cut() {
        let (a, _) = member(1, "alice");
        let (b, _) = member(2, &"é".repeat(40));
        let blob = encode_roster(&[a.clone(), b.clone()]).unwrap();
        let back = decode_roster(&blob).unwrap();
        assert_eq!(back[0], a);
        assert_eq!(
            back[1].nickname,
            "é".repeat(32),
            "cut at 64 bytes, on a character"
        );
        assert!(decode_roster(&blob[..blob.len() - 1]).is_none());
    }

    #[test]
    fn signed_state_and_messages() {
        let (alice, ask) = member(1, "alice");
        let (bob, bsk) = member(2, "bob");
        let id = [7u8; 16];
        let state = GroupState::signed(
            id,
            "amis",
            [9; 32],
            1,
            vec![alice.clone(), bob.clone()],
            &alice.fingerprint,
            &ask,
            vec![(bob.fingerprint.clone(), "ab".repeat(32))],
        )
        .unwrap();
        let back = GroupState::decode(&state.encode().unwrap()).unwrap();
        assert_eq!(back, state);
        assert!(back.verify());
        assert_eq!(
            back.address(&bob.fingerprint),
            Some("ab".repeat(32).as_str())
        );
        // Signed by someone else than the creator it names: refused.
        let forged = GroupState {
            signature: GroupState::signed(
                id,
                "amis",
                [9; 32],
                1,
                vec![alice.clone(), bob.clone()],
                &alice.fingerprint,
                &bsk,
                vec![],
            )
            .unwrap()
            .signature,
            ..state.clone()
        };
        assert!(!forged.verify());
        // Another name with the same signature: refused (the name is signed).
        assert!(
            !GroupState {
                name: "autre".into(),
                ..state.clone()
            }
            .verify()
        );

        let env = seal_message(&state, "M-1", "bob", &bsk, 1_800_000_000_000, "salut").unwrap();
        assert_eq!(envelope_header(&env), Some((id, 1)));
        let m = open_message(&env, &state.key).unwrap();
        assert_eq!(
            (m.content.as_str(), m.nickname.as_str(), m.sender.as_str()),
            ("salut", "bob", bob.signing_key.as_str())
        );
        assert!(open_message(&env, &[8; 32]).is_none(), "other key");
    }

    #[test]
    fn join_handshake() {
        let (bob, _) = member(2, "bob");
        let r = encode_join_request(&[3; 16], "amis", "alice").unwrap();
        assert_eq!(r[0], join::REQUEST);
        assert_eq!(
            decode_join_request(&r[1..]).unwrap(),
            ([3; 16], "amis".into(), "alice".into())
        );
        let a = encode_join_accept(&[3; 16], &bob, &"cd".repeat(32)).unwrap();
        assert_eq!(
            decode_join_accept(&a[1..]).unwrap(),
            ([3; 16], bob, "cd".repeat(32))
        );
    }
}
