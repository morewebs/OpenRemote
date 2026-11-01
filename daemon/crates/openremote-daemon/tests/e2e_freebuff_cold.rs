//! The real Freebuff plugin, cold: installed through the plugin routes, its
//! harness probed, a chat started, and the driver's own `initialized` (the
//! chat directory it found) reported back. No prompt is sent, so this costs
//! none of Freebuff's free allowance.
//!
//! It needs the CLI and the plugin folder, so it is ignored by default:
//!
//! ```text
//! set OPENREMOTE_FREEBUFF_PLUGIN=C:\path\to\OpenRemote-freebuff
//! cargo test -p openremote-daemon --test e2e_freebuff_cold -- --ignored
//! ```

mod common;

use std::path::Path;
use std::time::Duration;

use common::*;
use serde_json::json;

#[tokio::test]
#[ignore = "needs the Freebuff CLI and the plugin folder; starts the real TUI"]
async fn the_real_freebuff_plugin_installs_and_starts_a_cold_chat() {
    let plugin = std::env::var("OPENREMOTE_FREEBUFF_PLUGIN").expect(
        "set OPENREMOTE_FREEBUFF_PLUGIN to the OpenRemote-freebuff folder before running this",
    );
    assert!(
        Path::new(&plugin).join("openremote.json").is_file(),
        "OPENREMOTE_FREEBUFF_PLUGIN does not name a plugin folder: {plugin}"
    );

    let daemon = start_daemon_with_plugins(&[], &[]).await;

    let staged = call(
        &daemon,
        "POST",
        "/plugins/stage",
        Some(json!({
            "request_id": "fb-stage",
            "source": {"kind": "folder", "path": plugin}
        })),
    )
    .await;
    assert_eq!(staged.status, 200, "{}", staged.raw);
    let stage_id = staged.body["result"]["stage_id"]
        .as_str()
        .expect("stage id")
        .to_string();
    let installed = call(
        &daemon,
        "POST",
        &format!("/plugins/stage/{stage_id}/install"),
        Some(json!({"request_id": "fb-install"})),
    )
    .await;
    assert_eq!(installed.status, 200, "{}", installed.raw);

    let capabilities = call(&daemon, "GET", "/capabilities", None).await;
    let freebuff = capabilities.body["harnesses"]
        .as_array()
        .expect("harnesses")
        .iter()
        .find(|harness| harness["id"] == "freebuff")
        .expect("the installed Freebuff harness");
    assert_eq!(freebuff["available"], true, "{}", capabilities.raw);
    assert_eq!(freebuff["plugin"], "freebuff@local");
    assert_eq!(freebuff["name"], "Freebuff");

    let workspace = tempfile::tempdir().expect("workspace");
    let created = call(
        &daemon,
        "POST",
        "/sessions",
        Some(json!({
            "request_id": "fb-create",
            "harness": "freebuff",
            "workspace": workspace.path(),
        })),
    )
    .await;
    assert_eq!(created.status, 201, "raw: {}", created.raw);
    let id = created.body["id"].as_str().expect("session id").to_string();

    // The driver reports the chat directory it found on disk; that is the
    // conversation id a later `--continue` will be given.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
    let mut session_ref = String::new();
    while tokio::time::Instant::now() < deadline {
        let session = call(&daemon, "GET", &format!("/sessions/{id}"), None).await;
        if let Some(found) = session.body["harness_session_ref"].as_str() {
            if !found.is_empty() {
                session_ref = found.to_string();
                break;
            }
        }
        tokio::time::sleep(Duration::from_millis(300)).await;
    }
    assert!(
        !session_ref.is_empty(),
        "the Freebuff driver never reported its chat directory within 30s"
    );

    let stopped = call(
        &daemon,
        "POST",
        &format!("/sessions/{id}/stop"),
        Some(json!({"request_id": "fb-stop"})),
    )
    .await;
    assert_eq!(stopped.status, 200, "{}", stopped.raw);
    println!("freebuff cold chat {session_ref} started and stopped");
}
