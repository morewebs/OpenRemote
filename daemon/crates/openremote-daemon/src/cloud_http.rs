//! Cloud mode's console-facing routes: sign in with moreweb, sign out, and
//! the state the Cloud area renders. Signing in happens in the system
//! browser; moreweb sends the browser back to `/cloud/callback` here.

use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, Method, StatusCode, Uri, header};
use axum::response::{IntoResponse, Response};
use openremote_cloud::registry::RegistryError;
use openremote_cloud::{CloudError, MeshError};
use openremote_core::DeviceId;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::app::App;

pub async fn view(State(app): State<Arc<App>>) -> Response {
    Json(app.cloud.view()).into_response()
}

/// Starts a sign-in and hands back the page to open in the browser.
pub async fn sign_in(State(app): State<Arc<App>>) -> Response {
    let Some(port) = app.port.get().copied() else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error": "the daemon isn't listening yet"})),
        )
            .into_response();
    };
    let url = app.cloud.begin_signin(port);
    Json(json!({"authorize_url": url})).into_response()
}

pub async fn cancel_sign_in(State(app): State<Arc<App>>) -> Response {
    app.cloud.cancel_signin();
    Json(app.cloud.view()).into_response()
}

/// Signs this device out. Its synced chats stay on this computer; they stop
/// syncing until it signs in again.
pub async fn sign_out(State(app): State<Arc<App>>) -> Response {
    match app.cloud.signout().await {
        Ok(()) => Json(app.cloud.view()).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

#[derive(Deserialize)]
pub struct CallbackQuery {
    code: Option<String>,
    state: Option<String>,
    error: Option<String>,
}

/// The tab the browser is left on. It holds nothing from the sign-in.
fn page(status: StatusCode, title: &str, line: &str) -> Response {
    let html = format!(
        "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\"><title>{title} · OpenRemote</title>\
         <style>body{{margin:0;min-height:100vh;display:grid;place-content:center;background:#0a0a0c;color:#eeeef0;\
         font:15px/1.6 system-ui,sans-serif;text-align:center}}h1{{font-size:20px;font-weight:600;margin:0 0 6px}}\
         p{{margin:0;color:#aaaab5}}</style></head><body><main><h1>{title}</h1><p>{line}</p></main></body></html>"
    );
    (
        status,
        [
            (header::CONTENT_TYPE, "text/html; charset=utf-8"),
            (
                header::CONTENT_SECURITY_POLICY,
                "default-src 'none'; style-src 'unsafe-inline'",
            ),
            (header::REFERRER_POLICY, "no-referrer"),
            (header::CACHE_CONTROL, "no-store"),
        ],
        html,
    )
        .into_response()
}

pub async fn callback(State(app): State<Arc<App>>, Query(q): Query<CallbackQuery>) -> Response {
    if q.error.is_some() {
        app.cloud.cancel_signin();
        return page(
            StatusCode::OK,
            "Sign-in cancelled",
            "Nothing changed. You can close this tab.",
        );
    }
    let (Some(code), Some(state)) = (q.code, q.state) else {
        return page(
            StatusCode::BAD_REQUEST,
            "Sign-in didn't finish",
            "Go back to OpenRemote and try again.",
        );
    };
    match app.cloud.finish_signin(&code, &state).await {
        Ok(signed_in) => {
            if let Ok(mut store) = app.store.lock() {
                // Another account's synced chats don't belong to this one.
                if signed_in.previous_account.is_some() {
                    let _ = store.purge_synced();
                }
                let _ = store.set_this_device(Some(signed_in.device_id));
            }
            app.start_cloud();
            page(
                StatusCode::OK,
                "You're signed in",
                &format!(
                    "OpenRemote Cloud is on for {}. You can close this tab.",
                    escape(&signed_in.account.email)
                ),
            )
        }
        Err(e) => page(
            StatusCode::BAD_REQUEST,
            "Sign-in didn't finish",
            &format!(
                "{}. Go back to OpenRemote and try again.",
                escape(&e.to_string())
            ),
        ),
    }
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn json_error(status: StatusCode, message: &str) -> Response {
    (status, Json(json!({"error": message}))).into_response()
}

fn cloud_error(e: CloudError) -> Response {
    match e {
        CloudError::Registry(e) => registry_error(e),
        CloudError::SignedOut | CloudError::OAuth(_) => json_error(
            StatusCode::UNAUTHORIZED,
            "sign in to OpenRemote Cloud again to change your devices",
        ),
        other => json_error(StatusCode::INTERNAL_SERVER_ERROR, &other.to_string()),
    }
}

fn registry_error(e: RegistryError) -> Response {
    match e {
        RegistryError::Unauthorized => json_error(StatusCode::UNAUTHORIZED, &e.to_string()),
        RegistryError::Revoked => json_error(StatusCode::GONE, &e.to_string()),
        RegistryError::Refused {
            status, message, ..
        } => json_error(
            StatusCode::from_u16(status).unwrap_or(StatusCode::BAD_GATEWAY),
            &message,
        ),
        RegistryError::Http(e) => json_error(StatusCode::BAD_GATEWAY, &e.to_string()),
    }
}

/// The account's devices with who is online, from this device's view.
pub async fn devices(State(app): State<Arc<App>>) -> Response {
    let Some(mesh) = app.cloud.mesh() else {
        return Json(json!({"devices": [], "auto_update": true})).into_response();
    };
    let me = mesh.me();
    let devices: Vec<Value> = mesh
        .peers()
        .into_iter()
        .map(|peer| {
            let mut value = serde_json::to_value(&peer).unwrap_or(Value::Null);
            value["this_device"] = json!(peer.id == me);
            value
        })
        .collect();
    Json(json!({"devices": devices, "auto_update": mesh.auto_update()})).into_response()
}

#[derive(Deserialize)]
pub struct KindBody {
    kind: String,
}

/// Makes a desktop a machine (or back): this one or another of the user's.
pub async fn set_kind(
    State(app): State<Arc<App>>,
    Path(id): Path<String>,
    Json(body): Json<KindBody>,
) -> Response {
    if !matches!(body.kind.as_str(), "desktop" | "machine") {
        return json_error(StatusCode::BAD_REQUEST, "kind is desktop or machine");
    }
    let token = match app.cloud.account_token().await {
        Ok(token) => token,
        Err(e) => return cloud_error(e),
    };
    match app
        .cloud
        .registry()
        .patch(&token, &id, &json!({"kind": body.kind}))
        .await
    {
        Ok(device) => {
            if let Some(mesh) = app.cloud.mesh() {
                mesh.refresh_now();
            }
            Json(device).into_response()
        }
        Err(e) => registry_error(e),
    }
}

/// Removes a device from the account. Removing this one leaves Cloud here:
/// its synced chats go, its private chats stay.
pub async fn remove_device(State(app): State<Arc<App>>, Path(id): Path<String>) -> Response {
    if app.cloud.device_id().is_some_and(|me| me.to_string() == id) {
        if let Err(e) = app.cloud.leave().await {
            return cloud_error(e);
        }
        if let Ok(mut store) = app.store.lock() {
            let _ = store.purge_synced();
        }
        return Json(app.cloud.view()).into_response();
    }
    let token = match app.cloud.account_token().await {
        Ok(token) => token,
        Err(e) => return cloud_error(e),
    };
    match app.cloud.registry().delete(&token, &id).await {
        Ok(()) => {
            if let Some(mesh) = app.cloud.mesh() {
                mesh.refresh_now();
            }
            StatusCode::NO_CONTENT.into_response()
        }
        Err(e) => registry_error(e),
    }
}

/// How long an answer may take: installing a harness runs a real installer.
fn timeout_for(method: &Method, rest: &str) -> std::time::Duration {
    let secs = if method == Method::POST
        && rest.starts_with("machines/")
        && rest.ends_with("/harnesses")
    {
        900
    } else if method == Method::POST && rest == "sessions" {
        90
    } else {
        30
    };
    std::time::Duration::from_secs(secs)
}

/// The console's request to another device, carried over the mesh and
/// answered by that device's own API (through its gate).
pub async fn passthrough(
    State(app): State<Arc<App>>,
    Path((id, rest)): Path<(String, String)>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Response {
    let Some(mesh) = app.cloud.mesh() else {
        return json_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "not connected to OpenRemote Cloud",
        );
    };
    let Some(device) = DeviceId::parse(&id) else {
        return json_error(StatusCode::NOT_FOUND, "no such device");
    };
    let path = match uri.query() {
        Some(q) => format!("/{rest}?{q}"),
        None => format!("/{rest}"),
    };
    let request = openremote_cloud::rpc::Request {
        method: method.to_string(),
        path,
        content_type: headers
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .map(str::to_string),
        body: body.to_vec(),
    };
    match mesh
        .request(device, request, timeout_for(&method, &rest))
        .await
    {
        Ok(response) => {
            let status = StatusCode::from_u16(response.status).unwrap_or(StatusCode::BAD_GATEWAY);
            let content_type = response
                .content_type
                .unwrap_or_else(|| "application/octet-stream".into());
            (
                status,
                [(header::CONTENT_TYPE, content_type)],
                response.body,
            )
                .into_response()
        }
        Err(e @ MeshError::Offline(_)) => {
            json_error(StatusCode::SERVICE_UNAVAILABLE, &e.to_string())
        }
        Err(e @ (MeshError::Unknown | MeshError::KeyChanged(_))) => {
            json_error(StatusCode::NOT_FOUND, &e.to_string())
        }
        Err(e) => json_error(StatusCode::BAD_GATEWAY, &e.to_string()),
    }
}
