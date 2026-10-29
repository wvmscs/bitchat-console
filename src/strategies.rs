// SPDX-FileCopyrightText: 2026 William VITAL, FR (wvmscs)
// SPDX-License-Identifier: GPL-3.0-or-later

//! The relay strategies the console can follow (`--strategy`), built from the
//! two relay lists. `AS` (the apps) by default; the alternatives of
//! geohash-relays as they come (its `PLAN_ET1.md`).

use std::sync::Arc;

use geohash_relays::{AppStrategy, Apps, RelayDirectory, Strategy};

/// A strategy name of `--strategy`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Name {
    /// The apps' choice (`--apps` says which).
    As,
}

impl Name {
    pub const ALL: &'static str = "AS";

    pub fn parse(s: &str) -> Option<Name> {
        match s.to_ascii_uppercase().as_str() {
            "AS" => Some(Name::As),
            _ => None,
        }
    }
}

/// What a strategy is built from.
pub struct Inputs {
    pub ios: RelayDirectory,
    pub android: Option<RelayDirectory>,
    pub apps: Apps,
}

/// The strategy `name` on these lists.
pub fn build(name: Name, inputs: &Inputs) -> Arc<dyn Strategy> {
    match name {
        Name::As => Arc::new(AppStrategy::new(
            Some(inputs.ios.clone()),
            inputs.android.clone(),
            inputs.apps,
        )),
    }
}
