//! The browser's routes, for this device's console and the chats' MCP
//! server only: none of them answers the user's other devices (none is in
//! `PEER_ROUTES`, and a test in `http.rs` keeps it so). That matters most for
//! the import, which reads other browsers' files on this computer. Writes are
//! keyed (a bookmark's id, a setting's name) and the latest one wins, so a
//! retried request lands the same way twice. The bookmark write and delete
//! also record a receipt when the caller names a request, because the console
//! shows the answer and a second press must not read as a second change.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use axum::extract::{Path as AxumPath, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use openremote_core::{Receipt, ReceiptStatus, SessionId};
use serde::Deserialize;
use serde_json::{Value, json};

use super::store::Bookmark;
use super::{Chat, import, lock, tools};
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
        .route("/browser/import", get(import_sources).post(import_run))
        .route("/browser/tools", get(tool_list))
        .route("/sessions/{id}/browser/call", post(call_tool))
        .route("/sessions/{id}/browser/shared", post(shared))
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

/// The profiles of the other browsers on this computer, found in their own
/// folders: names and what each holds, never a path.
async fn import_sources() -> Response {
    let sources = tokio::task::spawn_blocking(|| import::sources(&import::Places::here()))
        .await
        .unwrap_or_default();
    let listed: Vec<Value> = sources.iter().map(import::Source::listing).collect();
    Json(json!({"sources": listed})).into_response()
}

fn yes() -> bool {
    true
}

/// One import: a profile [`import_sources`] listed (by its id), or a folder
/// the user picked in the console's own folder dialog.
#[derive(Deserialize)]
struct ImportBody {
    #[serde(default)]
    source: Option<String>,
    #[serde(default)]
    path: Option<String>,
    #[serde(default = "yes")]
    bookmarks: bool,
    #[serde(default = "yes")]
    history: bool,
}

/// One import at a time: a second press while one runs is refused rather
/// than read twice.
static IMPORTING: AtomicBool = AtomicBool::new(false);

/// The one import running: held by the work itself, released when it ends.
struct Importing;

impl Importing {
    fn begin() -> Option<Self> {
        IMPORTING
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .ok()
            .map(|_| Importing)
    }
}

impl Drop for Importing {
    fn drop(&mut self) {
        IMPORTING.store(false, Ordering::SeqCst);
    }
}

/// Runs an import's work on a blocking thread that holds `guard`. The guard
/// goes when the work ends, not when the request does: a console that
/// disconnects mid-import drops this future, but the read and the store
/// write carry on, and a second import cannot start beside them. None when
/// the work panicked.
async fn detached<T: Send + 'static>(
    guard: Importing,
    work: impl FnOnce() -> T + Send + 'static,
) -> Option<T> {
    tokio::task::spawn_blocking(move || {
        let _held = guard;
        work()
    })
    .await
    .ok()
}

/// Bring in another browser's bookmarks and history. The files are read on
/// a blocking thread from private copies (see [`import`]); the store then
/// takes what fits under the import's caps in one write.
async fn import_run(State(app): State<Arc<App>>, Json(body): Json<ImportBody>) -> Response {
    if !body.bookmarks && !body.history {
        return bad(
            StatusCode::BAD_REQUEST,
            "Pick bookmarks, history or both to import.",
        );
    }
    let source = match (body.source, body.path) {
        (Some(id), None) => {
            let found = tokio::task::spawn_blocking(move || {
                import::sources(&import::Places::here())
                    .into_iter()
                    .find(|s| s.id == id)
            })
            .await
            .ok()
            .flatten();
            match found {
                Some(source) => source,
                None => {
                    return bad(
                        StatusCode::NOT_FOUND,
                        "That browser profile isn't on this computer any more.",
                    );
                }
            }
        }
        (None, Some(path)) => {
            match tokio::task::spawn_blocking(move || {
                import::source_at(std::path::Path::new(&path))
            })
            .await
            {
                Ok(Ok(source)) => source,
                Ok(Err(why)) => return bad(StatusCode::BAD_REQUEST, &why),
                Err(_) => {
                    return bad(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "The import stopped unexpectedly.",
                    );
                }
            }
        }
        _ => {
            return bad(
                StatusCode::BAD_REQUEST,
                "Name a browser profile, or pick its folder.",
            );
        }
    };
    let Some(guard) = Importing::begin() else {
        return bad(StatusCode::CONFLICT, "An import is already running.");
    };
    let want = import::Want {
        bookmarks: body.bookmarks,
        history: body.history,
    };
    let now = openremote_core::now_ms();
    let reading = source.clone();
    let working = Arc::clone(&app);
    // Reading and the store's write both happen off the async threads: the
    // write appends thousands of rows and may compact the log.
    let outcome = detached(guard, move || {
        let folder = lock(&working.browser.store).folder().to_path_buf();
        let found = import::read(&reading, want, now, &folder)?;
        let refused = found.refused;
        let done = lock(&working.browser.store).import(found.bookmarks, found.visits);
        Ok::<_, String>((refused, done))
    })
    .await;
    let (refused, done) = match outcome {
        Some(Ok(done)) => done,
        Some(Err(why)) => return bad(StatusCode::UNPROCESSABLE_ENTITY, &why),
        None => {
            return bad(
                StatusCode::INTERNAL_SERVER_ERROR,
                "The import stopped unexpectedly.",
            );
        }
    };
    Json(json!({
        "browser": source.browser,
        "profile": source.profile,
        "bookmarks": done.bookmarks,
        "already": done.already,
        "over_cap": done.over_cap,
        "visits": done.visits,
        "refused": refused,
    }))
    .into_response()
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

/// The user shared a page, a picture of one or a picked element with a
/// chat from the browser: the browser is in play for that chat, as when its
/// agent calls a browser tool, so its MCP server lists the browser tools
/// from now on. The console's own call; no peer reaches it.
async fn shared(State(app): State<Arc<App>>, AxumPath(id): AxumPath<String>) -> Response {
    let Some(session_id) = SessionId::parse(&id) else {
        return bad(StatusCode::BAD_REQUEST, "bad session id");
    };
    match crate::supervisor::lock_recovering(&app.store).note_browser_used(&session_id) {
        Ok(()) => Json(json!({"shared": true})).into_response(),
        Err(openremote_core::store::StoreError::NotFound(_)) => {
            bad(StatusCode::NOT_FOUND, "no such chat")
        }
        Err(e) => bad(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    }
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
    // Any browser tool, even one that fails, puts the browser in play for
    // the chat: its MCP server lists the rest from now on. A name that is no
    // browser tool (old names included, as they still call) puts nothing.
    let real = !tools::current_name(&body.name, &body.arguments)
        .0
        .is_empty();
    if real {
        let kept = crate::supervisor::lock_recovering(&app.store).note_browser_used(&session_id);
        if let Err(e) = kept {
            eprintln!("browser use not kept for {session_id}: {e}");
        }
    }
    Json(
        app.browser
            .run_tool(&chat, &body.name, &body.arguments)
            .await,
    )
    .into_response()
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::{Importing, detached};

    /// A console that goes away mid-import (its request dropped) does not end
    /// the import's hold: a second import is refused until the first one's
    /// work has finished, and is welcome after.
    #[tokio::test]
    async fn an_import_holds_its_place_until_its_work_ends_not_its_request() {
        let guard = Importing::begin().expect("no import is running");
        let (finish, finished) = std::sync::mpsc::channel::<()>();
        let run = detached(guard, move || {
            let _ = finished.recv_timeout(Duration::from_secs(10));
        });
        // The request goes before the work ends.
        let _ = tokio::time::timeout(Duration::from_millis(20), run).await;
        assert!(Importing::begin().is_none(), "a second import could start");
        finish.send(()).unwrap();
        for _ in 0..500 {
            if Importing::begin().is_some() {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("the hold was never released");
    }
}
