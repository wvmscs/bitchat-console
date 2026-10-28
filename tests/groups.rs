//! Mixed groups between consoles, over Nostr (lot 10): the app's rules
//! (creator pinned, epoch up on every roster change, removal by roster),
//! with consent before joining.

mod common;

use std::sync::Arc;
use std::time::Duration;

use bitchat_console::Error;
use bitchat_console::connector::Loopback;
use bitchat_console::console::{Console, GroupOp, GroupsView};
use bitchat_console::identity::{NoiseIdentity, Seed};
use common::*;

fn member(urls: &[String], n: u8) -> Console {
    let mut c = config(urls, Arc::new(Loopback));
    c.seed = Arc::new(Seed::from_bytes([n; 32]));
    c.noise = Some(Arc::new(NoiseIdentity::from_bytes(
        [n.wrapping_add(100); 32],
        [n.wrapping_add(50); 32],
    )));
    Console::start(c)
}

async fn until(c: &Console, what: &str, check: impl Fn(&GroupsView) -> bool) -> GroupsView {
    for _ in 0..250 {
        let g = c.groups().await.unwrap();
        if check(&g) {
            return g;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("never {what}: {:#?}", c.groups().await.unwrap());
}

fn key(n: u8) -> String {
    Seed::from_bytes([n; 32])
        .cell("u09tv")
        .unwrap()
        .pubkey_hex()
}

#[tokio::test]
async fn a_group_lives_and_dies() {
    let relay = TestRelay::spawn("127.0.0.1:0").await.unwrap();
    let urls = [relay.url()];
    let (alice, bob, carol) = (member(&urls, 1), member(&urls, 2), member(&urls, 3));
    for c in [&alice, &bob, &carol] {
        c.set_area("u09tv", true).await.unwrap();
        status_until(c, "connected", all_connected).await;
    }
    alice
        .group(GroupOp::Create {
            name: "amis".into(),
            nickname: "alice".into(),
        })
        .await
        .unwrap();
    let g = alice.groups().await.unwrap().groups[0].clone();
    assert_eq!(
        (g.name.as_str(), g.epoch, g.creator_is_me, g.members.len()),
        ("amis", 1, true, 1)
    );
    let gid = g.id.clone();

    // Invitation, consent, then the newcomer is in, at epoch 2.
    alice
        .group(GroupOp::Invite {
            group: gid.clone(),
            peer: key(2),
            cell: "u09tv".into(),
        })
        .await
        .unwrap();
    let inv = until(&bob, "an invitation", |v| !v.invitations.is_empty())
        .await
        .invitations[0]
        .clone();
    assert_eq!(
        (inv.name.as_str(), inv.from.as_str(), inv.peer.as_str()),
        ("amis", "alice", key(1).as_str())
    );
    bob.group(GroupOp::Accept {
        group: gid.clone(),
        nickname: "bob".into(),
    })
    .await
    .unwrap();
    until(&alice, "bob added", |v| {
        v.groups[0].members.len() == 2 && v.groups[0].epoch == 2
    })
    .await;
    let b = until(&bob, "the group", |v| {
        v.groups.len() == 1 && v.groups[0].epoch == 2
    })
    .await;
    assert!(!b.groups[0].creator_is_me);

    // Messages both ways.
    alice
        .group(GroupOp::Send {
            group: gid.clone(),
            text: "salut 👋".into(),
            nickname: "alice".into(),
        })
        .await
        .unwrap();
    let m = until(&bob, "alice's message", |v| !v.messages.is_empty())
        .await
        .messages[0]
        .clone();
    assert_eq!(
        (m.content.as_str(), m.nickname.as_str(), m.own),
        ("salut 👋", "alice", false)
    );
    bob.group(GroupOp::Send {
        group: gid.clone(),
        text: "coucou".into(),
        nickname: "bob".into(),
    })
    .await
    .unwrap();
    until(&alice, "bob's message", |v| {
        v.messages.iter().any(|m| m.content == "coucou" && !m.own)
    })
    .await;

    // A third member: epoch 3 for everyone.
    alice
        .group(GroupOp::Invite {
            group: gid.clone(),
            peer: key(3),
            cell: "u09tv".into(),
        })
        .await
        .unwrap();
    until(&carol, "an invitation", |v| !v.invitations.is_empty()).await;
    carol
        .group(GroupOp::Accept {
            group: gid.clone(),
            nickname: "carol".into(),
        })
        .await
        .unwrap();
    for c in [&alice, &bob, &carol] {
        until(c, "epoch 3", |v| {
            v.groups.len() == 1 && v.groups[0].epoch == 3 && v.groups[0].members.len() == 3
        })
        .await;
    }
    carol
        .group(GroupOp::Send {
            group: gid.clone(),
            text: "me voilà".into(),
            nickname: "carol".into(),
        })
        .await
        .unwrap();
    for c in [&alice, &bob] {
        until(c, "carol's message", |v| {
            v.messages.iter().any(|m| m.content == "me voilà")
        })
        .await;
    }

    // Only the creator removes.
    let bob_fp = until(&alice, "members", |_| true).await.groups[0]
        .members
        .iter()
        .find(|m| m.nickname == "bob")
        .unwrap()
        .fingerprint
        .clone();
    assert!(matches!(
        carol
            .group(GroupOp::Remove {
                group: gid.clone(),
                fingerprint: bob_fp.clone()
            })
            .await,
        Err(Error::Group(_))
    ));
    alice
        .group(GroupOp::Remove {
            group: gid.clone(),
            fingerprint: bob_fp,
        })
        .await
        .unwrap();
    until(&bob, "removed", |v| v.groups.is_empty()).await;
    until(&carol, "epoch 4", |v| {
        v.groups.len() == 1 && v.groups[0].epoch == 4 && v.groups[0].members.len() == 2
    })
    .await;
    assert!(matches!(
        bob.group(GroupOp::Send {
            group: gid.clone(),
            text: "encore".into(),
            nickname: "bob".into()
        })
        .await,
        Err(Error::Group(_))
    ));

    // Destroyed: gone everywhere.
    alice
        .group(GroupOp::Destroy { group: gid.clone() })
        .await
        .unwrap();
    until(&carol, "destroyed", |v| v.groups.is_empty()).await;
    assert!(alice.groups().await.unwrap().groups.is_empty());
    for c in [alice, bob, carol] {
        c.shutdown().await;
    }
}

#[tokio::test]
async fn declining_and_unsolicited_answers() {
    let relay = TestRelay::spawn("127.0.0.1:0").await.unwrap();
    let urls = [relay.url()];
    let (alice, bob) = (member(&urls, 4), member(&urls, 5));
    for c in [&alice, &bob] {
        c.set_area("u09tv", true).await.unwrap();
        status_until(c, "connected", all_connected).await;
    }
    alice
        .group(GroupOp::Create {
            name: "x".into(),
            nickname: "a".into(),
        })
        .await
        .unwrap();
    let gid = alice.groups().await.unwrap().groups[0].id.clone();
    alice
        .group(GroupOp::Invite {
            group: gid.clone(),
            peer: key(5),
            cell: "u09tv".into(),
        })
        .await
        .unwrap();
    until(&bob, "an invitation", |v| v.invitations.len() == 1).await;
    bob.group(GroupOp::Decline { group: gid.clone() })
        .await
        .unwrap();
    assert!(bob.groups().await.unwrap().invitations.is_empty());
    // Accepting what was declined: nothing to accept.
    assert!(matches!(
        bob.group(GroupOp::Accept {
            group: gid.clone(),
            nickname: "b".into()
        })
        .await,
        Err(Error::Group(_))
    ));
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(alice.groups().await.unwrap().groups[0].members.len(), 1);
    alice.shutdown().await;
    bob.shutdown().await;
}
