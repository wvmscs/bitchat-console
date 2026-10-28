//! Groups in the console (lot 10, D6, D27): the app's group rules, carried
//! over Nostr between per-group addresses.
//!
//! Rules kept from the app (`ChatGroupCoordinator.applyGroupState`): state
//! signed by the creator it names; for a known group the creator (its
//! fingerprint and signing key) is pinned and the epoch never goes back;
//! a roster without me is a removal; every roster change rotates the key
//! (epoch + 1); 16 members at most. Over Nostr, the creator's Nostr address
//! is pinned too: state from any other address is dropped.

use std::collections::BTreeMap;

use serde::Serialize;

use super::{ConsoleEvent, Error, Result, State};
use crate::db::Db;
use crate::groups::{self as wire, GroupState, Member};
use crate::identity::hex;
use crate::nostr::unix_now;
use crate::packet::{self, Packet};

/// The inbox label of a group: its Nostr address lives under it.
pub(super) fn label(group_id: &str) -> String {
    format!("group:{group_id}")
}

pub(super) fn load(db: &Db) -> BTreeMap<String, (GroupState, String)> {
    let mut out = BTreeMap::new();
    for (blob, creator_addr) in db.groups().unwrap_or_default() {
        if let Some(state) = GroupState::decode(&blob) {
            out.insert(hex(&state.group_id), (state, creator_addr));
        }
    }
    out
}

/// What the page can ask about groups.
#[derive(Debug, Clone)]
pub enum GroupOp {
    Create {
        name: String,
        nickname: String,
    },
    /// Creator: ask `peer` (met in `cell`) to join.
    Invite {
        group: String,
        peer: String,
        cell: String,
    },
    Accept {
        group: String,
        nickname: String,
    },
    Decline {
        group: String,
    },
    Send {
        group: String,
        text: String,
        nickname: String,
    },
    /// Creator: remove a member (by fingerprint); the key rotates.
    Remove {
        group: String,
        fingerprint: String,
    },
    /// Leave, locally (as the app).
    Leave {
        group: String,
    },
    /// Creator: remove everyone, then forget the group.
    Destroy {
        group: String,
    },
}

impl GroupOp {
    /// The nickname the operation carries, for those that carry one.
    pub(crate) fn nickname_mut(&mut self) -> Option<&mut String> {
        match self {
            GroupOp::Create { nickname, .. }
            | GroupOp::Accept { nickname, .. }
            | GroupOp::Send { nickname, .. } => Some(nickname),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MemberView {
    pub fingerprint: String,
    pub nickname: String,
    pub me: bool,
    pub creator: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct GroupView {
    /// Group ID, hex.
    pub id: String,
    pub name: String,
    pub epoch: u32,
    pub creator_is_me: bool,
    pub members: Vec<MemberView>,
}

/// A group message, sent or received.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct GroupChat {
    pub group: String,
    pub id: String,
    /// Author's Ed25519 key, hex.
    pub sender: String,
    pub nickname: String,
    pub content: String,
    pub created_at: i64,
    pub own: bool,
}

/// A request to join a group, waiting for the user.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Invitation {
    pub group: String,
    pub name: String,
    /// The creator's nickname, as they gave it.
    pub from: String,
    /// The creator's key in the cell where they asked.
    pub peer: String,
    pub cell: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct GroupsView {
    pub groups: Vec<GroupView>,
    pub messages: Vec<GroupChat>,
    pub invitations: Vec<Invitation>,
}

const MAX_GROUP_MESSAGES: usize = 2000;

fn group_id(hex_id: &str) -> Option<[u8; wire::GROUP_ID_LEN]> {
    if hex_id.len() != 2 * wire::GROUP_ID_LEN {
        return None;
    }
    let v: Option<Vec<u8>> = (0..hex_id.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex_id[i..i + 2], 16).ok())
        .collect();
    v?.try_into().ok()
}

impl State {
    fn noise(&self) -> Result<&crate::identity::NoiseIdentity> {
        self.config
            .noise
            .as_deref()
            .ok_or(Error::Group("no Noise identity".into()))
    }

    fn me(&self, nickname: &str) -> Result<Member> {
        let n = self.noise()?;
        Ok(Member {
            fingerprint: n.fingerprint(),
            signing_key: hex(&n.signing_public()),
            nickname: nickname.to_owned(),
        })
    }

    /// My Nostr address in a group.
    fn group_address(&self, gid: &str) -> Result<String> {
        Ok(self.label_key(&label(gid))?.pubkey_hex())
    }

    pub(super) fn groups_view(&self) -> GroupsView {
        let me = self.noise().map(|n| n.fingerprint()).unwrap_or_default();
        GroupsView {
            groups: self
                .groups
                .iter()
                .map(|(id, (s, _))| GroupView {
                    id: id.clone(),
                    name: s.name.clone(),
                    epoch: s.epoch,
                    creator_is_me: s.creator == me,
                    members: s
                        .members
                        .iter()
                        .map(|m| MemberView {
                            fingerprint: m.fingerprint.clone(),
                            nickname: m.nickname.clone(),
                            me: m.fingerprint == me,
                            creator: m.fingerprint == s.creator,
                        })
                        .collect(),
                })
                .collect(),
            messages: self.group_msgs.iter().cloned().collect(),
            invitations: self.invitations.clone(),
        }
    }

    fn groups_changed(&mut self) {
        // New or removed groups change the inboxes to listen to.
        if let Some(center) = self.center.clone() {
            let teleport = self.teleport;
            if let Err(e) = self.set_area(&center, teleport) {
                tracing::warn!(error = %e, "inboxes not updated");
            }
        }
        self.emit(ConsoleEvent::Groups);
    }

    fn store_group(&mut self, state: GroupState, creator_addr: String) -> Result<()> {
        let id = hex(&state.group_id);
        self.db.save_group(
            &id,
            &state.encode().ok_or(Error::Group("cannot encode".into()))?,
            &creator_addr,
        )?;
        self.groups.insert(id, (state, creator_addr));
        Ok(())
    }

    fn forget_group(&mut self, gid: &str) {
        self.groups.remove(gid);
        let _ = self.db.delete_group(gid);
    }

    /// Sends a `noiseEncrypted` payload from my group address to a member.
    fn send_to_member(&mut self, gid: &str, address: &str, payload: Vec<u8>) {
        let from = label(gid);
        match self.envelope_to(address, &from, payload) {
            Ok(w) => self.publish_dm(&from, &w),
            Err(e) => tracing::warn!(error = %e, "group packet not sent"),
        }
    }

    /// Signs and sends a new state: `invite` to `newcomer`, key update to
    /// every other member but me.
    fn distribute(&mut self, state: &GroupState, newcomer: Option<&str>) -> Result<()> {
        let gid = hex(&state.group_id);
        let me = self.noise()?.fingerprint();
        let blob = state.encode().ok_or(Error::Group("cannot encode".into()))?;
        let targets: Vec<(String, String)> = state
            .members
            .iter()
            .filter(|m| m.fingerprint != me)
            .filter_map(|m| {
                Some((
                    m.fingerprint.clone(),
                    state.address(&m.fingerprint)?.to_owned(),
                ))
            })
            .collect();
        for (fp, addr) in targets {
            let kind = if Some(fp.as_str()) == newcomer {
                packet::payload::GROUP_INVITE
            } else {
                packet::payload::GROUP_KEY_UPDATE
            };
            let mut payload = vec![kind];
            payload.extend_from_slice(&blob);
            self.send_to_member(&gid, &addr, payload);
        }
        Ok(())
    }

    /// A new state from me (the creator), signed, kept and sent.
    fn rotate(
        &mut self,
        gid: &str,
        members: Vec<Member>,
        addresses: Vec<(String, String)>,
        newcomer: Option<&str>,
    ) -> Result<()> {
        let (old, creator_addr) = self
            .groups
            .get(gid)
            .cloned()
            .ok_or(Error::Group("unknown group".into()))?;
        let noise = self.noise()?;
        let state = GroupState::signed(
            old.group_id,
            &old.name,
            wire::new_key(),
            old.epoch + 1,
            members,
            &old.creator,
            noise.signing_key(),
            addresses,
        )
        .ok_or(Error::Group("cannot sign the state".into()))?;
        self.store_group(state.clone(), creator_addr)?;
        self.distribute(&state, newcomer)
    }

    fn creator_group(&self, gid: &str) -> Result<GroupState> {
        let (s, _) = self
            .groups
            .get(gid)
            .ok_or(Error::Group("unknown group".into()))?;
        if s.creator != self.noise()?.fingerprint() {
            return Err(Error::Group("only the creator can do that".into()));
        }
        Ok(s.clone())
    }

    pub(super) fn group_op(&mut self, op: GroupOp) -> Result<()> {
        match op {
            GroupOp::Create { name, nickname } => {
                let name = name.trim();
                if name.is_empty() {
                    return Err(Error::Group("a group needs a name".into()));
                }
                let me = self.me(nickname.trim())?;
                let id = wire::new_group_id();
                let gid = hex(&id);
                let address = self.group_address(&gid)?;
                let noise = self.noise()?;
                // Epoch 1 at creation, as the app.
                let state = GroupState::signed(
                    id,
                    name,
                    wire::new_key(),
                    1,
                    vec![me.clone()],
                    &me.fingerprint,
                    noise.signing_key(),
                    vec![(me.fingerprint.clone(), address.clone())],
                )
                .ok_or(Error::Group("cannot sign the state".into()))?;
                self.store_group(state, address)?;
                tracing::info!(group = %gid, "group created");
            }
            GroupOp::Invite { group, peer, cell } => {
                let s = self.creator_group(&group)?;
                if s.members.len() >= wire::MAX_MEMBERS {
                    return Err(Error::Group(format!(
                        "a group has {} members at most",
                        wire::MAX_MEMBERS
                    )));
                }
                if !self.identities.contains(&cell) {
                    return Err(Error::NotInArea(cell));
                }
                let mine = s
                    .member(&s.creator)
                    .map(|m| m.nickname.clone())
                    .unwrap_or_default();
                let request = wire::encode_join_request(&s.group_id, &s.name, &mine)
                    .ok_or(Error::Group("cannot encode".into()))?;
                let w = self.envelope_to(&peer, &cell, request)?;
                self.publish_dm(&cell, &w);
                self.invited
                    .retain(|(g, p, _)| !(g == &group && p == &peer));
                self.invited.push((group, peer, cell));
                return Ok(());
            }
            GroupOp::Accept { group, nickname } => {
                let i = self
                    .invitations
                    .iter()
                    .position(|i| i.group == group)
                    .ok_or(Error::Group("no such invitation".into()))?;
                let inv = self.invitations.remove(i);
                let me = self.me(nickname.trim())?;
                let id = group_id(&group).ok_or(Error::Group("bad group id".into()))?;
                let accept = wire::encode_join_accept(&id, &me, &self.group_address(&group)?)
                    .ok_or(Error::Group("cannot encode".into()))?;
                let w = self.envelope_to(&inv.peer, &inv.cell, accept)?;
                self.publish_dm(&inv.cell, &w);
                // The first state will come to my group address: listen.
                self.awaiting.insert(group.clone());
                if let Some(center) = self.center.clone() {
                    let teleport = self.teleport;
                    let _ = self.set_area(&center, teleport);
                }
                self.subscribe_group(&group);
            }
            GroupOp::Decline { group } => self.invitations.retain(|i| i.group != group),
            GroupOp::Send {
                group,
                text,
                nickname,
            } => {
                if text.trim().is_empty() {
                    return Err(Error::EmptyMessage);
                }
                let (s, _) = self
                    .groups
                    .get(&group)
                    .cloned()
                    .ok_or(Error::Group("unknown group".into()))?;
                let noise = self.noise()?;
                let id = packet::new_message_id();
                let now_ms = (unix_now() as u64) * 1000;
                let env = wire::seal_message(
                    &s,
                    &id,
                    nickname.trim(),
                    noise.signing_key(),
                    now_ms,
                    &text,
                )
                .ok_or(Error::Group("cannot seal".into()))?;
                let mine = noise.fingerprint();
                let me_key = hex(&noise.signing_public());
                let targets: Vec<String> = s
                    .members
                    .iter()
                    .filter(|m| m.fingerprint != mine)
                    .filter_map(|m| s.address(&m.fingerprint).map(str::to_owned))
                    .collect();
                let from = label(&group);
                for addr in targets {
                    let mut p = Packet::embedded(env.clone(), now_ms);
                    p.kind = wire::TYPE_GROUP_MESSAGE;
                    match self.packet_to(&addr, &from, p) {
                        Ok(w) => self.publish_dm(&from, &w),
                        Err(e) => tracing::warn!(error = %e, "group message not sent"),
                    }
                }
                self.keep_group_message(GroupChat {
                    group,
                    id,
                    sender: me_key,
                    nickname: nickname.trim().to_owned(),
                    content: text,
                    created_at: unix_now(),
                    own: true,
                });
                return Ok(());
            }
            GroupOp::Remove { group, fingerprint } => {
                let s = self.creator_group(&group)?;
                if fingerprint == s.creator {
                    return Err(Error::Group("the creator cannot remove themselves".into()));
                }
                let removed_addr = s.address(&fingerprint).map(str::to_owned);
                if s.member(&fingerprint).is_none() {
                    return Err(Error::Group("not a member".into()));
                }
                let members: Vec<Member> = s
                    .members
                    .iter()
                    .filter(|m| m.fingerprint != fingerprint)
                    .cloned()
                    .collect();
                let addresses: Vec<(String, String)> = s
                    .addresses
                    .iter()
                    .filter(|(f, _)| f != &fingerprint)
                    .cloned()
                    .collect();
                self.rotate(&group, members, addresses, None)?;
                // Tell the removed member, with a throwaway key (as the app).
                if let (Some(addr), Some((state, _))) =
                    (removed_addr, self.groups.get(&group).cloned())
                {
                    let notice = GroupState {
                        key: [0; wire::KEY_LEN],
                        ..state
                    };
                    let notice = GroupState::signed(
                        notice.group_id,
                        &notice.name,
                        notice.key,
                        notice.epoch,
                        notice.members,
                        &notice.creator,
                        self.noise()?.signing_key(),
                        notice.addresses,
                    )
                    .ok_or(Error::Group("cannot sign".into()))?;
                    let mut payload = vec![packet::payload::GROUP_KEY_UPDATE];
                    payload.extend(notice.encode().unwrap_or_default());
                    self.send_to_member(&group, &addr, payload);
                }
            }
            GroupOp::Leave { group } => self.forget_group(&group),
            GroupOp::Destroy { group } => {
                let s = self.creator_group(&group)?;
                let me = s
                    .member(&s.creator)
                    .cloned()
                    .ok_or(Error::Group("no creator".into()))?;
                let others: Vec<(String, String)> = s
                    .addresses
                    .iter()
                    .filter(|(f, _)| f != &s.creator)
                    .cloned()
                    .collect();
                // A roster of the creator alone removes everyone else.
                let last = GroupState::signed(
                    s.group_id,
                    &s.name,
                    [0; wire::KEY_LEN],
                    s.epoch + 1,
                    vec![me],
                    &s.creator,
                    self.noise()?.signing_key(),
                    s.addresses.clone(),
                )
                .ok_or(Error::Group("cannot sign".into()))?;
                let blob = last.encode().unwrap_or_default();
                for (_, addr) in others {
                    let mut payload = vec![packet::payload::GROUP_KEY_UPDATE];
                    payload.extend_from_slice(&blob);
                    self.send_to_member(&group, &addr, payload);
                }
                self.forget_group(&group);
            }
        }
        self.groups_changed();
        Ok(())
    }

    /// Makes sure the inbox of a group address is listened to, even before
    /// the group is stored (an accepted invitation).
    fn subscribe_group(&mut self, gid: &str) {
        let Ok(key) = self.label_key(&label(gid)) else {
            return;
        };
        let filter = crate::nostr::Filter {
            kinds: vec![crate::envelope::KIND_WRAP],
            since: Some(unix_now() - super::DM_LOOKBACK_SECS),
            limit: Some(100),
            tags: BTreeMap::from([('p', vec![key.pubkey_hex()])]),
            ..Default::default()
        };
        for relay in self.config.dm_relays.clone() {
            let k = (relay.clone(), label(gid));
            if !self.conns.contains_key(&k) {
                self.subs.insert(k, vec![filter.clone()]);
                self.connect(relay, label(gid), vec![filter.clone()]);
            }
        }
    }

    fn keep_group_message(&mut self, m: GroupChat) {
        self.group_msgs.push_back(m.clone());
        while self.group_msgs.len() > MAX_GROUP_MESSAGES {
            self.group_msgs.pop_front();
        }
        self.emit(ConsoleEvent::GroupMessage(m));
    }

    /// A join request, in a cell inbox: kept for the user to decide.
    pub(super) fn on_join_request(&mut self, cell: &str, sender: &str, body: &[u8]) {
        let Some((id, name, from)) = wire::decode_join_request(body) else {
            return;
        };
        let group = hex(&id);
        if self.groups.contains_key(&group) || self.invitations.iter().any(|i| i.group == group) {
            return;
        }
        self.invitations.push(Invitation {
            group,
            name,
            from,
            peer: sender.to_owned(),
            cell: cell.to_owned(),
        });
        self.emit(ConsoleEvent::Groups);
    }

    /// An acceptance: only from the key I asked, then the newcomer is added
    /// and the key rotates.
    pub(super) fn on_join_accept(&mut self, cell: &str, sender: &str, body: &[u8]) {
        let Some((id, member, address)) = wire::decode_join_accept(body) else {
            return;
        };
        let gid = hex(&id);
        let Some(at) = self
            .invited
            .iter()
            .position(|(g, p, c)| g == &gid && p == sender && c == cell)
        else {
            tracing::debug!("group acceptance nobody asked for, ignored");
            return;
        };
        self.invited.remove(at);
        let Ok(s) = self.creator_group(&gid) else {
            return;
        };
        if s.member(&member.fingerprint).is_some() || s.members.len() >= wire::MAX_MEMBERS {
            return;
        }
        let fp = member.fingerprint.clone();
        let mut members = s.members.clone();
        members.push(member);
        let mut addresses = s.addresses.clone();
        addresses.push((fp.clone(), address));
        if let Err(e) = self.rotate(&gid, members, addresses, Some(&fp)) {
            tracing::warn!(error = %e, "member not added");
            return;
        }
        self.emit(ConsoleEvent::Groups);
    }

    /// A state, on a group address: the app's checks, plus the pinned
    /// creator address.
    pub(super) fn on_group_state(&mut self, label: &str, sender: &str, body: &[u8]) {
        let Some(gid) = label.strip_prefix("group:").map(str::to_owned) else {
            return;
        };
        let Some(state) = GroupState::decode(body) else {
            return;
        };
        if hex(&state.group_id) != gid || !state.verify() {
            tracing::warn!(group = %gid, "group state dropped (malformed or not signed by its creator)");
            return;
        }
        let Ok(mine) = self.noise().map(|n| n.fingerprint()) else {
            return;
        };
        let creator_addr = match self.groups.get(&gid) {
            Some((existing, pinned)) => {
                let same_creator = existing.creator == state.creator
                    && existing.member(&existing.creator).map(|m| &m.signing_key)
                        == state.member(&state.creator).map(|m| &m.signing_key);
                if !same_creator || pinned != sender || state.epoch < existing.epoch {
                    tracing::warn!(group = %gid, "group state dropped (other creator, address or older epoch)");
                    return;
                }
                pinned.clone()
            }
            None => {
                // Only for a group I accepted, from the creator's own address.
                if !self.awaiting.contains(&gid) || state.address(&state.creator) != Some(sender) {
                    return;
                }
                sender.to_owned()
            }
        };
        if state.member(&mine).is_none() {
            // A roster without me: removed.
            if self.groups.contains_key(&gid) {
                tracing::info!(group = %gid, "removed from the group");
                self.forget_group(&gid);
                self.groups_changed();
            }
            return;
        }
        self.awaiting.remove(&gid);
        let new = !self.groups.contains_key(&gid);
        if let Err(e) = self.store_group(state, creator_addr) {
            tracing::warn!(error = %e, "group state not stored");
            return;
        }
        if new {
            self.groups_changed();
        } else {
            self.emit(ConsoleEvent::Groups);
        }
    }

    /// A group message: the current epoch's key, and an author in the roster.
    pub(super) fn on_group_message(&mut self, label: &str, sender: &str, envelope: &[u8]) {
        let Some(gid) = label.strip_prefix("group:") else {
            return;
        };
        let Some((state, _)) = self.groups.get(gid) else {
            return;
        };
        let Some((id, epoch)) = wire::envelope_header(envelope) else {
            return;
        };
        if hex(&id) != gid || epoch != state.epoch {
            return;
        }
        let Some(m) = wire::open_message(envelope, &state.key) else {
            return;
        };
        let Some(author) = state.members.iter().find(|x| x.signing_key == m.sender) else {
            tracing::debug!(group = %gid, "group message from a non-member, dropped");
            return;
        };
        // The Nostr sender must be that member's address.
        if state.address(&author.fingerprint) != Some(sender) {
            return;
        }
        if self
            .group_msgs
            .iter()
            .any(|x| x.group == gid && x.id == m.message_id)
        {
            return;
        }
        let chat = GroupChat {
            group: gid.to_owned(),
            id: m.message_id,
            sender: m.sender,
            nickname: m.nickname,
            content: m.content,
            created_at: ((m.timestamp_ms / 1000) as i64).min(unix_now()),
            own: false,
        };
        self.keep_group_message(chat);
    }
}
