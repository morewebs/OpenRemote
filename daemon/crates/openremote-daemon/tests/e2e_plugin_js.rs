//! A JavaScript plugin's routes (plan step 6): what its sandboxed frame
//! reaches through the console. Three promises are held here:
//!
//! - what a plugin may do comes from its install record, never from its
//!   folder as it is now;
//! - a project's plugin reaches only that project's chats;
//! - the bridge's calls are a closed list, and the network is the internet
//!   (or an address the user typed), never this machine at the plugin's
//!   own choice.

mod common;

use std::path::Path;

use common::*;
use serde_json::{Value, json};

/// A JavaScript plugin's folder: a page, a composer button and a view,
/// asking for `powers`.
fn js_plugin(name: &str, powers: &[&str]) -> tempfile::TempDir {
    let folder = tempfile::tempdir().expect("plugin folder");
    std::fs::create_dir_all(folder.path().join("ui")).unwrap();
    std::fs::write(folder.path().join("main.js"), "openremote.ui.toast('hi')").unwrap();
    std::fs::write(folder.path().join("ui/view.html"), "<p>view</p>").unwrap();
    std::fs::write(
        folder.path().join("openremote.json"),
        json!({
            "api": 1,
            "name": name,
            "runtime": "js",
            "entry": "main.js",
            "powers": powers,
            "contributes": {
                "composer_inputs": [{"id": "say", "label": "Say", "icon": "microphone"}],
                "views": [{"id": "board", "title": "Board", "place": "sidebar", "entry": "ui/view.html"}],
                "settings": [
                    {"key": "base_url", "type": "string", "label": "Address", "default": "https://api.example.com/v1"},
                    {"key": "api_key", "type": "secret", "label": "Key", "sendTo": "base_url"}
                ]
            }
        })
        .to_string(),
    )
    .unwrap();
    folder
}

fn rid() -> String {
    uuid::Uuid::new_v4().to_string()
}

async fn install(daemon: &TestDaemon, folder: &Path, extra: Value) -> String {
    let staged = call(
        daemon,
        "POST",
        "/plugins/stage",
        Some(json!({"request_id": rid(), "source": {"kind": "folder", "path": folder}})),
    )
    .await;
    assert_eq!(staged.status, 200, "{}", staged.raw);
    let stage_id = staged.body["result"]["stage_id"].as_str().unwrap();
    let mut body = json!({"request_id": rid()});
    for (k, v) in extra.as_object().unwrap() {
        body[k] = v.clone();
    }
    let installed = call(
        daemon,
        "POST",
        &format!("/plugins/stage/{stage_id}/install"),
        Some(body),
    )
    .await;
    assert_eq!(installed.status, 200, "{}", installed.raw);
    installed.body["result"]["id"].as_str().unwrap().to_string()
}

async fn project(daemon: &TestDaemon, folder: &Path) -> String {
    let saved = call(
        daemon,
        "POST",
        "/projects",
        Some(json!({"request_id": rid(), "folders": [folder]})),
    )
    .await;
    assert_eq!(saved.status, 201, "{}", saved.raw);
    saved.body["id"].as_str().unwrap().to_string()
}

async fn chat_in(daemon: &TestDaemon, folder: &Path) -> String {
    let created = call(
        daemon,
        "POST",
        "/sessions",
        Some(json!({"request_id": rid(), "harness": "claude", "workspace": folder})),
    )
    .await;
    assert_eq!(created.status, 201, "{}", created.raw);
    created.body["id"].as_str().unwrap().to_string()
}

fn path_of(id: &str) -> String {
    id.replace('~', "%7E")
}

async fn bridge(daemon: &TestDaemon, id: &str, method: &str, params: Value) -> Reply {
    call(
        daemon,
        "POST",
        &format!("/plugins/{}/call", path_of(id)),
        Some(json!({"method": method, "params": params})),
    )
    .await
}

async fn fetch(daemon: &TestDaemon, id: &str, request: Value) -> Reply {
    call(
        daemon,
        "POST",
        &format!("/plugins/{}/fetch", path_of(id)),
        Some(request),
    )
    .await
}

#[tokio::test]
async fn a_project_plugin_reaches_only_its_own_projects_chats() {
    let daemon = start_daemon(&[]).await;
    let (repo_a, repo_b, loose) = (workspace(None), workspace(None), workspace(None));
    let a = project(&daemon, repo_a.path()).await;
    let _b = project(&daemon, repo_b.path()).await;
    let folder = js_plugin("board", &["read_chats", "agent_tools"]);
    let id = install(
        &daemon,
        folder.path(),
        json!({"scope": "project_local", "project": a}),
    )
    .await;
    let (in_a, in_b, elsewhere) = (
        chat_in(&daemon, repo_a.path()).await,
        chat_in(&daemon, repo_b.path()).await,
        chat_in(&daemon, loose.path()).await,
    );

    // The list holds its project's chats and no other.
    let listed = bridge(&daemon, &id, "chats/list", json!({})).await;
    assert_eq!(listed.status, 200, "{}", listed.raw);
    let ids: Vec<&str> = listed.body["result"]["chats"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["id"].as_str().unwrap())
        .collect();
    assert_eq!(ids, [in_a.as_str()], "{}", listed.raw);

    // A chat outside it is neither read, steered nor stopped.
    for chat in [&in_b, &elsewhere] {
        for (method, params) in [
            ("chats/read", json!({"chat": chat})),
            ("chats/prompt", json!({"chat": chat, "text": "hi"})),
            ("chats/stop", json!({"chat": chat})),
        ] {
            let refused = bridge(&daemon, &id, method, params).await;
            assert_eq!(refused.status, 403, "{method}: {}", refused.raw);
            assert!(refused.raw.contains("outside"), "{}", refused.raw);
        }
    }
    let read = bridge(&daemon, &id, "chats/read", json!({"chat": in_a})).await;
    assert_eq!(read.status, 200, "{}", read.raw);

    // Nor does it start one elsewhere.
    for workspace in [repo_b.path(), loose.path()] {
        let refused = bridge(
            &daemon,
            &id,
            "chats/start",
            json!({"harness": "claude", "workspace": workspace}),
        )
        .await;
        assert_eq!(refused.status, 403, "{}", refused.raw);
    }
    let sessions = call(&daemon, "GET", "/sessions", None).await;
    assert_eq!(
        sessions.body.as_array().map(Vec::len).unwrap_or(3),
        3,
        "{}",
        sessions.raw
    );

    // A Machine install of the same powers reaches every chat.
    let other = js_plugin("everywhere", &["read_chats"]);
    let machine = install(&daemon, other.path(), json!({"scope": "machine"})).await;
    let listed = bridge(&daemon, &machine, "chats/list", json!({})).await;
    assert_eq!(listed.body["result"]["chats"].as_array().unwrap().len(), 3);
}

#[tokio::test]
async fn powers_come_from_the_install_record_not_the_folder() {
    let daemon = start_daemon(&[]).await;
    let folder = js_plugin("quiet", &[]);
    let id = install(&daemon, folder.path(), json!({"scope": "machine"})).await;
    let listed = call(&daemon, "GET", "/plugins", None).await;
    let record = listed.body["installed"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["id"] == id.as_str())
        .unwrap()
        .clone();
    // The console reads the page and the record's own contributions.
    assert_eq!(record["entry"], "main.js", "{record}");
    assert_eq!(record["powers"], json!([]), "{record}");
    assert_eq!(record["contributes"]["views"][0]["entry"], "ui/view.html");

    // Every chat call and the network need a power it was not given.
    for method in [
        "chats/list",
        "chats/read",
        "chats/start",
        "chats/prompt",
        "chats/stop",
    ] {
        let refused = bridge(&daemon, &id, method, json!({})).await;
        assert_eq!(refused.status, 403, "{method}: {}", refused.raw);
    }
    let request = json!({"url": "https://example.com/", "method": "GET"});
    assert_eq!(fetch(&daemon, &id, request.clone()).await.status, 403);

    // Its folder, edited after the review to ask for everything, changes
    // nothing: the record is what the user approved.
    let data_folder = daemon
        .data_dir()
        .join("plugins/local/quiet/openremote.json");
    assert!(
        data_folder.is_file(),
        "the installed copy is where the daemon keeps it"
    );
    let mut manifest: Value =
        serde_json::from_str(&std::fs::read_to_string(&data_folder).unwrap()).unwrap();
    manifest["powers"] = json!(["read_chats", "agent_tools", "network", "microphone"]);
    std::fs::write(&data_folder, manifest.to_string()).unwrap();
    assert_eq!(fetch(&daemon, &id, request).await.status, 403);
    assert_eq!(
        bridge(&daemon, &id, "chats/list", json!({})).await.status,
        403
    );

    // Off, it reaches nothing at all.
    let off = call(
        &daemon,
        "POST",
        &format!("/plugins/{}/enabled", path_of(&id)),
        Some(json!({"request_id": rid(), "enabled": false})),
    )
    .await;
    assert_eq!(off.status, 200, "{}", off.raw);
    assert_eq!(
        bridge(&daemon, &id, "chats/list", json!({})).await.status,
        403
    );
}

#[tokio::test]
async fn the_bridge_offers_a_closed_list_and_the_network_is_not_this_machine() {
    let daemon = start_daemon(&[]).await;
    let folder = js_plugin("net", &["network", "read_chats", "agent_tools"]);
    let id = install(&daemon, folder.path(), json!({"scope": "machine"})).await;

    // Anything not on the list is refused, whatever powers the plugin has.
    for method in [
        "sessions/create",
        "settings/set",
        "plugins/remove",
        "fs/read",
        "chats/delete",
        "",
    ] {
        let refused = bridge(&daemon, &id, method, json!({})).await;
        assert_eq!(refused.status, 404, "{method}: {}", refused.raw);
    }

    // The network is the internet: never this machine at the plugin's own
    // choice, even the daemon itself.
    let me = format!("http://{}", daemon.addr);
    for url in [
        format!("{me}/plugins"),
        "http://localhost:1/".to_string(),
        "http://10.0.0.1/".to_string(),
        "http://169.254.169.254/latest/meta-data/".to_string(),
    ] {
        let refused = fetch(&daemon, &id, json!({"url": url, "method": "GET"})).await;
        assert_eq!(refused.status, 403, "{url}: {}", refused.raw);
    }
    for (request, words) in [
        (json!({"url": "file:///etc/passwd"}), "http and https"),
        (
            json!({"url": "https://example.com", "method": "TRACE"}),
            "TRACE",
        ),
        (
            json!({"url": "https://example.com", "headers": [["x", "{{secret:nope}}"]]}),
            "secrets",
        ),
    ] {
        let refused = fetch(&daemon, &id, request.clone()).await;
        assert_eq!(refused.status, 400, "{request}: {}", refused.raw);
        assert!(refused.raw.contains(words), "{}", refused.raw);
    }

    // A secret goes only toward its address, and its value never comes back.
    let saved = call(
        &daemon,
        "POST",
        &format!("/plugins/{}/settings", path_of(&id)),
        Some(json!({"values": {"api_key": "sk-private-1", "base_url": "https://api.example.com/v1"}})),
    )
    .await;
    assert_eq!(saved.status, 200, "{}", saved.raw);
    assert!(!saved.raw.contains("sk-private-1"));
    let refused = fetch(
        &daemon,
        &id,
        json!({"url": "https://elsewhere.example.net/", "headers": [["authorization", "Bearer {{secret:api_key}}"]]}),
    )
    .await;
    assert_eq!(refused.status, 403, "{}", refused.raw);
    assert!(!refused.raw.contains("sk-private-1"), "{}", refused.raw);

    // A process or WASM plugin has no frame, and so none of these routes.
    let wasm = call(
        &daemon,
        "POST",
        "/plugins/nothing@local/call",
        Some(json!({"method": "chats/list"})),
    )
    .await;
    assert_eq!(wasm.status, 404, "{}", wasm.raw);
}

#[tokio::test]
async fn the_frames_routes_answer_this_devices_console_only() {
    use std::sync::Arc;
    use tower::ServiceExt;
    let (_daemon, app) = start_embedded(dead_cloud()).await;
    let peer = openremote_daemon::http::peer_router(Arc::clone(&app));
    for path in ["/plugins/x@local/call", "/plugins/x@local/fetch"] {
        let request = axum::http::Request::builder()
            .method("POST")
            .uri(path)
            .header("content-type", "application/json")
            .body(axum::body::Body::from("{}"))
            .unwrap();
        let response = peer.clone().oneshot(request).await.unwrap();
        assert!(
            matches!(response.status().as_u16(), 403 | 404 | 405),
            "{path}: {}",
            response.status()
        );
    }
    let daemon = start_daemon(&[]).await;
    let reply = call_with_token(
        &daemon,
        "wrong",
        "POST",
        "/plugins/x@local/call",
        Some(json!({"method": "chats/list"})),
    )
    .await;
    assert_eq!(reply.status, 401);
}
