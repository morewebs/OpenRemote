//! Cloud mode's console-facing routes: sign in with moreweb, sign out, and
//! the state the Cloud area renders. Signing in happens in the system
//! browser; moreweb sends the browser back to `/cloud/callback` here.

use std::sync::Arc;

use axum::Json;
use axum::extract::{Query, State};
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde::Deserialize;
use serde_json::json;

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
