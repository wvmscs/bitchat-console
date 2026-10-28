//! One connection to one relay for one cell (D20), through the connector
//! (Tor): subscription, events, publications, reconnection.
//!
//! Each connection is a task. It reports everything to the console on one
//! channel ([`RelayEvent`]) and takes [`RelayHandle`] commands.

use std::sync::{Arc, OnceLock};
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio_tungstenite::tungstenite::Message as WsMessage;

use crate::connector::Connector;
use crate::nostr::{Event, Filter};

/// State of a connection, for the page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConnState {
    Connecting,
    Connected,
    /// Lost; next try in this many seconds.
    Waiting {
        retry_in_secs: u64,
    },
    /// Given up: the relay requires authentication (NIP-42), which BitChat
    /// identities cannot give (no history, one key per cell).
    Abandoned {
        reason: String,
    },
}

/// What a connection reports. `relay` is the relay URL, `cell` the cell the
/// connection is for.
#[derive(Debug, Clone, PartialEq)]
pub enum RelayEvent {
    State {
        relay: String,
        cell: String,
        state: ConnState,
    },
    Event {
        relay: String,
        cell: String,
        event: Event,
    },
    /// End of stored events for the subscription.
    Eose { relay: String, cell: String },
    /// The relay's answer to a publication.
    Ok {
        relay: String,
        cell: String,
        id: String,
        accepted: bool,
        message: String,
    },
    Notice {
        relay: String,
        cell: String,
        text: String,
    },
    /// The relay closed the subscription.
    Closed {
        relay: String,
        cell: String,
        reason: String,
    },
    /// The relay asked for NIP-42 authentication; never answered.
    Auth { relay: String, cell: String },
}

enum Cmd {
    Publish(Event),
    Filters(Vec<Filter>),
}

/// A running connection. Dropping it closes the connection.
pub struct RelayHandle {
    tx: mpsc::UnboundedSender<Cmd>,
    task: JoinHandle<()>,
}

impl RelayHandle {
    /// Sends an event now, or as soon as the connection is up.
    pub fn publish(&self, event: Event) {
        let _ = self.tx.send(Cmd::Publish(event));
    }

    /// Replaces the subscription filters (sent again under the same id).
    pub fn set_filters(&self, filters: Vec<Filter>) {
        let _ = self.tx.send(Cmd::Filters(filters));
    }
}

impl Drop for RelayHandle {
    fn drop(&mut self) {
        self.task.abort();
    }
}

/// Reconnection delays: from 1 s, doubled, up to 5 min (as the apps).
pub const BACKOFF_MIN: Duration = Duration::from_secs(1);
pub const BACKOFF_MAX: Duration = Duration::from_secs(300);

/// Starts a connection to `url` for `cell`.
pub fn spawn(
    url: String,
    cell: String,
    filters: Vec<Filter>,
    connector: Arc<dyn Connector>,
    events: mpsc::UnboundedSender<RelayEvent>,
) -> RelayHandle {
    let (tx, rx) = mpsc::unbounded_channel();
    let task = tokio::spawn(run(url, cell, filters, connector, events, rx));
    RelayHandle { tx, task }
}

/// `ws[s]://host[:port][/path]` -> (host, port).
pub fn host_port(url: &str) -> Option<(String, u16)> {
    let (scheme, rest) = url.split_once("://")?;
    let default = match scheme {
        "wss" => 443,
        "ws" => 80,
        _ => return None,
    };
    let authority = rest.split('/').next()?;
    if let Some(v6) = authority.strip_prefix('[') {
        let (host, after) = v6.split_once(']')?;
        let port = match after.strip_prefix(':') {
            Some(p) => p.parse().ok()?,
            None => default,
        };
        return Some((host.to_owned(), port));
    }
    match authority.rsplit_once(':') {
        Some((h, p)) => Some((h.to_owned(), p.parse().ok()?)),
        None if !authority.is_empty() => Some((authority.to_owned(), default)),
        None => None,
    }
}

/// TLS client configuration: ring, the web's root certificates.
pub fn tls_config() -> Arc<rustls::ClientConfig> {
    static CONFIG: OnceLock<Arc<rustls::ClientConfig>> = OnceLock::new();
    CONFIG
        .get_or_init(|| {
            let roots =
                rustls::RootCertStore::from_iter(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
            let provider = Arc::new(rustls::crypto::ring::default_provider());
            Arc::new(
                rustls::ClientConfig::builder_with_provider(provider)
                    .with_safe_default_protocol_versions()
                    .expect("ring supports the default versions")
                    .with_root_certificates(roots)
                    .with_no_client_auth(),
            )
        })
        .clone()
}

/// Subscription id of the cell's channel.
fn sub_id(cell: &str) -> String {
    format!("geo-{cell}")
}

async fn run(
    url: String,
    cell: String,
    mut filters: Vec<Filter>,
    connector: Arc<dyn Connector>,
    events: mpsc::UnboundedSender<RelayEvent>,
    mut rx: mpsc::UnboundedReceiver<Cmd>,
) {
    let state = |s: ConnState| {
        let _ = events.send(RelayEvent::State {
            relay: url.clone(),
            cell: cell.clone(),
            state: s,
        });
    };
    let Some((host, port)) = host_port(&url) else {
        state(ConnState::Abandoned {
            reason: "not a ws:// or wss:// address".into(),
        });
        return;
    };
    let mut queue: Vec<Event> = Vec::new();
    let mut backoff = BACKOFF_MIN;
    loop {
        state(ConnState::Connecting);
        let attempt = async {
            let stream = connector.connect(&host, port, &cell).await?;
            let (ws, _) = tokio_tungstenite::client_async_tls_with_config(
                url.as_str(),
                stream,
                None,
                Some(tokio_tungstenite::Connector::Rustls(tls_config())),
            )
            .await
            .map_err(std::io::Error::other)?;
            Ok::<_, std::io::Error>(ws)
        };
        match attempt.await {
            Ok(ws) => {
                backoff = BACKOFF_MIN;
                state(ConnState::Connected);
                let (mut sink, mut source) = ws.split();
                let req = |filters: &[Filter]| {
                    let mut msg = vec![json!("REQ"), json!(sub_id(&cell))];
                    msg.extend(filters.iter().map(Filter::to_json));
                    WsMessage::text(Value::Array(msg).to_string())
                };
                // A connection for writing only (presence) subscribes to nothing.
                let mut alive = filters.is_empty() || sink.send(req(&filters)).await.is_ok();
                for e in queue.drain(..) {
                    if alive {
                        let text = json!(["EVENT", e]).to_string();
                        alive = sink.send(WsMessage::text(text)).await.is_ok();
                    }
                }
                while alive {
                    tokio::select! {
                        cmd = rx.recv() => match cmd {
                            None => return,
                            Some(Cmd::Publish(e)) => {
                                let text = json!(["EVENT", e]).to_string();
                                if sink.send(WsMessage::text(text)).await.is_err() {
                                    queue.push(e);
                                    alive = false;
                                }
                            }
                            Some(Cmd::Filters(f)) => {
                                filters = f;
                                alive = sink.send(req(&filters)).await.is_ok();
                            }
                        },
                        msg = source.next() => match msg {
                            Some(Ok(WsMessage::Text(text))) => {
                                if let Some(abandon) = on_message(&text, &url, &cell, &events) {
                                    state(ConnState::Abandoned { reason: abandon });
                                    return;
                                }
                            }
                            Some(Ok(WsMessage::Ping(p))) => {
                                alive = sink.send(WsMessage::Pong(p)).await.is_ok();
                            }
                            Some(Ok(WsMessage::Close(_))) | None | Some(Err(_)) => alive = false,
                            Some(Ok(_)) => {}
                        },
                    }
                }
            }
            Err(e) => {
                tracing::debug!(relay = %url, cell = %cell, error = %e, "relay unreachable");
            }
        }
        state(ConnState::Waiting {
            retry_in_secs: backoff.as_secs(),
        });
        // Commands keep coming while waiting: queue the publications.
        let wake = tokio::time::sleep(backoff);
        tokio::pin!(wake);
        loop {
            tokio::select! {
                _ = &mut wake => break,
                cmd = rx.recv() => match cmd {
                    None => return,
                    Some(Cmd::Publish(e)) => queue.push(e),
                    Some(Cmd::Filters(f)) => filters = f,
                },
            }
        }
        backoff = (backoff * 2).min(BACKOFF_MAX);
    }
}

/// Handles one relay message; returns a reason to give up on the relay.
fn on_message(
    text: &str,
    relay: &str,
    cell: &str,
    events: &mpsc::UnboundedSender<RelayEvent>,
) -> Option<String> {
    let Ok(Value::Array(msg)) = serde_json::from_str::<Value>(text) else {
        return None;
    };
    let s = |i: usize| {
        msg.get(i)
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned()
    };
    let (relay, cell) = (relay.to_owned(), cell.to_owned());
    let report = match s(0).as_str() {
        "EVENT" => match msg.get(2).cloned().map(serde_json::from_value::<Event>) {
            Some(Ok(event)) => RelayEvent::Event { relay, cell, event },
            _ => return None,
        },
        "EOSE" => RelayEvent::Eose { relay, cell },
        "OK" => RelayEvent::Ok {
            relay,
            cell,
            id: s(1),
            accepted: msg.get(2).and_then(Value::as_bool).unwrap_or(false),
            message: s(3),
        },
        "NOTICE" => RelayEvent::Notice {
            relay,
            cell,
            text: s(1),
        },
        "CLOSED" => {
            let reason = s(2);
            let auth = reason.starts_with("auth-required:");
            let _ = events.send(RelayEvent::Closed {
                relay,
                cell,
                reason: reason.clone(),
            });
            return auth.then_some(reason);
        }
        "AUTH" => RelayEvent::Auth { relay, cell },
        _ => return None,
    };
    let _ = events.send(report);
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn addresses() {
        assert_eq!(
            host_port("wss://relay.damus.io"),
            Some(("relay.damus.io".into(), 443))
        );
        assert_eq!(host_port("wss://nos.lol/"), Some(("nos.lol".into(), 443)));
        assert_eq!(
            host_port("wss://r.example:7447/x"),
            Some(("r.example".into(), 7447))
        );
        assert_eq!(
            host_port("ws://127.0.0.1:9000"),
            Some(("127.0.0.1".into(), 9000))
        );
        assert_eq!(host_port("ws://[::1]:9000"), Some(("::1".into(), 9000)));
        assert_eq!(host_port("https://x"), None);
        assert_eq!(host_port("wss://"), None);
    }
}
