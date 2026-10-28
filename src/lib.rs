//! BitChat console for location channels (geohash).
//!
//! Reads and writes a geohash cell and its 8 neighbours, through Tor, with
//! one key per cell like the app; DMs, mixed groups, and a page in the
//! browser. See `SPECIFICATIONS.md` and `PLAN_ET1.md`.

#[cfg(feature = "arti")]
pub mod arti;
pub mod connector;
pub mod console;
pub mod envelope;
mod error;
pub mod identity;
pub mod lists;
pub mod nostr;
pub mod packet;
pub mod pow;
pub mod relay;
pub mod server;
pub mod store;

pub use error::{Error, Result};

/// Installs ring as the process-wide rustls crypto provider, which libraries
/// that do not get an explicit one (Arti) look up. Calling it again is
/// harmless.
pub fn install_crypto_provider() {
    let _ = rustls::crypto::ring::default_provider().install_default();
}
