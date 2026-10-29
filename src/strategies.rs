// SPDX-FileCopyrightText: 2026 William VITAL, FR (wvmscs)
// SPDX-License-Identifier: GPL-3.0-or-later

//! The relay strategies the console can follow (`--strategy`), built from the
//! two relay lists. `AS` (the apps) by default; the alternatives of
//! geohash-relays as they come (its `PLAN_ET1.md`).

use std::sync::Arc;

use geohash_relays::a0::SharedStrategy;
use geohash_relays::a1::{HrwStrategy, Salts};
use geohash_relays::a5::Federation;
use geohash_relays::{AppStrategy, Apps, RelayDirectory, RelayHealth, Strategy};

use crate::console::Rebuild;

/// A strategy name of `--strategy`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Name {
    /// The apps' choice (`--apps` says which).
    As,
    /// One list (the iOS one) and the iOS rule, write 8, read 5, failed
    /// relays replaced, `shared` shared relays.
    A0,
    /// Salted rendezvous hashing on the iOS list (`--hrw-salt`).
    A1,
    /// A federation of synchronised relays (`--federation`).
    A5,
}

impl Name {
    pub const ALL: &'static str = "AS, A0, A1, A5";

    pub fn parse(s: &str) -> Option<Name> {
        match s.to_ascii_uppercase().as_str() {
            "AS" => Some(Name::As),
            "A0" => Some(Name::A0),
            "A1" => Some(Name::A1),
            "A5" => Some(Name::A5),
            _ => None,
        }
    }
}

/// What a strategy is built from.
pub struct Inputs {
    pub ios: RelayDirectory,
    pub android: Option<RelayDirectory>,
    pub apps: Apps,
    /// A0: shared relays added to every cell (0 to 2).
    pub shared: usize,
    /// A1: the salt (`None`: unsalted, which an attacker can grind).
    pub salt: Option<Vec<u8>>,
    /// A2: the monitors' candidates, for A1 instead of the iOS list.
    pub monitored: Option<RelayDirectory>,
    /// A5: the members of the federation.
    pub federation: Vec<String>,
}

/// The strategy `name` on these lists, and how to rebuild it when relays
/// fail (strategies that replace failed relays only).
pub fn build(name: Name, inputs: &Inputs) -> (Arc<dyn Strategy>, Option<Rebuild>) {
    match name {
        Name::As => (
            Arc::new(AppStrategy::new(
                Some(inputs.ios.clone()),
                inputs.android.clone(),
                inputs.apps,
            )),
            None,
        ),
        Name::A0 => {
            let base =
                SharedStrategy::from_directory(inputs.ios.clone()).with_shared(inputs.shared);
            let rebuild: Rebuild = Arc::new(move |health: &RelayHealth| {
                Arc::new(base.clone().with_health(health.clone())) as Arc<dyn Strategy>
            });
            (rebuild(&RelayHealth::new()), Some(rebuild))
        }
        Name::A5 => (Arc::new(Federation::new(&inputs.federation)), None),
        Name::A1 => {
            let salts = match &inputs.salt {
                Some(s) => Salts::Fixed(s.clone()),
                None => Salts::None,
            };
            (
                Arc::new(
                    HrwStrategy::new(inputs.monitored.as_ref().unwrap_or(&inputs.ios))
                        .with_salts(salts, crate::nostr::unix_now()),
                ),
                None,
            )
        }
    }
}
