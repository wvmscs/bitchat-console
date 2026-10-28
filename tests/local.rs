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
