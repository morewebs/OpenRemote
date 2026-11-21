//! An account-bearing request must fail before a default provider process can run.

mod common;

use common::*;
use serde_json::json;

#[tokio::test]
async fn the_desktop_console_can_preflight_account_updates() {
    let daemon = start_daemon(&[]).await;
    let reply = raw_http(
        &daemon,
        "OPTIONS",
        "/harnesses/codex/accounts/fdaf1dac-c4b1-441c-9daa-19fe60cf2594",
        &[
            ("Origin", "http://localhost:5175"),
            ("Access-Control-Request-Method", "PATCH"),
            (
                "Access-Control-Request-Headers",
                "authorization, content-type",
            ),
        ],
        None,
    )
    .await;
    assert_eq!(reply.status, 200, "{}", reply.raw);
    assert_eq!(
        reply.header("access-control-allow-origin"),
        Some("http://localhost:5175")
    );
    assert!(
        reply
            .header("access-control-allow-methods")
            .is_some_and(|methods| methods.split(',').any(|method| method.trim() == "PATCH")),
        "{}",
        reply.raw
    );
}

#[tokio::test]
async fn a_managed_account_request_never_falls_back_to_the_default() {
    let daemon = start_daemon(&[]).await;
    let missing = call(
        &daemon,
        "POST",
        "/sessions/with-app-account",
        Some(json!({"request_id": "missing", "harness": "codex"})),
    )
    .await;
    assert_eq!(missing.status, 400, "{}", missing.raw);

    let wrong_app = call(
        &daemon,
        "POST",
        "/sessions/with-app-account",
        Some(json!({
            "request_id": "wrong-app",
            "harness": "claude",
            "app_account_id": "fdaf1dac-c4b1-441c-9daa-19fe60cf2594"
        })),
    )
    .await;
    assert_eq!(wrong_app.status, 409, "{}", wrong_app.raw);
    assert!(wrong_app.raw.contains("accounts"), "{}", wrong_app.raw);

    let sessions = call(&daemon, "GET", "/sessions", None).await;
    assert!(sessions.body.as_array().unwrap().is_empty());
}
