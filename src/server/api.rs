//! `/api/*`: the state, the live stream and the commands.
//!
//! The page shows message contents and nicknames (that is its purpose);
//! nothing here logs them.

use std::collections::BTreeMap;
use std::convert::Infallible;
use std::sync::Arc;

use axum::Json;
use axum::extract::State;
use axum::extract::rejection::JsonRejection;
use axum::http::StatusCode;
use axum::response::sse::{Event as SseEvent, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use futures_util::StreamExt;
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::sync::{broadcast, watch};

use crate::console::{Console, ConsoleEvent};
use crate::error::Error;

#[derive(Clone)]
pub(crate) struct AppState {
    pub(crate) console: Arc<Console>,
    pub(crate) stop: watch::Receiver<bool>,
}

/// `{"error": code, "message": ...}` with a status.
#[derive(Debug)]
pub(crate) struct ApiError {
    status: StatusCode,
    body: Value,
}

impl ApiError {
    pub(crate) fn new(status: StatusCode, code: &str, message: impl Into<String>) -> Self {
        ApiError {
            status,
            body: json!({ "error": code, "message": message.into() }),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.status, Json(self.body)).into_response()
    }
}

impl From<Error> for ApiError {
    fn from(e: Error) -> Self {
        let message = e.to_string();
        let (status, code) = match e {
            Error::InvalidGeohash(_) => (StatusCode::BAD_REQUEST, "invalid_geohash"),
            Error::NotAChannel(_) => (StatusCode::BAD_REQUEST, "not_a_channel"),
            Error::NotInArea(_) => (StatusCode::UNPROCESSABLE_ENTITY, "not_in_area"),
            Error::NoArea => (StatusCode::CONFLICT, "no_area"),
            Error::EmptyMessage => (StatusCode::BAD_REQUEST, "empty_message"),
            Error::DmTooLong => (StatusCode::BAD_REQUEST, "too_long"),
            Error::InvalidKey(_) => (StatusCode::BAD_REQUEST, "invalid_key"),
            Error::ConsoleStopped => (StatusCode::SERVICE_UNAVAILABLE, "stopped"),
            _ => (StatusCode::INTERNAL_SERVER_ERROR, "internal"),
        };
        ApiError::new(status, code, message)
    }
}

impl From<JsonRejection> for ApiError {
    fn from(r: JsonRejection) -> Self {
        ApiError::new(r.status(), "bad_request", r.body_text())
    }
}

/// The whole state: area, messages of the 9 cells, private messages.
async fn snapshot(console: &Console) -> Result<Value, Error> {
    let status = console.status().await?;
    let mut messages = BTreeMap::new();
    for c in &status.cells {
        messages.insert(c.geohash.clone(), console.messages(&c.geohash).await?);
    }
    let dms = console.dms().await?;
    Ok(json!({ "status": status, "messages": messages, "dms": dms }))
}

/// `GET /api/state`.
pub(crate) async fn state(State(s): State<AppState>) -> Result<Json<Value>, ApiError> {
    Ok(Json(snapshot(&s.console).await?))
}

fn sse(name: &str, data: &Value) -> Result<SseEvent, Infallible> {
    Ok(SseEvent::default().event(name).data(data.to_string()))
}

/// `GET /api/events`: `state` first, then the changes. Behind, the stream
/// closes; the browser reconnects and gets a new `state`.
pub(crate) async fn events(State(s): State<AppState>) -> Response {
    let rx = s.console.subscribe();
    let first = match snapshot(&s.console).await {
        Ok(v) => v,
        Err(e) => return ApiError::from(e).into_response(),
    };
    let head = futures_util::stream::once(std::future::ready(sse("state", &first)));
    let rest = futures_util::stream::unfold(
        (rx, s.stop.clone(), s.console.clone()),
        |(mut rx, mut stop, console)| async move {
            if *stop.borrow() {
                return None;
            }
            let event = tokio::select! {
                e = rx.recv() => e,
                _ = stop.changed() => return None,
            };
            let (name, data) = match event {
                Ok(ConsoleEvent::Area) => ("state", snapshot(&console).await.ok()?),
                Ok(ConsoleEvent::Relays) => ("status", json!(console.status().await.ok()?)),
                Ok(ConsoleEvent::Message(m)) => ("message", json!(m)),
                Ok(ConsoleEvent::Delivered { id, relay }) => {
                    ("delivered", json!({ "id": id, "relay": relay }))
                }
                Ok(ConsoleEvent::Participants { cell, count }) => {
                    ("participants", json!({ "cell": cell, "count": count }))
                }
                Ok(ConsoleEvent::Dm(dm)) => ("dm", json!(dm)),
                Ok(ConsoleEvent::DmStatus { id, status }) => {
                    ("dm_status", json!({ "id": id, "status": status }))
                }
                Ok(ConsoleEvent::RelayText { relay, cell, text }) => (
                    "relay_text",
                    json!({ "relay": relay, "cell": cell, "text": text }),
                ),
                Err(broadcast::error::RecvError::Lagged(_)) => return None,
                Err(broadcast::error::RecvError::Closed) => return None,
            };
            Some((sse(name, &data), (rx, stop, console)))
        },
    );
    Sse::new(head.chain(rest))
        .keep_alive(KeepAlive::default())
        .into_response()
}

#[derive(Deserialize)]
pub(crate) struct AreaBody {
    geohash: String,
    #[serde(default)]
    teleport: bool,
}

/// `POST /api/area {"geohash": "u09tv", "teleport": true}`.
pub(crate) async fn area(
    State(s): State<AppState>,
    body: Result<Json<AreaBody>, JsonRejection>,
) -> Result<Json<Value>, ApiError> {
    let Json(b) = body?;
    let cells = s.console.set_area(&b.geohash, b.teleport).await?;
    Ok(Json(json!({ "cells": cells })))
}

#[derive(Deserialize)]
pub(crate) struct SendBody {
    text: String,
    cell: Option<String>,
    nickname: Option<String>,
}

/// `POST /api/send {"text": ..., "cell": ..., "nickname": ...}`.
pub(crate) async fn send(
    State(s): State<AppState>,
    body: Result<Json<SendBody>, JsonRejection>,
) -> Result<Json<Value>, ApiError> {
    let Json(b) = body?;
    let m = s
        .console
        .send(&b.text, b.cell.as_deref(), b.nickname.as_deref())
        .await?;
    Ok(Json(json!(m)))
}

#[derive(Deserialize)]
pub(crate) struct DmBody {
    peer: String,
    cell: String,
    text: String,
}

/// `POST /api/dm {"peer": hex, "cell": ..., "text": ...}`.
pub(crate) async fn dm(
    State(s): State<AppState>,
    body: Result<Json<DmBody>, JsonRejection>,
) -> Result<Json<Value>, ApiError> {
    let Json(b) = body?;
    Ok(Json(json!(
        s.console.send_dm(&b.peer, &b.cell, &b.text).await?
    )))
}

#[derive(Deserialize)]
pub(crate) struct ReadBody {
    peer: String,
}

/// `POST /api/read {"peer": hex}`: read receipts for that conversation.
pub(crate) async fn read(
    State(s): State<AppState>,
    body: Result<Json<ReadBody>, JsonRejection>,
) -> Result<Json<Value>, ApiError> {
    let Json(b) = body?;
    Ok(Json(json!({ "read": s.console.mark_read(&b.peer).await? })))
}
