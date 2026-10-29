// SPDX-FileCopyrightText: 2026 William VITAL, FR (wvmscs)
// SPDX-License-Identifier: GPL-3.0-or-later

//! `--qualify` and `--qualify-probe` (A6 of geohash-relays, §7 questions 1
//! to 3): the relays of both lists against the requirements, through Tor.
//!
//! `--qualify` only reads the NIP-11 documents. `--qualify-probe` also
//! writes one kind 20000 test event per relay, in the probe cell (open sea),
//! signed with a throwaway key, and reads it back: a real publication, so
//! only on the user's request.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::sync::Arc;
use std::time::Duration;

use geohash_relays::a6::{Prober, Qualification};
use geohash_relays::params::A6_PROBE_CELL;
use geohash_relays::{RelayDirectory, normalize_url};
use geohash_relays_net::ports::{ConnectorFetch, SystemClock};
use geohash_relays_net::runner::Runner;
use tokio::sync::watch;

use crate::connector::Connector;
use crate::nostr::{Event, unix_now};

/// A qualification run gives up after this long.
const RUN_LIMIT: Duration = Duration::from_secs(180);

/// Every relay of the lists, normalised, once.
pub fn relays_of(lists: &[&RelayDirectory]) -> Vec<String> {
    let set: BTreeSet<String> = lists
        .iter()
        .flat_map(|d| d.relays())
        .map(|r| normalize_url(&r.url).unwrap_or_else(|| r.url.clone()))
        .collect();
    set.into_iter().collect()
}

/// Qualifies `relays`: NIP-11 documents, and test events when `probe`.
pub async fn run(
    relays: Vec<String>,
    connector: Arc<dyn Connector>,
    probe: bool,
) -> (Prober, bool) {
    let mut tests = BTreeMap::new();
    if probe {
        let mut seed = [0u8; 32];
        rand_core::RngCore::fill_bytes(&mut rand_core::OsRng, &mut seed);
        let key = loop {
            if let Ok(k) = k256::schnorr::SigningKey::from_bytes(&seed) {
                break k;
            }
            seed[0] = seed[0].wrapping_add(1);
        };
        let now = unix_now();
        for (i, r) in relays.iter().enumerate() {
            let e = Event::signed(
                &key,
                now,
                crate::console::KIND_MESSAGE,
                vec![vec!["g".into(), A6_PROBE_CELL.into()]],
                format!("geohash-relays probe {i}"),
            );
            tests.insert(r.clone(), e.meta());
        }
    }
    let runner = Runner {
        connector: connector.clone(),
        clock: Arc::new(SystemClock),
        fetch: Some(Arc::new(ConnectorFetch(connector))),
        isolation: "qualify".into(),
        tick: Duration::from_secs(1),
    };
    let (stop, stop_rx) = watch::channel(false);
    let limit = stop.clone();
    let timer = tokio::spawn(async move {
        tokio::time::sleep(RUN_LIMIT).await;
        let _ = limit.send(true);
    });
    let prober = Prober::new(relays, tests, &[]);
    let p = runner
        .run(prober, stop_rx, move |p: &Prober| {
            if p.done() {
                let _ = stop.send(true);
            }
        })
        .await;
    timer.abort();
    let complete = p.done();
    if !complete {
        tracing::warn!("qualification stopped after {} s", RUN_LIMIT.as_secs());
    }
    (p, complete)
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
