// SPDX-FileCopyrightText: 2026 William VITAL, FR (wvmscs)
// SPDX-License-Identifier: GPL-3.0-or-later

//! `--qualify` and `--qualify-probe` (A6 of geohash-relays, §7 questions 1
//! to 3): the relays of both lists against the requirements, through Tor.
//!
//! `--qualify` only reads the NIP-11 documents. `--qualify-probe` also
//! writes one kind 20000 test event per relay, in the probe cell (open sea),
//! signed with a throwaway key, and reads it back: a real publication, so
//! only on the user's request.

use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::sync::Arc;
use std::time::Duration;

use geohash_relays::a6::{Prober, Qualification};
use geohash_relays::{RelayDirectory, normalize_url};

use crate::connector::Connector;
use crate::nostr::unix_now;

/// A qualification run gives up after this long.
const RUN_LIMIT: Duration = Duration::from_secs(600);

/// Every relay of the lists, normalised, once.
pub fn relays_of(lists: &[&RelayDirectory]) -> Vec<String> {
    let set: BTreeSet<String> = lists
        .iter()
        .flat_map(|d| d.relays())
        .map(|r| normalize_url(&r.url).unwrap_or_else(|| r.url.clone()))
        .collect();
    set.into_iter().collect()
}

/// Qualifies `relays`: NIP-11 documents, and test events when `probe`
/// (geohash-relays-net's run, shared with its census).
pub async fn run(
    relays: Vec<String>,
    connector: Arc<dyn Connector>,
    probe: bool,
) -> (Prober, bool) {
    geohash_relays_net::qualify::run(relays, connector, probe, RUN_LIMIT).await
}

/// The report, in Markdown.
pub fn report(prober: &Prober, probe: bool, complete: bool) -> String {
    let q: Qualification = prober.qualification();
    let mut out = String::new();
    let total = q.verdicts.len();
    let documents = q
        .verdicts
        .keys()
        .filter(|r| prober.nip11_of(r).is_some())
        .count();
    let refused = q.refused();
    let _ = writeln!(out, "# Relay qualification (A6), {}\n", unix_now());
    let _ = writeln!(
        out,
        "{total} relays, {documents} with a NIP-11 document, {} refused{}{}.\n",
        refused.len(),
        if probe {
            ", probed by writing"
        } else {
            ", declarations only"
        },
        if complete {
            ""
        } else {
            " (incomplete: time limit)"
        }
    );
    let _ = writeln!(out, "| Relay | Refused | Warnings |\n|---|---|---|");
    for (url, v) in &q.verdicts {
        if v.refusals.is_empty() && v.warnings.is_empty() {
            continue;
        }
        let names = |f: &[geohash_relays::a6::Finding]| {
            f.iter()
                .map(|x| format!("{} {x:?}", x.requirement()))
                .collect::<Vec<_>>()
                .join(", ")
        };
        let _ = writeln!(
            out,
            "| `{url}` | {} | {} |",
            names(&v.refusals),
            names(&v.warnings)
        );
    }
    out
}
