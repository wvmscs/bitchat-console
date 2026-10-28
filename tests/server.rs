//! The page and its API: files, state, protections, commands, live stream.

mod common;

use std::sync::Arc;
use std::time::Duration;

use bitchat_console::connector::Loopback;
use bitchat_console::console::{Console, KIND_MESSAGE};
use bitchat_console::server::{Asset, ServerConfig, serve};
use common::*;
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;

/// One HTTP/1.1 request, `Connection: close`: (status, body).
async fn http(
    addr: std::net::SocketAddr,
    method: &str,
    path: &str,
    headers: &[(&str, String)],
    body: &str,
) -> (u16, String) {
    let mut s = TcpStream::connect(addr).await.unwrap();
    let mut req = format!(
        "{method} {path} HTTP/1.1\r\nConnection: close\r\nContent-Length: {}\r\n",
        body.len()
    );
    for (k, v) in headers {
        req.push_str(&format!("{k}: {v}\r\n"));
    }
    req.push_str("\r\n");
    req.push_str(body);
    s.write_all(req.as_bytes()).await.unwrap();
    let mut out = String::new();
    s.read_to_string(&mut out).await.unwrap();
    let status = out.split_whitespace().nth(1).unwrap().parse().unwrap();
    let body = out
        .split_once("\r\n\r\n")
        .map(|(_, b)| b.to_owned())
        .unwrap_or_default();
    (status, body)
}

struct Running {
    addr: std::net::SocketAddr,
    relay: TestRelay,
    console: Arc<Console>,
}

impl Running {
    async fn start() -> Running {
        let relay = TestRelay::spawn("127.0.0.1:0").await.unwrap();
        let console = Arc::new(console(&[relay.url()], Arc::new(Loopback)));
        console.set_area("u09tv", true).await.unwrap();
        status_until(&console, "connected", all_connected).await;
        let server = serve(
            console.clone(),
            ServerConfig {
                listen: "127.0.0.1:0".parse().unwrap(),
                ..ServerConfig::default()
            },
        )
        .await
        .unwrap();
        let addr = server.local_addr();
        std::mem::forget(server);
        Running {
            addr,
            relay,
            console,
        }
    }

    fn host(&self) -> (&'static str, String) {
        ("Host", self.addr.to_string())
    }

    async fn post(&self, path: &str, body: &str) -> (u16, Value) {
        let (status, b) = http(
            self.addr,
            "POST",
            path,
            &[
                self.host(),
                ("Origin", format!("http://{}", self.addr)),
                ("Content-Type", "application/json".into()),
            ],
            body,
        )
        .await;
        (status, serde_json::from_str(&b).unwrap_or(Value::Null))
    }
}

#[tokio::test]
async fn page_and_state() {
    let r = Running::start().await;
    let (status, body) = http(r.addr, "GET", "/", &[r.host()], "").await;
    assert_eq!(status, 200);
    assert!(body.contains("BitChat console"));
    for a in Asset::all() {
        let (status, _) = http(
            r.addr,
            "GET",
            &format!("/assets/{}", a.path),
            &[r.host()],
            "",
        )
        .await;
        assert_eq!(status, 200, "{}", a.path);
    }
    let (status, body) = http(r.addr, "GET", "/api/state", &[r.host()], "").await;
    assert_eq!(status, 200);
    let v: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["status"]["center"], "u09tv");
    assert_eq!(v["status"]["teleport"], true);
    assert_eq!(v["status"]["cells"].as_array().unwrap().len(), 9);
    assert_eq!(
        v["status"]["keys"]["u09tv"],
        test_seed().cell("u09tv").unwrap().pubkey_hex()
    );
    assert!(v["messages"]["u09ty"].as_array().unwrap().is_empty());
    // The participants of a cell, on demand.
    let (status, body) = http(
        r.addr,
        "GET",
        "/api/participants?cell=u09tv",
        &[r.host()],
        "",
    )
    .await;
    assert_eq!((status, body.as_str()), (200, "[]"));
    let (status, _) = http(r.addr, "GET", "/api/participants", &[r.host()], "").await;
    assert_eq!(status, 400);
    // The nickname and the cells where it is not sent, in the state (D32).
    assert_eq!(v["local"]["nickname"], Value::Null);
    let (status, b) = r.post("/api/nickname", r#"{"nickname":" bob "}"#).await;
    assert_eq!((status, b["nickname"].as_str()), (200, Some("bob")));
    let (status, _) = r.post("/api/anon", r#"{"cell":"u09ty","anon":true}"#).await;
    assert_eq!(status, 200);
    let (status, b) = r.post("/api/anon", r#"{"cell":"u09","anon":true}"#).await;
    assert_eq!((status, b["error"].as_str()), (400, Some("not_a_channel")));
    let (_, body) = http(r.addr, "GET", "/api/state", &[r.host()], "").await;
    let v: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(
        (&v["local"]["nickname"], &v["local"]["anon_cells"]),
        (&json!("bob"), &json!(["u09ty"]))
    );
}

#[tokio::test]
async fn commands_are_protected() {
    let r = Running::start().await;
    // Another host name (DNS rebinding): refused.
    let (status, _) = http(
        r.addr,
        "GET",
        "/api/state",
        &[("Host", "evil.example".into())],
        "",
    )
    .await;
    assert_eq!(status, 421);
    // A command from another page (no or wrong Origin): refused.
    let (status, _) = http(
        r.addr,
        "POST",
        "/api/send",
        &[r.host(), ("Content-Type", "application/json".into())],
        r#"{"text":"x"}"#,
    )
    .await;
    assert_eq!(status, 403);
    let (status, _) = http(
        r.addr,
        "POST",
        "/api/send",
        &[
            r.host(),
            ("Origin", format!("http://{}", r.addr)),
            ("Content-Type", "text/plain".into()),
        ],
        r#"{"text":"x"}"#,
    )
    .await;
    assert_eq!(status, 415);
    // Refusals say why.
    let (status, v) = r.post("/api/area", r#"{"geohash":"u09"}"#).await;
    assert_eq!((status, v["error"].as_str()), (400, Some("not_a_channel")));
    let (status, v) = r.post("/api/send", r#"{"text":"x","cell":"u0bcd"}"#).await;
    assert_eq!((status, v["error"].as_str()), (422, Some("not_in_area")));
    let (status, v) = r
        .post(
            "/api/dm",
            &format!(
                r#"{{"peer":"{}","cell":"u09tv","text":"{}"}}"#,
                "ab".repeat(32),
                "x".repeat(300)
            ),
        )
        .await;
    assert_eq!((status, v["error"].as_str()), (400, Some("too_long")));
}

#[tokio::test]
async fn live_stream_and_sending() {
    let r = Running::start().await;
    let mut s = TcpStream::connect(r.addr).await.unwrap();
    s.write_all(format!("GET /api/events HTTP/1.1\r\nHost: {}\r\n\r\n", r.addr).as_bytes())
        .await
        .unwrap();
    let mut lines = BufReader::new(s).lines();
    let mut next_event = async || -> (String, Value) {
        let mut name = String::new();
        loop {
            let line = tokio::time::timeout(Duration::from_secs(5), lines.next_line())
                .await
                .expect("no SSE within 5 s")
                .unwrap()
                .unwrap();
            if let Some(n) = line.strip_prefix("event: ") {
                name = n.to_owned();
            } else if let Some(d) = line.strip_prefix("data: ")
                && !name.is_empty()
            {
                return (name, serde_json::from_str(d).unwrap());
            }
        }
    };
    let (name, first) = next_event().await;
    assert_eq!(name, "state");
    assert_eq!(first["status"]["center"], "u09tv");

    // A message from someone in a neighbour reaches the page.
    let who = Identity::generate();
    r.relay.publish(channel_event(
        &who,
        KIND_MESSAGE,
        "u09ty",
        &[["n", "alice"]],
        "bonjour",
    ));
    loop {
        let (name, d) = next_event().await;
        if name == "message" {
            assert_eq!(
                (d["content"].as_str(), d["nickname"].as_str()),
                (Some("bonjour"), Some("alice"))
            );
            assert_eq!(d["cell"], "u09ty");
            break;
        }
    }
    // Sending from the page.
    let (status, m) = r
        .post("/api/send", r#"{"text":"salut","nickname":"bob"}"#)
        .await;
    assert_eq!(status, 200);
    assert_eq!(
        (m["cell"].as_str(), m["own"].as_bool()),
        (Some("u09tv"), Some(true))
    );
    // Moving the area sends a new state.
    let (status, _) = r
        .post("/api/area", r#"{"geohash":"u09ty","teleport":false}"#)
        .await;
    assert_eq!(status, 200);
    loop {
        let (name, d) = next_event().await;
        if name == "state" {
            assert_eq!(
                (
                    d["status"]["center"].as_str(),
                    d["status"]["teleport"].as_bool()
                ),
                (Some("u09ty"), Some(false))
            );
            break;
        }
    }
    let _ = &r.console;
}
