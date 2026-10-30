// SPDX-FileCopyrightText: 2026 William VITAL, FR (wvmscs)
// SPDX-License-Identifier: GPL-3.0-or-later

//! What the user types for the cell to follow (feature `geocode`, lot 19,
//! D35): `#u09tv`, `u09tv`, a place or an IP address, through the
//! `geohash-lookup` crate; and the approximate name of a cell, for the page.
//!
//! Unlike the relays (D1), these requests do **not** go through Tor: BAN
//! (French addresses) and Nominatim (OpenStreetMap) see the text typed and
//! this machine's IP address. A geohash is never sent. IP addresses are
//! looked up locally, in a DB-IP file (`--geoip-db`), downloaded when
//! missing. Typed texts are never logged.

use std::collections::HashMap;
use std::path::Path;

use geohash_lookup::{CellInput, Geocoder, Location, Source, geoip};
use serde::Serialize;
use tokio::sync::Mutex;

use crate::console::CHANNEL_LEVELS;
use crate::error::{Error, Result};

/// Names kept at most (Nominatim allows one request a second).
const MAX_NAMES: usize = 256;

/// A place, as the page shows it.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Place {
    pub label: String,
    /// `BAN`, `Nominatim` or `GeoIP`.
    pub source: &'static str,
    /// The credit of the data, to show with the label.
    pub attribution: &'static str,
}

impl From<&Location> for Place {
    fn from(l: &Location) -> Self {
        Place {
            label: l.label.clone(),
            source: l.provider.as_str(),
            attribution: l.provider.attribution(),
        }
    }
}

/// A typed cell, resolved.
#[derive(Debug, Clone, PartialEq)]
pub struct Resolved {
    /// The cell to follow.
    pub geohash: String,
    /// What was found, when a place or an IP address was typed.
    pub place: Option<Place>,
}

/// The geocoder and the names already asked for.
pub struct Places {
    geocoder: Geocoder,
    names: Mutex<HashMap<String, Option<Place>>>,
}

impl std::fmt::Debug for Places {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Places")
            .field("geoip", &self.geocoder.has_geoip())
            .finish_non_exhaustive()
    }
}

/// User-Agent naming this program to the services.
fn user_agent() -> String {
    format!(
        "bitchat-console/{} (+https://github.com/wvmscs/bitchat-console)",
        env!("CARGO_PKG_VERSION")
    )
}

impl Places {
    /// The geocoder; with `geoip_db`, IP addresses too (the DB-IP file is
    /// downloaded there if missing, about 130 MB).
    pub async fn new(geoip_db: Option<&Path>) -> Result<Places> {
        let ua = user_agent();
        let mut builder = Geocoder::builder().user_agent(&ua);
        if let Some(path) = geoip_db {
            if let Some(url) = geoip::ensure(path, Some(&ua)).await.map_err(geocode)? {
                tracing::info!(%url, path = %path.display(), "GeoIP file downloaded ({})", geoip::DBIP_ATTRIBUTION);
            }
            builder = builder.geoip_db(path);
        }
        Ok(Places {
            geocoder: builder.build().map_err(geocode)?,
            names: Mutex::new(HashMap::new()),
        })
    }

    /// `#u09tv` or `u09tv` as is (lower case); a place or an IP address to
    /// the finest channel that holds it (2, 4, 5, 6 or 7 characters), or
    /// the region (2) around its point when it is larger.
    pub async fn resolve(&self, typed: &str) -> Result<Resolved> {
        match CellInput::parse_lenient(typed).map_err(geocode)? {
            CellInput::Geohash(geohash) => Ok(Resolved {
                geohash,
                place: None,
            }),
            CellInput::Query(q) => {
                let loc = self
                    .geocoder
                    .locate(&q, Source::Auto)
                    .await
                    .map_err(geocode)?
                    .ok_or(Error::PlaceNotFound)?;
                let cell = loc.cell_at(&CHANNEL_LEVELS).map_err(geocode)?;
                Ok(Resolved {
                    geohash: cell.geohash,
                    place: Some(Place::from(&loc)),
                })
            }
        }
    }

    /// The approximate name of a cell, asked once (errors are not kept:
    /// asked again next time).
    pub async fn name(&self, geohash: &str) -> Result<Option<Place>> {
        let g = geohash.trim().trim_start_matches('#').to_ascii_lowercase();
        if let Some(known) = self.names.lock().await.get(&g) {
            return Ok(known.clone());
        }
        let place = self
            .geocoder
            .reverse(&g)
            .await
            .map_err(geocode)?
            .as_ref()
            .map(Place::from);
        let mut names = self.names.lock().await;
        if names.len() >= MAX_NAMES {
            names.clear();
        }
        names.insert(g, place.clone());
        Ok(place)
    }
}

fn geocode(e: geohash_lookup::Error) -> Error {
    match e {
        geohash_lookup::Error::InvalidGeohash(g) => Error::InvalidGeohash(g),
        other => Error::Geocode(other.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Offline: geohashes are not geocoded.
    #[tokio::test]
    async fn geohashes_stay_as_typed() {
        let p = Places::new(None).await.unwrap();
        for (typed, want) in [("u09tv", "u09tv"), ("#U09TV", "u09tv"), (" #sr ", "sr")] {
            let r = p.resolve(typed).await.unwrap();
            assert_eq!((r.geohash.as_str(), r.place), (want, None));
        }
        assert!(matches!(
            p.resolve("#paris").await,
            Err(Error::InvalidGeohash(_))
        ));
        assert!(matches!(
            p.resolve("u0a").await,
            Err(Error::InvalidGeohash(_))
        ));
        assert!(matches!(p.resolve("  ").await, Err(Error::Geocode(_))));
        assert!(matches!(
            p.name("#paris").await,
            Err(Error::InvalidGeohash(_))
        ));
    }
}
