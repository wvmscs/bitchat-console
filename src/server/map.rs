//! The map of bitchat-geohash under the console (lot 9, D26): a running
//! bitchat-geohash server is relayed at `/`, its page gets the console as a
//! floating panel (`/console/` in a frame) and a "Follow in console" button
//! in the menu of a cell.
//!
//! Relayed, not linked in: bitchat-geohash 0.1.0 and Arti need different
//! SQLite versions, and bitchat-geohash stays untouched.

use std::sync::Arc;

use axum::Extension;
use axum::body::Body;
use axum::extract::Request;
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use http_body_util::BodyExt;
use hyper_util::rt::TokioIo;

use super::api::ApiError;
use crate::error::{Error, Result};

/// The bitchat-geohash server: `http://host:port`, on this machine.
#[derive(Debug, Clone)]
pub(crate) struct Upstream {
    authority: String,
    origin: String,
}

impl Upstream {
    pub(crate) fn new(url: &str) -> Result<Upstream> {
        let authority = url
            .strip_prefix("http://")
            .map(|r| r.trim_end_matches('/'))
            .filter(|a| !a.is_empty() && !a.contains('/'))
            .ok_or_else(|| {
                Error::Map(format!(
                    "{url}: expected http://host:port (bitchat-geohash without TLS)"
                ))
            })?;
        Ok(Upstream {
            authority: authority.to_owned(),
            origin: format!("http://{authority}"),
        })
    }
}

/// What the page of the map loads besides itself.
const INJECTED: &str = "<link rel=\"stylesheet\" href=\"/console/assets/overlay.css\">\
     <script type=\"module\" src=\"/console/assets/overlay.js\"></script>";

/// Headers that belong to one connection, not to the request.
fn hop_by_hop(name: &str) -> bool {
    matches!(
        name,
        "connection"
            | "keep-alive"
            | "transfer-encoding"
            | "upgrade"
            | "te"
            | "trailer"
            | "proxy-connection"
    )
}

/// Relays a request to the map server and its answer back; the map's page
/// gets the console added.
pub(crate) async fn proxy(Extension(up): Extension<Arc<Upstream>>, req: Request) -> Response {
    match relay(&up, req).await {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!(map = %up.authority, error = %e, "map unreachable");
            ApiError::new(
                StatusCode::BAD_GATEWAY,
                "map_unreachable",
                format!("map at {}: {e}", up.origin),
            )
            .into_response()
        }
    }
}

async fn relay(
    up: &Upstream,
    req: Request,
) -> std::result::Result<Response, Box<dyn std::error::Error + Send + Sync>> {
    let page = req.uri().path() == "/";
    let (parts, body) = req.into_parts();
    let mut out = axum::http::Request::builder()
        .method(parts.method.clone())
        .uri(
            parts
                .uri
                .path_and_query()
                .map(|p| p.as_str())
                .unwrap_or("/"),
        );
    for (name, value) in &parts.headers {
        let n = name.as_str();
        if hop_by_hop(n) || n == "host" || n == "origin" || n == "referer" {
            continue;
        }
        out = out.header(name, value);
    }
    // The map checks Host and, for commands, Origin: they are its own.
    out = out.header(header::HOST, &up.authority);
    if parts.headers.contains_key(header::ORIGIN) {
        out = out.header(header::ORIGIN, &up.origin);
    }
    let request = out.body(body)?;
    let stream = tokio::net::TcpStream::connect(&up.authority).await?;
    let (mut sender, conn) = hyper::client::conn::http1::handshake(TokioIo::new(stream)).await?;
    tokio::spawn(async move {
        let _ = conn.await;
    });
    let answer = sender.send_request(request).await?;
    let (mut parts, incoming) = answer.into_parts();
    let drop: Vec<_> = parts
        .headers
        .keys()
        .filter(|n| hop_by_hop(n.as_str()))
        .cloned()
        .collect();
    for n in drop {
        parts.headers.remove(n);
    }
    let html = parts
        .headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.starts_with("text/html"));
    if page && html {
        // The map page: the console's files, and a frame of the console.
        let bytes = incoming.collect().await?.to_bytes();
        let text =
            String::from_utf8_lossy(&bytes).replacen("</head>", &format!("{INJECTED}</head>"), 1);
        if let Some(csp) = parts
            .headers
            .get(header::CONTENT_SECURITY_POLICY)
            .and_then(|v| v.to_str().ok())
        {
            let csp = format!("{csp}; frame-src 'self'");
            parts.headers.insert(
                header::CONTENT_SECURITY_POLICY,
                HeaderValue::from_str(&csp)?,
            );
        }
        parts.headers.remove(header::CONTENT_LENGTH);
        return Ok(Response::from_parts(parts, Body::from(text)));
    }
    // Everything else as it comes, streams (SSE) included.
    Ok(Response::from_parts(parts, Body::new(incoming)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upstream_addresses() {
        let u = Upstream::new("http://127.0.0.1:8080/").unwrap();
        assert_eq!(
            (u.authority.as_str(), u.origin.as_str()),
            ("127.0.0.1:8080", "http://127.0.0.1:8080")
        );
        assert!(Upstream::new("https://127.0.0.1:8443").is_err());
        assert!(Upstream::new("http://h/x").is_err());
    }
}
