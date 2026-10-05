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
    let authed = Router::new()
        .route("/capabilities", get(capabilities))
        .route("/harnesses/{id}/models", get(harness_models))
        .route(
            "/harnesses/{id}/signin",
            post(start_sign_in).get(sign_in_view),
        )
        .route("/harnesses/{id}/signin/input", post(sign_in_input))
        .route("/harnesses/{id}/signin/stop", post(stop_sign_in))
        .route("/machines", get(list_machines).post(create_machine))
        .route("/machines/{id}", get(get_machine).delete(remove_machine))
        .route("/machines/{id}/harnesses", post(install_harness))
        .route("/plugins", get(list_plugins).post(create_plugin))
        .route("/plugins/marketplace", get(plugin_marketplace))
        .route("/plugins/{id}", axum::routing::delete(remove_plugin))
        .route("/plugins/{id}/enabled", post(set_plugin_enabled))
        .route("/plugins/{id}/key", post(acknowledge_plugin_key))
        .route("/automations", get(list_rules).post(save_rule))
        .route("/automations/{id}", axum::routing::delete(remove_rule))
        .route("/automations/{id}/enabled", post(set_rule_enabled))
        .route("/automations/{id}/run", post(run_rule))
        .route("/sessions", get(list_sessions).post(create_session))
        .route("/sessions/{id}", get(get_session))
        .route("/sessions/{id}/events", get(session_events))
        .route("/sessions/{id}/decisions", get(list_decisions))
        .route("/sessions/{id}/prompts", post(post_prompt))
        .route("/sessions/{id}/settings", post(post_settings))
        .route("/sessions/{id}/interrupt", post(post_interrupt))
        .route("/sessions/{id}/stop", post(post_stop))
        .route("/sessions/{id}/resume", post(post_resume))
        .route("/decisions/{id}/answer", post(post_answer))
        .route("/receipts/{request_id}", get(get_receipt))
        .layer(middleware::from_fn_with_state(Arc::clone(&app), auth));
    Router::new()
        .route("/healthz", get(healthz))
        // Webhooks arrive from outside the console's trust - the rule's
        // own key is the credential, not the daemon token. (The loopback
        // binding still keeps the surface local.)
        .route("/hooks/{id}", post(incoming_webhook))
        .merge(authed)
        .layer(console_cors())
        .with_state(app)
}

/// The console is a webview on a different origin than the daemon
/// (vite dev `http://localhost:5173`, Tauri production
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

async fn auth(State(app): State<Arc<App>>, request: Request, next: Next) -> Response {
    let expected = format!("Bearer {}", app.token);
    let header = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default();
    if header == expected {
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
/// real answer: the console's model slot stays reserved for that harness.
async fn harness_models(State(app): State<Arc<App>>, AxumPath(id): AxumPath<String>) -> Response {
    let models = app.supervisor.models(&id).await;
    Json(models).into_response()
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

async fn list_machines(State(app): State<Arc<App>>) -> Response {
    Json(app.supervisor.machines()).into_response()
}

async fn get_machine(State(app): State<Arc<App>>, AxumPath(id): AxumPath<String>) -> Response {
    match MachineId::parse(&id) {
        Some(id) => match app.supervisor.machine(&id) {
            Ok(view) => Json(view).into_response(),
            Err(err) => supervisor_error(err),
        },
        None => error(StatusCode::BAD_REQUEST, "bad machine id"),
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
}

// ---- plugins ----

async fn list_plugins(State(app): State<Arc<App>>) -> Response {
    Json(app.supervisor.plugins()).into_response()
}

async fn plugin_marketplace() -> Response {
    Json(crate::plugins::marketplace()).into_response()
}

#[derive(Deserialize)]
struct CreatePluginBody {
    request_id: String,
    machine: String,
    #[serde(default)]
    catalog_id: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    detail: Option<String>,
    #[serde(default)]
    command: Option<String>,
    #[serde(default)]
    needs_key: bool,
}

async fn create_plugin(
    State(app): State<Arc<App>>,
    Json(body): Json<CreatePluginBody>,
) -> Response {
    if let Some(receipt) = app.supervisor.receipt(&body.request_id) {
        return receipt_response(&receipt);
    }
    let Some(machine_id) = MachineId::parse(&body.machine) else {
        return error(StatusCode::BAD_REQUEST, "bad machine id");
    };
    // A marketplace install names the entry; a custom one carries its own
    // name, detail, and launch command.
    let custom = match (
        body.catalog_id.is_none(),
        &body.name,
        &body.detail,
        &body.command,
    ) {
        (true, Some(name), Some(detail), Some(command)) => {
            if name.trim().is_empty() || command.trim().is_empty() {
                return error(
                    StatusCode::UNPROCESSABLE_ENTITY,
                    "a custom plugin needs a name and a launch command",
                );
            }
            Some((name.trim(), detail.trim(), command.trim(), body.needs_key))
        }
        (true, _, _, _) => {
            return error(
                StatusCode::UNPROCESSABLE_ENTITY,
                "a custom plugin needs a name, a detail, and a launch command",
            );
        }
        _ => None,
    };
    match app.supervisor.install_plugin(
        &machine_id,
        body.catalog_id.as_deref(),
        custom.as_ref().map(|(n, d, c, k)| (*n, *d, *c, *k)),
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

async fn remove_plugin(
    State(app): State<Arc<App>>,
    AxumPath(id): AxumPath<String>,
    axum::extract::Query(query): axum::extract::Query<MachineActionQuery>,
) -> Response {
    if let Some(receipt) = app.supervisor.receipt(&query.request_id) {
        return receipt_response(&receipt);
    }
    match app.supervisor.remove_plugin(&id) {
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
struct PluginEnabledBody {
    request_id: String,
    enabled: bool,
}

async fn set_plugin_enabled(
    State(app): State<Arc<App>>,
    AxumPath(id): AxumPath<String>,
    Json(body): Json<PluginEnabledBody>,
) -> Response {
    if let Some(receipt) = app.supervisor.receipt(&body.request_id) {
        return receipt_response(&receipt);
    }
    match app.supervisor.set_plugin_enabled(&id, body.enabled) {
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
struct PluginKeyBody {
    request_id: String,
}

async fn acknowledge_plugin_key(
    State(app): State<Arc<App>>,
    AxumPath(id): AxumPath<String>,
    Json(body): Json<PluginKeyBody>,
) -> Response {
    if let Some(receipt) = app.supervisor.receipt(&body.request_id) {
        return receipt_response(&receipt);
    }
    match app.supervisor.acknowledge_plugin_key(&id) {
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
    Json(body): Json<PluginEnabledBody>,
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
    Json(body): Json<PluginKeyBody>,
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
/// credential: `/hooks/{rule-id}?key=…`.
async fn incoming_webhook(
    State(app): State<Arc<App>>,
    AxumPath(id): AxumPath<String>,
    axum::extract::Query(query): axum::extract::Query<WebhookQuery>,
) -> Response {
    let Some(rule_id) = openremote_core::RuleId::parse(&id) else {
        return error(StatusCode::BAD_REQUEST, "bad rule id");
    };
    match app
        .supervisor
        .fire_webhook(&rule_id, query.key.as_deref())
        .await
    {
        Ok(session) => (
            StatusCode::OK,
            Json(json!({"fired": true, "chat": session.id.to_string()})),
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
    // npm runs minutes, not milliseconds - accepted first, terminal state
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
        .install_harness(&machine_id, &body.harness)
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
    workspace: String,
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
}

async fn create_session(
    State(app): State<Arc<App>>,
    Json(body): Json<CreateSessionBody>,
) -> Response {
    if let Some(receipt) = app.supervisor.receipt(&body.request_id) {
        return receipt_response(&receipt);
    }
    let workspace = std::path::PathBuf::from(&body.workspace);
    // Workspace-root containment: absolute, existing, a directory.
    if !workspace.is_absolute() || !workspace.is_dir() {
        return error(
            StatusCode::UNPROCESSABLE_ENTITY,
            "workspace must be an absolute path to an existing directory",
        );
    }
    match app
        .supervisor
        .create_session(
            &body.harness,
            workspace,
            body.model,
            body.effort,
            body.permission_mode,
            body.fast,
        )
        .await
    {
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
            (StatusCode::CREATED, Json(session)).into_response()
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
    text: String,
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
}

/// Model, effort, and fast on a live chat. A harness that only accepts the
/// change at process start answers 409 while the chat is running.
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
    match app.supervisor.prompt(&session_id, &body.text).await {
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

fn receipt_response(receipt: &Receipt) -> Response {
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
    let after = query.and_then(|q| serde_urlencoded_form_from(&q));
    let stream = crate::app::event_stream(Arc::clone(&app), session_id, after);
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "text/event-stream")
        .header(header::CACHE_CONTROL, "no-cache")
        .body(Body::from_stream(stream))
        .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
}

/// Minimal `after=<seq>` extraction without pulling a query-string crate
/// for one parameter.
fn serde_urlencoded_form_from(query: &str) -> Option<u64> {
    query.split('&').find_map(|pair| {
        let (key, value) = pair.split_once('=')?;
        (key == "after").then(|| value.parse().ok())?
    })
}
