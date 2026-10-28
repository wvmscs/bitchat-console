//! The map of bitchat-geohash relayed under the console, against a stand-in
//! that checks Host and Origin as bitchat-geohash does.

mod common;

use std::convert::Infallible;
use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use axum::extract::Request;
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::sse::{Event, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use bitchat_console::connector::Loopback;
use bitchat_console::server::{ServerConfig, serve};
use common::*;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};

const MAP_CSP: &str =
    "default-src 'none'; script-src 'self'; img-src 'self' https://tile.openstreetmap.org";

/// A stand-in for bitchat-geohash: its own Host only, commands from its own
/// origin only.
async fn fake_map() -> std::net::SocketAddr {
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = l.local_addr().unwrap();
    let own = addr.to_string();
    let check = move |h: &HeaderMap, post: bool| -> Option<Response> {
        if h.get(header::HOST).and_then(|v| v.to_str().ok()) != Some(own.as_str()) {
            return Some(StatusCode::MISDIRECTED_REQUEST.into_response());
        }
        if post
            && h.get(header::ORIGIN).and_then(|v| v.to_str().ok()) != Some(&format!("http://{own}"))
        {
            return Some(StatusCode::FORBIDDEN.into_response());
        }
        None
    };
    let (c1, c2, c3) = (check.clone(), check.clone(), check);
    let app = Router::new()
        .route(
            "/",
            get(move |req: Request| async move {
                if let Some(r) = c1(req.headers(), false) {
                    return r;
                }
                (
                    [(header::CONTENT_TYPE, "text/html; charset=utf-8"), (header::CONTENT_SECURITY_POLICY, MAP_CSP)],
                    "<!doctype html><html><head><title>map</title></head><body><div id=map></div></body></html>",
                )
                    .into_response()
            }),
        )
        .route(
            "/api/descend",
            post(move |req: Request| async move {
                if let Some(r) = c2(req.headers(), true) {
                    return r;
                }
                axum::Json(serde_json::json!({ "added": ["u09tvw"] })).into_response()
            }),
        )
        .route(
            "/api/events",
            get(move |req: Request| async move {
                if let Some(r) = c3(req.headers(), false) {
                    return r;
                }
                let s = futures_util::stream::unfold(0u32, |n| async move {
                    if n > 0 {
                        tokio::time::sleep(Duration::from_millis(200)).await;
                    }
                    Some((Ok::<_, Infallible>(Event::default().event("hello").data(n.to_string())), n + 1))
                });
                Sse::new(s).into_response()
            }),
        );
    tokio::spawn(async move { axum::serve(l, app).await.unwrap() });
    addr
}

async fn http(
    addr: std::net::SocketAddr,
    method: &str,
    path: &str,
    headers: &[(&str, String)],
) -> (u16, String, String) {
    let mut s = TcpStream::connect(addr).await.unwrap();
    let mut req = format!("{method} {path} HTTP/1.1\r\nConnection: close\r\nContent-Length: 2\r\n");
    for (k, v) in headers {
        req.push_str(&format!("{k}: {v}\r\n"));
    }
    req.push_str("\r\n{}");
    s.write_all(req.as_bytes()).await.unwrap();
    let mut out = String::new();
    s.read_to_string(&mut out).await.unwrap();
    let (head, body) = out.split_once("\r\n\r\n").unwrap_or((&out, ""));
    (
        head.split_whitespace().nth(1).unwrap().parse().unwrap(),
        head.to_owned(),
        body.to_owned(),
    )
}

async fn console_over(map: Option<String>) -> std::net::SocketAddr {
    let relay = TestRelay::spawn("127.0.0.1:0").await.unwrap();
    let c = Arc::new(console(&[relay.url()], Arc::new(Loopback)));
    c.set_area("u09tv", true).await.unwrap();
    std::mem::forget(relay);
    let server = serve(
        c,
        ServerConfig {
            listen: "127.0.0.1:0".parse().unwrap(),
            map,
            ..ServerConfig::default()
        },
    )
    .await
    .unwrap();
    let addr = server.local_addr();
    std::mem::forget(server);
    addr
}

#[tokio::test]
async fn the_map_with_the_console_over_it() {
    let map = fake_map().await;
    let addr = console_over(Some(format!("http://{map}"))).await;
    let host = ("Host", addr.to_string());

    // The map page, with the console's overlay and a frame allowed.
    let (status, head, body) = http(addr, "GET", "/", std::slice::from_ref(&host)).await;
    assert_eq!(status, 200);
    assert!(
        body.contains("/console/assets/overlay.js") && body.contains("<div id=map>"),
        "{body}"
    );
    // The hook goes in as a classic script, before the overlay's module.
    let hook = body
        .find("<script src=\"/console/assets/hook.js\"></script>")
        .unwrap();
    assert!(
        hook < body.find("/console/assets/overlay.js").unwrap(),
        "{body}"
    );
    let csp = head
        .lines()
        .find(|l| l.to_lowercase().starts_with("content-security-policy"))
        .unwrap();
    assert!(
        csp.contains("https://tile.openstreetmap.org") && csp.contains("frame-src 'self'"),
        "{csp}"
    );

    // The console at /console/, its API there too.
    let (status, _, body) = http(addr, "GET", "/console/", std::slice::from_ref(&host)).await;
    assert_eq!(status, 200);
    assert!(body.contains("BitChat console"));
    let (status, _, body) = http(
        addr,
        "GET",
        "/console/api/state",
        std::slice::from_ref(&host),
    )
    .await;
    assert_eq!(status, 200);
    assert!(body.contains("\"center\":\"u09tv\""));
    let (status, _, _) = http(
        addr,
        "GET",
        "/console/assets/overlay.css",
        std::slice::from_ref(&host),
    )
    .await;
    assert_eq!(status, 200);

    // A command of the map page goes through with the map's own origin.
    let (status, _, body) = http(
        addr,
        "POST",
        "/api/descend",
        &[
            host.clone(),
            ("Origin", format!("http://{addr}")),
            ("Content-Type", "application/json".into()),
        ],
    )
    .await;
    assert_eq!((status, body.contains("u09tvw")), (200, true), "{body}");
    // But not from another page: the console's own check comes first.
    let (status, _, _) = http(
        addr,
        "POST",
        "/api/descend",
        &[
            host.clone(),
            ("Origin", "http://evil.example".into()),
            ("Content-Type", "application/json".into()),
        ],
    )
    .await;
    assert_eq!(status, 403);

    // The live stream of the map flows through.
    let mut s = TcpStream::connect(addr).await.unwrap();
    s.write_all(format!("GET /api/events HTTP/1.1\r\nHost: {addr}\r\n\r\n").as_bytes())
        .await
        .unwrap();
    let mut lines = BufReader::new(s).lines();
    let mut data = Vec::new();
    while data.len() < 2 {
        let line = tokio::time::timeout(Duration::from_secs(5), lines.next_line())
            .await
            .expect("stream stalled")
            .unwrap()
            .unwrap();
        if let Some(d) = line.strip_prefix("data:") {
            data.push(d.trim().to_owned());
        }
    }
    assert_eq!(data, ["0", "1"]);
}

#[tokio::test]
async fn a_map_that_is_not_running() {
    let closed = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let gone = closed.local_addr().unwrap();
    drop(closed);
    let addr = console_over(Some(format!("http://{gone}"))).await;
    let (status, _, body) = http(addr, "GET", "/", &[("Host", addr.to_string())]).await;
    assert_eq!(status, 502);
    assert!(body.contains("map_unreachable"), "{body}");
    // The console itself still works.
    let (status, _, _) = http(
        addr,
        "GET",
        "/console/api/state",
        &[("Host", addr.to_string())],
    )
    .await;
    assert_eq!(status, 200);
}

#[tokio::test]
async fn stopping_ends_the_map_streams() {
    let map = fake_map().await;
    let relay = TestRelay::spawn("127.0.0.1:0").await.unwrap();
    let c = Arc::new(console(&[relay.url()], Arc::new(Loopback)));
    let server = serve(
        c,
        ServerConfig {
            listen: "127.0.0.1:0".parse().unwrap(),
            map: Some(format!("http://{map}")),
            ..ServerConfig::default()
        },
    )
    .await
    .unwrap();
    let addr = server.local_addr();
    let mut s = TcpStream::connect(addr).await.unwrap();
    s.write_all(format!("GET /api/events HTTP/1.1\r\nHost: {addr}\r\n\r\n").as_bytes())
        .await
        .unwrap();
    let mut lines = BufReader::new(s).lines();
    while !lines
        .next_line()
        .await
        .unwrap()
        .unwrap()
        .starts_with("data:")
    {}
    // A browser that keeps the map open does not hold the server.
    tokio::time::timeout(Duration::from_secs(5), server.shutdown())
        .await
        .expect("the server waited for the map's stream");
    let end = tokio::time::timeout(Duration::from_secs(5), async {
        while let Ok(Some(_)) = lines.next_line().await {}
    })
    .await;
    assert!(end.is_ok(), "the stream did not end");
}
