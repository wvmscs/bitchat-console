// SPDX-FileCopyrightText: 2026 William VITAL, FR (wvmscs)
// SPDX-License-Identifier: GPL-3.0-or-later

//! `bitchat-history` (lot 17): the history from the command line.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use bitchat_console::history::{HISTORY_FILE, HistoryFile, Record};
use bitchat_console::nostr::unix_now;
use serde_json::Value;

fn tool(dir: &Path, args: &[&str]) -> (i32, String, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_bitchat-history"))
        .arg("--data-dir")
        .arg(dir)
        .args(args)
        .output()
        .unwrap();
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8(out.stdout).unwrap(),
        String::from_utf8(out.stderr).unwrap(),
    )
}

fn lines(s: &str) -> Vec<Value> {
    s.lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect()
}

fn row(id: &str, text: &str, at: i64) -> Record {
    Record {
        id: 0,
        kind: "channel".into(),
        msg_id: id.into(),
        own: false,
        geohash: Some("u09tv".into()),
        group_id: None,
        sender: "ab".repeat(32),
        recipient: None,
        nickname: Some("zoé".into()),
        teleport: false,
        content: Some(text.into()),
        created_at: at,
        received_at: at,
        status: None,
        deleted_at: None,
    }
}

#[cfg(unix)]
fn mode(p: &Path) -> u32 {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(p).unwrap().permissions().mode() & 0o777
}

#[test]
fn the_history_from_the_command_line() {
    let dir: PathBuf =
        std::env::temp_dir().join(format!("bitchat-history-cli-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let (code, _, err) = tool(&dir, &["query"]);
    assert_eq!(code, 1, "no history yet: {err}");

    let now = unix_now();
    let f = HistoryFile::open(&dir.join(HISTORY_FILE)).unwrap();
    f.insert(&row("old", "d'hier", now - 86_400)).unwrap();
    f.insert(&row("a", "bonjour", now - 3600)).unwrap();
    f.insert(&row("b", "récent", now - 60)).unwrap();
    drop(f);

    // Query: all, newest first; by age and text.
    let (code, out, _) = tool(&dir, &["query"]);
    assert_eq!(code, 0);
    let all = lines(&out);
    assert_eq!(all.len(), 3);
    assert_eq!(all[0]["content"], "récent");
    let (_, out, _) = tool(&dir, &["query", "--from", "2h", "--text", "BONJOUR"]);
    assert_eq!(lines(&out).len(), 1);
    let id_a = lines(&out)[0]["id"].as_i64().unwrap().to_string();

    // Edit, delete, get.
    assert_eq!(tool(&dir, &["update", &id_a, "[modéré]"]).0, 0);
    let (_, out, _) = tool(&dir, &["get", &id_a]);
    assert_eq!(lines(&out)[0]["content"], "[modéré]");
    let (_, _, err) = tool(&dir, &["delete", &id_a]);
    assert_eq!(err.trim(), "1 deleted");
    let (_, out, _) = tool(&dir, &["query"]);
    assert_eq!(lines(&out).len(), 2, "deleted rows are left out");
    let (_, out, _) = tool(&dir, &["query", "--deleted"]);
    assert_eq!(lines(&out).len(), 3);

    // Insert from stdin (an archive put back): once.
    let mut child = Command::new(env!("CARGO_BIN_EXE_bitchat-history"))
        .arg("--data-dir")
        .arg(&dir)
        .arg("insert")
        .stdin(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let back = serde_json::to_string(&row("c", "remis", now - 30)).unwrap();
    writeln!(child.stdin.take().unwrap(), "{back}\n{back}").unwrap();
    let out = child.wait_with_output().unwrap();
    assert_eq!(
        String::from_utf8(out.stderr).unwrap().trim(),
        "1 added, 1 already there"
    );

    // Backup, then the daily rotation: archive of everything, the last 2 h
    // kept, the base starting there.
    let (code, out, _) = tool(&dir, &["backup"]);
    assert_eq!(code, 0);
    let copy = PathBuf::from(out.trim());
    assert!(copy.starts_with(dir.join("backups")));
    #[cfg(unix)]
    assert_eq!(mode(&copy), 0o600);
    // A base started three days ago.
    rusqlite::Connection::open(dir.join(HISTORY_FILE))
        .unwrap()
        .execute(
            "UPDATE meta SET value = ?1 WHERE key = 'since'",
            [(now - 3 * 86_400).to_string()],
        )
        .unwrap();
    let (code, out, err) = tool(&dir, &["rotate", "--keep", "2h"]);
    assert_eq!(code, 0, "{err}");
    let r: Value = serde_json::from_str(out.trim()).unwrap();
    assert_eq!(r["removed"], 1, "yesterday's row");
    let since = r["since"].as_i64().unwrap();
    assert!((now - 7201..=unix_now() - 7199).contains(&since));
    let archive = PathBuf::from(r["archive"].as_str().unwrap());
    #[cfg(unix)]
    assert_eq!(mode(&archive), 0o600);
    let kept = HistoryFile::open(&archive).unwrap();
    assert_eq!(
        kept.query(&bitchat_console::history::Query {
            deleted: true,
            ..Default::default()
        })
        .unwrap()
        .len(),
        4,
        "the archive has everything"
    );
    let (_, out, _) = tool(&dir, &["query", "--deleted"]);
    assert_eq!(lines(&out).len(), 3);
    // A second rotation never moves the start back.
    let (_, out, _) = tool(&dir, &["rotate", "--keep", "7d"]);
    let again: Value = serde_json::from_str(out.trim()).unwrap();
    assert_eq!(
        (again["since"].as_i64(), &again["removed"]),
        (Some(since), &Value::from(0))
    );
    assert_eq!(
        HistoryFile::open(&dir.join(HISTORY_FILE))
            .unwrap()
            .since()
            .unwrap(),
        since
    );

    // Purge, cron line, wipe; wrong command lines.
    let (_, _, err) = tool(&dir, &["purge", "--before", "10m"]);
    assert_eq!(err.trim(), "1 removed");
    let (_, out, _) = tool(&dir, &["cron", "--at", "03:30"]);
    assert!(
        out.starts_with("30 3 * * * '") && out.contains("rotate --keep 2h"),
        "{out}"
    );
    assert_eq!(tool(&dir, &["query", "--nope", "x"]).0, 2);
    assert_eq!(tool(&dir, &["frobnicate"]).0, 2);
    assert_eq!(tool(&dir, &["delete"]).0, 2);
    assert_eq!(tool(&dir, &["wipe"]).0, 0);
    assert!(!dir.join(HISTORY_FILE).exists());
    std::fs::remove_dir_all(&dir).unwrap();
}
