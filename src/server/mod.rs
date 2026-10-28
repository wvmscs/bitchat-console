//! The page of the console (lot 7): served locally, live updates by SSE,
//! commands by `POST`, protected like bitchat-geohash's map (Host, Origin,
//! CSP), HTTPS optional.

mod api;
mod assets;
mod guard;
mod map;
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
pub use tls::TlsFiles;

use crate::console::Console;
use crate::error::{Error, Result};

#[derive(Debug, Clone)]
pub struct ServerConfig {
    pub listen: SocketAddr,
    /// Listening on a non-loopback address needs it, and TLS.
    pub allow_remote: bool,
    pub tls: Option<TlsFiles>,
    pub max_body: usize,
    /// A running bitchat-geohash map (`http://127.0.0.1:8080`): served at
    /// `/`, with the console floating over it; the console is always at
    /// `/console/` too (lot 9, D26).
    pub map: Option<String>,
}

impl Default for ServerConfig {
    fn default() -> Self {
        ServerConfig {
            listen: SocketAddr::from(([127, 0, 0, 1], 8090)),
            allow_remote: false,
            tls: None,
            max_body: 64 * 1024,
            map: None,
        }
    }
}

/// A running server.
pub struct Server {
    addr: SocketAddr,
    https: bool,
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
        .route(&p("/api/send"), post(api::send))
        .route(&p("/api/dm"), post(api::dm))
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

/// Serves the page around a running console.
pub async fn serve(console: Arc<Console>, config: ServerConfig) -> Result<Server> {
    if !config.listen.ip().is_loopback() {
        if !config.allow_remote {
            return Err(Error::RemoteListen(config.listen.to_string()));
        }
        if config.tls.is_none() {
            return Err(Error::Tls(format!(
                "listening on {} needs TLS (--tls-cert, --tls-key): the page shows private messages",
                config.listen
            )));
        }
    }
    let tls = config.tls.as_ref().map(tls::load).transpose()?;
    let listener = tokio::net::TcpListener::bind(config.listen).await?;
    let addr = listener.local_addr()?;
    let (stop, stop_rx) = watch::channel(false);
    let state = api::AppState {
        console,
        stop: stop_rx.clone(),
    };
    let guard = Arc::new(guard::Guard::new(
        addr,
        tls.as_ref().map(|t| t.leaf.clone()),
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
                axum::serve(listener, app)
                    .with_graceful_shutdown(async move {
                        let _ = stop.wait_for(|s| *s).await;
                    })
                    .await
            })
        }
    };
    tracing::info!(url = %format!("{}://{addr}/", if https { "https" } else { "http" }), "console page served");
    Ok(Server {
        addr,
        https,
        stop,
        task,
    })
}
