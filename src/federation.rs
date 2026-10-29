// SPDX-FileCopyrightText: 2026 William VITAL, FR (wvmscs)
// SPDX-License-Identifier: GPL-3.0-or-later

//! `--federation-check` (A5 of geohash-relays): reads the last hour of the
//! area's cells from every member of the federation, through Tor, and says
//! what each member lacks. Members can only omit events (they are signed).

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::sync::Arc;
use std::time::Duration;

use geohash_relays::a5::Comparer;
use geohash_relays_net::ports::SystemClock;
use geohash_relays_net::runner::Runner;
use tokio::sync::watch;

use crate::connector::Connector;

/// A check gives up after this long.
const RUN_LIMIT: Duration = Duration::from_secs(60);

/// What each member lacks, and the members that could not be read.
pub async fn check(
    members: &[String],
    cells: &[String],
    connector: Arc<dyn Connector>,
) -> (BTreeMap<String, BTreeSet<String>>, BTreeSet<String>) {
    let runner = Runner {
        connector,
        clock: Arc::new(SystemClock),
        fetch: None,
        isolation: "federation".into(),
        tick: Duration::from_millis(500),
    };
    let (stop, stop_rx) = watch::channel(false);
    let limit = stop.clone();
    let timer = tokio::spawn(async move {
        tokio::time::sleep(RUN_LIMIT).await;
        let _ = limit.send(true);
    });
    let c = runner
        .run(
            Comparer::new(members, cells),
            stop_rx,
            move |c: &Comparer| {
                if c.done() {
                    let _ = stop.send(true);
                }
            },
        )
        .await;
    timer.abort();
    (c.omissions(), c.failed().clone())
}

/// The report, in Markdown.
pub fn report(omissions: &BTreeMap<String, BTreeSet<String>>, failed: &BTreeSet<String>) -> String {
    let mut out = String::from("| Member | Events it lacks |\n|---|---|\n");
    for (m, ids) in omissions {
        let _ = writeln!(out, "| `{m}` | {} |", ids.len());
    }
    for m in failed {
        let _ = writeln!(out, "| `{m}` | not reachable |");
    }
    out
}
