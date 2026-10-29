// SPDX-FileCopyrightText: 2026 William VITAL, FR (wvmscs)
// SPDX-License-Identifier: GPL-3.0-or-later

//! `--monitors` (A2 of geohash-relays): the relay directory of trusted NIP-66
//! monitors, read through Tor from `--monitor-relays`, at start. Its
//! candidates feed A1; its shared verdicts filter the lists of `AS` and A0
//! (unless they would remove more than half of a list). The last directory
//! is kept in `monitors.csv` of the data directory, used when the monitors
//! cannot be read; without it, the GitHub lists are used as they are.

use std::collections::BTreeSet;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use geohash_relays::a2::{Directory, Watcher};
use geohash_relays::{Platform, RelayDirectory};
use geohash_relays_net::ports::SystemClock;
use geohash_relays_net::runner::Runner;
use tokio::sync::watch;

use crate::connector::Connector;

/// Reading the monitors gives up after this long.
const READ_LIMIT: Duration = Duration::from_secs(60);
/// The cached directory in the data directory.
pub const CACHE_FILE: &str = "monitors.csv";

/// The monitors' directory: read now, else the cached one, else `None`.
pub async fn load(
    monitors: BTreeSet<String>,
    relays: Vec<String>,
    connector: Arc<dyn Connector>,
    data_dir: &Path,
) -> Option<(Directory, RelayDirectory)> {
    let runner = Runner {
        connector,
        clock: Arc::new(SystemClock),
        fetch: None,
        isolation: "monitors".into(),
        tick: Duration::from_secs(1),
    };
    let (stop, stop_rx) = watch::channel(false);
    let limit = stop.clone();
    let timer = tokio::spawn(async move {
        tokio::time::sleep(READ_LIMIT).await;
        let _ = limit.send(true);
    });
    let w = runner
        .run(
            Watcher::new(relays, monitors),
            stop_rx,
            move |w: &Watcher| {
                if w.caught_up() {
                    let _ = stop.send(true);
                }
            },
        )
        .await;
    timer.abort();
    let dir = w.default_directory();
    let cache = data_dir.join(CACHE_FILE);
    if !dir.candidates.is_empty() {
        let list = dir.to_list();
        let csv: String = std::iter::once("Relay URL,Latitude,Longitude\n".to_owned())
            .chain(
                list.relays()
                    .iter()
                    .map(|r| format!("{},{},{}\n", r.url, r.lat, r.lon)),
            )
            .collect();
        if let Err(e) = std::fs::write(&cache, csv) {
            tracing::warn!(error = %e, "cannot keep the monitors' directory");
        }
        tracing::info!(
            candidates = dir.candidates.len(),
            reports = w.reports().len(),
            "monitors read"
        );
        return Some((dir, list));
    }
    tracing::warn!("no usable report from the monitors");
    let csv = std::fs::read_to_string(&cache).ok()?;
    let list = RelayDirectory::parse(&csv, Platform::Ios);
    (!list.is_empty()).then(|| {
        tracing::info!(
            candidates = list.len(),
            "monitors' directory from the cache"
        );
        (Directory::default(), list)
    })
}
