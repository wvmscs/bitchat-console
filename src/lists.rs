//! The apps' relay lists: downloaded through the connector (Tor, D21), the
//! last valid copy kept in the data directory as a fallback, or read from a
//! local file.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use geohash_relays::{Platform, RelayDirectory};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::connector::Connector;
use crate::error::{Error, Result};

/// Largest list accepted (the real ones are about 20 KB).
pub const MAX_LIST_BYTES: usize = 2 * 1024 * 1024;

/// Where a list comes from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ListSource {
    /// `https://…` (or `http://` on the loopback, for tests).
    Url(String),
    Path(PathBuf),
}

impl ListSource {
    pub fn official(platform: Platform) -> Self {
        ListSource::Url(platform.list_url().to_owned())
    }

    /// `http://` or `https://` is a URL, anything else a path.
    pub fn parse(s: &str) -> Self {
        if s.starts_with("https://") || s.starts_with("http://") {
            ListSource::Url(s.to_owned())
        } else {
            ListSource::Path(s.into())
        }
    }
}

/// Reads a list, falling back on the cached copy. A list without relays,
/// or an iOS list with a host at two places (the app refuses it), is not
/// used: the cached copy is, if any.
pub async fn load(
    source: &ListSource,
    platform: Platform,
    connector: Arc<dyn Connector>,
    cache_dir: &Path,
) -> Result<RelayDirectory> {
    let cache = cache_dir.join(format!("{}.csv", platform.as_str()));
    let fresh = match source {
        ListSource::Path(p) => std::fs::read_to_string(p).map_err(Error::from),
        ListSource::Url(url) => fetch(url, connector.as_ref()).await,
    };
    let refused = match fresh {
        Ok(csv) => {
            let dir = RelayDirectory::parse(&csv, platform);
            match (dir.is_empty(), dir.conflict()) {
                (false, None) => {
                    if matches!(source, ListSource::Url(_)) {
                        std::fs::create_dir_all(cache_dir)?;
                        std::fs::write(&cache, &csv)?;
                    }
                    tracing::info!(
                        platform = platform.as_str(),
                        relays = dir.len(),
                        "relay list loaded"
                    );
                    return Ok(dir);
                }
                (true, _) => "no usable relay".to_owned(),
                (false, Some(host)) => format!("host {host} at two places (the app refuses it)"),
            }
        }
        Err(e) => e.to_string(),
    };
    match std::fs::read_to_string(&cache) {
        Ok(csv) => {
            tracing::warn!(platform = platform.as_str(), reason = %refused, "relay list not usable, using the cached copy");
            Ok(RelayDirectory::parse(&csv, platform))
        }
        Err(_) => Err(Error::NoRelayList {
            platform: platform.as_str().to_owned(),
            reason: refused,
        }),
    }
}

/// `GET url` through the connector, HTTP/1.0 (no chunked body), TLS for
/// `https://`.
pub async fn fetch(url: &str, connector: &dyn Connector) -> Result<String> {
    let bad = |what: &str| Error::Http(format!("{url}: {what}"));
    let (tls, rest) = if let Some(r) = url.strip_prefix("https://") {
        (true, r)
    } else if let Some(r) = url.strip_prefix("http://") {
        (false, r)
    } else {
        return Err(bad("not an http(s) URL"));
    };
    let (authority, path) = rest.split_at(rest.find('/').unwrap_or(rest.len()));
    let path = if path.is_empty() { "/" } else { path };
    let (host, port) = match authority.rsplit_once(':') {
        Some((h, p)) => (h, p.parse().map_err(|_| bad("port"))?),
        None => (authority, if tls { 443 } else { 80 }),
    };
    let request = format!(
        "GET {path} HTTP/1.0\r\nHost: {host}\r\nUser-Agent: bitchat-console\r\nAccept: text/csv, text/plain\r\n\r\n"
    );
    let stream = connector.connect(host, port, "relay-lists").await?;
    let mut body = Vec::new();
    if tls {
        let name = rustls::pki_types::ServerName::try_from(host.to_owned())
            .map_err(|_| bad("host name"))?;
        let mut s = tokio_rustls::TlsConnector::from(crate::relay::tls_config())
            .connect(name, stream)
            .await?;
        s.write_all(request.as_bytes()).await?;
        (&mut s)
            .take(MAX_LIST_BYTES as u64 + 64 * 1024)
            .read_to_end(&mut body)
            .await?;
    } else {
        let mut s = stream;
        s.write_all(request.as_bytes()).await?;
        (&mut s)
            .take(MAX_LIST_BYTES as u64 + 64 * 1024)
            .read_to_end(&mut body)
            .await?;
    }
    let split = body
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .ok_or_else(|| bad("no HTTP header"))?;
    let head = String::from_utf8_lossy(&body[..split]);
    let status = head.split_whitespace().nth(1).unwrap_or("");
    if status != "200" {
        return Err(bad(&format!("HTTP status {status}")));
    }
    let content = &body[split + 4..];
    if content.len() > MAX_LIST_BYTES {
        return Err(bad("list too large"));
    }
    String::from_utf8(content.to_vec()).map_err(|_| bad("not UTF-8 text"))
}
