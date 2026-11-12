//! The browser's routes, for this device's console and the chats' MCP
//! server only: none of them answers the user's other devices. Writes are
//! keyed (a bookmark's id, a setting's name) and the latest one wins, so a
//! retried request lands the same way twice. The bookmark write and delete
//! also record a receipt when the caller names a request, because the console
//! shows the answer and a second press must not read as a second change.

use std::sync::Arc;

use axum::extract::{Path as AxumPath, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use openremote_core::{Receipt, ReceiptStatus, SessionId};
use serde::Deserialize;
use serde_json::{Value, json};

use super::store::Bookmark;
use super::{Chat, lock, tools};
use crate::app::App;
use crate::http::receipt_response;

pub fn routes() -> Router<Arc<App>> {
    Router::new()
        .route("/browser", get(state))
        .route("/browser/history", get(history))
        .route("/browser/history/remove", post(remove_history))
        .route("/browser/history/clear", post(clear_history))
        .route("/browser/suggest", get(suggest))
        .route("/browser/tiles", get(tiles).post(pin_tile))
        .route("/browser/tiles/remove", post(remove_tile))
        .route("/browser/bookmarks", get(bookmarks).post(save_bookmark))
        .route("/browser/bookmarks/{id}/remove", post(remove_bookmark))
        .route("/browser/settings", get(settings).post(set_settings))
        .route("/browser/permissions", post(set_permission))
        .route("/browser/session", post(save_session))
        .route("/browser/tabs/{tab}/control", post(control))
        .route("/browser/tools", get(tool_list))
        .route("/sessions/{id}/browser/call", post(call_tool))
        .merge(super::adblock::routes())
}

fn bad(status: StatusCode, message: &str) -> Response {
    (status, Json(json!({"error": message}))).into_response()
}

/// Everything the console needs to draw the browser: the link, the tabs
/// (with who holds each), bookmarks, settings, permissions, the tabs to
/// bring back.
async fn state(State(app): State<Arc<App>>) -> Response {
    let browser = &app.browser;
    let store = lock(&browser.store);
    Json(json!({
        "status": browser.status(),
        "tabs": browser.tabs(),
        "downloads": browser.downloads(),
        "bookmarks": store.bookmarks(),
        "settings": store.settings(),
        "permissions": store.permissions(),
        "session": store.session(),
    }))
    .into_response()
}

#[derive(Deserialize)]
struct HistoryQuery {
    #[serde(default)]
    q: String,
    #[serde(default)]
    before: Option<i64>,
    #[serde(default)]
    limit: Option<usize>,
}

async fn history(State(app): State<Arc<App>>, Query(query): Query<HistoryQuery>) -> Response {
    let limit = query.limit.unwrap_or(100).min(500);
    Json(lock(&app.browser.store).history(&query.q, query.before, limit)).into_response()
}

#[derive(Deserialize)]
struct RemoveHistory {
    ids: Vec<String>,
}

async fn remove_history(State(app): State<Arc<App>>, Json(body): Json<RemoveHistory>) -> Response {
    lock(&app.browser.store).forget_visits(&body.ids);
    Json(json!({"ok": true})).into_response()
}

#[derive(Deserialize)]
struct ClearHistory {
    #[serde(default)]
    since: Option<i64>,
}

async fn clear_history(State(app): State<Arc<App>>, Json(body): Json<ClearHistory>) -> Response {
    lock(&app.browser.store).clear_history(body.since);
    Json(json!({"ok": true})).into_response()
}

#[derive(Deserialize)]
struct SuggestQuery {
    #[serde(default)]
    q: String,
}

async fn suggest(State(app): State<Arc<App>>, Query(query): Query<SuggestQuery>) -> Response {
    Json(lock(&app.browser.store).suggest(&query.q, 6)).into_response()
}

async fn tiles(State(app): State<Arc<App>>) -> Response {
    Json(lock(&app.browser.store).tiles(8)).into_response()
}

#[derive(Deserialize)]
struct TileBody {
    url: String,
    #[serde(default)]
    title: String,
}

async fn pin_tile(State(app): State<Arc<App>>, Json(body): Json<TileBody>) -> Response {
    if body.url.trim().is_empty() {
        return bad(StatusCode::BAD_REQUEST, "a tile needs an address");
    }
    lock(&app.browser.store).pin_tile(body.url.trim(), body.title.trim());
    Json(json!({"ok": true})).into_response()
}

async fn remove_tile(State(app): State<Arc<App>>, Json(body): Json<TileBody>) -> Response {
    lock(&app.browser.store).remove_tile(&body.url);
    Json(json!({"ok": true})).into_response()
}

/// The bookmark rows, in the shape a sync note carries: the list the console
/// draws and the one another device's note merges into.
async fn bookmarks(State(app): State<Arc<App>>) -> Response {
    let store = lock(&app.browser.store);
    Json(json!({"bookmarks": store.bookmarks()})).into_response()
}

/// The settings the console changed, as the map it draws. A null value is a
/// setting put back to its default, which is why it is absent here.
async fn settings(State(app): State<Arc<App>>) -> Response {
    let store = lock(&app.browser.store);
    Json(store.settings()).into_response()
}

/// One bookmark row to write: the console's fields, and the request it names.
#[derive(Deserialize)]
struct SaveBookmarkBody {
    /// Absent for a caller that does not want the answer remembered (the
    /// chats' tools); the console always names one.
    #[serde(default)]
    request_id: Option<String>,
    #[serde(flatten)]
    mark: Bookmark,
}

/// Write one bookmark row (add or edit). The write is keyed by the row's id,
/// so a retried request lands the same way twice; a named request also records
/// a receipt and answers with it, so the console's second press reads as one
/// change rather than two.
async fn save_bookmark(
    State(app): State<Arc<App>>,
    Json(body): Json<SaveBookmarkBody>,
) -> Response {
    if body.mark.url.trim().is_empty() {
        return bad(StatusCode::BAD_REQUEST, "a bookmark needs an address");
    }
    let Some(request_id) = body.request_id else {
        return Json(lock(&app.browser.store).save_bookmark(body.mark)).into_response();
    };
    if let Some(receipt) = app.supervisor.receipt(&request_id) {
        return receipt_response(&receipt);
    }
    let saved = lock(&app.browser.store).save_bookmark(body.mark);
    let receipt = Receipt {
        request_id,
        session_id: None,
        status: ReceiptStatus::Completed,
        result: Some(json!({"bookmark": saved})),
        error: None,
        updated_at: openremote_core::now_ms(),
    };
    app.supervisor.record_receipt(receipt.clone());
    receipt_response(&receipt)
}

/// Delete one bookmark row: the row keeps a tombstone (a null value), which is
/// what the merge understands, so the deletion travels to the account's other
/// devices instead of the row quietly reappearing from an older copy.
async fn remove_bookmark(
    State(app): State<Arc<App>>,
    AxumPath(id): AxumPath<String>,
    body: Option<Json<RequestIdBody>>,
) -> Response {
    let named = body.and_then(|Json(body)| body.request_id);
    let Some(request_id) = named else {
        lock(&app.browser.store).remove_bookmark(&id);
        return Json(json!({"ok": true})).into_response();
    };
    if let Some(receipt) = app.supervisor.receipt(&request_id) {
        return receipt_response(&receipt);
    }
    lock(&app.browser.store).remove_bookmark(&id);
    let receipt = Receipt {
        request_id,
        session_id: None,
        status: ReceiptStatus::Completed,
        result: Some(json!({"removed": id})),
        error: None,
        updated_at: openremote_core::now_ms(),
    };
    app.supervisor.record_receipt(receipt.clone());
    receipt_response(&receipt)
}

#[derive(Deserialize)]
struct RequestIdBody {
    request_id: Option<String>,
}

/// Settings the console changed: `{name: value}`; a null puts one back to
/// its default.
async fn set_settings(
    State(app): State<Arc<App>>,
    Json(body): Json<serde_json::Map<String, Value>>,
) -> Response {
    let mut store = lock(&app.browser.store);
    for (key, value) in body {
        store.set_setting(&key, value);
    }
    Json(store.settings()).into_response()
}

#[derive(Deserialize)]
struct PermissionBody {
    origin: String,
    kind: String,
    /// `allow`, `deny`, or none to ask again next time.
    #[serde(default)]
    state: Option<String>,
}

async fn set_permission(State(app): State<Arc<App>>, Json(body): Json<PermissionBody>) -> Response {
    if !matches!(body.state.as_deref(), None | Some("allow" | "deny")) {
        return bad(StatusCode::BAD_REQUEST, "state is allow, deny or none");
    }
    let mut store = lock(&app.browser.store);
    store.set_permission(&body.origin, &body.kind, body.state.as_deref());
    Json(store.permissions()).into_response()
}

/// The tabs open now, to bring back at the next start.
async fn save_session(State(app): State<Arc<App>>, Json(body): Json<Value>) -> Response {
    lock(&app.browser.store).save_session(body);
    Json(json!({"ok": true})).into_response()
}

#[derive(Deserialize)]
struct ControlBody {
    action: String,
}

/// The user taking over a tab an agent is using, handing it back, or
/// letting the agent's hold go.
async fn control(
    State(app): State<Arc<App>>,
    AxumPath(tab): AxumPath<String>,
    Json(body): Json<ControlBody>,
) -> Response {
    match app.browser.control(&tab, &body.action) {
        Ok(controller) => Json(json!({"controller": controller})).into_response(),
        Err(error) => bad(StatusCode::CONFLICT, &error),
    }
}

/// The browser tools a chat's MCP server lists: none where no app hosts a
/// browser (a machine's service, a daemon started by hand).
async fn tool_list(State(app): State<Arc<App>>) -> Response {
    let tools: Vec<Value> = if app.browser.offers_tools() {
        tools::TOOLS.iter().map(tools::Tool::listing).collect()
    } else {
        Vec::new()
    };
    Json(json!({"tools": tools})).into_response()
}

#[derive(Deserialize)]
struct CallBody {
    name: String,
    #[serde(default)]
    arguments: Value,
}

/// One of a chat's browser tools (OpenRemote's MCP server calls this for
/// the chat's agent). Always answers an MCP tool result: a refusal is an
/// error result the agent reads, not a failed request.
async fn call_tool(
    State(app): State<Arc<App>>,
    AxumPath(id): AxumPath<String>,
    Json(body): Json<CallBody>,
) -> Response {
    let Some(session_id) = SessionId::parse(&id) else {
        return bad(StatusCode::BAD_REQUEST, "bad session id");
    };
    let chat = {
        let store = crate::supervisor::lock_recovering(&app.store);
        match store.session(&session_id) {
            Ok(session) => Chat {
                id: session.id.to_string(),
                title: session
                    .title
                    .clone()
                    .filter(|t| !t.trim().is_empty())
                    .unwrap_or_else(|| "A chat".into()),
                workspace: session.workspace.clone(),
            },
            Err(_) => return bad(StatusCode::NOT_FOUND, "no such chat"),
        }
    };
    Json(
        app.browser
            .run_tool(&chat, &body.name, &body.arguments)
            .await,
    )
    .into_response()
}
