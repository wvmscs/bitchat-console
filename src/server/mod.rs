// SPDX-FileCopyrightText: 2026 William VITAL, FR (wvmscs)
// SPDX-License-Identifier: GPL-3.0-or-later

//! The page of the console (lot 7): served locally, live updates by SSE,
//! commands by `POST`, protected like bitchat-geohash's map (Host, Origin,
//! CSP), HTTPS optional, or behind a TLS proxy (D34).

mod api;
mod assets;
mod guard;
mod map;
mod proxy;
mod tls;

use std::net::SocketAddr;
use std::sync::Arc;

use axum::Router;
use axum::extract::{DefaultBodyLimit, Path};
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use tokio::sync::watch;
use tokio::task::JoinHandle;

pub use assets::Asset;
pub use proxy::{Net, ProxyConfig};
pub use tls::TlsFiles;

use crate::console::Console;
use crate::error::{Error, Result};

#[derive(Debug, Clone)]
pub struct ServerConfig {
    pub listen: SocketAddr,
    /// Listening on a non-loopback address needs it, and TLS here or in a
    /// proxy.
    pub allow_remote: bool,
    pub tls: Option<TlsFiles>,
    /// Behind a TLS proxy (D34): clear HTTP from the proxy only; no `tls`.
    pub proxy: Option<ProxyConfig>,
    pub max_body: usize,
    /// A running bitchat-geohash map (`http://127.0.0.1:8080`): served at
    /// `/`, with the console floating over it; the console is always at
    /// `/console/` too (lot 9, D26).
    pub map: Option<String>,
    /// Places for the cell field and the name of the cell (lot 19, D35):
    /// none, geohashes only.
    #[cfg(feature = "geocode")]
    pub places: Option<Arc<crate::geocode::Places>>,
}

impl Default for ServerConfig {
    fn default() -> Self {
        ServerConfig {
            listen: SocketAddr::from(([127, 0, 0, 1], 8090)),
            allow_remote: false,
            tls: None,
            proxy: None,
            max_body: 64 * 1024,
            map: None,
            #[cfg(feature = "geocode")]
            places: None,
        }
    }
}

/// A running server.
pub struct Server {
    addr: SocketAddr,
    https: bool,
    public: Option<String>,
    stop: watch::Sender<bool>,
    task: JoinHandle<std::io::Result<()>>,
}

impl Server {
    pub fn local_addr(&self) -> SocketAddr {
        self.addr
    }

    /// `http://addr` or `https://addr`, without a trailing slash.
    pub fn url(&self) -> String {
        format!(
            "{}://{}",
            if self.https { "https" } else { "http" },
            self.addr
        )
    }

    /// Behind a proxy, the public origin (`https://name`); the browser
    /// goes there, not to [`Server::url`].
    pub fn public_url(&self) -> Option<&str> {
        self.public.as_deref()
    }

    /// Closes the streams and the listener.
    pub async fn shutdown(self) {
        let _ = self.stop.send(true);
        let _ = self.task.await;
    }
}

fn serve_asset(path: &str) -> Response {
    match Asset::get(path) {
        Some(a) => (
            [
                (header::CONTENT_TYPE, a.mime),
                (header::CACHE_CONTROL, "no-cache"),
            ],
            a.bytes,
        )
            .into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

/// The page, its files and the API under `prefix` (`""` or `/console`).
fn console_routes(prefix: &str) -> Router<api::AppState> {
    let p = |path: &str| format!("{prefix}{path}");
    Router::new()
        .route(&p("/"), get(|| async { serve_asset("index.html") }))
        .route(
            &p("/assets/{*path}"),
            get(|Path(p): Path<String>| async move { serve_asset(&p) }),
        )
        .route(&p("/api/state"), get(api::state))
        .route(&p("/api/participants"), get(api::participants))
        .route(&p("/api/events"), get(api::events))
        .route(&p("/api/area"), post(api::area))
        .route(&p("/api/place"), get(api::place))
        .route(&p("/api/send"), post(api::send))
        .route(&p("/api/dm"), post(api::dm))
        .route(&p("/api/dm/delete"), post(api::delete_dms))
        .route(&p("/api/present"), get(api::present))
        .route(
            &p("/history"),
            get(|| async { serve_asset("history.html") }),
        )
        .route(&p("/api/history"), get(api::history_query))
        .route(&p("/api/history/{id}"), get(api::history_get))
        .route(&p("/api/history/delete"), post(api::history_delete))
        .route(&p("/api/history/update"), post(api::history_update))
        .route(&p("/api/history/insert"), post(api::history_insert))
        .route(&p("/api/read"), post(api::read))
        .route(&p("/api/mute"), post(api::mute))
        .route(&p("/api/nickname"), post(api::nickname))
        .route(&p("/api/anon"), post(api::anon))
        .route(&p("/api/preset"), post(api::add_preset))
        .route(&p("/api/preset/delete"), post(api::delete_preset))
        .route(&p("/api/preset/send"), post(api::send_preset))
        .route(&p("/api/schedule"), post(api::add_schedule))
        .route(&p("/api/schedule/delete"), post(api::delete_schedule))
        .route(&p("/api/group"), post(api::group))
}

/// Serves the page around a running console. Behind a proxy (D34), the
/// public name must lead here, unless `ProxyConfig::check` is off.
pub async fn serve(console: Arc<Console>, config: ServerConfig) -> Result<Server> {
    if config.proxy.is_some() && config.tls.is_some() {
        return Err(Error::Proxy(
            "TLS is the proxy's: no certificate here".into(),
        ));
    }
    if !config.listen.ip().is_loopback() {
        if !config.allow_remote {
            return Err(Error::RemoteListen(config.listen.to_string()));
        }
        if config.tls.is_none() && config.proxy.is_none() {
            return Err(Error::Tls(format!(
                "listening on {} needs TLS (--tls-cert, --tls-key) or a TLS proxy in front \
                 (--behind-proxy): the page shows private messages",
                config.listen
            )));
        }
    }
    let behind = config
        .proxy
        .as_ref()
        .map(|p| {
            Ok::<_, Error>(guard::Behind {
                public: proxy::Public::parse(&p.public)?,
                from: p.from.clone(),
                token: proxy::token(),
            })
        })
        .transpose()?;
    let probe = config
        .proxy
        .as_ref()
        .filter(|p| p.check)
        .zip(behind.as_ref())
        .map(|(_, b)| (b.public.origin.clone(), b.token.clone()));
    let public = behind.as_ref().map(|b| b.public.origin.clone());
    let tls = config.tls.as_ref().map(tls::load).transpose()?;
    let listener = tokio::net::TcpListener::bind(config.listen).await?;
    let addr = listener.local_addr()?;
    let (stop, stop_rx) = watch::channel(false);
    let state = api::AppState {
        console,
        stop: stop_rx.clone(),
        #[cfg(feature = "geocode")]
        places: config.places.clone(),
    };
    let guard = Arc::new(guard::Guard::new(
        addr,
        tls.as_ref().map(|t| t.leaf.clone()),
        behind,
    ));
    let mut app = console_routes("/console").route(
        "/console",
        get(|| async { axum::response::Redirect::permanent("/console/") }),
    );
    app = match &config.map {
        // The map at `/`: anything that is not the console's goes to it.
        Some(upstream) => app
            .fallback(map::proxy)
            .layer(axum::Extension(Arc::new(map::Upstream::new(upstream)?))),
        None => app.merge(console_routes("")),
    };
    let app = app
        .with_state(state)
        .layer(DefaultBodyLimit::max(config.max_body))
        .layer(axum::middleware::from_fn_with_state(guard, guard::guard));
    let https = tls.is_some();
    let task = match tls {
        Some(tls) => tokio::spawn(tls::serve(listener, tls.acceptor, app, stop_rx)),
        None => {
            let mut stop = stop_rx;
            tokio::spawn(async move {
                // The peer address: behind a proxy, only the proxy's (D34).
                axum::serve(
                    listener,
                    app.into_make_service_with_connect_info::<SocketAddr>(),
                )
                .with_graceful_shutdown(async move {
                    let _ = stop.wait_for(|s| *s).await;
                })
                .await
            })
        }
    };
    tracing::info!(url = %format!("{}://{addr}/", if https { "https" } else { "http" }), "console page served");
    let server = Server {
        addr,
        https,
        public,
        stop,
        task,
    };
    if let Some((origin, token)) = probe {
        if let Err(e) = proxy::probe(&origin, &token).await {
            server.shutdown().await;
            return Err(e);
        }
        tracing::info!("console behind the proxy on {origin}/ (checked)");
    }
    Ok(server)
}
