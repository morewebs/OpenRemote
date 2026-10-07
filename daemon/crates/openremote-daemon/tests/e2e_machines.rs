//! Machines e2e: this machine is real from the first boot (its inventory,
//! its sessions, its presence), added machines wait honestly, and the
//! install chain actually materializes a harness - the registry swap is
//! the product's install enforcement.

mod common;

use common::*;
use serde_json::json;

/// The store's slug, mirrored: lowercase alphanumeric, runs of other
/// characters as single dashes, trimmed at both ends.
fn slugify(name: &str) -> String {
    let mut out = String::new();
    let mut dash = false;
    for c in name.trim().chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
            dash = false;
        } else if !dash && !out.is_empty() {
            out.push('-');
            dash = true;
        }
    }
    out.trim_end_matches('-').to_string()
}

fn this_machine(machines: &serde_json::Value) -> serde_json::Value {
    machines
        .as_array()
        .expect("machines array")
        .iter()
        .find(|m| m["machine"]["this_machine"] == json!(true))
        .cloned()
        .expect("this machine")
}

#[tokio::test]
async fn this_machine_is_real_from_the_first_boot() {
    let daemon = start_daemon(&[]).await;

    let machines = call(&daemon, "GET", "/machines", None).await;
    assert_eq!(machines.status, 200, "raw: {}", machines.raw);
    assert_eq!(
        machines.body.as_array().map(Vec::len),
        Some(1),
        "raw: {}",
        machines.raw
    );

    let view = this_machine(&machines.body);
    let machine = &view["machine"];
    assert_eq!(machine["status"], "online");
    assert_eq!(machine["platform"], std::env::consts::OS);
    // The name is the hostname, slugified, found the daemon's way.
    let raw_name = openremote_core::hostname();
    assert_eq!(
        machine["name"],
        json!(slugify(&raw_name)),
        "raw name: {raw_name}"
    );

    // The inventory is the daemon's own probe: every slot fixture-backed.
    let harnesses = view["harnesses"].as_array().expect("harnesses");
    assert!(harnesses.iter().all(|h| h["available"] == json!(true)));
    // Nothing is installable - everything is already installed.
    assert_eq!(
        view["installable"].as_array().map(Vec::len),
        Some(0),
        "raw: {}",
        machines.raw
    );
    // Presence: 48 half-hour slices, the agent present in the latest.
    let presence = view["presence"].as_array().expect("presence");
    assert_eq!(presence.len(), 48);
    assert_eq!(presence[47], json!(true));
    // No sessions yet, no install command for the machine that is here.
    assert_eq!(view["sessions"].as_array().map(Vec::len), Some(0));
    assert!(view.get("install_command").is_none());
}

#[tokio::test]
async fn added_machines_wait_and_carry_their_install_command() {
    let daemon = start_daemon(&[]).await;

    let added = call(
        &daemon,
        "POST",
        "/machines",
        Some(json!({
            "request_id": "m-add",
            "name": "Build Box",
            "platform": "macos"
        })),
    )
    .await;
    assert_eq!(added.status, 201, "raw: {}", added.raw);
    assert_eq!(added.body["name"], "build-box");
    assert_eq!(added.body["status"], "waiting");
    assert!(
        added.body["enrollment_token"]
            .as_str()
            .is_some_and(|t| !t.is_empty())
    );

    // The waiting machine's view carries the install command for its OS
    // (the enrollment token rides beside it - the check-in consumes both).
    let machines = call(&daemon, "GET", "/machines", None).await;
    let waiting = machines
        .body
        .as_array()
        .expect("machines")
        .iter()
        .find(|m| m["machine"]["name"] == "build-box")
        .cloned()
        .expect("waiting machine");
    assert_eq!(
        waiting["install_command"],
        json!("curl -fsSL openremote.space/install | sh")
    );
    // Nothing is real on it yet: no inventory, no sessions, no presence.
    assert_eq!(waiting["harnesses"].as_array().map(Vec::len), Some(0));
    assert_eq!(waiting["sessions"].as_array().map(Vec::len), Some(0));
    assert!(
        waiting["presence"]
            .as_array()
            .unwrap()
            .iter()
            .all(|up| *up == json!(false))
    );

    // Duplicate hostnames are rejected - whatever they were typed as.
    let dup = call(
        &daemon,
        "POST",
        "/machines",
        Some(json!({
            "request_id": "m-dup",
            "name": "build  BOX",
            "platform": "linux"
        })),
    )
    .await;
    assert_eq!(dup.status, 409, "raw: {}", dup.raw);
    assert!(
        dup.body["error"]
            .as_str()
            .unwrap_or("")
            .contains("already in the list"),
        "raw: {}",
        dup.raw
    );

    // Waiting machines go away; this machine never does.
    let id = added.body["id"].as_str().expect("machine id").to_string();
    let removed = call(
        &daemon,
        "DELETE",
        &format!("/machines/{id}?request_id=m-remove"),
        None,
    )
    .await;
    assert_eq!(removed.status, 200, "raw: {}", removed.raw);
    let machines = call(&daemon, "GET", "/machines", None).await;
    assert_eq!(machines.body.as_array().map(Vec::len), Some(1));

    let this_id = this_machine(&machines.body)["machine"]["id"]
        .as_str()
        .expect("this machine id")
        .to_string();
    let keep = call(
        &daemon,
        "DELETE",
        &format!("/machines/{this_id}?request_id=m-keep"),
        None,
    )
    .await;
    assert_eq!(keep.status, 409, "raw: {}", keep.raw);
}

#[tokio::test]
async fn installs_reject_their_honest_preconditions() {
    let daemon = start_daemon(&[]).await;
    let this_id =
        this_machine(&call(&daemon, "GET", "/machines", None).await.body)["machine"]["id"]
            .as_str()
            .expect("this machine id")
            .to_string();

    // Already installed (every slot is fixture-backed): 409, no installer run.
    let installed = call(
        &daemon,
        "POST",
        &format!("/machines/{this_id}/harnesses"),
        Some(json!({"request_id": "m-i1", "harness": "claude"})),
    )
    .await;
    assert_eq!(installed.status, 409, "raw: {}", installed.raw);
    assert!(
        installed.body["error"]
            .as_str()
            .unwrap_or("")
            .contains("already installed"),
        "raw: {}",
        installed.raw
    );

    // A waiting machine can't install anything - it hasn't checked in.
    let added = call(
        &daemon,
        "POST",
        "/machines",
        Some(json!({"request_id": "m-add2", "name": "server", "platform": "linux"})),
    )
    .await;
    let waiting_id = added.body["id"].as_str().expect("id").to_string();
    let not_checked_in = call(
        &daemon,
        "POST",
        &format!("/machines/{waiting_id}/harnesses"),
        Some(json!({"request_id": "m-i2", "harness": "codex"})),
    )
    .await;
    assert_eq!(not_checked_in.status, 409, "raw: {}", not_checked_in.raw);
    assert!(
        not_checked_in.body["error"]
            .as_str()
            .unwrap_or("")
            .contains("has not checked in"),
        "raw: {}",
        not_checked_in.raw
    );

    // A harness the daemon doesn't know: an explicit error, not a run.
    let unknown = call(
        &daemon,
        "POST",
        &format!("/machines/{this_id}/harnesses"),
        Some(json!({"request_id": "m-i3", "harness": "no-such-harness"})),
    )
    .await;
    assert_eq!(unknown.status, 400, "raw: {}", unknown.raw);
}

#[tokio::test]
async fn an_install_materializes_the_harness_on_this_machine() {
    // The real daemon binary as its own process: codex's override points
    // at a path that doesn't exist yet (authoritative, so codex starts
    // honestly missing) and the install command is a script that
    // materializes the harness at that path - exactly what a real npm
    // install does to the resolver's layout. Every other harness slot
    // points at the fixture, so the machine starts honestly "installed"
    // everywhere but codex - the installable list is then codex alone on
    // any machine, not just one where the other harnesses happen to be
    // installed.
    let dir = tempfile::tempdir().expect("temp dir");
    let staged = dir.path().join("codex.exe");
    let fixture = fixture_agent();
    let script = dir.path().join("copy.mjs");
    std::fs::write(
        &script,
        "import { copyFileSync } from 'node:fs'; copyFileSync(process.argv[2], process.argv[3]);",
    )
    .expect("write copy script");
    let fixture_str = fixture.display().to_string();
    let daemon = spawn_daemon_process(&[
        ("OPENREMOTE_CODEX_PATH", staged.display().to_string()),
        ("OPENREMOTE_CLAUDE_PATH", fixture_str.clone()),
        ("OPENREMOTE_GROK_PATH", fixture_str.clone()),
        ("OPENREMOTE_PI_PATH", fixture_str.clone()),
        ("OPENREMOTE_OPENCODE_PATH", fixture_str.clone()),
        ("OPENREMOTE_AGY_PATH", fixture_str.clone()),
        (
            "OPENREMOTE_INSTALL_CMD",
            format!(
                "node {} {} {}",
                script.display(),
                fixture.display(),
                staged.display()
            ),
        ),
    ])
    .await;

    let machines = call(&daemon, "GET", "/machines", None).await;
    let view = this_machine(&machines.body);
    let installable: Vec<(String, String)> = view["installable"]
        .as_array()
        .expect("installable")
        .iter()
        .map(|r| {
            (
                r["harness_id"].as_str().unwrap_or_default().to_string(),
                r["command"].as_str().unwrap_or_default().to_string(),
            )
        })
        .collect();
    // The row is the owner's own installer for this platform.
    let codex_command = openremote_daemon::install::command_for("codex").expect("codex command");
    assert!(
        codex_command.contains("chatgpt.com/codex/install"),
        "got: {codex_command}"
    );
    assert_eq!(
        installable,
        vec![("codex".to_string(), codex_command)],
        "raw: {}",
        machines.raw
    );

    let this_id = view["machine"]["id"].as_str().expect("id").to_string();
    let install = call(
        &daemon,
        "POST",
        &format!("/machines/{this_id}/harnesses"),
        Some(json!({"request_id": "m-install", "harness": "codex"})),
    )
    .await;
    assert_eq!(install.status, 200, "raw: {}", install.raw);

    // The receipt's result is the machine's refreshed view: codex is on
    // it, the install row is gone.
    let result = &install.body["result"];
    let codex = result["harnesses"]
        .as_array()
        .expect("harnesses")
        .iter()
        .find(|h| h["id"] == "codex")
        .expect("codex in the inventory");
    assert_eq!(codex["available"], json!(true));
    assert_eq!(
        result["installable"].as_array().map(Vec::len),
        Some(0),
        "raw: {}",
        install.raw
    );

    // The swapped registry is live: the models endpoint speaks through it.
    let models = call(&daemon, "GET", "/harnesses/codex/models", None).await;
    assert_eq!(models.status, 200, "raw: {}", models.raw);
    assert!(
        models.body.as_array().is_some_and(|list| !list.is_empty()),
        "raw: {}",
        models.raw
    );

    // A second install of the now-present harness is an honest 409.
    let again = call(
        &daemon,
        "POST",
        &format!("/machines/{this_id}/harnesses"),
        Some(json!({"request_id": "m-again", "harness": "codex"})),
    )
    .await;
    assert_eq!(again.status, 409, "raw: {}", again.raw);
}
