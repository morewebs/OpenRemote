//! Plugins e2e: the marketplace, installs on this machine (dedup, custom,
//! needs-key lifecycle), and the ride-along — a session with plugins
//! enabled still spawns and turns through the real fixture path.

mod common;

use common::*;
use serde_json::json;

async fn this_machine_id(daemon: &TestDaemon) -> String {
    let machines = call(daemon, "GET", "/machines", None).await;
    machines
        .body
        .as_array()
        .expect("machines")
        .iter()
        .find(|m| m["machine"]["this_machine"] == json!(true))
        .expect("this machine")["machine"]["id"]
        .as_str()
        .expect("machine id")
        .to_string()
}

// Every install mints its own request_id — the receipt contract dedups by
// it, so a reused id answers with the recorded outcome instead of running.
fn install_body(machine: &str, over: serde_json::Value) -> serde_json::Value {
    let mut body = json!({"request_id": uuid::Uuid::new_v4().to_string(), "machine": machine});
    let obj = body.as_object_mut().unwrap();
    for (key, value) in over.as_object().unwrap() {
        obj.insert(key.clone(), value.clone());
    }
    body
}

#[tokio::test]
async fn the_marketplace_installs_and_lives_its_lifecycle() {
    let daemon = start_daemon(&[]).await;
    let machine = this_machine_id(&daemon).await;

    // The catalog is facts: ids, launch commands, key needs.
    let market = call(&daemon, "GET", "/plugins/marketplace", None).await;
    assert_eq!(market.status, 200, "raw: {}", market.raw);
    let entries = market.body.as_array().expect("entries");
    assert_eq!(entries.len(), 7);
    let github = entries
        .iter()
        .find(|e| e["id"] == "github")
        .expect("github");
    assert_eq!(
        github["command"].as_str().expect("command"),
        "npx -y @modelcontextprotocol/server-github"
    );
    assert_eq!(github["needs_key"], false);

    // Install github: it rides immediately (no key needed).
    let installed = call(
        &daemon,
        "POST",
        "/plugins",
        Some(install_body(&machine, json!({"catalog_id": "github"}))),
    )
    .await;
    assert_eq!(installed.status, 201, "raw: {}", installed.raw);
    assert_eq!(installed.body["id"], format!("github@{machine}"));
    assert_eq!(installed.body["enabled"], true);
    assert_eq!(installed.body["needs_key"], false);
    assert_eq!(installed.body["has_key"], false);

    // Install gitlab: it waits on its key.
    let needs_key = call(
        &daemon,
        "POST",
        "/plugins",
        Some(install_body(&machine, json!({"catalog_id": "gitlab"}))),
    )
    .await;
    assert_eq!(needs_key.status, 201, "raw: {}", needs_key.raw);

    // Duplicates never install twice.
    let dup = call(
        &daemon,
        "POST",
        "/plugins",
        Some(install_body(&machine, json!({"catalog_id": "github"}))),
    )
    .await;
    assert_eq!(dup.status, 409, "raw: {}", dup.raw);

    // Disable: github goes off.
    let github_id = format!("github@{machine}");
    let off = call(
        &daemon,
        "POST",
        &format!("/plugins/{github_id}/enabled"),
        Some(json!({"request_id": "p-off", "enabled": false})),
    )
    .await;
    assert_eq!(off.status, 200, "raw: {}", off.raw);
    assert_eq!(off.body["enabled"], false);

    // The key acknowledgment: gitlab has one after this (the key itself
    // never crosses the API).
    let gitlab_id = format!("gitlab@{machine}");
    let key = call(
        &daemon,
        "POST",
        &format!("/plugins/{gitlab_id}/key"),
        Some(json!({"request_id": "p-key"})),
    )
    .await;
    assert_eq!(key.status, 200, "raw: {}", key.raw);
    assert_eq!(key.body["has_key"], true);

    // GitHub doesn't need a key — an explicit error, not a silent ok.
    let no_key = call(
        &daemon,
        "POST",
        &format!("/plugins/{github_id}/key"),
        Some(json!({"request_id": "p-nokey"})),
    )
    .await;
    assert_eq!(no_key.status, 409, "raw: {}", no_key.raw);

    // Remove both.
    for id in [&github_id, &gitlab_id] {
        let removed = call(
            &daemon,
            "DELETE",
            &format!("/plugins/{id}?request_id=p-remove-{id}"),
            None,
        )
        .await;
        assert_eq!(removed.status, 200, "raw: {}", removed.raw);
    }
    let list = call(&daemon, "GET", "/plugins", None).await;
    assert_eq!(
        list.body.as_array().map(Vec::len),
        Some(0),
        "raw: {}",
        list.raw
    );
}

#[tokio::test]
async fn custom_plugins_carry_their_own_command_and_dedup() {
    let daemon = start_daemon(&[]).await;
    let machine = this_machine_id(&daemon).await;

    let custom = call(
        &daemon,
        "POST",
        "/plugins",
        Some(install_body(
            &machine,
            json!({
                "name": "Notes",
                "detail": "Read the notes repo",
                "command": "npx -y @scope/server",
                "needs_key": false
            }),
        )),
    )
    .await;
    assert_eq!(custom.status, 201, "raw: {}", custom.raw);
    assert!(
        custom.body["id"]
            .as_str()
            .unwrap_or_default()
            .starts_with("custom-")
    );
    assert_eq!(custom.body["catalog_id"], serde_json::Value::Null);

    // Same name on the same machine: a duplicate, whatever the id would be.
    let dup = call(
        &daemon,
        "POST",
        "/plugins",
        Some(install_body(
            &machine,
            json!({"name": "Notes", "detail": "again", "command": "npx -y other", "needs_key": false}),
        )),
    )
    .await;
    assert_eq!(dup.status, 409, "raw: {}", dup.raw);

    // A custom plugin without a launch command is not a plugin.
    let bare = call(
        &daemon,
        "POST",
        "/plugins",
        Some(install_body(
            &machine,
            json!({"name": "Bare", "detail": "no command"}),
        )),
    )
    .await;
    assert_eq!(bare.status, 422, "raw: {}", bare.raw);

    // The marketplace only sells what it has.
    let unknown = call(
        &daemon,
        "POST",
        "/plugins",
        Some(install_body(
            &machine,
            json!({"catalog_id": "no-such-entry"}),
        )),
    )
    .await;
    assert_eq!(unknown.status, 404, "raw: {}", unknown.raw);
}

#[tokio::test]
async fn plugins_ride_sessions_without_breaking_their_spawns() {
    let daemon = start_daemon(&[]).await;
    let machine = this_machine_id(&daemon).await;
    let ws = workspace(Some("plain"));

    // One marketplace plugin (running) and one custom plugin (needs-key —
    // it does NOT ride until its key is acknowledged).
    let github = call(
        &daemon,
        "POST",
        "/plugins",
        Some(install_body(&machine, json!({"catalog_id": "github"}))),
    )
    .await;
    assert_eq!(github.status, 201, "raw: {}", github.raw);
    let notes = call(
        &daemon,
        "POST",
        "/plugins",
        Some(install_body(
            &machine,
            json!({"name": "Notes", "detail": "notes", "command": "npx -y @scope/server", "needs_key": true}),
        )),
    )
    .await;
    assert_eq!(notes.status, 201, "raw: {}", notes.raw);

    // The session spawns through the real driver argv with the plugins in
    // SpawnOptions (the fixture takes the flags in stride) and the turn
    // completes — plugins never break a chat.
    let created = call(
        &daemon,
        "POST",
        "/sessions",
        Some(json!({"request_id": "p-session", "harness": "claude", "workspace": ws.path()})),
    )
    .await;
    assert_eq!(created.status, 201, "raw: {}", created.raw);
    let id = created.body["id"].as_str().expect("session id").to_string();

    call(
        &daemon,
        "POST",
        &format!("/sessions/{id}/prompts"),
        Some(json!({"request_id": "p-prompt", "text": "hello fixture"})),
    )
    .await;
    let events = until_kinds(
        &daemon,
        &format!("/sessions/{id}/events"),
        &[
            "turn.started",
            "message.added",
            "message.added",
            "turn.completed",
            "session.status_changed",
        ],
    )
    .await;
    assert!(
        events.iter().any(|(_, p)| kind(p) == "turn.completed"),
        "events:\n{}",
        dump(&events)
    );
}
