// SPDX-FileCopyrightText: 2026 William VITAL, FR (wvmscs)
// SPDX-License-Identifier: GPL-3.0-or-later

//! `--republish` (A4 of geohash-relays, its decision G10): the console also
//! republishes, between the iOS relay set and the Android relay set of each
//! followed cell, the events one set has and the other has not. Off by
//! default: it publishes other people's (signed, unchanged) events.
//!
//! The republisher follows the console's area: when the area changes, it
//! starts again on the new cells. Its connections go through the console's
//! connector (Tor), on their own circuit (`republish`).

use std::sync::Arc;
use std::time::Duration;

use geohash_relays::a4::{Plan, Republisher};
use geohash_relays::{AppStrategy, RelayDirectory, Strategy};
use geohash_relays_net::ports::SystemClock;
use geohash_relays_net::runner::Runner;
use tokio::sync::watch;
use tokio::task::JoinHandle;

use crate::connector::Connector;
use crate::console::{Console, ConsoleEvent};

/// Ticks of the republisher, and how often it logs its counters.
const TICK: Duration = Duration::from_secs(1);
const LOG_EVERY_TICKS: u32 = 60;

/// Starts republishing for the console's area, following it.
pub fn spawn(
    console: Arc<Console>,
    ios: RelayDirectory,
    android: Option<RelayDirectory>,
    connector: Arc<dyn Connector>,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut events = console.subscribe();
        let sets: Vec<Box<dyn Strategy>> =
            std::iter::once(Box::new(AppStrategy::ios(ios)) as Box<dyn Strategy>)
                .chain(android.map(|a| Box::new(AppStrategy::android(a)) as Box<dyn Strategy>))
                .collect();
        loop {
            let cells: Vec<String> = match console.status().await {
                Ok(s) => s.cells.into_iter().map(|c| c.geohash).collect(),
                Err(_) => return,
            };
            let refs: Vec<&dyn Strategy> = sets.iter().map(|s| s.as_ref()).collect();
            let (stop, stop_rx) = watch::channel(false);
            let run = match Plan::of(&cells, &refs) {
                Ok(plan) if !cells.is_empty() => {
                    tracing::info!(
                        cells = cells.len(),
                        relays = plan.relays().len(),
                        "republishing"
                    );
                    let runner = Runner {
                        connector: connector.clone(),
                        clock: Arc::new(SystemClock),
                        fetch: None,
                        isolation: "republish".into(),
                        tick: TICK,
                    };
                    Some(tokio::spawn(async move {
                        let mut ticks = 0u32;
                        runner
                            .run(Republisher::new(plan), stop_rx, |m: &Republisher| {
                                ticks += 1;
                                if ticks.is_multiple_of(LOG_EVERY_TICKS) {
                                    let c = m.counters();
                                    tracing::info!(
                                        seen = c.seen,
                                        republished = c.republished,
                                        skipped = c.skipped,
                                        limited = c.limited,
                                        refused = c.refused,
                                        "republisher"
                                    );
                                }
                            })
                            .await;
                    }))
                }
                _ => None,
            };
            // Until the area changes (or the console stops).
            loop {
                match events.recv().await {
                    Ok(ConsoleEvent::Area) => break,
                    Ok(_) => {}
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                    Err(_) => {
                        let _ = stop.send(true);
                        return;
                    }
                }
            }
            let _ = stop.send(true);
            if let Some(run) = run {
                let _ = run.await;
            }
        }
    })
}
