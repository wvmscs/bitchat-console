//! BitChat console for location channels (geohash).
//!
//! Reads and writes a geohash cell and its 8 neighbours, through Tor, with
//! one key per cell like the app; DMs, mixed groups, and a page in the
//! browser. See `SPECIFICATIONS.md` and `PLAN_ET1.md`.

mod error;

pub use error::{Error, Result};
