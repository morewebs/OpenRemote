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
use openremote_core::{DecisionId, Receipt, ReceiptStatus, SessionId};
use serde::Deserialize;
use serde_json::{Value, json};
use tower_http::cors::{AllowOrigin, CorsLayer};

use crate::app::App;
use crate::supervisor::SupervisorError;

pub fn router(app: Arc<App>) -> Router {
    let authed = Router::new()
        .route("/capabilities", get(capabilities))
        .route("/harnesses/{id}/models", get(harness_models))
        .route("/sessions", get(list_sessions).post(create_session))
        .route("/sessions/{id}", get(get_session))
        .route("/sessions/{id}/events", get(session_events))
        .route("/sessions/{id}/decisions", get(list_decisions))
        .route("/sessions/{id}/prompts", post(post_prompt))
        .route("/sessions/{id}/interrupt", post(post_interrupt))
        .route("/sessions/{id}/stop", post(post_stop))
        .route("/sessions/{id}/resume", post(post_resume))
        .route("/decisions/{id}/answer", post(post_answer))
        .route("/receipts/{request_id}", get(get_receipt))
        .layer(middleware::from_fn_with_state(Arc::clone(&app), auth));
    Router::new()
        .route("/healthz", get(healthz))
        .merge(authed)
        .layer(console_cors())
        .with_state(app)
}

/// The console is a webview on a different origin than the daemon
/// (vite dev `http://localhost:5173`, Tauri production
/// `http://tauri.localhost` / `tauri://localhost`), and webviews enforce
/// same-origin on fetch — without these headers the console can never
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
        .allow_methods([Method::GET, Method::POST])
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

#[derive(Deserialize)]
struct CreateSessionBody {
    request_id: String,
    harness: String,
    workspace: String,
    #[serde(default)]
    model: Option<String>,
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
    // — surfaced to the console as unknown work, never resent.
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
        // A request the daemon has no record of is unknown — that is the
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

/// `GET /sessions/:id/events?after=<seq>` — replay from the store, then
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
