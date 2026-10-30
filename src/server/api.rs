// SPDX-FileCopyrightText: 2026 William VITAL, FR (wvmscs)
// SPDX-License-Identifier: GPL-3.0-or-later

//! `/api/*`: the state, the live stream and the commands.
//!
//! The page shows message contents and nicknames (that is its purpose);
//! nothing here logs them.

use std::collections::BTreeMap;
use std::convert::Infallible;
use std::sync::Arc;

use axum::Json;
use axum::extract::rejection::JsonRejection;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::sse::{Event as SseEvent, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use futures_util::StreamExt;
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::sync::{broadcast, watch};

use crate::console::{Console, ConsoleEvent, GroupOp};
use crate::error::Error;
use crate::history::{History, Record};

#[derive(Clone)]
pub(crate) struct AppState {
    pub(crate) console: Arc<Console>,
    pub(crate) stop: watch::Receiver<bool>,
    #[cfg(feature = "geocode")]
    pub(crate) places: Option<Arc<crate::geocode::Places>>,
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
            Error::PlaceNotFound => (StatusCode::NOT_FOUND, "place_not_found"),
            Error::Geocode(_) => (StatusCode::BAD_GATEWAY, "geocode"),
            Error::NotInArea(_) => (StatusCode::UNPROCESSABLE_ENTITY, "not_in_area"),
            Error::NoArea => (StatusCode::CONFLICT, "no_area"),
            Error::EmptyMessage => (StatusCode::BAD_REQUEST, "empty_message"),
            Error::DmTooLong => (StatusCode::BAD_REQUEST, "too_long"),
            Error::InvalidKey(_) => (StatusCode::BAD_REQUEST, "invalid_key"),
            Error::ConsoleStopped => (StatusCode::SERVICE_UNAVAILABLE, "stopped"),
            Error::NotFound(_) => (StatusCode::NOT_FOUND, "not_found"),
            Error::Schedule(_) => (StatusCode::BAD_REQUEST, "bad_schedule"),
            Error::Group(_) => (StatusCode::UNPROCESSABLE_ENTITY, "group"),
            Error::NoHistory => (StatusCode::CONFLICT, "no_history"),
            Error::NoRow(_) => (StatusCode::NOT_FOUND, "not_found"),
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
    let local = console.local().await?;
    let groups = console.groups().await?;
    Ok(
        json!({ "status": status, "messages": messages, "dms": dms, "local": local, "groups": groups }),
    )
}

/// `GET /api/state`.
pub(crate) async fn state(State(s): State<AppState>) -> Result<Json<Value>, ApiError> {
    Ok(Json(snapshot(&s.console).await?))
}

#[derive(Deserialize)]
pub(crate) struct CellQuery {
    cell: String,
}

/// `GET /api/participants?cell=u09tv`: who was seen there within the
/// window, the most recent first.
pub(crate) async fn participants(
    State(s): State<AppState>,
    Query(q): Query<CellQuery>,
) -> Result<Json<Value>, ApiError> {
    Ok(Json(json!(s.console.participants(&q.cell).await?)))
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
                Ok(ConsoleEvent::Dms) => ("dms", json!(console.dms().await.ok()?)),
                Ok(ConsoleEvent::History) => ("history", json!({})),
                Ok(ConsoleEvent::DmStatus { id, status }) => {
                    ("dm_status", json!({ "id": id, "status": status }))
                }
                Ok(ConsoleEvent::Local) => ("local", json!(console.local().await.ok()?)),
                Ok(ConsoleEvent::Groups) => ("groups", json!(console.groups().await.ok()?)),
                Ok(ConsoleEvent::GroupMessage(m)) => ("group_message", json!(m)),
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

/// `POST /api/area {"geohash": "u09tv", "teleport": true}`. With places
/// (lot 19, D35), `"geohash"` may also be `#u09tv`, a place or an IP
/// address: the answer then has the `place` found.
pub(crate) async fn area(
    State(s): State<AppState>,
    body: Result<Json<AreaBody>, JsonRejection>,
) -> Result<Json<Value>, ApiError> {
    let Json(b) = body?;
    #[cfg(feature = "geocode")]
    let (geohash, place) = match &s.places {
        Some(places) => {
            let r = places.resolve(&b.geohash).await?;
            (r.geohash, r.place)
        }
        None => (b.geohash, None),
    };
    #[cfg(not(feature = "geocode"))]
    let (geohash, place) = (b.geohash, None::<()>);
    let cells = s.console.set_area(&geohash, b.teleport).await?;
    Ok(Json(json!({ "cells": cells, "place": place })))
}

#[derive(Deserialize)]
pub(crate) struct PlaceQuery {
    #[cfg_attr(not(feature = "geocode"), allow(dead_code))]
    geohash: String,
}

/// `GET /api/place?geohash=u09tv` (lot 19): the approximate name of a cell,
/// `{"place": {"label", "source", "attribution"}}` or `{"place": null}`
/// (open sea); `404 no_places` when the console does not look places up.
pub(crate) async fn place(
    State(s): State<AppState>,
    Query(q): Query<PlaceQuery>,
) -> Result<Json<Value>, ApiError> {
    #[cfg(feature = "geocode")]
    if let Some(places) = &s.places {
        let place = places.name(&q.geohash).await?;
        return Ok(Json(json!({ "geohash": q.geohash, "place": place })));
    }
    let _ = (&s, &q);
    Err(ApiError::new(
        StatusCode::NOT_FOUND,
        "no_places",
        "place names are off",
    ))
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

/// `GET /api/present`: when each key was last seen in each cell, and after
/// how long it has left (lot 15).
pub(crate) async fn present(State(s): State<AppState>) -> Result<Json<Value>, ApiError> {
    Ok(Json(json!(s.console.presence().await?)))
}

#[derive(Deserialize)]
pub(crate) struct DeleteDmsBody {
    peer: String,
    cell: String,
    /// None: the whole conversation.
    ids: Option<Vec<String>>,
}

/// `POST /api/dm/delete {"peer", "cell", "ids"?}`: locally (lot 15).
pub(crate) async fn delete_dms(
    State(s): State<AppState>,
    body: Result<Json<DeleteDmsBody>, JsonRejection>,
) -> Result<Json<Value>, ApiError> {
    let Json(b) = body?;
    let n = s.console.delete_dms(&b.peer, &b.cell, b.ids).await?;
    Ok(Json(json!({ "deleted": n })))
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

#[derive(Deserialize)]
pub(crate) struct MuteBody {
    pubkey: String,
    muted: bool,
}

/// `POST /api/mute {"pubkey": hex, "muted": true}`.
pub(crate) async fn mute(
    State(s): State<AppState>,
    body: Result<Json<MuteBody>, JsonRejection>,
) -> Result<Json<Value>, ApiError> {
    let Json(b) = body?;
    s.console.set_muted(&b.pubkey, b.muted).await?;
    Ok(Json(json!({ "muted": b.muted })))
}

#[derive(Deserialize)]
pub(crate) struct NicknameBody {
    nickname: Option<String>,
}

/// `POST /api/nickname {"nickname": "bob"}`: for every cell (D32); empty or
/// null removes it.
pub(crate) async fn nickname(
    State(s): State<AppState>,
    body: Result<Json<NicknameBody>, JsonRejection>,
) -> Result<Json<Value>, ApiError> {
    let Json(b) = body?;
    s.console.set_nickname(b.nickname.as_deref()).await?;
    Ok(Json(
        json!({ "nickname": s.console.local().await?.nickname }),
    ))
}

#[derive(Deserialize)]
pub(crate) struct AnonBody {
    cell: String,
    anon: bool,
}

/// `POST /api/anon {"cell": "u09tv", "anon": true}`: the nickname is not sent
/// in that cell (D32).
pub(crate) async fn anon(
    State(s): State<AppState>,
    body: Result<Json<AnonBody>, JsonRejection>,
) -> Result<Json<Value>, ApiError> {
    let Json(b) = body?;
    s.console.set_anon(&b.cell, b.anon).await?;
    Ok(Json(json!({ "anon": b.anon })))
}

#[derive(Deserialize)]
pub(crate) struct PresetBody {
    #[serde(default)]
    name: String,
    text: String,
    cell: Option<String>,
    nickname: Option<String>,
}

/// `POST /api/preset {"name", "text", "cell"?, "nickname"?}`.
pub(crate) async fn add_preset(
    State(s): State<AppState>,
    body: Result<Json<PresetBody>, JsonRejection>,
) -> Result<Json<Value>, ApiError> {
    let Json(b) = body?;
    let id = s
        .console
        .add_preset(&b.name, &b.text, b.cell.as_deref(), b.nickname.as_deref())
        .await?;
    Ok(Json(json!({ "id": id })))
}

#[derive(Deserialize)]
pub(crate) struct IdBody {
    id: i64,
    cell: Option<String>,
}

/// `POST /api/preset/delete {"id"}`.
pub(crate) async fn delete_preset(
    State(s): State<AppState>,
    body: Result<Json<IdBody>, JsonRejection>,
) -> Result<Json<Value>, ApiError> {
    let Json(b) = body?;
    Ok(Json(
        json!({ "deleted": s.console.delete_preset(b.id).await? }),
    ))
}

/// `POST /api/preset/send {"id", "cell"?}`.
pub(crate) async fn send_preset(
    State(s): State<AppState>,
    body: Result<Json<IdBody>, JsonRejection>,
) -> Result<Json<Value>, ApiError> {
    let Json(b) = body?;
    Ok(Json(json!(
        s.console.send_preset(b.id, b.cell.as_deref()).await?
    )))
}

#[derive(Deserialize)]
pub(crate) struct ScheduleBody {
    text: String,
    cell: Option<String>,
    nickname: Option<String>,
    /// Unix time.
    at: i64,
    repeat_secs: Option<i64>,
}

/// `POST /api/schedule {"text", "at", "repeat_secs"?, "cell"?, "nickname"?}`.
pub(crate) async fn add_schedule(
    State(s): State<AppState>,
    body: Result<Json<ScheduleBody>, JsonRejection>,
) -> Result<Json<Value>, ApiError> {
    let Json(b) = body?;
    let id = s
        .console
        .add_schedule(
            &b.text,
            b.cell.as_deref(),
            b.nickname.as_deref(),
            b.at,
            b.repeat_secs,
        )
        .await?;
    Ok(Json(json!({ "id": id })))
}

/// `POST /api/schedule/delete {"id"}`.
pub(crate) async fn delete_schedule(
    State(s): State<AppState>,
    body: Result<Json<IdBody>, JsonRejection>,
) -> Result<Json<Value>, ApiError> {
    let Json(b) = body?;
    Ok(Json(
        json!({ "deleted": s.console.delete_schedule(b.id).await? }),
    ))
}

#[derive(Deserialize)]
pub(crate) struct GroupBody {
    op: String,
    #[serde(default)]
    group: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    nickname: String,
    #[serde(default)]
    peer: String,
    #[serde(default)]
    cell: String,
    #[serde(default)]
    text: String,
    #[serde(default)]
    fingerprint: String,
}

/// `POST /api/group {"op": "create"|"invite"|"accept"|"decline"|"send"|
/// "remove"|"leave"|"destroy", ...}`.
pub(crate) async fn group(
    State(s): State<AppState>,
    body: Result<Json<GroupBody>, JsonRejection>,
) -> Result<Json<Value>, ApiError> {
    let Json(b) = body?;
    let op = match b.op.as_str() {
        "create" => GroupOp::Create {
            name: b.name,
            nickname: b.nickname,
        },
        "invite" => GroupOp::Invite {
            group: b.group,
            peer: b.peer,
            cell: b.cell,
        },
        "accept" => GroupOp::Accept {
            group: b.group,
            nickname: b.nickname,
        },
        "decline" => GroupOp::Decline { group: b.group },
        "send" => GroupOp::Send {
            group: b.group,
            text: b.text,
            nickname: b.nickname,
        },
        "remove" => GroupOp::Remove {
            group: b.group,
            fingerprint: b.fingerprint,
        },
        "leave" => GroupOp::Leave { group: b.group },
        "destroy" => GroupOp::Destroy { group: b.group },
        other => {
            return Err(ApiError::new(
                StatusCode::BAD_REQUEST,
                "bad_request",
                format!("unknown group operation {other:?}"),
            ));
        }
    };
    s.console.group(op).await?;
    Ok(Json(json!({ "ok": true })))
}

// --- History (lots 16 and 17, D31) ------------------------------------------

fn history(s: &AppState) -> Result<&History, ApiError> {
    s.console.history().ok_or_else(|| Error::NoHistory.into())
}

/// `GET /api/history?kind=&cell=&group=&key=&text=&from=&to=&deleted=true&order=new|old|sender&limit=&offset=`:
/// rows of the history, the newest first by default.
pub(crate) async fn history_query(
    State(s): State<AppState>,
    Query(q): Query<crate::history::Query>,
) -> Result<Json<Value>, ApiError> {
    Ok(Json(json!({ "records": history(&s)?.query(q).await? })))
}

/// `GET /api/history/{id}`.
pub(crate) async fn history_get(
    State(s): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<Value>, ApiError> {
    match history(&s)?.get(id).await? {
        Some(r) => Ok(Json(json!(r))),
        None => Err(Error::NoRow(id).into()),
    }
}

#[derive(Deserialize)]
pub(crate) struct IdsBody {
    ids: Vec<i64>,
}

/// `POST /api/history/delete {"ids": [..]}`: content removed, rows kept; the
/// page hides them.
pub(crate) async fn history_delete(
    State(s): State<AppState>,
    body: Result<Json<IdsBody>, JsonRejection>,
) -> Result<Json<Value>, ApiError> {
    let Json(b) = body?;
    Ok(Json(
        json!({ "deleted": history(&s)?.delete(b.ids).await? }),
    ))
}

#[derive(Deserialize)]
pub(crate) struct UpdateBody {
    id: i64,
    content: String,
}

/// `POST /api/history/update {"id", "content"}`: the page shows the new
/// content (moderation).
pub(crate) async fn history_update(
    State(s): State<AppState>,
    body: Result<Json<UpdateBody>, JsonRejection>,
) -> Result<Json<Value>, ApiError> {
    let Json(b) = body?;
    if !history(&s)?.update(b.id, b.content).await? {
        return Err(Error::NoRow(b.id).into());
    }
    Ok(Json(json!({ "updated": true })))
}

/// `POST /api/history/insert {record}`: a row put back (from an archive);
/// `id` null if that message is already there.
pub(crate) async fn history_insert(
    State(s): State<AppState>,
    body: Result<Json<Record>, JsonRejection>,
) -> Result<Json<Value>, ApiError> {
    let Json(r) = body?;
    Ok(Json(json!({ "id": history(&s)?.insert(r).await? })))
}
