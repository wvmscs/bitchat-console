// SPDX-FileCopyrightText: 2026 William VITAL, FR (wvmscs)
// SPDX-License-Identifier: GPL-3.0-or-later

//! `bitchat-history`: the console's message history (`history.sqlite`, D31)
//! from the command line, while the console runs or not (lot 17): query,
//! edit, delete, back up, rotate (D33), wipe. For the daily cron and for
//! local tools (an AI); see `skills/bitchat-history/SKILL.md`.
//!
//! Exit code 2 for a wrong command line, 1 when the command fails.

use std::io::BufRead;
use std::path::PathBuf;

use bitchat_console::history::{self, HISTORY_FILE, HistoryFile, Query, Record};
use bitchat_console::nostr::unix_now;

const USAGE: &str = "usage: bitchat-history [--data-dir <dir>] [--file <history.sqlite>] <command>

  query [--kind channel|dm|group] [--cell <geohash or prefix>] [--group <id>]
        [--key <hex>] [--text <text>] [--from <time>] [--to <time>] [--deleted]
        [--order new|old|sender] [--limit <n>] [--offset <n>]
                         rows as JSON, one per line (newest first, 200 by default)
  get <id>               one row as JSON
  insert                 rows as JSON on stdin, one per line (an archive put back)
  update <id> <content>  new content (the console shows it within seconds)
  delete <id>...         content removed, row kept (the console hides it)
  remove <id>...         rows removed outright (the console's feed keeps them)
  purge --before <time>  rows older than that, removed outright
  backup [<file>]        consistent copy, 0600
                         (default: <data-dir>/backups/history-<UTC time>.sqlite)
  rotate [--keep 2h] [--dir <dir>]
                         daily (D33): back up into <dir> (default <data-dir>/backups),
                         keep the last 2 h, start the base there, compact
  wipe                   delete the history (the console makes a new, empty one)
  cron [--at 04:17]      the crontab line for the daily rotation

  <time>: Unix seconds, or an age: 90s, 30m, 2h, 7d (that long ago).
  --data-dir: the console's (default ~/.bitchat-console).";

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("{USAGE}");
        return;
    }
    match run(args) {
        Ok(()) => {}
        Err(Fail::Usage(m)) => {
            eprintln!("bitchat-history: {m}\n\n{USAGE}");
            std::process::exit(2);
        }
        Err(Fail::Run(m)) => {
            eprintln!("bitchat-history: {m}");
            std::process::exit(1);
        }
    }
}

enum Fail {
    Usage(String),
    Run(String),
}

impl From<bitchat_console::Error> for Fail {
    fn from(e: bitchat_console::Error) -> Self {
        Fail::Run(e.to_string())
    }
}

fn usage(m: impl Into<String>) -> Fail {
    Fail::Usage(m.into())
}

/// Seconds in `90s`, `30m`, `2h`, `7d`, or a bare number of seconds.
fn duration(v: &str) -> Option<i64> {
    let (n, unit) = match v.char_indices().last()? {
        (i, c) if c.is_ascii_alphabetic() => (&v[..i], c),
        _ => (v, 's'),
    };
    let n: i64 = n.parse().ok().filter(|n| *n >= 0)?;
    let k = match unit {
        's' => 1,
        'm' => 60,
        'h' => 3600,
        'd' => 86_400,
        _ => return None,
    };
    n.checked_mul(k)
}

/// Unix seconds, or an age (that long ago).
fn time(v: &str) -> Option<i64> {
    if v.bytes().all(|b| b.is_ascii_digit()) && v.len() >= 9 {
        return v.parse().ok();
    }
    duration(v).map(|d| unix_now() - d)
}

fn default_data_dir() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".bitchat-console")
}

fn json(v: &impl serde::Serialize) -> String {
    serde_json::to_string(v).expect("serializable")
}

fn ids(v: &[String]) -> Result<Vec<i64>, Fail> {
    if v.is_empty() {
        return Err(usage("which rows? give their ids"));
    }
    v.iter()
        .map(|s| s.parse().map_err(|_| usage(format!("not a row id: {s:?}"))))
        .collect()
}

fn run(args: Vec<String>) -> Result<(), Fail> {
    let mut it = args.into_iter().peekable();
    let (mut dir, mut file) = (default_data_dir(), None::<PathBuf>);
    let command = loop {
        let Some(a) = it.next() else {
            return Err(usage("a command is missing"));
        };
        match a.as_str() {
            "--data-dir" => {
                dir = it
                    .next()
                    .ok_or_else(|| usage("--data-dir needs a value"))?
                    .into()
            }
            "--file" => {
                file = Some(
                    it.next()
                        .ok_or_else(|| usage("--file needs a value"))?
                        .into(),
                )
            }
            _ => break a,
        }
    };
    let rest: Vec<String> = it.collect();
    let path = file.unwrap_or_else(|| dir.join(HISTORY_FILE));
    // Options of the command: `--name value` pairs, flags, and the others.
    let mut opts = std::collections::BTreeMap::<String, String>::new();
    let mut flags = std::collections::BTreeSet::<String>::new();
    let mut words = Vec::new();
    let mut r = rest.into_iter();
    while let Some(a) = r.next() {
        if let Some(name) = a.strip_prefix("--") {
            if name == "deleted" {
                flags.insert(name.to_owned());
            } else {
                let v = r
                    .next()
                    .ok_or_else(|| usage(format!("{a} needs a value")))?;
                opts.insert(name.to_owned(), v);
            }
        } else {
            words.push(a);
        }
    }
    let allowed = |names: &[&str]| -> Result<(), Fail> {
        for k in opts.keys().chain(flags.iter()) {
            if !names.contains(&k.as_str()) {
                return Err(usage(format!("{command}: unknown option --{k}")));
            }
        }
        Ok(())
    };
    let opt_time = |k: &str| -> Result<Option<i64>, Fail> {
        opts.get(k)
            .map(|v| time(v).ok_or_else(|| usage(format!("--{k}: a time, got {v:?}"))))
            .transpose()
    };
    let opt_num = |k: &str| -> Result<Option<usize>, Fail> {
        opts.get(k)
            .map(|v| {
                v.parse()
                    .map_err(|_| usage(format!("--{k}: a number, got {v:?}")))
            })
            .transpose()
    };
    // Commands that do not need the base open.
    match command.as_str() {
        "wipe" => {
            allowed(&[])?;
            if !path.exists() {
                return Err(Fail::Run(format!("no history at {}", path.display())));
            }
            history::wipe(&path)?;
            eprintln!("deleted {}", path.display());
            return Ok(());
        }
        "cron" => {
            allowed(&["at"])?;
            let at = opts.get("at").map(String::as_str).unwrap_or("04:17");
            let (h, m) = at
                .split_once(':')
                .and_then(|(h, m)| Some((h.parse::<u8>().ok()?, m.parse::<u8>().ok()?)))
                .filter(|(h, m)| *h < 24 && *m < 60)
                .ok_or_else(|| usage(format!("--at: HH:MM, got {at:?}")))?;
            let exe = std::env::current_exe().map_err(|e| Fail::Run(e.to_string()))?;
            println!(
                "{m} {h} * * * '{}' --data-dir '{}' rotate --keep 2h >> '{}' 2>&1",
                exe.display(),
                dir.display(),
                dir.join("rotate.log").display()
            );
            return Ok(());
        }
        _ => {}
    }
    if !path.exists() {
        return Err(Fail::Run(format!(
            "no history at {} (the console makes it; --data-dir or --file)",
            path.display()
        )));
    }
    let h = HistoryFile::open(&path)?;
    match command.as_str() {
        "query" => {
            allowed(&[
                "kind", "cell", "group", "key", "text", "from", "to", "deleted", "order", "limit",
                "offset",
            ])?;
            let q = Query {
                kind: opts.get("kind").cloned(),
                cell: opts.get("cell").cloned(),
                group: opts.get("group").cloned(),
                key: opts.get("key").cloned(),
                text: opts.get("text").cloned(),
                from: opt_time("from")?,
                to: opt_time("to")?,
                deleted: flags.contains("deleted"),
                order: opts.get("order").cloned(),
                limit: opt_num("limit")?,
                offset: opt_num("offset")?,
            };
            for r in h.query(&q)? {
                println!("{}", json(&r));
            }
        }
        "get" => {
            allowed(&[])?;
            let [id] = ids(&words)?[..] else {
                return Err(usage("get: one row id"));
            };
            match h.get(id)? {
                Some(r) => println!("{}", json(&r)),
                None => return Err(Fail::Run(format!("no row {id}"))),
            }
        }
        "insert" => {
            allowed(&[])?;
            let (mut added, mut there) = (0, 0);
            for (n, line) in std::io::stdin().lock().lines().enumerate() {
                let line = line.map_err(|e| Fail::Run(e.to_string()))?;
                if line.trim().is_empty() {
                    continue;
                }
                let r: Record = serde_json::from_str(&line)
                    .map_err(|e| Fail::Run(format!("line {}: {e}", n + 1)))?;
                match h.insert(&r)? {
                    Some(_) => added += 1,
                    None => there += 1,
                }
            }
            eprintln!("{added} added, {there} already there");
        }
        "update" => {
            allowed(&[])?;
            let (Some(id), Some(content), 2) = (words.first(), words.get(1), words.len()) else {
                return Err(usage("update: <id> <content>"));
            };
            let id = ids(std::slice::from_ref(id))?[0];
            if !h.update(id, content)? {
                return Err(Fail::Run(format!("no row {id}, or deleted")));
            }
        }
        "delete" => {
            allowed(&[])?;
            eprintln!("{} deleted", h.delete(&ids(&words)?)?);
        }
        "remove" => {
            allowed(&[])?;
            eprintln!("{} removed", h.remove(&ids(&words)?)?);
        }
        "purge" => {
            allowed(&["before"])?;
            let before = opt_time("before")?.ok_or_else(|| usage("purge: --before <time>"))?;
            eprintln!("{} removed", h.purge(before)?);
        }
        "backup" => {
            allowed(&[])?;
            let to = match &words[..] {
                [] => history::archive_path(&dir.join("backups"), unix_now()),
                [f] => f.into(),
                _ => return Err(usage("backup: at most one file")),
            };
            h.backup(&to)?;
            println!("{}", to.display());
        }
        "rotate" => {
            allowed(&["keep", "dir"])?;
            let keep = match opts.get("keep") {
                Some(v) => {
                    duration(v).ok_or_else(|| usage(format!("--keep: a duration, got {v:?}")))?
                }
                None => 2 * 3600,
            };
            let to = opts
                .get("dir")
                .map(PathBuf::from)
                .unwrap_or_else(|| dir.join("backups"));
            let r = h.rotate(keep, &to)?;
            println!("{}", json(&r));
        }
        other => return Err(usage(format!("unknown command {other:?}"))),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn durations_and_times() {
        assert_eq!(duration("90s"), Some(90));
        assert_eq!(duration("30m"), Some(1800));
        assert_eq!(duration("2h"), Some(7200));
        assert_eq!(duration("7d"), Some(604_800));
        assert_eq!(duration("45"), Some(45));
        assert_eq!(duration("2w"), None);
        assert_eq!(duration("-1h"), None);
        assert_eq!(time("1793220448"), Some(1_793_220_448));
        let t = time("1h").unwrap();
        assert!((unix_now() - 3601..=unix_now() - 3599).contains(&t));
    }
}
