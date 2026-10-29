// SPDX-FileCopyrightText: 2026 William VITAL, FR (wvmscs)
// SPDX-License-Identifier: GPL-3.0-or-later

//! BitChat console for location channels (geohash).
//!
//! Reads and writes a geohash cell and its 8 neighbours, through Tor, with
//! one key per cell like the app; DMs, mixed groups, and a page in the
//! browser. See `SPECIFICATIONS.md` and `PLAN_ET1.md`.

pub mod console;
pub mod db;
pub mod envelope;
mod error;
pub mod federation;
pub mod groups;
pub mod history;
pub mod identity;
pub mod monitors;
pub mod packet;
pub mod pow;
pub mod qualify;
pub mod republish;
pub mod server;
pub mod store;
pub mod strategies;

// The communication, moved to geohash-relays-net (feature `console`): same
// paths as before.
#[cfg(feature = "arti")]
pub use geohash_relays_net::arti;
pub use geohash_relays_net::{connector, http, lists, nostr, relay};

pub use error::{Error, Result};

/// Installs ring as the process-wide rustls crypto provider, which libraries
/// that do not get an explicit one (Arti) look up. Calling it again is
/// harmless.
pub fn install_crypto_provider() {
    let _ = rustls::crypto::ring::default_provider().install_default();
}
