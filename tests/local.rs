// SPDX-FileCopyrightText: 2026 William VITAL, FR (wvmscs)
// SPDX-License-Identifier: GPL-3.0-or-later

//! What the user keeps: muted keys (across restarts), saved messages,
//! scheduled messages sent when due.

mod common;

use std::sync::Arc;
use std::time::Duration;

use bitchat_console::Error;
use bitchat_console::connector::Loopback;
use bitchat_console::console::Console;
use bitchat_console::nostr::unix_now;
use common::*;
use serde_json::json;

fn db_file(name: &str) -> std::path::PathBuf {
    let p = std::env::temp_dir().join(format!(
        "bitchat-console-{name}-{}.sqlite",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&p);
    p
}

fn console_with_db(relay: &TestRelay, db: &std::path::Path) -> Console {
    let mut c = config(&[relay.url()], Arc::new(Loopback));
    c.db_path = Some(db.to_owned());
    Console::start(c)
}

#[tokio::test]
async fn mutes_survive_a_restart() {
    let relay = TestRelay::spawn("127.0.0.1:0").await.unwrap();
    let db = db_file("mutes");
    let key = Identity::generate().public_key_hex();
    let c = console_with_db(&relay, &db);
    c.set_muted(&key, true).await.unwrap();
    assert!(matches!(
        c.set_muted("nope", true).await,
        Err(Error::InvalidKey(_))
    ));
    c.shutdown().await;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&db).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "the database is private");
    }
    let c = console_with_db(&relay, &db);
    assert_eq!(c.local().await.unwrap().mutes, std::slice::from_ref(&key));
    c.set_muted(&key, false).await.unwrap();
    assert!(c.local().await.unwrap().mutes.is_empty());
    c.shutdown().await;
    let _ = std::fs::remove_file(db);
}

#[tokio::test]
async fn saved_and_scheduled_messages() {
    let relay = TestRelay::spawn("127.0.0.1:0").await.unwrap();
    let db = db_file("saved");
    let c = console_with_db(&relay, &db);
    c.set_area("u09tv", true).await.unwrap();
    status_until(&c, "connected", all_connected).await;
    let mut seen = reader(&relay.url(), json!({"kinds": [20000]})).await;

    // A saved message, sent on demand in a neighbour.
    let id = c
        .add_preset("hello", "bonjour à tous", None, Some("bob"))
        .await
        .unwrap();
    let m = c.send_preset(id, Some("u09ty")).await.unwrap();
    let e = next(&mut seen).await;
    assert_eq!(
        (e.id.as_str(), e.content.as_str()),
        (m.id.as_str(), "bonjour à tous")
    );
    assert_eq!(e.tags[0], ["g", "u09ty"]);
    assert_eq!(e.tags[1], ["n", "bob"]);
    assert!(matches!(
        c.send_preset(999, None).await,
        Err(Error::NotFound(999))
    ));

    // Due now: sent within a second or two, then done.
    let s = c
        .add_schedule("à l'heure", None, None, unix_now(), None)
        .await
        .unwrap();
    let e = next(&mut seen).await;
    assert_eq!(e.content, "à l'heure");
    assert_eq!(e.tags[0], ["g", "u09tv"]);
    // A repeating one comes back later; one for a cell not followed is
    // skipped and says why.
    let r = c
        .add_schedule("chaque jour", None, None, unix_now(), Some(86_400))
        .await
        .unwrap();
    let far = c
        .add_schedule("ailleurs", Some("u0bcd"), None, unix_now(), None)
        .await
        .unwrap();
    assert_eq!(next(&mut seen).await.content, "chaque jour");
    tokio::time::sleep(Duration::from_millis(1500)).await;
    let local = c.local().await.unwrap();
    let get = |id| local.schedules.iter().find(|x| x.id == id).unwrap().clone();
    assert_eq!(
        (get(s).next_at, get(s).last_result.as_deref()),
        (None, Some("sent"))
    );
    assert!(get(r).next_at.unwrap() > unix_now() + 86_000);
    assert!(
        get(far).last_result.unwrap().starts_with("skipped"),
        "{:?}",
        get(far)
    );
    assert!(matches!(
        c.add_schedule("trop souvent", None, None, unix_now(), Some(10))
            .await,
        Err(Error::Schedule(_))
    ));
    c.shutdown().await;
    let _ = std::fs::remove_file(db);
}

/// The `n` tag of an event, if any.
fn nick(e: &bitchat_console::nostr::Event) -> Option<&str> {
    e.tags
        .iter()
        .find(|t| t.first().is_some_and(|k| k == "n"))
        .and_then(|t| t.get(1))
        .map(String::as_str)
}

#[tokio::test]
async fn one_nickname_not_sent_where_chosen() {
    let relay = TestRelay::spawn("127.0.0.1:0").await.unwrap();
    let db = db_file("nickname");
    let c = console_with_db(&relay, &db);
    c.set_area("u09tv", true).await.unwrap();
    status_until(&c, "connected", all_connected).await;
    let mut seen = reader(&relay.url(), json!({"kinds": [20000]})).await;

    // The console's nickname by default, another one if given.
    c.set_nickname(Some(" bob ")).await.unwrap();
    c.send("a", None, None).await.unwrap();
    assert_eq!(nick(&next(&mut seen).await), Some("bob"));
    c.send("b", None, Some("carol")).await.unwrap();
    assert_eq!(nick(&next(&mut seen).await), Some("carol"));

    // Not sent in a cell the user chose, whatever is given; still sent in
    // the others.
    c.set_anon("u09tv", true).await.unwrap();
    c.send("c", None, Some("carol")).await.unwrap();
    assert_eq!(nick(&next(&mut seen).await), None);
    c.send("d", Some("u09ty"), None).await.unwrap();
    assert_eq!(nick(&next(&mut seen).await), Some("bob"));

    // A saved message without a nickname takes the one of the moment.
    let p = c.add_preset("p", "e", None, None).await.unwrap();
    c.set_nickname(Some("dave")).await.unwrap();
    c.send_preset(p, Some("u09ty")).await.unwrap();
    assert_eq!(nick(&next(&mut seen).await), Some("dave"));
    // Scheduled ones too, and not in a cell where it is not sent.
    c.add_schedule("f", None, None, unix_now(), None)
        .await
        .unwrap();
    let e = next(&mut seen).await;
    assert_eq!((e.content.as_str(), nick(&e)), ("f", None));

    assert!(matches!(
        c.set_anon("u09", true).await,
        Err(Error::NotAChannel(_))
    ));
    assert!(matches!(
        c.set_anon("u09a", true).await,
        Err(Error::InvalidGeohash(_))
    ));
    c.shutdown().await;

    // Kept across a restart.
    let c = console_with_db(&relay, &db);
    let local = c.local().await.unwrap();
    assert_eq!(
        (local.nickname.as_deref(), local.anon_cells.as_slice()),
        (Some("dave"), &["u09tv".to_owned()][..])
    );
    c.set_nickname(None).await.unwrap();
    assert_eq!(c.local().await.unwrap().nickname, None);
    c.shutdown().await;
    let _ = std::fs::remove_file(db);
}

#[tokio::test]
async fn who_is_still_there() {
    // Lot 15: when each key was last seen in each cell.
    let relay = TestRelay::spawn("127.0.0.1:0").await.unwrap();
    let c = console(&[relay.url()], Arc::new(Loopback));
    c.set_area("u09tv", true).await.unwrap();
    status_until(&c, "connected", all_connected).await;
    let dan = Identity::generate();
    relay.publish(channel_event(
        &dan,
        bitchat_console::console::KIND_MESSAGE,
        "u09ty",
        &[],
        "là",
    ));
    let p = loop {
        let p = c.presence().await.unwrap();
        if p.cells
            .get("u09ty")
            .is_some_and(|m| m.contains_key(&dan.public_key_hex()))
        {
            break p;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    };
    assert_eq!(p.departure_secs, 600);
    let at = p.cells["u09ty"][&dan.public_key_hex()];
    assert!((p.now - 5..=p.now).contains(&at));
    c.shutdown().await;
}
