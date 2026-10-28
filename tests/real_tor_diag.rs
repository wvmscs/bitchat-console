// SPDX-FileCopyrightText: 2026 William VITAL, FR (wvmscs)
// SPDX-License-Identifier: GPL-3.0-or-later

//! Diagnostic: each step of an HTTPS GET through Arti, timed.
#![cfg(feature = "arti")]

use std::sync::Arc;
use std::time::{Duration, Instant};

use bitchat_console::arti::Arti;
use bitchat_console::connector::Connector;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[tokio::test]
#[ignore]
async fn https_get_step_by_step() {
    bitchat_console::install_crypto_provider();
    let dir = std::env::temp_dir().join("bitchat-console-arti-test");
    let tor = Arc::new(Arti::bootstrap(&dir).await.unwrap());
    let host = "raw.githubusercontent.com";
    let t = Instant::now();
    let stream = tor.connect(host, 443, "relay-lists").await.unwrap();
    println!(
        "connected through tor in {:.1} s",
        t.elapsed().as_secs_f64()
    );
    let name = rustls::pki_types::ServerName::try_from(host.to_owned()).unwrap();
    let t = Instant::now();
    let mut s = tokio::time::timeout(
        Duration::from_secs(30),
        tokio_rustls::TlsConnector::from(bitchat_console::relay::tls_config())
            .connect(name, stream),
    )
    .await
    .expect("TLS timed out")
    .unwrap();
    println!("TLS in {:.1} s", t.elapsed().as_secs_f64());
    let req = format!(
        "GET /permissionlesstech/bitchat/refs/heads/main/relays/online_relays_gps.csv HTTP/1.0\r\nHost: {host}\r\nConnection: close\r\n\r\n"
    );
    s.write_all(req.as_bytes()).await.unwrap();
    s.flush().await.unwrap();
    let t = Instant::now();
    let mut buf = vec![0u8; 4096];
    let n = tokio::time::timeout(Duration::from_secs(30), s.read(&mut buf))
        .await
        .expect("read timed out")
        .unwrap();
    println!(
        "first {n} bytes in {:.1} s: {:?}",
        t.elapsed().as_secs_f64(),
        String::from_utf8_lossy(&buf[..n.min(300)])
    );
}
