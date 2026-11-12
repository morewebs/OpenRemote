//! The plugin runtime's routes, for this device's console and its chats'
//! MCP server only (token auth, merged beside the browser's routes and
//! never into the API a peer reaches): a plugin's settings, its files for
//! the console's sandboxed frames, its views' calls to its program, and the
//! agent tools a chat gets. Settings are keyed writes - the same change
//! applied twice lands the same way - so they need no receipts.

use std::path::{Component, Path};
use std::sync::Arc;

use axum::extract::{Path as AxumPath, State};
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use openremote_core::SessionId;
use serde::Deserialize;
use serde_json::{Map, Value, json};

use super::{ChatInfo, settings};
use crate::app::App;
use crate::supervisor::lock_recovering;

/// The console's and the chats' routes.
pub fn routes() -> Router<Arc<App>> {
    Router::new()
        .route(
            "/plugins/{id}/settings",
            get(get_settings).post(set_settings),
        )
        .route("/plugins/{id}/files/{*path}", get(file))
        .route("/plugins/{id}/request", post(view_request))
        .route("/sessions/{id}/plugins/tools", get(chat_tools))
        .route("/sessions/{id}/plugins/call", post(call_tool))
}

fn bad(status: StatusCode, message: &str) -> Response {
    (status, Json(json!({"error": message}))).into_response()
}

fn installed(app: &App, id: &str) -> Result<openremote_core::InstalledPlugin, Box<Response>> {
    app.plugin_runtime.plugin(id).ok_or_else(|| {
        Box::new(bad(
            StatusCode::NOT_FOUND,
            &format!("{id} is not installed"),
        ))
    })
}

/// A plugin's settings: its schema, every plain value, and for each secret
/// only `set` or `unset`.
async fn get_settings(State(app): State<Arc<App>>, AxumPath(id): AxumPath<String>) -> Response {
    let plugin = match installed(&app, &id) {
        Ok(p) => p,
        Err(r) => return *r,
    };
    let stored = app.plugin_runtime.settings.load(&id);
    Json(settings::view(&id, &plugin.contributes.settings, &stored)).into_response()
}

/// A `request_id` beside the values is welcome and unneeded: a settings
/// change is keyed, so a retry lands the same way.
#[derive(Deserialize)]
struct SettingsBody {
    values: Map<String, Value>,
}

/// Change some settings: `{"values": {"key": value}}`, a null putting one
/// back to its default (a secret, to unset). A running program hears the
/// new values at once.
async fn set_settings(
    State(app): State<Arc<App>>,
    AxumPath(id): AxumPath<String>,
    Json(body): Json<SettingsBody>,
) -> Response {
    let plugin = match installed(&app, &id) {
        Ok(p) => p,
        Err(r) => return *r,
    };
    let mut stored = app.plugin_runtime.settings.load(&id);
    if let Err(why) = settings::apply(&plugin.contributes.settings, &mut stored, &body.values) {
        return bad(StatusCode::BAD_REQUEST, &why);
    }
    if let Err(e) = app.plugin_runtime.settings.save(&id, &stored) {
        return bad(
            StatusCode::INTERNAL_SERVER_ERROR,
            &format!("the settings could not be saved: {e}"),
        );
    }
    app.plugin_runtime.settings_changed(&plugin).await;
    Json(settings::view(&id, &plugin.contributes.settings, &stored)).into_response()
}

/// The content type a file is served with, by its extension.
fn content_type(path: &Path) -> &'static str {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    match ext.as_str() {
        "html" | "htm" => "text/html; charset=utf-8",
        "js" | "mjs" | "cjs" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "json" | "map" => "application/json; charset=utf-8",
        "txt" | "md" => "text/plain; charset=utf-8",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "ico" => "image/x-icon",
        "avif" => "image/avif",
        "wasm" => "application/wasm",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "ttf" => "font/ttf",
        "otf" => "font/otf",
        "mp3" => "audio/mpeg",
        "wav" => "audio/wav",
        "ogg" => "audio/ogg",
        "mp4" => "video/mp4",
        "webm" => "video/webm",
        _ => "application/octet-stream",
    }
}

/// One of an enabled plugin's own files, for the console's sandboxed
/// frames: only a file inside that plugin's folder, checked after every
/// link is followed, so no path climbs out of it. Served as a sandboxed
/// document of its own, so a page opened outside its frame still runs in
/// no origin of OpenRemote's.
async fn file(
    State(app): State<Arc<App>>,
    AxumPath((id, path)): AxumPath<(String, String)>,
) -> Response {
    let plugin = match installed(&app, &id) {
        Ok(p) => p,
        Err(r) => return *r,
    };
    if !plugin.enabled {
        return bad(StatusCode::NOT_FOUND, "the plugin is turned off");
    }
    let Some(root) = app.plugin_runtime.folder(&plugin) else {
        return bad(StatusCode::NOT_FOUND, "the plugin's files are gone");
    };
    // A repo plugin's files are served only while they are what was
    // approved, as its tools run only then.
    if !app.plugin_runtime.approved(&plugin) {
        return bad(
            StatusCode::CONFLICT,
            "its files in the project changed since you approved them; review it again under Plugins",
        );
    }
    let relative = Path::new(&path);
    if path.is_empty()
        || path.contains('\\')
        || relative
            .components()
            .any(|c| !matches!(c, Component::Normal(_)))
    {
        return bad(StatusCode::BAD_REQUEST, "not a path inside the plugin");
    }
    let (Ok(root), Ok(target)) = (
        std::fs::canonicalize(&root),
        std::fs::canonicalize(root.join(relative)),
    ) else {
        return bad(StatusCode::NOT_FOUND, "no such file in the plugin");
    };
    if !target.starts_with(&root) || !target.is_file() {
        return bad(StatusCode::NOT_FOUND, "no such file in the plugin");
    }
    match tokio::fs::read(&target).await {
        Ok(bytes) => (
            [
                (header::CONTENT_TYPE, content_type(&target)),
                (header::X_CONTENT_TYPE_OPTIONS, "nosniff"),
                (header::CACHE_CONTROL, "no-cache"),
                (header::CONTENT_SECURITY_POLICY, "sandbox allow-scripts"),
            ],
            bytes,
        )
            .into_response(),
        Err(_) => bad(StatusCode::NOT_FOUND, "no such file in the plugin"),
    }
}

#[derive(Deserialize)]
struct RequestBody {
    name: String,
    #[serde(default)]
    params: Value,
}

/// A plugin view asks its own program something (the console relays it
/// from the view's sandboxed frame): `{"name", "params"}` in, the
/// program's answer out.
async fn view_request(
    State(app): State<Arc<App>>,
    AxumPath(id): AxumPath<String>,
    Json(body): Json<RequestBody>,
) -> Response {
    match app
        .plugin_runtime
        .request(&id, &body.name, body.params)
        .await
    {
        Ok(result) => Json(json!({"result": result})).into_response(),
        Err(why) => bad(StatusCode::BAD_GATEWAY, &why),
    }
}

fn chat(app: &App, raw: &str) -> Result<ChatInfo, Box<Response>> {
    let id = SessionId::parse(raw)
        .ok_or_else(|| Box::new(bad(StatusCode::BAD_REQUEST, "bad session id")))?;
    let store = lock_recovering(&app.store);
    let session = store
        .session(&id)
        .map_err(|_| Box::new(bad(StatusCode::NOT_FOUND, "no such chat")))?;
    Ok(ChatInfo {
        id,
        workspace: session.workspace.clone(),
    })
}

/// The plugin tools a chat's agent gets (OpenRemote's MCP server lists
/// them beside its own).
async fn chat_tools(State(app): State<Arc<App>>, AxumPath(id): AxumPath<String>) -> Response {
    let chat = match chat(&app, &id) {
        Ok(c) => c,
        Err(r) => return *r,
    };
    // Only the plugins this chat gets: a project's own ride its chats alone.
    Json(json!({"tools": app.plugin_runtime.tool_listing(chat.workspace.as_deref())}))
        .into_response()
}

#[derive(Deserialize)]
struct CallBody {
    name: String,
    #[serde(default)]
    arguments: Value,
}

/// Run a plugin tool for a chat's agent. Always an MCP tool result.
async fn call_tool(
    State(app): State<Arc<App>>,
    AxumPath(id): AxumPath<String>,
    Json(body): Json<CallBody>,
) -> Response {
    let chat = match chat(&app, &id) {
        Ok(c) => c,
        Err(r) => return *r,
    };
    let arguments = if body.arguments.is_null() {
        json!({})
    } else {
        body.arguments
    };
    Json(
        app.plugin_runtime
            .call_tool(&chat, &body.name, arguments)
            .await,
    )
    .into_response()
}
