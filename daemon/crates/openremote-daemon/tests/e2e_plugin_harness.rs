//! A plugin-provided harness end to end: the registry probes the driver
//! process, the daemon starts that process for a chat, and a real prompt
//! completes a real turn through the supervisor's existing event vocabulary.

mod common;

use std::path::{Path, PathBuf};

use common::*;
use openremote_daemon::plugins::HarnessSpec;
use serde_json::json;

/// The driver fixture, built once per test process - the same rescue the
/// agent fixture uses when CI has not built it up front.
fn fixture_driver() -> PathBuf {
    static DRIVER: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    DRIVER
        .get_or_init(|| {
            let target = std::env::var("CARGO_TARGET_DIR")
                .map(PathBuf::from)
                .unwrap_or_else(|_| Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target"));
            let name = if cfg!(windows) {
                "fixture-driver.exe"
            } else {
                "fixture-driver"
            };
            let path = target.join("debug").join(name);
            if !path.is_file() {
                let status = std::process::Command::new("cargo")
                    .args(["build", "-p", "fixture-driver"])
                    .status()
                    .expect("spawn cargo to build the fixture driver");
                assert!(status.success(), "cargo build -p fixture-driver failed");
            }
            assert!(
                path.is_file(),
                "fixture driver binary not found at {}",
                path.display()
            );
            path
        })
        .clone()
}

/// The spec `App::new` would build from an installed plugin's folder. The
/// folder itself is empty: the fixture driver needs no files beside it.
fn spec(folder: &Path) -> HarnessSpec {
    HarnessSpec {
        id: "fixture-harness".into(),
        name: "Fixture Harness".into(),
        plugin: "fixture@local".into(),
        dir: folder.to_path_buf(),
        driver: fixture_driver(),
        args: Vec::new(),
        models: None,
        default_model: None,
        enabled: true,
        problem: None,
    }
}

#[tokio::test]
async fn a_plugin_harness_is_probed_listed_and_runs_a_real_turn() {
    let folder = tempfile::tempdir().expect("plugin folder");
    let daemon = start_daemon_with_plugins(&[], &[spec(folder.path())]).await;

    // The harness is in the machine's own list, named by the plugin that
    // provides it, with the driver's probe facts and no invented extras.
    let capabilities = call(&daemon, "GET", "/capabilities", None).await;
    let harness = capabilities.body["harnesses"]
        .as_array()
        .expect("harnesses")
        .iter()
        .find(|harness| harness["id"] == "fixture-harness")
        .expect("the plugin harness");
    assert_eq!(harness["available"], true, "{}", capabilities.raw);
    assert_eq!(harness["plugin"], "fixture@local");
    assert_eq!(harness["name"], "Fixture Harness");
    assert_eq!(harness["version"], "0.0.1");
    assert_eq!(harness["signed_in"], true);
    assert_eq!(harness["enabled"], true);

    // The Plugins page's built-in section stays the six; this one is an
    // installed plugin's harness, not a seventh built-in.
    let plugins = call(&daemon, "GET", "/plugins", None).await;
    assert!(
        plugins.body["builtin"]
            .as_array()
            .expect("builtin")
            .iter()
            .all(|harness| harness["id"] != "fixture-harness"),
        "{}",
        plugins.raw
    );

    let workspace = tempfile::tempdir().expect("workspace");
    let created = call(
        &daemon,
        "POST",
        "/sessions",
        Some(json!({
            "request_id": "ph-create",
            "harness": "fixture-harness",
            "workspace": workspace.path(),
        })),
    )
    .await;
    assert_eq!(created.status, 201, "raw: {}", created.raw);
    let id = created.body["id"].as_str().expect("session id").to_string();

    call(
        &daemon,
        "POST",
        &format!("/sessions/{id}/prompts"),
        Some(json!({"request_id": "ph-p1", "text": "ship the notes"})),
    )
    .await;

    let events = until_kinds(
        &daemon,
        &format!("/sessions/{id}/events"),
        &["message.added", "turn.completed", "session.status_changed"],
    )
    .await;
    let completed = events
        .iter()
        .find(|(_, payload)| kind(payload) == "turn.completed")
        .expect("a completed turn");
    assert_eq!(completed.1["coarse"], "completed");
    assert_eq!(completed.1["outcome"], "finished");
    let assistant = events
        .iter()
        .find(|(_, payload)| {
            kind(payload) == "message.added" && payload["message"]["role"] == "assistant"
        })
        .expect("an assistant message");
    assert!(
        assistant.1["message"]["text"]
            .as_str()
            .unwrap_or_default()
            .contains("fixture heard: ship the notes"),
        "events:\n{}",
        dump(&events)
    );

    // The driver named the harness's own conversation; a resume can use it.
    let session = call(&daemon, "GET", &format!("/sessions/{id}"), None).await;
    assert_eq!(
        session.body["harness_session_ref"], "fixture-1",
        "{}",
        session.raw
    );
}

#[tokio::test]
async fn a_disabled_plugin_harness_is_listed_but_not_pickable() {
    let folder = tempfile::tempdir().expect("plugin folder");
    let mut disabled = spec(folder.path());
    disabled.enabled = false;
    assert!(!disabled.enabled, "the test spec is disabled");
    let daemon = start_daemon_with_plugins(&[], &[disabled]).await;

    let capabilities = call(&daemon, "GET", "/capabilities", None).await;
    let harness = capabilities.body["harnesses"]
        .as_array()
        .expect("harnesses")
        .iter()
        .find(|harness| harness["id"] == "fixture-harness")
        .expect("the plugin harness");
    assert_eq!(harness["available"], true);
    assert_eq!(harness["enabled"], false, "{}", capabilities.raw);
}

/// The whole path a person takes: stage a folder, approve it, and the
/// harness is in the machine's list without restarting the daemon.
#[tokio::test]
async fn installing_a_harness_plugin_adds_its_harness_without_a_restart() {
    let folder = tempfile::tempdir().expect("plugin folder");
    let driver = fixture_driver();
    let windows = "bin/windows-x64/fixture-driver.exe";
    let default = "bin/fixture-driver";
    std::fs::create_dir_all(folder.path().join("bin/windows-x64")).expect("driver folder");
    std::fs::copy(&driver, folder.path().join(windows)).expect("windows driver");
    std::fs::copy(&driver, folder.path().join(default)).expect("default driver");
    std::fs::write(
        folder.path().join("openremote.json"),
        json!({
            "api": 1,
            "name": "fixture-plugin",
            "version": "0.1.0",
            "description": "A harness plugin fixture",
            "runtime": "process",
            "powers": ["run_programs"],
            "harness": {
                "api": 1,
                "id": "fixture-plugin-harness",
                "name": "Fixture Plugin Harness",
                "driver": {"windows": windows, "default": default},
                "args": []
            }
        })
        .to_string(),
    )
    .expect("manifest");

    let daemon = start_daemon_with_plugins(&[], &[]).await;
    let before = call(&daemon, "GET", "/capabilities", None).await;
    assert!(
        before.body["harnesses"]
            .as_array()
            .expect("harnesses")
            .iter()
            .all(|harness| harness["id"] != "fixture-plugin-harness"),
        "{}",
        before.raw
    );

    let staged = call(
        &daemon,
        "POST",
        "/plugins/stage",
        Some(json!({
            "request_id": "ph-stage",
            "source": {"kind": "folder", "path": folder.path()}
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
        Some(json!({"request_id": "ph-install"})),
    )
    .await;
    assert_eq!(installed.status, 200, "{}", installed.raw);
    assert_eq!(
        installed.body["result"]["harness"]["id"],
        "fixture-plugin-harness"
    );

    // No restart: the registry re-probed when the plugin landed.
    let after = call(&daemon, "GET", "/capabilities", None).await;
    let harness = after.body["harnesses"]
        .as_array()
        .expect("harnesses")
        .iter()
        .find(|harness| harness["id"] == "fixture-plugin-harness")
        .expect("the installed harness");
    assert_eq!(harness["available"], true, "{}", after.raw);
    assert_eq!(harness["plugin"], "fixture-plugin@local");
    assert_eq!(harness["name"], "Fixture Plugin Harness");

    // The record remembers the harness, so the Plugins page can say which
    // installed plugin also fills a harness row.
    let plugins = call(&daemon, "GET", "/plugins", None).await;
    assert_eq!(
        plugins.body["installed"][0]["harness"]["name"], "Fixture Plugin Harness",
        "{}",
        plugins.raw
    );

    // A chat can start on it, which is the backend actually being wired.
    let workspace = tempfile::tempdir().expect("workspace");
    let created = call(
        &daemon,
        "POST",
        "/sessions",
        Some(json!({
            "request_id": "ph-installed-create",
            "harness": "fixture-plugin-harness",
            "workspace": workspace.path(),
        })),
    )
    .await;
    assert_eq!(created.status, 201, "raw: {}", created.raw);
}

/// A harness plugin whose name is also its harness id - which is the shape
/// every official plugin has, freebuff included - can be installed and then
/// staged again for an update.
///
/// The reserved ids are OpenRemote's own six. Once the plugin is installed its
/// harness is in the registry, and treating that as reserved made the daemon
/// refuse the plugin its own name: the first install worked, and every update
/// after it failed with "'name' is taken by OpenRemote itself", with Discover
/// showing the entry as uninstallable.
#[tokio::test]
async fn a_harness_plugin_whose_name_is_its_harness_id_can_be_updated() {
    let driver = fixture_driver();

    // A fresh copy of the plugin folder, so an update stages the same manifest
    // after the plugin is already installed.
    fn plugin_folder(driver: &Path) -> tempfile::TempDir {
        let windows = "bin/windows-x64/fixture-driver.exe";
        let default = "bin/fixture-driver";
        let folder = tempfile::tempdir().expect("plugin folder");
        std::fs::create_dir_all(folder.path().join("bin/windows-x64")).expect("driver folder");
        std::fs::copy(driver, folder.path().join(windows)).expect("windows driver");
        std::fs::copy(driver, folder.path().join(default)).expect("default driver");
        std::fs::write(
            folder.path().join("openremote.json"),
            json!({
                "api": 1,
                "name": "fixture-self",
                "version": "0.1.0",
                "description": "A harness plugin named like its harness",
                "runtime": "process",
                "powers": ["run_programs"],
                "harness": {
                    "api": 1,
                    "id": "fixture-self",
                    "name": "Fixture Self",
                    "driver": {"windows": windows, "default": default},
                    "args": []
                }
            })
            .to_string(),
        )
        .expect("manifest");
        folder
    }

    let daemon = start_daemon_with_plugins(&[], &[]).await;

    let first = plugin_folder(&driver);
    let staged = call(
        &daemon,
        "POST",
        "/plugins/stage",
        Some(json!({
            "request_id": "self-stage-1",
            "source": {"kind": "folder", "path": first.path()}
        })),
    )
    .await;
    assert_eq!(staged.status, 200, "first stage: {}", staged.raw);
    let stage_id = staged.body["result"]["stage_id"]
        .as_str()
        .expect("stage id")
        .to_string();
    let installed = call(
        &daemon,
        "POST",
        &format!("/plugins/stage/{stage_id}/install"),
        Some(json!({"request_id": "self-install-1"})),
    )
    .await;
    assert_eq!(installed.status, 200, "first install: {}", installed.raw);

    // The harness is now in the registry under the plugin's own name.
    let capabilities = call(&daemon, "GET", "/capabilities", None).await;
    assert!(
        capabilities.body["harnesses"]
            .as_array()
            .expect("harnesses")
            .iter()
            .any(|harness| harness["id"] == "fixture-self"
                && harness["plugin"] == "fixture-self@local"),
        "{}",
        capabilities.raw
    );

    // Staging it again is an update, and must be allowed.
    let second = plugin_folder(&driver);
    let again = call(
        &daemon,
        "POST",
        "/plugins/stage",
        Some(json!({
            "request_id": "self-stage-2",
            "source": {"kind": "folder", "path": second.path()}
        })),
    )
    .await;
    assert_eq!(
        again.status, 200,
        "staging a harness plugin whose name is its harness id must not be refused: {}",
        again.raw
    );
}

/// A third-party driver may ask for an approval. The contract documents
/// `approval_requested` and `answer`; this drives both through the real
/// seam, with the fixture's own scenario choosing to ask.
#[tokio::test]
async fn a_plugin_harness_approval_can_be_answered() {
    let folder = tempfile::tempdir().expect("plugin folder");
    let mut with_approval = spec(folder.path());
    with_approval.args = vec!["approval".into()];
    let daemon = start_daemon_with_plugins(&[], &[with_approval]).await;

    let workspace = tempfile::tempdir().expect("workspace");
    let created = call(
        &daemon,
        "POST",
        "/sessions",
        Some(json!({
            "request_id": "ph-approval-create",
            "harness": "fixture-harness",
            "workspace": workspace.path(),
        })),
    )
    .await;
    assert_eq!(created.status, 201, "raw: {}", created.raw);
    let id = created.body["id"].as_str().expect("session id").to_string();

    call(
        &daemon,
        "POST",
        &format!("/sessions/{id}/prompts"),
        Some(json!({"request_id": "ph-approval-p1", "text": "write it"})),
    )
    .await;

    let events = until_kinds(
        &daemon,
        &format!("/sessions/{id}/events"),
        &["decision.requested", "session.status_changed"],
    )
    .await;
    let decision = events
        .iter()
        .find(|(_, payload)| kind(payload) == "decision.requested")
        .expect("the driver's approval arrives");
    let decision_id = decision.1["decision"]["id"]
        .as_str()
        .expect("decision id")
        .to_string();
    assert_eq!(
        decision.1["decision"]["options"][0]["id"],
        "allow",
        "the driver's own words ride the card: {}",
        dump(&events)
    );

    let answered = call(
        &daemon,
        "POST",
        &format!("/decisions/{decision_id}/answer"),
        Some(json!({"request_id": "ph-approval-answer", "choice": "allow"})),
    )
    .await;
    assert_eq!(answered.status, 200, "raw: {}", answered.raw);
    assert_eq!(
        answered.body["answer"]["choice"], "allow",
        "{}",
        answered.raw
    );

    let events = until_kinds(
        &daemon,
        &format!("/sessions/{id}/events"),
        &["message.added", "turn.completed"],
    )
    .await;
    let assistant = events
        .iter()
        .find(|(_, payload)| {
            kind(payload) == "message.added" && payload["message"]["role"] == "assistant"
        })
        .expect("an assistant message");
    assert!(
        assistant.1["message"]["text"]
            .as_str()
            .unwrap_or_default()
            .contains("fixture answered: allow"),
        "the driver received the answer: {}",
        dump(&events)
    );
    let completed = events
        .iter()
        .find(|(_, payload)| kind(payload) == "turn.completed")
        .expect("the turn completes");
    assert_eq!(completed.1["coarse"], "completed");
}
