//! The HTTP API. Bearer-token loopback auth (`/healthz` open); every
//! mutating call carries a client `request_id` that lands in the receipt
//! table, so retries dedup and crashes surface as `unknown`, never resent.

use std::sync::Arc;

use axum::body::Body;
use axum::extract::Path as AxumPath;
use axum::extract::{Request, State};
use axum::http::{Method, StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use openremote_core::{DecisionId, MachineId, Receipt, ReceiptStatus, SessionId};
use serde::Deserialize;
use serde_json::{Value, json};
use tower_http::cors::{AllowOrigin, CorsLayer};

use crate::app::App;
use crate::supervisor::SupervisorError;

pub fn router(app: Arc<App>) -> Router {
    let authed = api_routes()
        .route("/cloud", get(crate::cloud_http::view))
        .route("/cloud/signin", post(crate::cloud_http::sign_in))
        .route(
            "/cloud/signin/cancel",
            post(crate::cloud_http::cancel_sign_in),
        )
        .route("/cloud/signout", post(crate::cloud_http::sign_out))
        .route("/cloud/devices", get(crate::cloud_http::devices))
        .route(
            "/cloud/devices/{id}",
            axum::routing::delete(crate::cloud_http::remove_device),
        )
        .route(
            "/cloud/devices/{id}/kind",
            post(crate::cloud_http::set_kind),
        )
        .route(
            "/cloud/enrollments",
            post(crate::cloud_http::create_enrollment),
        )
        // The third-party notices ride in the binary itself: the file is
        // the same one bundled beside the installers, but the standalone
        // Linux daemon has no install dir to read - so it serves what it
        // was built with. Console-only: a peer never needs it.
        .route("/notices", get(notices))
        // The chats' pictures, by hash: this device's console only. A peer
        // asks for one chat's picture through the chat's own route.
        .route("/blobs/{sha}", get(get_blob))
        // The built-in browser: this device's console and its chats' MCP
        // server only, never another device.
        .merge(crate::browser::routes::routes())
        // Agent teams: the user's limits, a chat's team tools, Stop all.
        // This device's console and its chats' MCP server only.
        .merge(crate::team::routes())
        // The installed plugins' settings, files, view requests and agent
        // tools: this device's console and its chats' MCP server only,
        // never another device (not in `api_routes`, so no peer reaches
        // them).
        .merge(crate::plugin_runtime::routes::routes())
        // The console reaches another device's API through this one.
        .route(
            "/devices/{id}/{*rest}",
            axum::routing::any(crate::cloud_http::passthrough).layer(
                axum::extract::DefaultBodyLimit::max(openremote_cloud::rpc::MAX_REQUEST),
            ),
        )
        .route_layer(middleware::from_fn_with_state(
            Arc::clone(&app),
            crate::cloud_http::forward_remote,
        ))
        .layer(middleware::from_fn_with_state(Arc::clone(&app), auth));
    Router::new()
        .route("/healthz", get(healthz))
        // Webhooks arrive from outside the console's trust - the rule's
        // own key is the credential, not the daemon token. (The loopback
        // binding still keeps the surface local.)
        .route("/hooks/{id}", post(incoming_webhook))
        // The browser lands here after moreweb sign-in. No daemon token:
        // the single-use state and PKCE are what protect it.
        .route("/cloud/callback", get(crate::cloud_http::callback))
        .merge(authed)
        .layer(console_cors())
        .with_state(app)
}

/// The API as the user's other devices see it: the same handlers, behind
/// `peer_gate`. No daemon token: the mesh already proved the caller is one
/// of the account's devices.
pub fn peer_router(app: Arc<App>) -> Router {
    api_routes()
        .route_layer(middleware::from_fn_with_state(Arc::clone(&app), peer_gate))
        .with_state(app)
}

#[derive(Clone, Copy, PartialEq)]
enum Tier {
    /// Copying synced chats: any of the account's devices.
    Sync,
    /// Driving this computer: only when it is a machine.
    Machine,
    /// Acting on one chat: a machine, and the chat is synced and runs here.
    Chat,
    /// Reading one chat's pictures: any of the account's devices, as with
    /// copying the chat's events, and only a synced chat that runs here -
    /// the device that keeps its pictures.
    Read,
}

/// What another of the user's devices may ask of this one. MCP servers,
/// automations, webhooks, receipts and Cloud itself never answer a peer.
const PEER_ROUTES: &[(&str, &str, Tier)] = &[
    ("GET", "/sync/summary", Tier::Sync),
    ("GET", "/sync/sessions/{id}/events", Tier::Sync),
    ("GET", "/capabilities", Tier::Machine),
    ("GET", "/machines", Tier::Machine),
    ("GET", "/harnesses/{id}/models", Tier::Machine),
    ("GET", "/fs/dirs", Tier::Machine),
    // What a composer's / and @ menus offer for a chat on this machine.
    ("GET", "/commands", Tier::Machine),
    ("GET", "/files", Tier::Machine),
    ("POST", "/harnesses/{id}/signin", Tier::Machine),
    ("GET", "/harnesses/{id}/signin", Tier::Machine),
    ("POST", "/harnesses/{id}/signin/input", Tier::Machine),
    ("POST", "/harnesses/{id}/signin/stop", Tier::Machine),
    ("POST", "/machines/{id}/harnesses", Tier::Machine),
    ("POST", "/sessions", Tier::Machine),
    // A project lives on the machine that owns its folder, so the phone
    // and every other device manage them there.
    ("GET", "/projects", Tier::Machine),
    ("POST", "/projects", Tier::Machine),
    ("DELETE", "/projects/{id}", Tier::Machine),
    ("POST", "/sessions/{id}/prompts", Tier::Chat),
    ("POST", "/sessions/{id}/queue", Tier::Chat),
    ("POST", "/sessions/{id}/queue/resume", Tier::Chat),
    ("POST", "/sessions/{id}/queue/{item}/remove", Tier::Chat),
    ("POST", "/sessions/{id}/queue/{item}/steer", Tier::Chat),
    ("POST", "/sessions/{id}/steer", Tier::Chat),
    ("POST", "/sessions/{id}/settings", Tier::Chat),
    ("POST", "/sessions/{id}/endpoint", Tier::Chat),
    ("GET", "/model-endpoints", Tier::Machine),
    ("GET", "/model-endpoints/{id}/models", Tier::Machine),
    ("POST", "/sessions/{id}/interrupt", Tier::Chat),
    ("POST", "/sessions/{id}/stop", Tier::Chat),
    ("POST", "/sessions/{id}/resume", Tier::Chat),
    ("POST", "/sessions/{id}/title", Tier::Chat),
    ("POST", "/decisions/{id}/answer", Tier::Chat),
    // A synced chat's pictures, for the devices that show it.
    ("GET", "/sessions/{id}/blobs/{sha}", Tier::Read),
    // The account's Global plugins: their records, and the files of one a
    // device lacks (only a copy that is exactly the one its record names).
    // What arrives is reviewed on that device before anything installs.
    ("GET", "/sync/plugins", Tier::Sync),
    ("GET", "/sync/plugins/{id}/archive", Tier::Sync),
];

async fn peer_gate(
    State(app): State<Arc<App>>,
    matched: Option<axum::extract::MatchedPath>,
    request: Request,
    next: Next,
) -> Response {
    let path = matched.as_ref().map(|m| m.as_str()).unwrap_or_default();
    let method = request.method().as_str();
    let Some((_, _, tier)) = PEER_ROUTES
        .iter()
        .find(|(m, p, _)| *m == method && *p == path)
    else {
        return error(StatusCode::FORBIDDEN, "not available to your other devices");
    };
    if *tier == Tier::Sync {
        return next.run(request).await;
    }
    if *tier == Tier::Read {
        if !synced_here(&app, request.uri().path(), path) {
            return error(StatusCode::NOT_FOUND, "no such chat here");
        }
        return next.run(request).await;
    }
    // A desktop that isn't a machine only ever syncs; nobody else drives it.
    if !app.cloud.mesh().is_some_and(|m| m.kind() == "machine") {
        return (
            StatusCode::CONFLICT,
            Json(json!({"error": "this computer isn't a machine", "code": "not_a_machine"})),
        )
            .into_response();
    }
    // Private chats are invisible to peers: the same answer as none.
    if *tier == Tier::Chat && !synced_here(&app, request.uri().path(), path) {
        return error(StatusCode::NOT_FOUND, "no such chat here");
    }
    next.run(request).await
}

/// Whether the chat a peer's request names (by its own id, or by one of
/// its decisions) is synced and runs on this device.
fn synced_here(app: &App, uri_path: &str, matched: &str) -> bool {
    let id = uri_path.split('/').nth(2).unwrap_or_default();
    let store = crate::supervisor::lock_recovering(&app.store);
    let session = if matched.starts_with("/decisions/") {
        DecisionId::parse(id)
            .and_then(|d| store.decision(&d).ok().map(|d| d.session_id))
            .and_then(|s| store.session(&s).ok())
    } else {
        SessionId::parse(id).and_then(|s| store.session(&s).ok())
    };
    session.is_some_and(|s| s.executor.is_some() && store.executes_here(s))
}

/// Every route the console and (through the gate) peers share.
fn api_routes() -> Router<Arc<App>> {
    Router::new()
        .route("/capabilities", get(capabilities))
        .route("/fs/dirs", get(fs_dirs))
        .route("/commands", get(get_commands))
        .route("/files", get(get_files))
        .route("/sessions/{id}/mentions/{chat}", get(get_mentioned_chat))
        .route("/sessions/{id}/title", post(post_title))
        .route("/sync/summary", get(sync_summary))
        .route("/sync/sessions/{id}/events", get(sync_range))
        .route("/harnesses/{id}/models", get(harness_models))
        .route(
            "/harnesses/{id}/catalog/refresh",
            post(refresh_harness_catalog),
        )
        .route(
            "/harnesses/{id}/signin",
            post(start_sign_in).get(sign_in_view),
        )
        .route("/harnesses/{id}/signin/input", post(sign_in_input))
        .route("/harnesses/{id}/signin/stop", post(stop_sign_in))
        .route("/machines", get(list_machines).post(create_machine))
        .route("/machines/{id}", get(get_machine).delete(remove_machine))
        .route("/machines/{id}/harnesses", post(install_harness))
        .route(
            "/mcp-servers",
            get(list_mcp_servers).post(create_mcp_server),
        )
        .route("/plugins", get(list_plugins))
        .route("/plugins/stage", post(stage_plugin))
        .route("/plugins/stage/{id}/install", post(install_staged))
        .route("/plugins/stage/{id}/cancel", post(cancel_staged))
        .route("/plugins/{id}/enabled", post(set_plugin_enabled))
        .route("/plugins/{id}/remove", post(remove_plugin))
        .route(
            "/marketplaces",
            get(list_marketplaces).post(add_marketplace),
        )
        .route("/marketplaces/catalog", get(marketplace_catalog))
        .route("/marketplaces/{name}/refresh", post(refresh_marketplace))
        .route(
            "/marketplaces/{name}/enabled",
            post(set_marketplace_enabled),
        )
        .route("/marketplaces/{name}/remove", post(remove_marketplace))
        .route(
            "/mcp-servers/{id}",
            axum::routing::delete(remove_mcp_server),
        )
        .route("/mcp-servers/{id}/enabled", post(set_mcp_server_enabled))
        .route("/mcp-servers/{id}/key", post(acknowledge_mcp_server_key))
        .route("/automations", get(list_rules).post(save_rule))
        .route("/automations/{id}", axum::routing::delete(remove_rule))
        .route("/automations/{id}/enabled", post(set_rule_enabled))
        .route("/automations/{id}/run", post(run_rule))
        .route(
            "/model-endpoints",
            get(list_model_endpoints).post(save_model_endpoint),
        )
        .route("/model-endpoints/default", post(set_default_model_endpoint))
        .route(
            "/model-endpoints/{id}",
            axum::routing::delete(remove_model_endpoint),
        )
        .route(
            "/model-endpoints/{id}/enabled",
            post(set_model_endpoint_enabled),
        )
        .route("/model-endpoints/{id}/models", get(model_endpoint_models))
        .route("/projects", get(list_projects).post(save_project))
        .route("/projects/{id}", axum::routing::delete(remove_project))
        .route("/sessions", get(list_sessions).post(create_session))
        .route("/sessions/{id}", get(get_session).delete(delete_session))
        .route("/sessions/{id}/events", get(session_events))
        .route("/sessions/{id}/decisions", get(list_decisions))
        // A message can carry pictures: past axum's 2 MB default.
        .route(
            "/sessions/{id}/prompts",
            post(post_prompt).layer(message_body_limit()),
        )
        .route(
            "/sessions/{id}/queue",
            post(post_queue).layer(message_body_limit()),
        )
        .route("/sessions/{id}/queue/resume", post(post_queue_resume))
        .route(
            "/sessions/{id}/queue/{item}/remove",
            post(post_queue_remove),
        )
        .route("/sessions/{id}/queue/{item}/steer", post(post_queue_steer))
        .route(
            "/sessions/{id}/steer",
            post(post_steer).layer(message_body_limit()),
        )
        .route("/sessions/{id}/blobs/{sha}", get(get_session_blob))
        .route("/sessions/{id}/settings", post(post_settings))
        .route("/sessions/{id}/endpoint", post(post_session_endpoint))
        .route("/sessions/{id}/workspace", post(post_workspace))
        .route("/sessions/{id}/interrupt", post(post_interrupt))
        .route("/sessions/{id}/stop", post(post_stop))
        .route("/sessions/{id}/resume", post(post_resume))
        .route("/sessions/{id}/sync", post(post_sync))
        .route("/decisions/{id}/answer", post(post_answer))
        .route("/receipts/{request_id}", get(get_receipt))
        .merge(crate::plugin_runtime::global::peer_routes())
}

/// The console is a webview on a different origin than the daemon
/// (vite dev `http://localhost:5175`, Tauri production
/// `http://tauri.localhost` / `tauri://localhost`), and webviews enforce
/// same-origin on fetch - without these headers the console can never
/// reach the daemon. Loopback-only console origins; everything else
/// stays blocked (the bearer token still guards every real route).
fn console_cors() -> CorsLayer {
    CorsLayer::new()
        .allow_origin(AllowOrigin::predicate(|origin, _| {
            let Ok(s) = origin.to_str() else {
                return false;
            };
            let rest = s
                .strip_prefix("http://")
                .or_else(|| s.strip_prefix("https://"))
                .or_else(|| s.strip_prefix("tauri://"))
                .or_else(|| s.strip_prefix("ios://"))
                .unwrap_or("");
            let host = rest.split([':', '/']).next().unwrap_or("");
            matches!(
                host,
                "localhost" | "127.0.0.1" | "[::1]" | "tauri.localhost"
            )
        }))
        .allow_methods([Method::GET, Method::POST, Method::DELETE])
        .allow_headers([header::AUTHORIZATION, header::CONTENT_TYPE])
}

async fn healthz() -> impl IntoResponse {
    Json(json!({"ok": true}))
}

/// The third-party notices, embedded at build time from the repo's
/// THIRD-PARTY_NOTICES.md (the repo root, three dirs above this crate's
/// src). Served as text so the console can show it verbatim; the token
/// guards it like every other console route.
async fn notices() -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "text/plain; charset=utf-8")],
        include_str!("../../../../THIRD-PARTY_NOTICES.md"),
    )
}

async fn auth(State(app): State<Arc<App>>, request: Request, next: Next) -> Response {
    let presented = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .unwrap_or_default();
    if openremote_core::same_secret(presented, &app.token) {
        return next.run(request).await;
    }
    (StatusCode::UNAUTHORIZED, "unauthorized").into_response()
}

async fn capabilities(State(app): State<Arc<App>>) -> Response {
    Json(json!({
        "daemon": env!("CARGO_PKG_VERSION"),
        "harnesses": app.supervisor.harnesses(),
    }))
    .into_response()
}

/// The models a harness advertises, in its own words. An empty list is a
/// real answer - the harness advertises nothing, and the console's model
/// slot stays reserved for it. A probe that failed (the CLI not signed
/// in, slow to boot, or a spawn failure) is not an answer: 503, the same
/// transient signal the cloud routes give, so no caller can hold a
/// failed probe as the harness's empty advertisement.
async fn harness_models(State(app): State<Arc<App>>, AxumPath(id): AxumPath<String>) -> Response {
    match app.supervisor.models(&id).await {
        // A harness that doesn't advertise through us answers [] on the
        // wire: the genuine-empty shape, byte-identical to a harness
        // that advertises no models.
        Ok(models) => Json(models.unwrap_or_default()).into_response(),
        Err(SupervisorError::Driver(err)) => error(
            StatusCode::SERVICE_UNAVAILABLE,
            &format!("could not ask '{id}' for its models: {err}"),
        ),
        Err(err) => supervisor_error(err),
    }
}

/// Capture this harness's model catalog again through its driver's own
/// `--catalog`. The driver writes the catalog file the next probe reads, so
/// the answer is the harness row as it now reads, not the file itself; the
/// console asks `/harnesses/{id}/models` for the list.
///
/// A chat on the harness refuses first: the capture drives the same CLI in
/// the plugin's folder, so a running chat and a capture cannot share it.
async fn refresh_harness_catalog(
    State(app): State<Arc<App>>,
    AxumPath(id): AxumPath<String>,
    Json(body): Json<RequestBody>,
) -> Response {
    let task = {
        let app = Arc::clone(&app);
        async move {
            let row = app
                .supervisor
                .harnesses()
                .into_iter()
                .find(|harness| harness.id == id)
                .ok_or_else(|| (StatusCode::NOT_FOUND, format!("no such harness '{id}'")))?;
            let Some(plugin) = row.plugin.clone() else {
                return Err((
                    StatusCode::BAD_REQUEST,
                    format!(
                        "'{}' ships its model list with OpenRemote; there is nothing to capture",
                        row.name
                    ),
                ));
            };
            let chats = app.supervisor.plugin_chats(&plugin).await;
            if !chats.is_empty() {
                return Err((
                    StatusCode::CONFLICT,
                    format!("a {} chat is running; refresh when it stops", row.name),
                ));
            }
            let spec = app.supervisor.plugin_spec(&id).ok_or_else(|| {
                (
                    StatusCode::BAD_REQUEST,
                    format!("'{}' has no driver to capture a catalog with", row.name),
                )
            })?;
            crate::plugin_driver::capture_catalog(&spec)
                .await
                .map_err(|sentence| (StatusCode::SERVICE_UNAVAILABLE, sentence))?;
            // The new file and its capture time are the row's, so the probe
            // is what the answer reads, never the driver's own line.
            app.supervisor.reprobe_harnesses().await;
            let row = app
                .supervisor
                .harnesses()
                .into_iter()
                .find(|harness| harness.id == id)
                .ok_or_else(|| (StatusCode::NOT_FOUND, format!("no such harness '{id}'")))?;
            Ok(json!({ "harness": row }))
        }
    };
    receipted_plain(&app, body.request_id, task).await
}

// ---- custom model endpoints (machine-local) ----

/// The endpoint records on this machine, metadata only - the secret never
/// leaves the store except into a harness child. `?harness=` narrows to
/// one harness; the console passes it for the Manage-models rows.
async fn list_model_endpoints(
    State(app): State<Arc<App>>,
    axum::extract::Query(query): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> Response {
    let harness = query.get("harness").map(|s| s.as_str());
    if let Some(harness) = harness {
        if openremote_core::model::validate_endpoint_harness(harness).is_err() {
            return error(
                StatusCode::BAD_REQUEST,
                "custom endpoints support claude, codex, and grok",
            );
        }
    }
    let views: Vec<openremote_core::ModelEndpointView> = {
        let store = crate::supervisor::lock_recovering(&app.store);
        // `view` reads the endpoint by reference, so the iterator's items need no clone.
        store
            .model_endpoints(harness)
            .into_iter()
            .map(|e| e.view())
            .collect()
    };
    Json(views).into_response()
}

#[derive(Deserialize)]
struct SaveEndpointBody {
    request_id: String,
    #[serde(default)]
    id: Option<String>,
    harness: String,
    name: String,
    base_url: String,
    #[serde(default)]
    models_path: Option<String>,
    #[serde(default)]
    default_model: Option<String>,
    auth_kind: openremote_core::EndpointAuthKind,
    /// The credential. On an edit without a key the stored one is kept,
    /// so the console never has to read one back to change a name.
    #[serde(default)]
    key: Option<String>,
    #[serde(default = "default_true")]
    enabled: bool,
    #[serde(default)]
    is_default: bool,
}

fn default_true() -> bool {
    true
}

/// Create or replace one endpoint record. The stored secret is never in
/// the answer - only the metadata view.
async fn save_model_endpoint(
    State(app): State<Arc<App>>,
    Json(body): Json<SaveEndpointBody>,
) -> Response {
    if let Some(receipt) = app.supervisor.receipt(&body.request_id) {
        return receipt_response(&receipt);
    }
    let outcome = {
        let mut store = crate::supervisor::lock_recovering(&app.store);
        let id = body
            .id
            .clone()
            .filter(|id| !id.trim().is_empty())
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        let key = match body
            .key
            .clone()
            .map(|k| k.trim().to_string())
            .filter(|k| !k.is_empty())
        {
            Some(secret) => Some(openremote_core::EndpointSecret::new(secret)),
            None => store
                .model_endpoint(&id)
                .ok()
                .and_then(|existing| existing.key.clone()),
        };
        let created_at = store
            .model_endpoint(&id)
            .map(|e| e.created_at)
            .unwrap_or_else(|_| openremote_core::now_ms());
        let entry = openremote_core::ModelEndpoint {
            id,
            harness: body.harness.trim().to_string(),
            name: body.name.clone(),
            base_url: body.base_url.clone(),
            models_path: body.models_path.clone(),
            default_model: body.default_model.clone(),
            auth_kind: body.auth_kind,
            key,
            enabled: body.enabled,
            is_default: body.is_default,
            created_at,
            updated_at: openremote_core::now_ms(),
        };
        store.put_model_endpoint(entry).map(|saved| saved.view())
    };
    match outcome {
        Ok(view) => {
            let receipt = Receipt {
                request_id: body.request_id.clone(),
                session_id: None,
                status: ReceiptStatus::Completed,
                result: Some(serde_json::to_value(&view).unwrap_or(Value::Null)),
                error: None,
                updated_at: openremote_core::now_ms(),
            };
            app.supervisor.record_receipt(receipt.clone());
            (StatusCode::CREATED, Json(view)).into_response()
        }
        Err(err) => {
            let receipt = Receipt {
                request_id: body.request_id.clone(),
                session_id: None,
                status: ReceiptStatus::Failed,
                result: None,
                error: Some(store_error_message(&err)),
                updated_at: openremote_core::now_ms(),
            };
            app.supervisor.record_receipt(receipt.clone());
            store_http_error(err)
        }
    }
}

async fn remove_model_endpoint(
    State(app): State<Arc<App>>,
    AxumPath(id): AxumPath<String>,
    axum::extract::RawQuery(query): axum::extract::RawQuery,
) -> Response {
    let request_id = query
        .as_deref()
        .and_then(|q| {
            q.split('&').find_map(|pair| {
                let (k, v) = pair.split_once('=')?;
                (k == "request_id").then(|| v.to_string())
            })
        })
        .unwrap_or_default();
    if !request_id.is_empty() {
        if let Some(receipt) = app.supervisor.receipt(&request_id) {
            return receipt_response(&receipt);
        }
    }
    match crate::supervisor::lock_recovering(&app.store).remove_model_endpoint(&id) {
        Ok(()) => {
            if !request_id.is_empty() {
                app.supervisor.record_receipt(Receipt {
                    request_id: request_id.clone(),
                    session_id: None,
                    status: ReceiptStatus::Completed,
                    result: Some(Value::Null),
                    error: None,
                    updated_at: openremote_core::now_ms(),
                });
            }
            (StatusCode::OK, Json(json!({"removed": id}))).into_response()
        }
        Err(err) => store_http_error(err),
    }
}

#[derive(Deserialize)]
struct EndpointEnabledBody {
    request_id: String,
    enabled: bool,
}

async fn set_model_endpoint_enabled(
    State(app): State<Arc<App>>,
    AxumPath(id): AxumPath<String>,
    Json(body): Json<EndpointEnabledBody>,
) -> Response {
    if let Some(receipt) = app.supervisor.receipt(&body.request_id) {
        return receipt_response(&receipt);
    }
    match crate::supervisor::lock_recovering(&app.store)
        .set_model_endpoint_enabled(&id, body.enabled)
    {
        Ok(view) => {
            let view = view.view();
            app.supervisor.record_receipt(Receipt {
                request_id: body.request_id.clone(),
                session_id: None,
                status: ReceiptStatus::Completed,
                result: Some(serde_json::to_value(&view).unwrap_or(Value::Null)),
                error: None,
                updated_at: openremote_core::now_ms(),
            });
            Json(view).into_response()
        }
        Err(err) => store_http_error(err),
    }
}

#[derive(Deserialize)]
struct EndpointDefaultBody {
    request_id: String,
    harness: String,
    #[serde(default)]
    id: Option<String>,
}

/// Name a harness's machine default endpoint, or clear it with no id.
async fn set_default_model_endpoint(
    State(app): State<Arc<App>>,
    Json(body): Json<EndpointDefaultBody>,
) -> Response {
    if let Some(receipt) = app.supervisor.receipt(&body.request_id) {
        return receipt_response(&receipt);
    }
    let cleaned = body.id.as_deref().map(str::trim).filter(|s| !s.is_empty());
    match crate::supervisor::lock_recovering(&app.store)
        .set_model_endpoint_default(&body.harness, cleaned)
        .map(|saved| saved.map(|e| e.view()))
    {
        Ok(view) => {
            app.supervisor.record_receipt(Receipt {
                request_id: body.request_id.clone(),
                session_id: None,
                status: ReceiptStatus::Completed,
                result: Some(serde_json::to_value(&view).unwrap_or(Value::Null)),
                error: None,
                updated_at: openremote_core::now_ms(),
            });
            Json(view).into_response()
        }
        Err(err) => store_http_error(err),
    }
}

/// The models a custom endpoint lists today: fetched live from its
/// `/v1/models`-compatible listing with its own key, attached to the
/// endpoint so the console can group custom rows. A gateway that is
/// down, slow, or answers another shape is a 502 with its own words -
/// never an empty advertisement the picker could mistake for "none".
async fn model_endpoint_models(
    State(app): State<Arc<App>>,
    AxumPath(id): AxumPath<String>,
) -> Response {
    let entry = match crate::supervisor::lock_recovering(&app.store)
        .model_endpoint(&id)
        .cloned()
    {
        Ok(entry) => entry,
        Err(_) => return error(StatusCode::NOT_FOUND, "no such model endpoint"),
    };
    match fetch_endpoint_models(&entry).await {
        Ok(models) => Json(models).into_response(),
        Err(message) => error(StatusCode::BAD_GATEWAY, &message),
    }
}

async fn fetch_endpoint_models(
    entry: &openremote_core::ModelEndpoint,
) -> Result<Vec<openremote_core::EndpointModel>, String> {
    let url =
        openremote_core::model::endpoint_models_url(&entry.base_url, entry.models_path.as_deref())
            .map_err(|e| format!("endpoint '{}' has a bad listing address: {e}", entry.name))?;
    let mut headers: Vec<(&str, &str)> = Vec::new();
    // Assigned once, at the point it is decided: the two arms below always set it, so a
    // declaration without a value only moved the reader's eye away from that.
    let secret: Option<String> = match entry.auth_kind {
        openremote_core::EndpointAuthKind::None => None,
        _ => {
            let Some(key) = entry.key.as_ref().filter(|k| !k.is_empty()) else {
                return Err(format!("endpoint '{}' needs a key", entry.name));
            };
            Some(key.expose_secret().to_string())
        }
    };
    let owned;
    match (&entry.auth_kind, secret.as_deref()) {
        (openremote_core::EndpointAuthKind::Bearer, Some(key)) => {
            owned = format!("Bearer {key}");
            headers.push(("authorization", owned.as_str()));
        }
        (openremote_core::EndpointAuthKind::ApiKey, Some(key)) => {
            owned = key.to_string();
            headers.push(("x-api-key", owned.as_str()));
        }
        _ => {}
    }
    let response = openremote_cloud::https::Http::new()
        .request("GET", &url, &headers, None)
        .await
        .map_err(|e| format!("could not reach endpoint '{}': {e}", entry.name))?;
    if !(200..300).contains(&response.status) {
        return Err(format!(
            "endpoint '{}' answered {} - check its base URL and key",
            entry.name, response.status
        ));
    }
    parse_models_listing(&response.body, entry)
}

/// An OpenAI `/v1/models`-compatible listing: `{data: [{id}]}`. Also
/// tolerated: a bare array, `{models: [...]}`, and entries naming the id
/// `model` or `name` instead of `id`.
fn parse_models_listing(
    body: &[u8],
    entry: &openremote_core::ModelEndpoint,
) -> Result<Vec<openremote_core::EndpointModel>, String> {
    let value: Value = serde_json::from_slice(body)
        .map_err(|_| format!("endpoint '{}' answered no JSON listing", entry.name))?;
    let items: &Vec<Value> = if let Some(data) = value.get("data").and_then(Value::as_array) {
        data
    } else if let Some(models) = value.get("models").and_then(Value::as_array) {
        models
    } else if let Some(array) = value.as_array() {
        array
    } else {
        return Err(format!(
            "endpoint '{}' answered another shape - expected an OpenAI /v1/models-compatible listing",
            entry.name
        ));
    };
    let mut out = Vec::new();
    for item in items {
        let id = item
            .get("id")
            .or_else(|| item.get("model"))
            .or_else(|| item.get("name"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty());
        let Some(id) = id else { continue };
        if openremote_core::model::validate_model_id(id).is_err() {
            continue;
        }
        out.push(openremote_core::EndpointModel {
            endpoint_id: entry.id.clone(),
            endpoint_name: entry.name.clone(),
            model: id.to_string(),
            display_name: item
                .get("display_name")
                .or_else(|| item.get("displayName"))
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string),
        });
    }
    Ok(out)
}

fn store_error_message(err: &openremote_core::store::StoreError) -> String {
    err.to_string()
}

fn store_http_error(err: openremote_core::store::StoreError) -> Response {
    match &err {
        openremote_core::store::StoreError::NotFound(_) => {
            error(StatusCode::NOT_FOUND, &err.to_string())
        }
        openremote_core::store::StoreError::Conflict(_) => {
            error(StatusCode::CONFLICT, &err.to_string())
        }
        _ => error(StatusCode::INTERNAL_SERVER_ERROR, &err.to_string()),
    }
}

#[derive(Deserialize)]
struct SessionEndpointBody {
    request_id: String,
    #[serde(default)]
    endpoint_id: Option<String>,
}

/// Point a stopped chat at another custom endpoint (or back at the
/// harness's own provider with none). Live chats refuse with 409.
async fn post_session_endpoint(
    State(app): State<Arc<App>>,
    AxumPath(id): AxumPath<String>,
    Json(body): Json<SessionEndpointBody>,
) -> Response {
    if let Some(receipt) = app.supervisor.receipt(&body.request_id) {
        return receipt_response(&receipt);
    }
    let Some(session_id) = SessionId::parse(&id) else {
        return error(StatusCode::BAD_REQUEST, "bad session id");
    };
    match app
        .supervisor
        .set_session_endpoint(&session_id, body.endpoint_id.as_deref())
        .await
    {
        Ok(session) => {
            let receipt = Receipt {
                request_id: body.request_id,
                session_id: Some(session_id),
                status: ReceiptStatus::Completed,
                result: Some(serde_json::to_value(&session).unwrap_or(Value::Null)),
                error: None,
                updated_at: openremote_core::now_ms(),
            };
            app.supervisor.record_receipt(receipt.clone());
            (StatusCode::OK, Json(session)).into_response()
        }
        Err(err) => {
            let receipt = Receipt {
                request_id: body.request_id,
                session_id: Some(session_id),
                status: ReceiptStatus::Failed,
                result: None,
                error: Some(err.to_string()),
                updated_at: openremote_core::now_ms(),
            };
            app.supervisor.record_receipt(receipt.clone());
            supervisor_error(err)
        }
    }
}

#[derive(Deserialize)]
struct WorkspaceBody {
    request_id: String,
    /// One of this machine's projects; null (or absent) moves the chat out of
    /// every project, into the home folder.
    #[serde(default)]
    project_id: Option<String>,
}

/// Move a chat to another project: from its next message its agent works in
/// that project's folder. A chat with a turn in flight is refused with the
/// words the console shows; a chat whose harness is up and idle restarts in
/// the new folder.
async fn post_workspace(
    State(app): State<Arc<App>>,
    AxumPath(id): AxumPath<String>,
    Json(body): Json<WorkspaceBody>,
) -> Response {
    let project = match body.project_id.as_deref().filter(|p| !p.is_empty()) {
        None => None,
        Some(raw) => match openremote_core::ProjectId::parse(raw) {
            Some(project) => Some(project),
            None => return error(StatusCode::BAD_REQUEST, "bad project id"),
        },
    };
    let sup = Arc::clone(&app.supervisor);
    receipted(&app, body.request_id, &id, |sid| async move {
        let session = sup.move_chat(&sid, project.as_ref()).await?;
        Ok(serde_json::to_value(&session).unwrap_or(Value::Null))
    })
    .await
}

// ---- harness sign-in ----

#[derive(Deserialize)]
struct SignInBody {
    request_id: String,
}

/// Start the harness's own login command, relayed. The CLI's words arrive
/// through the view; a human's answers ride `/signin/input`. The receipt
/// contract dedups the start itself - the relay continues after it.
async fn start_sign_in(
    State(app): State<Arc<App>>,
    AxumPath(id): AxumPath<String>,
    Json(body): Json<SignInBody>,
) -> Response {
    if let Some(receipt) = app.supervisor.receipt(&body.request_id) {
        return receipt_response(&receipt);
    }
    match app.supervisor.start_sign_in(&id).await {
        Ok(view) => {
            let receipt = Receipt {
                request_id: body.request_id,
                session_id: None,
                status: ReceiptStatus::Completed,
                result: Some(serde_json::to_value(&view).unwrap_or(Value::Null)),
                error: None,
                updated_at: openremote_core::now_ms(),
            };
            app.supervisor.record_receipt(receipt.clone());
            receipt_response(&receipt)
        }
        Err(err) => {
            let receipt = Receipt {
                request_id: body.request_id,
                session_id: None,
                status: ReceiptStatus::Failed,
                result: None,
                error: Some(err.to_string()),
                updated_at: openremote_core::now_ms(),
            };
            app.supervisor.record_receipt(receipt.clone());
            supervisor_error(err)
        }
    }
}

/// The current beat of a harness's sign-in relay - the console polls it.
async fn sign_in_view(State(app): State<Arc<App>>, AxumPath(id): AxumPath<String>) -> Response {
    match app.supervisor.sign_in_view(&id) {
        Some(view) => Json(view).into_response(),
        None => error(StatusCode::NOT_FOUND, "no sign-in has been started"),
    }
}

#[derive(Deserialize)]
struct SignInInputBody {
    request_id: String,
    text: String,
}

/// Feed one line to the harness's own login prompt (claude's pasted
/// code). The CLI's words in answer arrive through the view.
async fn sign_in_input(
    State(app): State<Arc<App>>,
    AxumPath(id): AxumPath<String>,
    Json(body): Json<SignInInputBody>,
) -> Response {
    if let Some(receipt) = app.supervisor.receipt(&body.request_id) {
        return receipt_response(&receipt);
    }
    match app.supervisor.feed_sign_in(&id, &body.text) {
        Ok(()) => {
            let receipt = Receipt {
                request_id: body.request_id,
                session_id: None,
                status: ReceiptStatus::Completed,
                result: Some(json!({"delivered": true})),
                error: None,
                updated_at: openremote_core::now_ms(),
            };
            app.supervisor.record_receipt(receipt.clone());
            receipt_response(&receipt)
        }
        Err(err) => supervisor_error(err),
    }
}

/// Stop a running sign-in relay - the abandoned-browser-flow answer. The
/// settled view (`done: stopped`) arrives through the poll.
async fn stop_sign_in(
    State(app): State<Arc<App>>,
    AxumPath(id): AxumPath<String>,
    Json(body): Json<SignInBody>,
) -> Response {
    if let Some(receipt) = app.supervisor.receipt(&body.request_id) {
        return receipt_response(&receipt);
    }
    app.supervisor.stop_sign_in(&id);
    let receipt = Receipt {
        request_id: body.request_id,
        session_id: None,
        status: ReceiptStatus::Completed,
        result: Some(json!({"stopped": true})),
        error: None,
        updated_at: openremote_core::now_ms(),
    };
    app.supervisor.record_receipt(receipt.clone());
    receipt_response(&receipt)
}

// ---- machines ----

/// A peer sees only the chats this machine runs for the account; private
/// chats never leave the computer.
fn for_peer(app: &App, view: &mut Value) {
    let me = crate::supervisor::lock_recovering(&app.store).this_device();
    if let Some(sessions) = view.get_mut("sessions").and_then(Value::as_array_mut) {
        sessions.retain(|s| {
            me.is_some_and(|me| s["executor"].as_str() == Some(me.to_string().as_str()))
        });
    }
}

async fn list_machines(
    State(app): State<Arc<App>>,
    peer: Option<axum::Extension<crate::host::PeerOrigin>>,
) -> Response {
    let mut views = serde_json::to_value(app.supervisor.machines()).unwrap_or(Value::Null);
    if peer.is_some() {
        for view in views.as_array_mut().into_iter().flatten() {
            for_peer(&app, view);
        }
    }
    Json(views).into_response()
}

async fn get_machine(
    State(app): State<Arc<App>>,
    AxumPath(id): AxumPath<String>,
    peer: Option<axum::Extension<crate::host::PeerOrigin>>,
) -> Response {
    match MachineId::parse(&id) {
        Some(id) => match app.supervisor.machine(&id) {
            Ok(view) => {
                let mut view = serde_json::to_value(view).unwrap_or(Value::Null);
                if peer.is_some() {
                    for_peer(&app, &mut view);
                }
                Json(view).into_response()
            }
            Err(err) => supervisor_error(err),
        },
        None => error(StatusCode::BAD_REQUEST, "bad machine id"),
    }
}

#[derive(Deserialize)]
struct FsQuery {
    path: Option<String>,
}

async fn fs_dirs(axum::extract::Query(q): axum::extract::Query<FsQuery>) -> Response {
    match tokio::task::spawn_blocking(move || crate::fsdirs::list(q.path.as_deref())).await {
        Ok(Ok(listing)) => Json(listing).into_response(),
        Ok(Err(e)) => error(StatusCode::BAD_REQUEST, &e),
        Err(_) => error(StatusCode::INTERNAL_SERVER_ERROR, "folder listing failed"),
    }
}

#[derive(Deserialize)]
struct CreateMachineBody {
    request_id: String,
    name: String,
    /// `windows` / `macos` / `linux` - picks the install command shown.
    platform: String,
}

async fn create_machine(
    State(app): State<Arc<App>>,
    Json(body): Json<CreateMachineBody>,
) -> Response {
    if let Some(receipt) = app.supervisor.receipt(&body.request_id) {
        return receipt_response(&receipt);
    }
    match app.supervisor.create_machine(&body.name, &body.platform) {
        Ok(machine) => {
            let receipt = Receipt {
                request_id: body.request_id,
                session_id: None,
                status: ReceiptStatus::Completed,
                result: Some(serde_json::to_value(&machine).unwrap_or(Value::Null)),
                error: None,
                updated_at: openremote_core::now_ms(),
            };
            app.supervisor.record_receipt(receipt.clone());
            (StatusCode::CREATED, Json(machine)).into_response()
        }
        Err(err) => {
            let receipt = Receipt {
                request_id: body.request_id,
                session_id: None,
                status: ReceiptStatus::Failed,
                result: None,
                error: Some(err.to_string()),
                updated_at: openremote_core::now_ms(),
            };
            app.supervisor.record_receipt(receipt.clone());
            supervisor_error(err)
        }
    }
}

// DELETE carries no body - the request_id rides as a query param.
#[derive(Deserialize)]
struct MachineActionQuery {
    request_id: String,
}

async fn remove_machine(
    State(app): State<Arc<App>>,
    AxumPath(id): AxumPath<String>,
    axum::extract::Query(query): axum::extract::Query<MachineActionQuery>,
) -> Response {
    if let Some(receipt) = app.supervisor.receipt(&query.request_id) {
        return receipt_response(&receipt);
    }
    let Some(machine_id) = MachineId::parse(&id) else {
        return error(StatusCode::BAD_REQUEST, "bad machine id");
    };
    match app.supervisor.remove_machine(&machine_id) {
        Ok(()) => {
            let receipt = Receipt {
                request_id: query.request_id,
                session_id: None,
                status: ReceiptStatus::Completed,
                result: Some(json!({"removed": true})),
                error: None,
                updated_at: openremote_core::now_ms(),
            };
            app.supervisor.record_receipt(receipt.clone());
            receipt_response(&receipt)
        }
        Err(err) => {
            let receipt = Receipt {
                request_id: query.request_id,
                session_id: None,
                status: ReceiptStatus::Failed,
                result: None,
                error: Some(err.to_string()),
                updated_at: openremote_core::now_ms(),
            };
            app.supervisor.record_receipt(receipt.clone());
            supervisor_error(err)
        }
    }
}

#[derive(Deserialize)]
struct InstallHarnessBody {
    request_id: String,
    harness: String,
    /// The human's go-ahead to set up a runtime the harness needs (pi's
    /// Node.js) - never assumed.
    #[serde(default)]
    with_runtime: bool,
}

// ---- plugins (Plugins): this machine's own - never a peer's ----

/// The receipt flow for an action that belongs to no chat: a replayed
/// request answers from its receipt; a new one is recorded, run, settled.
async fn receipted_plain<Fut>(app: &Arc<App>, request_id: String, run: Fut) -> Response
where
    Fut: std::future::Future<Output = Result<Value, PluginFailure>>,
{
    if let Some(receipt) = app.supervisor.receipt(&request_id) {
        return receipt_response(&receipt);
    }
    app.supervisor.record_receipt(Receipt {
        request_id: request_id.clone(),
        session_id: None,
        status: ReceiptStatus::Accepted,
        result: None,
        error: None,
        updated_at: openremote_core::now_ms(),
    });
    match run.await {
        Ok(result) => {
            let receipt = Receipt {
                request_id,
                session_id: None,
                status: ReceiptStatus::Completed,
                result: Some(result),
                error: None,
                updated_at: openremote_core::now_ms(),
            };
            app.supervisor.record_receipt(receipt.clone());
            receipt_response(&receipt)
        }
        Err(failure) => {
            app.supervisor.record_receipt(Receipt {
                request_id,
                session_id: None,
                status: ReceiptStatus::Failed,
                result: None,
                error: Some(failure.1.clone()),
                updated_at: openremote_core::now_ms(),
            });
            error(failure.0, &failure.1)
        }
    }
}

/// A plugin action's failure: the status it answers with, and its words.
type PluginFailure = (StatusCode, String);

fn plugin_failure(err: crate::plugins::PluginError) -> PluginFailure {
    use crate::plugins::PluginError;
    let status = match &err {
        PluginError::Invalid(_) | PluginError::Source(_) => StatusCode::BAD_REQUEST,
        PluginError::NotFound(_) => StatusCode::NOT_FOUND,
        PluginError::Io(_) => StatusCode::INTERNAL_SERVER_ERROR,
    };
    (status, err.to_string())
}

fn store_failure(err: openremote_core::store::StoreError) -> PluginFailure {
    match err {
        openremote_core::store::StoreError::NotFound(what) => {
            (StatusCode::NOT_FOUND, format!("{what} is not installed"))
        }
        other => (StatusCode::INTERNAL_SERVER_ERROR, other.to_string()),
    }
}

/// The harness ids OpenRemote itself carries: the six built-ins, never one an
/// installed plugin provides.
///
/// This list is what a plugin may not take as its name or its harness id. It
/// has to be the built-ins alone: an installed plugin's own harness is in the
/// registry too, and reading it back as "taken by OpenRemote" refused the
/// plugin its own name - so a harness plugin could be installed once and never
/// staged again for an update, and Discover showed it as uninstallable.
fn builtin_ids(app: &App) -> Vec<String> {
    app.supervisor
        .harnesses()
        .into_iter()
        .filter(|h| h.plugin.is_none())
        .map(|h| h.id)
        .collect()
}

/// The six built-in harness plugins and what the user installed, each
/// installed one with its id (`name@marketplace`).
async fn list_plugins(State(app): State<Arc<App>>) -> Response {
    // The built-in section is the six the daemon carries. A plugin-provided
    // harness is listed under Installed, with the plugin that provides it,
    // so the same harness never appears twice on the page.
    let builtin: Vec<Value> = app
        .supervisor
        .harnesses()
        .into_iter()
        .filter(|h| h.plugin.is_none())
        .map(|h| {
            json!({
                "id": h.id,
                "name": h.name,
                "kind": "harness",
                "available": h.available,
                "enabled": h.enabled,
                "version": h.version,
            })
        })
        .collect();
    let (installed, found, global_pending) = {
        let store = crate::supervisor::lock_recovering(&app.store);
        let installed: Vec<openremote_core::InstalledPlugin> =
            store.installed_plugins().into_iter().cloned().collect();
        (
            installed,
            crate::plugin_runtime::scope::found(&store),
            crate::plugin_runtime::global::pending(&store, &app.data_dir),
        )
    };
    let installed: Vec<Value> = installed.iter().map(|p| installed_json(&app, p)).collect();
    Json(json!({
        "builtin": builtin,
        "installed": installed,
        // Plugins a project's repo carries that wait for this machine's
        // review: new ones, and ones whose files changed.
        "found": found,
        // The account's Global plugins not on this device yet (or a newer
        // copy of one), each waiting for this device's own review.
        "global_pending": global_pending,
    }))
    .into_response()
}

/// An installed plugin as the console reads it: its record, its id, and
/// for a process plugin how its program stands.
fn installed_json(app: &App, plugin: &openremote_core::InstalledPlugin) -> Value {
    let mut value = serde_json::to_value(plugin).unwrap_or(Value::Null);
    if let Some(fields) = value.as_object_mut() {
        fields.insert("id".into(), Value::String(plugin.id()));
        // A JavaScript plugin's page, which the console builds in a frame
        // for its composer buttons (its views name their own).
        if let Some(page) = app.plugin_runtime.page(plugin) {
            fields.insert("entry".into(), Value::String(page));
        }
        if plugin.runtime == openremote_core::PluginRuntime::Process {
            fields.insert(
                "process".into(),
                serde_json::to_value(app.plugin_runtime.processes.status(&plugin.id()))
                    .unwrap_or(Value::Null),
            );
        }
    }
    value
}

#[derive(Deserialize)]
struct StageBody {
    request_id: String,
    source: crate::plugins::SourceRequest,
}

/// What a stage request names, resolved: a fetch to stage, or - for a
/// `command` entry whose exact command the user has not approved yet - the
/// command to show them first.
enum Resolved {
    Fetch(Box<crate::plugins::StageRequest>),
    NeedsCommand(Value),
}

/// A direct source stages as it is; a marketplace entry through its
/// catalog, as the entry names it.
fn resolve_stage(
    app: &App,
    source: crate::plugins::SourceRequest,
) -> Result<Resolved, PluginFailure> {
    use crate::marketplace::{EntrySource, entry_source, read_catalog};
    if let crate::plugins::SourceRequest::Project { project, name } = &source {
        return crate::plugin_runtime::install::project_source(app, project, name)
            .map(|request| Resolved::Fetch(Box::new(request)));
    }
    if let Some(direct) = source.direct() {
        return Ok(Resolved::Fetch(Box::new(direct)));
    }
    let crate::plugins::SourceRequest::Marketplace {
        marketplace,
        plugin,
        approve_command,
    } = source
    else {
        unreachable!("direct() answers every other source, and an account's copy is fetched first");
    };
    let record = {
        let store = crate::supervisor::lock_recovering(&app.store);
        store.marketplace(&marketplace).cloned()
    }
    .map_err(|_| {
        (
            StatusCode::NOT_FOUND,
            format!("no marketplace named {marketplace}"),
        )
    })?;
    let catalog = read_catalog(&app.marketplaces.catalog_dir(&record)).map_err(|_| {
        (
            StatusCode::CONFLICT,
            format!("{marketplace} has not been fetched yet - refresh it first"),
        )
    })?;
    let entry = catalog.entry(&plugin).cloned().ok_or_else(|| {
        (
            StatusCode::NOT_FOUND,
            format!("{marketplace} lists no plugin named {plugin}"),
        )
    })?;
    let cannot = |why: String| {
        (
            StatusCode::BAD_REQUEST,
            format!("{plugin} can't be installed: {why}"),
        )
    };
    let source = entry_source(&entry, catalog.plugin_root.as_deref()).map_err(cannot)?;
    if let EntrySource::Command {
        command,
        timeout_secs,
    } = &source
    {
        if approve_command.as_deref() != Some(command.as_str()) {
            return Ok(Resolved::NeedsCommand(json!({
                "id": format!("{plugin}@{marketplace}"),
                "command": command,
                "timeout_secs": timeout_secs,
            })));
        }
    }
    let fetch = source
        .fetch(app.marketplaces.files(&record).as_deref())
        .map_err(cannot)?;
    Ok(Resolved::Fetch(Box::new(crate::plugins::StageRequest {
        fetch,
        recorded: openremote_core::PluginSource::Marketplace {
            marketplace: marketplace.clone(),
            plugin: plugin.clone(),
            from: source.describe(),
        },
        marketplace,
        entry_name: Some(plugin),
        entry: Some(entry),
    })))
}

/// Fetch a plugin and read what it is - nothing is installed until the
/// user approves what the answer shows (name, runtime, powers).
async fn stage_plugin(State(app): State<Arc<App>>, Json(body): Json<StageBody>) -> Response {
    let ids = builtin_ids(&app);
    // A repo's plugin read for review belongs to its project; an account's
    // Global plugin is reviewed here as any other is.
    let for_project = match &body.source {
        crate::plugins::SourceRequest::Project { project, .. } => {
            openremote_core::ProjectId::parse(project)
        }
        _ => None,
    };
    let for_account = matches!(body.source, crate::plugins::SourceRequest::Account { .. });
    let task = {
        let app = Arc::clone(&app);
        async move {
            let request = match body.source {
                crate::plugins::SourceRequest::Account { id } => {
                    crate::plugin_runtime::global::fetch_for_review(&app, &id).await?
                }
                source => match resolve_stage(&app, source)? {
                    Resolved::NeedsCommand(command) => {
                        return Ok(json!({"needs_command": command}));
                    }
                    Resolved::Fetch(request) => *request,
                },
            };
            // An account's copy was unpacked for this review only.
            let unpacked = match (&request.fetch, for_account) {
                (crate::plugins::Fetch::Folder(folder), true) => Some(folder.clone()),
                _ => None,
            };
            let command = match &request.fetch {
                crate::plugins::Fetch::Command(approved) => Some(approved.clone()),
                _ => None,
            };
            let marketplace = request.marketplace.clone();
            let staged = app.plugins.stage(request, ids).await;
            if let Some(folder) = unpacked {
                let _ = std::fs::remove_dir_all(folder);
            }
            let (stage_id, manifest, name) = staged.map_err(plugin_failure)?;
            let id = match for_project {
                Some(project) => openremote_core::plugin_id(
                    &name,
                    &marketplace,
                    openremote_core::PluginScope::Project,
                    Some(project),
                ),
                None => format!("{name}@{marketplace}"),
            };
            let installed = {
                let store = crate::supervisor::lock_recovering(&app.store);
                store.installed_plugin(&id).ok().cloned()
            };
            let scopes =
                crate::plugin_runtime::install::allowed_scopes(&manifest, command.is_some());
            let new_powers: Vec<openremote_core::PluginPower> = match &installed {
                Some(have) => manifest
                    .powers
                    .iter()
                    .filter(|p| !have.powers.contains(p))
                    .copied()
                    .collect(),
                None => Vec::new(),
            };
            Ok(json!({
                // Who it may be for (global, machine, project_local,
                // project), and where the copy it would replace is.
                "scopes": scopes,
                "installed_scope": installed.as_ref().map(|p| p.scope),
                "installed_project": installed.as_ref().and_then(|p| p.project),
                // Read from a project's repo: it installs for that project.
                "for_project": for_project,
                // The account's Global plugin, offered for every device.
                "for_account": for_account,
                "stage_id": stage_id,
                "id": id,
                "name": name,
                "marketplace": marketplace,
                "manifest": manifest,
                // An install of an id already here replaces it - the
                // approval screen says so, and calls out any power the
                // new version adds.
                "replaces": installed.as_ref().map(|p| p.version.clone().unwrap_or_default()),
                "new_powers": new_powers,
                "command": command,
            }))
        }
    };
    receipted_plain(&app, body.request_id, task).await
}

#[derive(Deserialize)]
struct InstallBody {
    request_id: String,
    /// Who it is for: `global`, `machine` (the default), `project_local` or
    /// `project`. Who it is for never adds to what it may do: the powers
    /// are the reviewed manifest's, and nothing here can name one.
    #[serde(default)]
    scope: Option<openremote_core::PluginScope>,
    /// The project, for the two project scopes.
    #[serde(default)]
    project: Option<String>,
}

/// The user approved: place the staged plugin where its scope keeps it and
/// record it. An update keeps the plugin on or off as it was.
async fn install_staged(
    State(app): State<Arc<App>>,
    AxumPath(id): AxumPath<String>,
    Json(body): Json<InstallBody>,
) -> Response {
    let task = {
        let app = Arc::clone(&app);
        async move {
            let project = match body.project.as_deref() {
                Some(raw) => Some(
                    openremote_core::ProjectId::parse(raw)
                        .ok_or((StatusCode::BAD_REQUEST, "bad project id".to_string()))?,
                ),
                None => None,
            };
            let staged = app.plugins.take_staged(&id).ok_or_else(|| {
                (
                    StatusCode::NOT_FOUND,
                    "that install is no longer waiting - stage it again".to_string(),
                )
            })?;
            let scope = body.scope.unwrap_or(openremote_core::PluginScope::Machine);
            let plugin =
                crate::plugin_runtime::install::install(&app, staged, scope, project).await?;
            Ok(installed_json(&app, &plugin))
        }
    };
    receipted_plain(&app, body.request_id, task).await
}

async fn cancel_staged(
    State(app): State<Arc<App>>,
    AxumPath(id): AxumPath<String>,
    Json(body): Json<RequestBody>,
) -> Response {
    let cancelled = app.plugins.cancel(&id);
    receipted_plain(&app, body.request_id, async move {
        Ok(json!({"cancelled": cancelled}))
    })
    .await
}

#[derive(Deserialize)]
struct PluginEnabledBody {
    request_id: String,
    enabled: bool,
}

/// A built-in harness's id has no `@`; an installed plugin's always does.
fn is_builtin(app: &App, id: &str) -> bool {
    !id.contains('@') && builtin_ids(app).iter().any(|b| b == id)
}

/// On or off. A built-in harness plugin that is off hides its harness from
/// every picker; its chats keep working.
async fn set_plugin_enabled(
    State(app): State<Arc<App>>,
    AxumPath(id): AxumPath<String>,
    Json(body): Json<PluginEnabledBody>,
) -> Response {
    let builtin = is_builtin(&app, &id);
    let task = {
        let app = Arc::clone(&app);
        async move {
            if builtin {
                crate::supervisor::lock_recovering(&app.store)
                    .set_builtin_enabled(&id, body.enabled)
                    .map_err(store_failure)?;
                Ok(json!({"id": id, "enabled": body.enabled}))
            } else {
                let plugin = crate::supervisor::lock_recovering(&app.store)
                    .set_installed_plugin_enabled(&id, body.enabled)
                    .map_err(store_failure)?;
                // Off stops its program; on, the next need starts it.
                app.plugin_runtime.changed(&id).await;
                Ok(installed_json(&app, &plugin))
            }
        }
    };
    let response = receipted_plain(&app, body.request_id, task).await;
    // A plugin's harness follows its plugin's on/off switch without a
    // restart. The store moved first; the probe reads it fresh.
    app.supervisor.reprobe_harnesses().await;
    response
}

/// Uninstall: its folder and its record go. A built-in can only be off.
async fn remove_plugin(
    State(app): State<Arc<App>>,
    AxumPath(id): AxumPath<String>,
    Json(body): Json<RequestBody>,
) -> Response {
    let builtin = is_builtin(&app, &id);
    let task = {
        let app = Arc::clone(&app);
        async move {
            if builtin {
                return Err((
                    StatusCode::CONFLICT,
                    format!("{id} is built in - turn it off instead"),
                ));
            }
            // Its files, record and settings go; a Global plugin goes from
            // every device.
            crate::plugin_runtime::install::remove(&app, &id, true).await?;
            Ok(json!({"removed": id}))
        }
    };
    receipted_plain(&app, body.request_id, task).await
}

// ---- marketplaces (Plugins > Discover, Marketplaces): this machine's own ----

async fn list_marketplaces(State(app): State<Arc<App>>) -> Response {
    let records: Vec<openremote_core::MarketplaceRecord> = {
        let store = crate::supervisor::lock_recovering(&app.store);
        store.marketplaces().into_iter().cloned().collect()
    };
    Json(records).into_response()
}

#[derive(Deserialize)]
struct AddMarketplaceBody {
    request_id: String,
    /// As typed: `owner/repo`, a git URL, a folder, or a marketplace.json
    /// link.
    source: String,
}

/// Add a marketplace: fetch its catalog, check the name it gives itself,
/// keep both.
async fn add_marketplace(
    State(app): State<Arc<App>>,
    Json(body): Json<AddMarketplaceBody>,
) -> Response {
    let task = {
        let app = Arc::clone(&app);
        async move {
            let source = crate::marketplace::parse_source(&body.source).map_err(plugin_failure)?;
            let fetched = app
                .marketplaces
                .fetch(&source)
                .await
                .map_err(plugin_failure)?;
            let name = fetched.catalog.name.clone();
            crate::marketplace::check_name(&name, &source).map_err(plugin_failure)?;
            {
                let store = crate::supervisor::lock_recovering(&app.store);
                if store.marketplace(&name).is_ok() {
                    return Err((
                        StatusCode::CONFLICT,
                        format!("a marketplace named {name} is already added"),
                    ));
                }
            }
            let catalog = app
                .marketplaces
                .settle(&name, fetched)
                .map_err(plugin_failure)?;
            let now = openremote_core::now_ms();
            let record = openremote_core::MarketplaceRecord {
                name,
                source,
                enabled: true,
                removable: true,
                owner: catalog.owner,
                description: catalog.description,
                plugin_count: catalog.entries.len(),
                added_at: now,
                refreshed_at: Some(now),
                error: None,
                heads: Default::default(),
            };
            let mut store = crate::supervisor::lock_recovering(&app.store);
            store
                .put_marketplace(record.clone())
                .map_err(store_failure)?;
            Ok(serde_json::to_value(&record).unwrap_or(Value::Null))
        }
    };
    receipted_plain(&app, body.request_id, task).await
}

/// Fetch a marketplace's catalog again. A failed fetch keeps the cached
/// copy serving Discover and says why on the record.
async fn refresh_marketplace(
    State(app): State<Arc<App>>,
    AxumPath(name): AxumPath<String>,
    Json(body): Json<RequestBody>,
) -> Response {
    let task = {
        let app = Arc::clone(&app);
        async move {
            let (mut record, installed) = {
                let store = crate::supervisor::lock_recovering(&app.store);
                let record = store.marketplace(&name).cloned().map_err(store_failure)?;
                let installed: Vec<openremote_core::InstalledPlugin> = store
                    .installed_plugins()
                    .into_iter()
                    .filter(|p| p.marketplace == name)
                    .cloned()
                    .collect();
                (record, installed)
            };
            record.refreshed_at = Some(openremote_core::now_ms());
            match app.marketplaces.fetch(&record.source).await {
                Ok(fetched) if fetched.catalog.name != record.name => {
                    record.error = Some(format!(
                        "its catalog now calls itself {}",
                        fetched.catalog.name
                    ));
                }
                Ok(fetched) => {
                    let catalog = app
                        .marketplaces
                        .settle(&name, fetched)
                        .map_err(plugin_failure)?;
                    record.heads = crate::marketplace::followed_heads(&catalog, &installed).await;
                    record.owner = catalog.owner;
                    record.description = catalog.description;
                    record.plugin_count = catalog.entries.len();
                    record.error = None;
                }
                Err(e) => record.error = Some(e.to_string()),
            }
            let mut store = crate::supervisor::lock_recovering(&app.store);
            // Removed while it fetched: stays removed.
            if store.marketplace(&name).is_ok() {
                store
                    .put_marketplace(record.clone())
                    .map_err(store_failure)?;
            }
            Ok(serde_json::to_value(&record).unwrap_or(Value::Null))
        }
    };
    receipted_plain(&app, body.request_id, task).await
}

/// Off hides a marketplace from Discover; what was installed from it stays.
async fn set_marketplace_enabled(
    State(app): State<Arc<App>>,
    AxumPath(name): AxumPath<String>,
    Json(body): Json<PluginEnabledBody>,
) -> Response {
    let task = {
        let app = Arc::clone(&app);
        async move {
            let mut store = crate::supervisor::lock_recovering(&app.store);
            let mut record = store.marketplace(&name).cloned().map_err(store_failure)?;
            record.enabled = body.enabled;
            store
                .put_marketplace(record.clone())
                .map_err(store_failure)?;
            Ok(serde_json::to_value(&record).unwrap_or(Value::Null))
        }
    };
    receipted_plain(&app, body.request_id, task).await
}

/// Remove a marketplace. The plugins installed from it stay installed and
/// keep working; they just get no more updates. OpenRemote's own can only
/// be turned off.
async fn remove_marketplace(
    State(app): State<Arc<App>>,
    AxumPath(name): AxumPath<String>,
    Json(body): Json<RequestBody>,
) -> Response {
    let task = {
        let app = Arc::clone(&app);
        async move {
            let record = {
                let store = crate::supervisor::lock_recovering(&app.store);
                store.marketplace(&name).cloned().map_err(store_failure)?
            };
            if !record.removable {
                return Err((
                    StatusCode::CONFLICT,
                    format!("{name} is OpenRemote's own marketplace - turn it off instead"),
                ));
            }
            app.marketplaces.forget(&record);
            let mut store = crate::supervisor::lock_recovering(&app.store);
            store.remove_marketplace(&name).map_err(store_failure)?;
            Ok(json!({"removed": name}))
        }
    };
    receipted_plain(&app, body.request_id, task).await
}

/// Every enabled marketplace's entries, as Discover shows them, from the
/// cached catalogs - no network.
async fn marketplace_catalog(State(app): State<Arc<App>>) -> Response {
    let (records, installed) = {
        let store = crate::supervisor::lock_recovering(&app.store);
        let records: Vec<openremote_core::MarketplaceRecord> = store
            .marketplaces()
            .into_iter()
            .filter(|r| r.enabled)
            .cloned()
            .collect();
        let installed: Vec<openremote_core::InstalledPlugin> =
            store.installed_plugins().into_iter().cloned().collect();
        (records, installed)
    };
    let ids = builtin_ids(&app);
    let reader = Arc::clone(&app);
    let entries = tokio::task::spawn_blocking(move || {
        let mut entries = Vec::new();
        for record in &records {
            let Ok(catalog) =
                crate::marketplace::read_catalog(&reader.marketplaces.catalog_dir(record))
            else {
                continue;
            };
            entries.extend(crate::marketplace::entry_views(
                record,
                &catalog,
                reader.marketplaces.files(record).as_deref(),
                &installed,
                &ids,
            ));
        }
        entries
    })
    .await
    .unwrap_or_default();
    Json(json!({"entries": entries})).into_response()
}

// ---- the composer's menus (/ and @) ----

#[derive(Deserialize)]
struct ComposerQuery {
    #[serde(default)]
    harness: Option<String>,
    #[serde(default)]
    workspace: Option<String>,
    /// A chat: its harness and workspace stand in for the two above.
    #[serde(default)]
    session: Option<String>,
    #[serde(default)]
    q: Option<String>,
}

/// The harness and workspace a composer query is about: the chat's own,
/// or the ones it names (a chat not started yet - the home screen).
fn composer_target(
    app: &App,
    query: &ComposerQuery,
) -> Result<(Option<String>, Option<std::path::PathBuf>), Box<Response>> {
    if let Some(raw) = &query.session {
        let Some(id) = SessionId::parse(raw) else {
            return Err(Box::new(error(StatusCode::BAD_REQUEST, "bad session id")));
        };
        let session = app
            .supervisor
            .session(&id)
            .map_err(|e| Box::new(supervisor_error(e)))?;
        return Ok((Some(session.harness), session.workspace));
    }
    Ok((
        query.harness.clone(),
        query
            .workspace
            .as_deref()
            .filter(|w| !w.is_empty())
            .map(std::path::PathBuf::from),
    ))
}

/// What `/` and `@` offer: the harness's commands and agents, the
/// plugins' (dimmed where the harness can't run them), the MCP servers.
async fn get_commands(
    State(app): State<Arc<App>>,
    axum::extract::Query(query): axum::extract::Query<ComposerQuery>,
) -> Response {
    let (harness, workspace) = match composer_target(&app, &query) {
        Ok(target) => target,
        Err(response) => return *response,
    };
    let Some(harness) = harness else {
        return error(StatusCode::BAD_REQUEST, "name a harness or a chat");
    };
    match app.supervisor.commands(&harness, workspace).await {
        Ok(view) => Json(view).into_response(),
        Err(err) => supervisor_error(err),
    }
}

/// @ file suggestions in a chat's workspace (or a named one), ranked for
/// the query.
async fn get_files(
    State(app): State<Arc<App>>,
    axum::extract::Query(query): axum::extract::Query<ComposerQuery>,
) -> Response {
    let (_, workspace) = match composer_target(&app, &query) {
        Ok(target) => target,
        Err(response) => return *response,
    };
    let root = workspace.unwrap_or_else(|| {
        std::env::var_os("USERPROFILE")
            .or_else(|| std::env::var_os("HOME"))
            .map(std::path::PathBuf::from)
            .unwrap_or_else(std::env::temp_dir)
    });
    match app
        .files
        .search(&root, query.q.as_deref().unwrap_or(""))
        .await
    {
        Ok((entries, truncated)) => {
            Json(json!({"entries": entries, "truncated": truncated})).into_response()
        }
        Err(message) => error(StatusCode::BAD_REQUEST, &message),
    }
}

#[derive(Deserialize)]
struct TitleBody {
    request_id: String,
    title: String,
}

/// Rename a chat (the composer's /rename): the name every device shows.
async fn post_title(
    State(app): State<Arc<App>>,
    AxumPath(id): AxumPath<String>,
    Json(body): Json<TitleBody>,
) -> Response {
    let sup = Arc::clone(&app.supervisor);
    receipted(&app, body.request_id, &id, |sid| async move {
        let session = sup.rename(&sid, &body.title)?;
        Ok(serde_json::to_value(&session).unwrap_or(Value::Null))
    })
    .await
}

/// The read-a-chat tool's call: a chat this one was pointed at, condensed.
/// Local only - the MCP server runs on this machine.
async fn get_mentioned_chat(
    State(app): State<Arc<App>>,
    AxumPath((id, chat)): AxumPath<(String, String)>,
) -> Response {
    let Some(reader) = SessionId::parse(&id) else {
        return error(StatusCode::BAD_REQUEST, "bad session id");
    };
    match app.supervisor.read_mentioned(&reader, &chat) {
        Ok(text) => Json(json!({"text": text})).into_response(),
        Err(err) => supervisor_error(err),
    }
}

// ---- MCP servers ----

async fn list_mcp_servers(State(app): State<Arc<App>>) -> Response {
    Json(app.supervisor.mcp_server_entries()).into_response()
}

#[derive(Deserialize)]
struct CreateMcpServerBody {
    request_id: String,
    machine: String,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    detail: Option<String>,
    #[serde(default)]
    command: Option<String>,
    #[serde(default)]
    needs_key: bool,
}

async fn create_mcp_server(
    State(app): State<Arc<App>>,
    Json(body): Json<CreateMcpServerBody>,
) -> Response {
    if let Some(receipt) = app.supervisor.receipt(&body.request_id) {
        return receipt_response(&receipt);
    }
    let Some(machine_id) = MachineId::parse(&body.machine) else {
        return error(StatusCode::BAD_REQUEST, "bad machine id");
    };
    // An MCP server is written by hand: its name, detail and launch command.
    let (Some(name), Some(detail), Some(command)) = (&body.name, &body.detail, &body.command)
    else {
        return error(
            StatusCode::UNPROCESSABLE_ENTITY,
            "an MCP server needs a name, a detail, and a launch command",
        );
    };
    if name.trim().is_empty() || command.trim().is_empty() {
        return error(
            StatusCode::UNPROCESSABLE_ENTITY,
            "an MCP server needs a name and a launch command",
        );
    }
    match app.supervisor.add_mcp_server(
        &machine_id,
        name.trim(),
        detail.trim(),
        command.trim(),
        body.needs_key,
    ) {
        Ok(plugin) => {
            let receipt = Receipt {
                request_id: body.request_id,
                session_id: None,
                status: ReceiptStatus::Completed,
                result: Some(serde_json::to_value(&plugin).unwrap_or(Value::Null)),
                error: None,
                updated_at: openremote_core::now_ms(),
            };
            app.supervisor.record_receipt(receipt.clone());
            (StatusCode::CREATED, Json(plugin)).into_response()
        }
        Err(err) => {
            let receipt = Receipt {
                request_id: body.request_id,
                session_id: None,
                status: ReceiptStatus::Failed,
                result: None,
                error: Some(err.to_string()),
                updated_at: openremote_core::now_ms(),
            };
            app.supervisor.record_receipt(receipt.clone());
            supervisor_error(err)
        }
    }
}

async fn remove_mcp_server(
    State(app): State<Arc<App>>,
    AxumPath(id): AxumPath<String>,
    axum::extract::Query(query): axum::extract::Query<MachineActionQuery>,
) -> Response {
    if let Some(receipt) = app.supervisor.receipt(&query.request_id) {
        return receipt_response(&receipt);
    }
    match app.supervisor.remove_mcp_server(&id) {
        Ok(()) => {
            let receipt = Receipt {
                request_id: query.request_id,
                session_id: None,
                status: ReceiptStatus::Completed,
                result: Some(json!({"removed": true})),
                error: None,
                updated_at: openremote_core::now_ms(),
            };
            app.supervisor.record_receipt(receipt.clone());
            receipt_response(&receipt)
        }
        Err(err) => {
            let receipt = Receipt {
                request_id: query.request_id,
                session_id: None,
                status: ReceiptStatus::Failed,
                result: None,
                error: Some(err.to_string()),
                updated_at: openremote_core::now_ms(),
            };
            app.supervisor.record_receipt(receipt.clone());
            supervisor_error(err)
        }
    }
}

#[derive(Deserialize)]
struct EnabledBody {
    request_id: String,
    enabled: bool,
}

async fn set_mcp_server_enabled(
    State(app): State<Arc<App>>,
    AxumPath(id): AxumPath<String>,
    Json(body): Json<EnabledBody>,
) -> Response {
    if let Some(receipt) = app.supervisor.receipt(&body.request_id) {
        return receipt_response(&receipt);
    }
    match app.supervisor.set_mcp_server_enabled(&id, body.enabled) {
        Ok(plugin) => {
            let receipt = Receipt {
                request_id: body.request_id,
                session_id: None,
                status: ReceiptStatus::Completed,
                result: Some(serde_json::to_value(&plugin).unwrap_or(Value::Null)),
                error: None,
                updated_at: openremote_core::now_ms(),
            };
            app.supervisor.record_receipt(receipt.clone());
            (StatusCode::OK, Json(plugin)).into_response()
        }
        Err(err) => {
            let receipt = Receipt {
                request_id: body.request_id,
                session_id: None,
                status: ReceiptStatus::Failed,
                result: None,
                error: Some(err.to_string()),
                updated_at: openremote_core::now_ms(),
            };
            app.supervisor.record_receipt(receipt.clone());
            supervisor_error(err)
        }
    }
}

#[derive(Deserialize)]
struct RequestIdBody {
    request_id: String,
}

async fn acknowledge_mcp_server_key(
    State(app): State<Arc<App>>,
    AxumPath(id): AxumPath<String>,
    Json(body): Json<RequestIdBody>,
) -> Response {
    if let Some(receipt) = app.supervisor.receipt(&body.request_id) {
        return receipt_response(&receipt);
    }
    match app.supervisor.acknowledge_mcp_server_key(&id) {
        Ok(plugin) => {
            let receipt = Receipt {
                request_id: body.request_id,
                session_id: None,
                status: ReceiptStatus::Completed,
                result: Some(serde_json::to_value(&plugin).unwrap_or(Value::Null)),
                error: None,
                updated_at: openremote_core::now_ms(),
            };
            app.supervisor.record_receipt(receipt.clone());
            (StatusCode::OK, Json(plugin)).into_response()
        }
        Err(err) => {
            let receipt = Receipt {
                request_id: body.request_id,
                session_id: None,
                status: ReceiptStatus::Failed,
                result: None,
                error: Some(err.to_string()),
                updated_at: openremote_core::now_ms(),
            };
            app.supervisor.record_receipt(receipt.clone());
            supervisor_error(err)
        }
    }
}

// ---- automations ----

async fn list_rules(State(app): State<Arc<App>>) -> Response {
    Json(app.supervisor.rules()).into_response()
}

#[derive(Deserialize)]
struct SaveRuleBody {
    request_id: String,
    #[serde(default)]
    id: Option<String>,
    name: String,
    trigger: openremote_core::Trigger,
    harness: String,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    effort: Option<String>,
    #[serde(default)]
    permission_mode: Option<String>,
    workspace: String,
    machine: String,
    task: String,
    #[serde(default)]
    enabled: bool,
}

async fn save_rule(State(app): State<Arc<App>>, Json(body): Json<SaveRuleBody>) -> Response {
    if let Some(receipt) = app.supervisor.receipt(&body.request_id) {
        return receipt_response(&receipt);
    }
    let (Some(machine_id), existing) = (
        openremote_core::MachineId::parse(&body.machine),
        body.id.as_deref().and_then(openremote_core::RuleId::parse),
    ) else {
        return error(StatusCode::BAD_REQUEST, "bad machine id");
    };
    let rule = openremote_core::AutomationRule {
        id: existing.unwrap_or_default(),
        name: body.name,
        trigger: body.trigger,
        harness: body.harness,
        model: body.model,
        effort: body.effort,
        permission_mode: body.permission_mode,
        workspace: std::path::PathBuf::from(&body.workspace),
        machine: machine_id,
        task: body.task,
        // the store rules on save; the body's flag only applies to edits
        enabled: existing.is_none() || body.enabled,
        last_chat: None,
        last_run: None,
        created_at: openremote_core::now_ms(),
        updated_at: openremote_core::now_ms(),
    };
    match app.supervisor.save_rule(rule, existing) {
        Ok(rule) => {
            let receipt = Receipt {
                request_id: body.request_id,
                session_id: None,
                status: ReceiptStatus::Completed,
                result: Some(serde_json::to_value(&rule).unwrap_or(Value::Null)),
                error: None,
                updated_at: openremote_core::now_ms(),
            };
            app.supervisor.record_receipt(receipt.clone());
            (StatusCode::CREATED, Json(rule)).into_response()
        }
        Err(err) => {
            let receipt = Receipt {
                request_id: body.request_id,
                session_id: None,
                status: ReceiptStatus::Failed,
                result: None,
                error: Some(err.to_string()),
                updated_at: openremote_core::now_ms(),
            };
            app.supervisor.record_receipt(receipt.clone());
            supervisor_error(err)
        }
    }
}

async fn remove_rule(
    State(app): State<Arc<App>>,
    AxumPath(id): AxumPath<String>,
    axum::extract::Query(query): axum::extract::Query<MachineActionQuery>,
) -> Response {
    if let Some(receipt) = app.supervisor.receipt(&query.request_id) {
        return receipt_response(&receipt);
    }
    let Some(rule_id) = openremote_core::RuleId::parse(&id) else {
        return error(StatusCode::BAD_REQUEST, "bad rule id");
    };
    match app.supervisor.remove_rule(&rule_id) {
        Ok(()) => {
            let receipt = Receipt {
                request_id: query.request_id,
                session_id: None,
                status: ReceiptStatus::Completed,
                result: Some(json!({"removed": true})),
                error: None,
                updated_at: openremote_core::now_ms(),
            };
            app.supervisor.record_receipt(receipt.clone());
            receipt_response(&receipt)
        }
        Err(err) => {
            let receipt = Receipt {
                request_id: query.request_id,
                session_id: None,
                status: ReceiptStatus::Failed,
                result: None,
                error: Some(err.to_string()),
                updated_at: openremote_core::now_ms(),
            };
            app.supervisor.record_receipt(receipt.clone());
            supervisor_error(err)
        }
    }
}

async fn set_rule_enabled(
    State(app): State<Arc<App>>,
    AxumPath(id): AxumPath<String>,
    Json(body): Json<EnabledBody>,
) -> Response {
    if let Some(receipt) = app.supervisor.receipt(&body.request_id) {
        return receipt_response(&receipt);
    }
    let Some(rule_id) = openremote_core::RuleId::parse(&id) else {
        return error(StatusCode::BAD_REQUEST, "bad rule id");
    };
    match app.supervisor.set_rule_enabled(&rule_id, body.enabled) {
        Ok(rule) => {
            let receipt = Receipt {
                request_id: body.request_id,
                session_id: None,
                status: ReceiptStatus::Completed,
                result: Some(serde_json::to_value(&rule).unwrap_or(Value::Null)),
                error: None,
                updated_at: openremote_core::now_ms(),
            };
            app.supervisor.record_receipt(receipt.clone());
            (StatusCode::OK, Json(rule)).into_response()
        }
        Err(err) => {
            let receipt = Receipt {
                request_id: body.request_id,
                session_id: None,
                status: ReceiptStatus::Failed,
                result: None,
                error: Some(err.to_string()),
                updated_at: openremote_core::now_ms(),
            };
            app.supervisor.record_receipt(receipt.clone());
            supervisor_error(err)
        }
    }
}

async fn run_rule(
    State(app): State<Arc<App>>,
    AxumPath(id): AxumPath<String>,
    Json(body): Json<RequestIdBody>,
) -> Response {
    if let Some(receipt) = app.supervisor.receipt(&body.request_id) {
        return receipt_response(&receipt);
    }
    let Some(rule_id) = openremote_core::RuleId::parse(&id) else {
        return error(StatusCode::BAD_REQUEST, "bad rule id");
    };
    match app.supervisor.run_rule(&rule_id).await {
        Ok(session) => {
            let receipt = Receipt {
                request_id: body.request_id,
                session_id: Some(session.id),
                status: ReceiptStatus::Completed,
                result: Some(serde_json::to_value(&session).unwrap_or(Value::Null)),
                error: None,
                updated_at: openremote_core::now_ms(),
            };
            app.supervisor.record_receipt(receipt.clone());
            (StatusCode::OK, Json(session)).into_response()
        }
        Err(err) => {
            let receipt = Receipt {
                request_id: body.request_id,
                session_id: None,
                status: ReceiptStatus::Failed,
                result: None,
                error: Some(err.to_string()),
                updated_at: openremote_core::now_ms(),
            };
            app.supervisor.record_receipt(receipt.clone());
            supervisor_error(err)
        }
    }
}

/// A webhook arriving for a rule - open route, the rule's key is the
/// credential: `/hooks/{rule-id}?key=…`. The raw body is the delivery's
/// identity (hook senders bring no request_id), so it is read whole and
/// hashed for the dedup window. A refused delivery - a replay, a
/// cooldown - answers 200 with `fired: false`, never an error status:
/// an error is what sends a retrying sender around again, and the retry
/// is exactly what the refusal exists to stop.
async fn incoming_webhook(
    State(app): State<Arc<App>>,
    AxumPath(id): AxumPath<String>,
    axum::extract::Query(query): axum::extract::Query<WebhookQuery>,
    body: axum::body::Bytes,
) -> Response {
    let Some(rule_id) = openremote_core::RuleId::parse(&id) else {
        return error(StatusCode::BAD_REQUEST, "bad rule id");
    };
    match app
        .supervisor
        .fire_webhook(&rule_id, query.key.as_deref(), &body)
        .await
    {
        Ok(crate::supervisor::WebhookFire::Fired(session)) => (
            StatusCode::OK,
            Json(json!({"fired": true, "chat": session.id.to_string()})),
        )
            .into_response(),
        Ok(crate::supervisor::WebhookFire::Refused(reason)) => (
            StatusCode::OK,
            Json(json!({"fired": false, "reason": reason})),
        )
            .into_response(),
        Err(err) => supervisor_error(err),
    }
}

#[derive(Deserialize)]
struct WebhookQuery {
    #[serde(default)]
    key: Option<String>,
}

// ---- projects ----

async fn list_projects(State(app): State<Arc<App>>) -> Response {
    Json(app.supervisor.projects()).into_response()
}

#[derive(Deserialize)]
struct SaveProjectBody {
    request_id: String,
    /// One folder this release; the array is the multi-folder future.
    folders: Vec<String>,
}

async fn save_project(State(app): State<Arc<App>>, Json(body): Json<SaveProjectBody>) -> Response {
    if let Some(receipt) = app.supervisor.receipt(&body.request_id) {
        return receipt_response(&receipt);
    }
    let folders: Vec<std::path::PathBuf> =
        body.folders.iter().map(std::path::PathBuf::from).collect();
    match app.supervisor.save_project(folders) {
        Ok(project) => {
            let receipt = Receipt {
                request_id: body.request_id,
                session_id: None,
                status: ReceiptStatus::Completed,
                result: Some(serde_json::to_value(&project).unwrap_or(Value::Null)),
                error: None,
                updated_at: openremote_core::now_ms(),
            };
            app.supervisor.record_receipt(receipt.clone());
            (StatusCode::CREATED, Json(project)).into_response()
        }
        Err(err) => {
            let receipt = Receipt {
                request_id: body.request_id,
                session_id: None,
                status: ReceiptStatus::Failed,
                result: None,
                error: Some(err.to_string()),
                updated_at: openremote_core::now_ms(),
            };
            app.supervisor.record_receipt(receipt.clone());
            supervisor_error(err)
        }
    }
}

async fn remove_project(
    State(app): State<Arc<App>>,
    AxumPath(id): AxumPath<String>,
    axum::extract::Query(query): axum::extract::Query<MachineActionQuery>,
) -> Response {
    if let Some(receipt) = app.supervisor.receipt(&query.request_id) {
        return receipt_response(&receipt);
    }
    let Some(project_id) = openremote_core::ProjectId::parse(&id) else {
        return error(StatusCode::BAD_REQUEST, "bad project id");
    };
    match app.supervisor.remove_project(&project_id) {
        Ok(()) => {
            let receipt = Receipt {
                request_id: query.request_id,
                session_id: None,
                status: ReceiptStatus::Completed,
                result: Some(json!({"removed": true})),
                error: None,
                updated_at: openremote_core::now_ms(),
            };
            app.supervisor.record_receipt(receipt.clone());
            receipt_response(&receipt)
        }
        Err(err) => {
            let receipt = Receipt {
                request_id: query.request_id,
                session_id: None,
                status: ReceiptStatus::Failed,
                result: None,
                error: Some(err.to_string()),
                updated_at: openremote_core::now_ms(),
            };
            app.supervisor.record_receipt(receipt.clone());
            supervisor_error(err)
        }
    }
}

async fn install_harness(
    State(app): State<Arc<App>>,
    AxumPath(id): AxumPath<String>,
    Json(body): Json<InstallHarnessBody>,
) -> Response {
    if let Some(receipt) = app.supervisor.receipt(&body.request_id) {
        return receipt_response(&receipt);
    }
    let Some(machine_id) = MachineId::parse(&id) else {
        return error(StatusCode::BAD_REQUEST, "bad machine id");
    };
    // Installers run minutes, not milliseconds - accepted first, terminal state
    // when the install settles (a crash between surfaces as unknown work).
    let accepted = Receipt {
        request_id: body.request_id.clone(),
        session_id: None,
        status: ReceiptStatus::Accepted,
        result: None,
        error: None,
        updated_at: openremote_core::now_ms(),
    };
    app.supervisor.record_receipt(accepted);
    match app
        .supervisor
        .install_harness(&machine_id, &body.harness, body.with_runtime)
        .await
    {
        Ok(view) => {
            let receipt = Receipt {
                request_id: body.request_id,
                session_id: None,
                status: ReceiptStatus::Completed,
                result: Some(serde_json::to_value(&view).unwrap_or(Value::Null)),
                error: None,
                updated_at: openremote_core::now_ms(),
            };
            app.supervisor.record_receipt(receipt.clone());
            receipt_response(&receipt)
        }
        Err(err) => {
            let receipt = Receipt {
                request_id: body.request_id,
                session_id: None,
                status: ReceiptStatus::Failed,
                result: None,
                error: Some(err.to_string()),
                updated_at: openremote_core::now_ms(),
            };
            app.supervisor.record_receipt(receipt.clone());
            supervisor_error(err)
        }
    }
}

#[derive(Deserialize)]
struct CreateSessionBody {
    request_id: String,
    harness: String,
    /// Absent for a project-less chat - it runs in the home folder.
    #[serde(default)]
    workspace: Option<String>,
    #[serde(default)]
    model: Option<String>,
    /// The harness's own effort tier word (claude/codex high, grok's
    /// answer effort), verbatim - an unknown word surfaces as the
    /// harness's own spawn error, not ours.
    #[serde(default)]
    effort: Option<String>,
    #[serde(default)]
    permission_mode: Option<String>,
    /// Run the harness's own fast mode (claude `fastMode`, codex service
    /// tier `fast`).
    #[serde(default)]
    fast: bool,
    /// A machine-local custom model endpoint this chat runs through.
    /// Absent (or blank) means the harness's own provider or its
    /// machine default.
    #[serde(default)]
    endpoint_id: Option<String>,
    /// Cloud: run the chat on another of the user's devices.
    #[serde(default)]
    pub(crate) device_id: Option<String>,
    /// Cloud: sync the chat to the user's other devices.
    #[serde(default)]
    synced: bool,
}

async fn create_session(
    State(app): State<Arc<App>>,
    peer: Option<axum::Extension<crate::host::PeerOrigin>>,
    Json(body): Json<CreateSessionBody>,
) -> Response {
    if let Some(receipt) = app.supervisor.receipt(&body.request_id) {
        return receipt_response(&receipt);
    }
    let me = app.cloud.device_id();
    // A `device_id` that is present but unparseable is a broken client
    // (a truncated UUID, a stale cache), not "no device named": the old
    // parse folded it into absent and silently created the chat on this
    // machine, ignoring the user's intent to run it elsewhere.
    let target = match device_target(body.device_id.as_deref()) {
        DeviceTarget::Invalid => {
            return error(
                StatusCode::BAD_REQUEST,
                "device_id must be a device id - name none to run here",
            );
        }
        DeviceTarget::Here => None,
        DeviceTarget::There(device) => Some(device),
    };
    if let Some(device) = target.filter(|d| Some(*d) != me) {
        if peer.is_some() {
            return error(StatusCode::BAD_REQUEST, "a chat runs where it is created");
        }
        return crate::cloud_http::create_remote(&app, device, &body.request_id, &body.raw()).await;
    }
    if openremote_cloud::config::is_phone(&app.cloud.config().platform) {
        return error(
            StatusCode::CONFLICT,
            "a phone can't run chats - start this one on one of your machines",
        );
    }
    // A chat a peer asks for, or one made in Cloud, syncs to every device.
    let executor = if body.synced || target.is_some() || peer.is_some() {
        match me {
            Some(me) => Some(me),
            None => {
                return error(
                    StatusCode::CONFLICT,
                    "sign in to OpenRemote Cloud to sync chats",
                );
            }
        }
    } else {
        None
    };

    let workspace = body
        .workspace
        .as_deref()
        .filter(|w| !w.trim().is_empty())
        .map(std::path::PathBuf::from);
    // Workspace-root containment: absolute, existing, a directory. A
    // project-less chat names none and runs in the home folder.
    if let Some(workspace) = &workspace {
        if !workspace.is_absolute() || !workspace.is_dir() {
            return error(
                StatusCode::UNPROCESSABLE_ENTITY,
                "workspace must be an absolute path to an existing directory",
            );
        }
    }
    match app
        .supervisor
        .create_session(
            &body.harness,
            workspace,
            body.model.clone(),
            body.effort.clone(),
            body.permission_mode.clone(),
            body.fast,
            executor,
            body.endpoint_id.clone(),
        )
        .await
    {
        Ok(session) => {
            let receipt = Receipt {
                request_id: body.request_id.clone(),
                session_id: Some(session.id),
                status: ReceiptStatus::Completed,
                result: Some(serde_json::to_value(&session).unwrap_or(Value::Null)),
                error: None,
                updated_at: openremote_core::now_ms(),
            };
            app.supervisor.record_receipt(receipt.clone());
            (StatusCode::CREATED, Json(session)).into_response()
        }
        Err(err) => {
            let receipt = Receipt {
                request_id: body.request_id.clone(),
                session_id: None,
                status: ReceiptStatus::Failed,
                result: None,
                error: Some(err.to_string()),
                updated_at: openremote_core::now_ms(),
            };
            app.supervisor.record_receipt(receipt.clone());
            supervisor_error(err)
        }
    }
}

impl CreateSessionBody {
    /// The request as the other device should receive it.
    fn raw(&self) -> Value {
        let mut body = json!({
            "request_id": self.request_id,
            "harness": self.harness,
            "model": self.model,
            "effort": self.effort,
            "permission_mode": self.permission_mode,
            "fast": self.fast,
            "endpoint_id": self.endpoint_id,
        });
        // An absent workspace is omitted, never named null - the peer's
        // own deserializer treats missing and null alike, and a named null
        // would read as an empty workspace string on older builds.
        if let Some(workspace) = &self.workspace {
            body["workspace"] = json!(workspace);
        }
        body
    }
}

/// The `device_id` a create names: absent (a local chat, the default),
/// a device to run it on, or present-but-garbage. The third state is
/// the whole point of the enum: the old parse folded it into absent and
/// silently ran the chat here when the user had named their machine.
enum DeviceTarget {
    Here,
    There(openremote_core::DeviceId),
    Invalid,
}

fn device_target(named: Option<&str>) -> DeviceTarget {
    match named {
        None => DeviceTarget::Here,
        Some(raw) => match openremote_core::DeviceId::parse(raw) {
            Some(id) => DeviceTarget::There(id),
            None => DeviceTarget::Invalid,
        },
    }
}

/// Syncs a private chat to the user's other devices (it keeps running here).
async fn post_sync(State(app): State<Arc<App>>, AxumPath(id): AxumPath<String>) -> Response {
    let Some(id) = SessionId::parse(&id) else {
        return error(StatusCode::NOT_FOUND, "no such chat");
    };
    let Some(me) = app.cloud.device_id() else {
        return error(StatusCode::CONFLICT, "sign in to OpenRemote Cloud first");
    };
    match app.supervisor.sync_session(&id, me) {
        Ok(session) => Json(session).into_response(),
        Err(err) => supervisor_error(err),
    }
}

async fn sync_summary(State(app): State<Arc<App>>) -> Response {
    Json(crate::sync::summary(&app)).into_response()
}

#[derive(Deserialize)]
struct RangeQuery {
    #[serde(default)]
    from: u64,
}

async fn sync_range(
    State(app): State<Arc<App>>,
    AxumPath(id): AxumPath<String>,
    axum::extract::Query(q): axum::extract::Query<RangeQuery>,
) -> Response {
    let Some(id) = SessionId::parse(&id) else {
        return error(StatusCode::NOT_FOUND, "no such chat");
    };
    match crate::sync::range(&app, id, q.from) {
        Ok(range) => Json(range).into_response(),
        Err((status, message)) => error(
            StatusCode::from_u16(status).unwrap_or(StatusCode::NOT_FOUND),
            message,
        ),
    }
}

/// Deletes a synced chat everywhere: stopped if it runs here, removed from
/// this device, and a tombstone for every other device.
async fn delete_session(State(app): State<Arc<App>>, AxumPath(id): AxumPath<String>) -> Response {
    let Some(id) = SessionId::parse(&id) else {
        return error(StatusCode::NOT_FOUND, "no such chat");
    };
    let synced = crate::supervisor::lock_recovering(&app.store)
        .session(&id)
        .map(|s| s.executor.is_some());
    match synced {
        Err(_) => return error(StatusCode::NOT_FOUND, "no such chat"),
        Ok(false) => return error(StatusCode::CONFLICT, "only synced chats can be deleted"),
        Ok(true) => {}
    }
    let Some(me) = app.cloud.device_id() else {
        return error(StatusCode::CONFLICT, "sign in to OpenRemote Cloud first");
    };
    let stone = openremote_core::store::Tombstone {
        at: openremote_core::now_ms(),
        by: me,
    };
    crate::sync::apply_tombstone(&app, id, stone).await;
    crate::sync::announce_tombstone(&app, id, stone).await;
    StatusCode::NO_CONTENT.into_response()
}

async fn list_sessions(State(app): State<Arc<App>>) -> Response {
    Json(app.supervisor.sessions()).into_response()
}

async fn get_session(State(app): State<Arc<App>>, AxumPath(id): AxumPath<String>) -> Response {
    match SessionId::parse(&id) {
        Some(id) => match app.supervisor.session(&id) {
            Ok(session) => Json(session).into_response(),
            Err(err) => supervisor_error(err),
        },
        None => error(StatusCode::BAD_REQUEST, "bad session id"),
    }
}

async fn list_decisions(State(app): State<Arc<App>>, AxumPath(id): AxumPath<String>) -> Response {
    match SessionId::parse(&id) {
        Some(id) => Json(app.supervisor.decisions(&id)).into_response(),
        None => error(StatusCode::BAD_REQUEST, "bad session id"),
    }
}

#[derive(Deserialize)]
struct RequestBody {
    request_id: String,
}

#[derive(Deserialize)]
struct PromptBody {
    request_id: String,
    /// The words; empty for a message of pictures alone.
    #[serde(default)]
    text: String,
    /// The chats this message points at with @: from now on this chat's
    /// agent may read them, and only them.
    #[serde(default)]
    mentioned_chats: Vec<String>,
    /// Pictures riding the message: what was pasted or dropped, a page's
    /// screenshot, a picked element's picture.
    #[serde(default)]
    attachments: Vec<crate::images::Attachment>,
}

fn mentioned(body: &PromptBody) -> Vec<SessionId> {
    body.mentioned_chats
        .iter()
        .filter_map(|id| SessionId::parse(id))
        .collect()
}

/// A message's pictures refused before anything is written for it, where
/// the chat's harness reads none. An id that is no chat id at all is left
/// to the route's own answer.
fn refused_pictures(app: &App, id: &str, images: &[crate::images::NewImage]) -> Option<Response> {
    let chat = SessionId::parse(id)?;
    app.supervisor
        .refuse_pictures(&chat, images)
        .err()
        .map(supervisor_error)
}

/// The body limit of the routes a message takes: its pictures ride as
/// base64.
fn message_body_limit() -> axum::extract::DefaultBodyLimit {
    axum::extract::DefaultBodyLimit::max(crate::images::MESSAGE_BODY_LIMIT)
}

/// A picture from the blob store, for this device's console: an `<img>`
/// can't send the token, so the console fetches it into an object URL.
async fn get_blob(State(app): State<Arc<App>>, AxumPath(sha): AxumPath<String>) -> Response {
    blob_response(&app, sha, None).await
}

/// One chat's picture - how another device showing a synced chat asks the
/// device that runs it. Only a picture that chat keeps answers.
async fn get_session_blob(
    State(app): State<Arc<App>>,
    AxumPath((id, sha)): AxumPath<(String, String)>,
) -> Response {
    let Some(chat) = SessionId::parse(&id) else {
        return error(StatusCode::NOT_FOUND, "no such chat");
    };
    blob_response(&app, sha, Some(chat)).await
}

async fn blob_response(app: &App, sha: String, chat: Option<SessionId>) -> Response {
    let blobs = app.supervisor.blobs();
    let found = tokio::task::spawn_blocking(move || {
        if !openremote_core::blobs::Blobs::is_name(&sha) {
            return Ok(None);
        }
        if chat.is_some_and(|chat| !blobs.held_by(&sha, chat)) {
            return Ok(None);
        }
        blobs.get(&sha)
    })
    .await;
    match found {
        Ok(Ok(Some((mime, bytes)))) => (
            [
                (header::CONTENT_TYPE, mime),
                // Named by its hash, so it never changes.
                (
                    header::CACHE_CONTROL,
                    "private, max-age=31536000, immutable".to_string(),
                ),
                (header::X_CONTENT_TYPE_OPTIONS, "nosniff".to_string()),
            ],
            bytes,
        )
            .into_response(),
        Ok(Ok(None)) => error(StatusCode::NOT_FOUND, "no such picture"),
        Ok(Err(e)) => error(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
        Err(e) => error(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    }
}

#[derive(Deserialize)]
struct SettingsBody {
    request_id: String,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    effort: Option<String>,
    #[serde(default)]
    fast: Option<bool>,
    #[serde(default)]
    permission_mode: Option<String>,
}

/// Model, effort, and fast on a chat. A harness that only accepts the
/// change at process start restarts an idle chat onto the new word. A
/// turn that is still working answers 409.
async fn post_settings(
    State(app): State<Arc<App>>,
    AxumPath(id): AxumPath<String>,
    Json(body): Json<SettingsBody>,
) -> Response {
    if let Some(receipt) = app.supervisor.receipt(&body.request_id) {
        return receipt_response(&receipt);
    }
    let Some(session_id) = SessionId::parse(&id) else {
        return error(StatusCode::BAD_REQUEST, "bad session id");
    };
    let settings = openremote_harness::SessionSettings {
        model: body.model.filter(|m| !m.trim().is_empty()),
        effort: body.effort.filter(|e| !e.trim().is_empty()),
        fast: body.fast,
        permission_mode: body.permission_mode.filter(|p| !p.trim().is_empty()),
    };
    match app.supervisor.update_settings(&session_id, settings).await {
        Ok(session) => {
            let receipt = Receipt {
                request_id: body.request_id,
                session_id: Some(session_id),
                status: ReceiptStatus::Completed,
                result: Some(serde_json::to_value(&session).unwrap_or(Value::Null)),
                error: None,
                updated_at: openremote_core::now_ms(),
            };
            app.supervisor.record_receipt(receipt.clone());
            (StatusCode::OK, Json(session)).into_response()
        }
        Err(err) => {
            let receipt = Receipt {
                request_id: body.request_id,
                session_id: Some(session_id),
                status: ReceiptStatus::Failed,
                result: None,
                error: Some(err.to_string()),
                updated_at: openremote_core::now_ms(),
            };
            app.supervisor.record_receipt(receipt.clone());
            supervisor_error(err)
        }
    }
}

async fn post_prompt(
    State(app): State<Arc<App>>,
    AxumPath(id): AxumPath<String>,
    Json(body): Json<PromptBody>,
) -> Response {
    if let Some(receipt) = app.supervisor.receipt(&body.request_id) {
        return receipt_response(&receipt);
    }
    let Some(session_id) = SessionId::parse(&id) else {
        return error(StatusCode::BAD_REQUEST, "bad session id");
    };
    // The pictures are checked before anything is recorded.
    let images = match crate::images::decode(&body.attachments) {
        Ok(images) => images,
        Err(why) => return error(StatusCode::BAD_REQUEST, &why),
    };
    if let Err(err) = app.supervisor.refuse_pictures(&session_id, &images) {
        return supervisor_error(err);
    }
    let accepted = Receipt {
        request_id: body.request_id.clone(),
        session_id: Some(session_id),
        status: ReceiptStatus::Accepted,
        result: None,
        error: None,
        updated_at: openremote_core::now_ms(),
    };
    // Crash after this line, before the terminal write, leaves `accepted`
    // - surfaced to the console as unknown work, never resent.
    app.supervisor.record_receipt(accepted);
    let pointed = mentioned(&body);
    if let Err(err) = app.supervisor.note_mentions(&session_id, &pointed) {
        return supervisor_error(err);
    }
    match app
        .supervisor
        .prompt_with(&session_id, &body.text, images)
        .await
    {
        Ok(()) => {
            let receipt = Receipt {
                request_id: body.request_id,
                session_id: Some(session_id),
                status: ReceiptStatus::Completed,
                result: Some(json!({"delivered": true})),
                error: None,
                updated_at: openremote_core::now_ms(),
            };
            app.supervisor.record_receipt(receipt.clone());
            receipt_response(&receipt)
        }
        Err(err) => {
            let receipt = Receipt {
                request_id: body.request_id,
                session_id: Some(session_id),
                status: ReceiptStatus::Failed,
                result: None,
                error: Some(err.to_string()),
                updated_at: openremote_core::now_ms(),
            };
            app.supervisor.record_receipt(receipt.clone());
            supervisor_error(err)
        }
    }
}

/// The receipt flow every queue action shares: a replayed request answers
/// from its receipt; a new one is recorded as accepted, run, and settled.
async fn receipted<F, Fut>(app: &Arc<App>, request_id: String, id: &str, run: F) -> Response
where
    F: FnOnce(SessionId) -> Fut,
    Fut: std::future::Future<Output = Result<Value, crate::supervisor::SupervisorError>>,
{
    if let Some(receipt) = app.supervisor.receipt(&request_id) {
        return receipt_response(&receipt);
    }
    let Some(session_id) = SessionId::parse(id) else {
        return error(StatusCode::BAD_REQUEST, "bad session id");
    };
    app.supervisor.record_receipt(Receipt {
        request_id: request_id.clone(),
        session_id: Some(session_id),
        status: ReceiptStatus::Accepted,
        result: None,
        error: None,
        updated_at: openremote_core::now_ms(),
    });
    match run(session_id).await {
        Ok(result) => {
            let receipt = Receipt {
                request_id,
                session_id: Some(session_id),
                status: ReceiptStatus::Completed,
                result: Some(result),
                error: None,
                updated_at: openremote_core::now_ms(),
            };
            app.supervisor.record_receipt(receipt.clone());
            receipt_response(&receipt)
        }
        Err(err) => {
            let receipt = Receipt {
                request_id,
                session_id: Some(session_id),
                status: ReceiptStatus::Failed,
                result: None,
                error: Some(err.to_string()),
                updated_at: openremote_core::now_ms(),
            };
            app.supervisor.record_receipt(receipt.clone());
            supervisor_error(err)
        }
    }
}

/// Queue a message behind the running turn.
async fn post_queue(
    State(app): State<Arc<App>>,
    AxumPath(id): AxumPath<String>,
    Json(body): Json<PromptBody>,
) -> Response {
    if let Some(receipt) = app.supervisor.receipt(&body.request_id) {
        return receipt_response(&receipt);
    }
    // The pictures are checked before anything is recorded.
    let images = match crate::images::decode(&body.attachments) {
        Ok(images) => images,
        Err(why) => return error(StatusCode::BAD_REQUEST, &why),
    };
    if let Some(err) = refused_pictures(&app, &id, &images) {
        return err;
    }
    let sup = Arc::clone(&app.supervisor);
    let pointed = mentioned(&body);
    receipted(&app, body.request_id, &id, |sid| async move {
        sup.note_mentions(&sid, &pointed)?;
        let session = sup.queue_add_with(&sid, &body.text, images).await?;
        Ok(serde_json::to_value(&session).unwrap_or(Value::Null))
    })
    .await
}

async fn post_queue_remove(
    State(app): State<Arc<App>>,
    AxumPath((id, item)): AxumPath<(String, String)>,
    Json(body): Json<RequestBody>,
) -> Response {
    let sup = Arc::clone(&app.supervisor);
    receipted(&app, body.request_id, &id, |sid| async move {
        let session = sup.queue_remove(&sid, &item).await?;
        Ok(serde_json::to_value(&session).unwrap_or(Value::Null))
    })
    .await
}

async fn post_queue_steer(
    State(app): State<Arc<App>>,
    AxumPath((id, item)): AxumPath<(String, String)>,
    Json(body): Json<RequestBody>,
) -> Response {
    let sup = Arc::clone(&app.supervisor);
    receipted(&app, body.request_id, &id, |sid| async move {
        sup.queue_steer(&sid, &item).await?;
        Ok(json!({"steered": true}))
    })
    .await
}

async fn post_queue_resume(
    State(app): State<Arc<App>>,
    AxumPath(id): AxumPath<String>,
    Json(body): Json<RequestBody>,
) -> Response {
    let sup = Arc::clone(&app.supervisor);
    receipted(&app, body.request_id, &id, |sid| async move {
        let session = sup.queue_resume(&sid).await?;
        Ok(serde_json::to_value(&session).unwrap_or(Value::Null))
    })
    .await
}

/// Steer a message straight into the running turn (the composer's
/// Ctrl+Enter), never through the queue.
async fn post_steer(
    State(app): State<Arc<App>>,
    AxumPath(id): AxumPath<String>,
    Json(body): Json<PromptBody>,
) -> Response {
    if let Some(receipt) = app.supervisor.receipt(&body.request_id) {
        return receipt_response(&receipt);
    }
    // The pictures are checked before anything is recorded.
    let images = match crate::images::decode(&body.attachments) {
        Ok(images) => images,
        Err(why) => return error(StatusCode::BAD_REQUEST, &why),
    };
    if let Some(err) = refused_pictures(&app, &id, &images) {
        return err;
    }
    let sup = Arc::clone(&app.supervisor);
    let pointed = mentioned(&body);
    receipted(&app, body.request_id, &id, |sid| async move {
        sup.note_mentions(&sid, &pointed)?;
        sup.steer_with(&sid, &body.text, images).await?;
        Ok(json!({"steered": true}))
    })
    .await
}

async fn post_interrupt(
    State(app): State<Arc<App>>,
    AxumPath(id): AxumPath<String>,
    Json(body): Json<RequestBody>,
) -> Response {
    if let Some(receipt) = app.supervisor.receipt(&body.request_id) {
        return receipt_response(&receipt);
    }
    let Some(session_id) = SessionId::parse(&id) else {
        return error(StatusCode::BAD_REQUEST, "bad session id");
    };
    match app.supervisor.interrupt(&session_id).await {
        Ok(()) => {
            let receipt = Receipt {
                request_id: body.request_id,
                session_id: Some(session_id),
                status: ReceiptStatus::Completed,
                result: Some(json!({"interrupted": true})),
                error: None,
                updated_at: openremote_core::now_ms(),
            };
            app.supervisor.record_receipt(receipt.clone());
            receipt_response(&receipt)
        }
        Err(err) => supervisor_error(err),
    }
}

async fn post_stop(
    State(app): State<Arc<App>>,
    AxumPath(id): AxumPath<String>,
    Json(body): Json<RequestBody>,
) -> Response {
    if let Some(receipt) = app.supervisor.receipt(&body.request_id) {
        return receipt_response(&receipt);
    }
    let Some(session_id) = SessionId::parse(&id) else {
        return error(StatusCode::BAD_REQUEST, "bad session id");
    };
    match app.supervisor.stop(&session_id).await {
        Ok(session) => {
            let receipt = Receipt {
                request_id: body.request_id,
                session_id: Some(session_id),
                status: ReceiptStatus::Completed,
                result: Some(serde_json::to_value(&session).unwrap_or(Value::Null)),
                error: None,
                updated_at: openremote_core::now_ms(),
            };
            app.supervisor.record_receipt(receipt.clone());
            Json(session).into_response()
        }
        Err(err) => supervisor_error(err),
    }
}

async fn post_resume(
    State(app): State<Arc<App>>,
    AxumPath(id): AxumPath<String>,
    Json(body): Json<RequestBody>,
) -> Response {
    if let Some(receipt) = app.supervisor.receipt(&body.request_id) {
        return receipt_response(&receipt);
    }
    let Some(session_id) = SessionId::parse(&id) else {
        return error(StatusCode::BAD_REQUEST, "bad session id");
    };
    match app.supervisor.resume(&session_id).await {
        Ok(session) => {
            let receipt = Receipt {
                request_id: body.request_id,
                session_id: Some(session_id),
                status: ReceiptStatus::Completed,
                result: Some(serde_json::to_value(&session).unwrap_or(Value::Null)),
                error: None,
                updated_at: openremote_core::now_ms(),
            };
            app.supervisor.record_receipt(receipt.clone());
            Json(session).into_response()
        }
        Err(err) => supervisor_error(err),
    }
}

#[derive(Deserialize)]
struct AnswerBody {
    request_id: String,
    choice: String,
}

async fn post_answer(
    State(app): State<Arc<App>>,
    AxumPath(id): AxumPath<String>,
    Json(body): Json<AnswerBody>,
) -> Response {
    if let Some(receipt) = app.supervisor.receipt(&body.request_id) {
        return receipt_response(&receipt);
    }
    let Some(decision_id) = DecisionId::parse(&id) else {
        return error(StatusCode::BAD_REQUEST, "bad decision id");
    };
    match app
        .supervisor
        .answer_decision(&decision_id, &body.choice)
        .await
    {
        Ok(decision) => {
            let receipt = Receipt {
                request_id: body.request_id,
                session_id: Some(decision.session_id),
                status: ReceiptStatus::Completed,
                result: Some(serde_json::to_value(&decision).unwrap_or(Value::Null)),
                error: None,
                updated_at: openremote_core::now_ms(),
            };
            app.supervisor.record_receipt(receipt.clone());
            Json(decision).into_response()
        }
        Err(err) => supervisor_error(err),
    }
}

async fn get_receipt(
    State(app): State<Arc<App>>,
    AxumPath(request_id): AxumPath<String>,
) -> Response {
    match app.supervisor.receipt(&request_id) {
        Some(receipt) => receipt_response(&receipt),
        // A request the daemon has no record of is unknown - that is the
        // crash window, and it is a legitimate answer, not an error.
        None => Json(json!({"request_id": request_id, "status": "unknown"})).into_response(),
    }
}

pub(crate) fn receipt_response(receipt: &Receipt) -> Response {
    let status = match receipt.status {
        ReceiptStatus::Accepted => StatusCode::ACCEPTED,
        ReceiptStatus::Completed => StatusCode::OK,
        ReceiptStatus::Failed => StatusCode::INTERNAL_SERVER_ERROR,
    };
    (
        status,
        Json(serde_json::to_value(receipt).unwrap_or(Value::Null)),
    )
        .into_response()
}

fn supervisor_error(err: SupervisorError) -> Response {
    let status = match &err {
        SupervisorError::NotFound(_) => StatusCode::NOT_FOUND,
        SupervisorError::Conflict(_) => StatusCode::CONFLICT,
        SupervisorError::Harness(_) => StatusCode::BAD_REQUEST,
        SupervisorError::Store(_) | SupervisorError::Driver(_) => StatusCode::INTERNAL_SERVER_ERROR,
    };
    error(status, &err.to_string())
}

fn error(status: StatusCode, message: &str) -> Response {
    (status, Json(json!({"error": message}))).into_response()
}

// ---- SSE ----

/// `GET /sessions/:id/events?after=<seq>` - replay from the store, then
/// live. SSE frames carry the event's `seq` as the id and its JSON as data.
async fn session_events(
    State(app): State<Arc<App>>,
    AxumPath(id): AxumPath<String>,
    axum::extract::RawQuery(query): axum::extract::RawQuery,
) -> Response {
    let Some(session_id) = SessionId::parse(&id) else {
        return error(StatusCode::BAD_REQUEST, "bad session id");
    };
    if app.supervisor.session(&session_id).is_err() {
        return error(StatusCode::NOT_FOUND, "no such session");
    }
    // A cursor that is present but unparseable is a broken client (a value
    // truncated by local storage, a mangled reconnect): answer 400 so the
    // bug surfaces, instead of treating it as a fresh connect and silently
    // replaying the whole history. Absent stays a full replay.
    let after = match query.as_deref().map(after_cursor) {
        Some(Cursor::Seq(seq)) => Some(seq),
        Some(Cursor::Invalid) => {
            return error(
                StatusCode::BAD_REQUEST,
                "the after cursor must be a seq number",
            );
        }
        Some(Cursor::Absent) | None => None,
    };
    let stream = crate::app::event_stream(Arc::clone(&app), session_id, after);
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "text/event-stream")
        .header(header::CACHE_CONTROL, "no-cache")
        .body(Body::from_stream(stream))
        .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
}

/// The `after` cursor: absent (a fresh connect - full replay), a valid seq,
/// or present-but-garbage. The third state is the whole point of the enum:
/// the old parser folded it into absent and silently replayed everything.
enum Cursor {
    Absent,
    Seq(u64),
    Invalid,
}

/// Minimal `after=<seq>` extraction without pulling a query-string crate
/// for one parameter. Present-but-unparseable is `Invalid`, not `Absent`.
fn after_cursor(query: &str) -> Cursor {
    let Some(pair) = query.split('&').find(|pair| {
        // The key is everything before the first `=`, present or not.
        pair.split('=').next().is_some_and(|key| key == "after")
    }) else {
        return Cursor::Absent;
    };
    match pair.split_once('=') {
        // `after` with no `=` at all: present but valueless, so invalid.
        None => Cursor::Invalid,
        Some((_, value)) => match value.parse() {
            Ok(seq) => Cursor::Seq(seq),
            Err(_) => Cursor::Invalid,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_absent_cursor_is_a_full_replay_and_a_garbage_one_is_invalid() {
        assert!(matches!(after_cursor(""), Cursor::Absent));
        assert!(matches!(after_cursor("x=1&y=2"), Cursor::Absent));
        assert!(matches!(after_cursor("after=41"), Cursor::Seq(41)));
        assert!(matches!(after_cursor("x=1&after=7"), Cursor::Seq(7)));
        // A present-but-garbage cursor must not fold into "absent": that
        // silently replayed the whole history to a struggling client.
        assert!(matches!(after_cursor("after=oops"), Cursor::Invalid));
        assert!(matches!(after_cursor("after="), Cursor::Invalid));
        assert!(matches!(after_cursor("after"), Cursor::Invalid));
        assert!(matches!(after_cursor("after=-1"), Cursor::Invalid));
        assert!(matches!(after_cursor("after=41x"), Cursor::Invalid));
    }
}
