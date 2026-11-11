//! A plugin-provided harness end to end: the registry probes the driver
//! process, the daemon starts that process for a chat, and a real prompt
//! completes a real turn through the supervisor's existing event vocabulary.

mod common;

use std::path::{Path, PathBuf};

use common::*;
use openremote_core::PluginPower;
use openremote_core::model::PermissionWord;
use openremote_daemon::plugins::{HarnessSpec, HarnessTraits};
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
            // Built every test process, not only when missing: `cargo test`
            // does not rebuild another package's binary, so a binary left
            // from an older run would be tested in place of today's source.
            // Once per process (the OnceLock), and a no-op when it is fresh.
            // The PATH cargo, as the agent fixture's rescue uses.
            let status = std::process::Command::new("cargo")
                .args(["build", "-p", "fixture-driver"])
                .current_dir(env!("CARGO_MANIFEST_DIR"))
                .status()
                .expect("spawn cargo to build the fixture driver");
            assert!(status.success(), "cargo build -p fixture-driver failed");
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
        models_captured_at: None,
        settings_live: false,
        traits: HarnessTraits::default(),
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

/// The spec a plugin with live settings would produce: the driver takes the
/// change on the running process, and the test's flag makes it hold a turn.
fn live_spec(folder: &Path, args: &[&str]) -> HarnessSpec {
    HarnessSpec {
        args: args.iter().map(|arg| (*arg).to_string()).collect(),
        settings_live: true,
        ..spec(folder)
    }
}

/// The plugin fixture's folder as a harness plugin whose driver captures a
/// catalog: the shipped list, the file it writes, and the live settings word.
fn catalog_plugin_folder(driver: &Path) -> tempfile::TempDir {
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
            "name": "fixture-catalog",
            "version": "0.1.0",
            "description": "A harness plugin whose driver captures a catalog",
            "runtime": "process",
            "powers": ["run_programs"],
            "harness": {
                "api": 1,
                "id": "fixture-catalog-harness",
                "name": "Fixture Catalog",
                "driver": {"windows": windows, "default": default},
                "args": [],
                "models": [{"model": "shipped-model", "display_name": "Shipped"}],
                "models_file": "models.json",
                "settings": "live"
            }
        })
        .to_string(),
    )
    .expect("manifest");
    folder
}

/// The whole catalog seam: a plugin's shipped list, the driver's own capture
/// replacing it, and a refusal while a chat holds the folder.
#[tokio::test]
async fn the_refresh_route_returns_the_harness_row_and_refuses_while_a_chat_runs() {
    let driver = fixture_driver();
    let folder = catalog_plugin_folder(&driver);
    let daemon = start_daemon_with_plugins(&[], &[]).await;

    let staged = call(
        &daemon,
        "POST",
        "/plugins/stage",
        Some(json!({
            "request_id": "cat-stage",
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
        Some(json!({"request_id": "cat-install"})),
    )
    .await;
    assert_eq!(installed.status, 200, "{}", installed.raw);

    // Before any capture the row carries the manifest's shipped list and no
    // capture time.
    let before = call(&daemon, "GET", "/capabilities", None).await;
    let row = before.body["harnesses"]
        .as_array()
        .expect("harnesses")
        .iter()
        .find(|harness| harness["id"] == "fixture-catalog-harness")
        .expect("the harness");
    assert!(row["models_captured_at"].is_null(), "{}", before.raw);
    assert_eq!(row["settings_live"], true, "{}", before.raw);
    let shipped = call(
        &daemon,
        "GET",
        "/harnesses/fixture-catalog-harness/models",
        None,
    )
    .await;
    assert_eq!(shipped.body[0]["model"], "shipped-model", "{}", shipped.raw);

    // The driver's --catalog writes models.json beside itself; the re-probe
    // reads it, and the answer is the row that now carries the capture time.
    let refreshed = call(
        &daemon,
        "POST",
        "/harnesses/fixture-catalog-harness/catalog/refresh",
        Some(json!({"request_id": "cat-refresh"})),
    )
    .await;
    assert_eq!(refreshed.status, 200, "{}", refreshed.raw);
    let harness = &refreshed.body["result"]["harness"];
    assert_eq!(harness["id"], "fixture-catalog-harness");
    assert_eq!(harness["models_captured_at"], 1_699_999_999_000u64);
    let captured = call(
        &daemon,
        "GET",
        "/harnesses/fixture-catalog-harness/models",
        None,
    )
    .await;
    assert_eq!(
        captured.body[0]["model"], "fixture-model-a",
        "{}",
        captured.raw
    );
    assert_eq!(
        captured.body[0]["detail"], "3x · Recommended",
        "{}",
        captured.raw
    );
    assert_eq!(
        captured.body[0]["note"], "May use data for AI training",
        "{}",
        captured.raw
    );

    // A chat on the harness refuses a capture: the driver is running in the
    // folder the capture would run from.
    let workspace = tempfile::tempdir().expect("workspace");
    let created = call(
        &daemon,
        "POST",
        "/sessions",
        Some(json!({
            "request_id": "cat-chat",
            "harness": "fixture-catalog-harness",
            "workspace": workspace.path()
        })),
    )
    .await;
    assert_eq!(created.status, 201, "{}", created.raw);
    let id = created.body["id"].as_str().expect("session id").to_string();
    let refused = call(
        &daemon,
        "POST",
        "/harnesses/fixture-catalog-harness/catalog/refresh",
        Some(json!({"request_id": "cat-refused"})),
    )
    .await;
    assert_eq!(refused.status, 409, "{}", refused.raw);
    assert!(
        refused.body["error"]
            .as_str()
            .unwrap_or_default()
            .contains("refresh when it stops"),
        "{}",
        refused.raw
    );
    let _ = call(
        &daemon,
        "POST",
        &format!("/sessions/{id}/stop"),
        Some(json!({"request_id": "cat-stop"})),
    )
    .await;
}

/// A live harness takes a model change on the running process, but only
/// between turns: the daemon's own status decides, before any driver write.
#[tokio::test]
async fn a_live_model_change_is_refused_while_a_turn_works() {
    let folder = tempfile::tempdir().expect("plugin folder");
    let daemon = start_daemon_with_plugins(&[], &[live_spec(folder.path(), &["--hold"])]).await;

    let workspace = tempfile::tempdir().expect("workspace");
    let created = call(
        &daemon,
        "POST",
        "/sessions",
        Some(json!({
            "request_id": "live-create",
            "harness": "fixture-harness",
            "workspace": workspace.path()
        })),
    )
    .await;
    assert_eq!(created.status, 201, "{}", created.raw);
    let id = created.body["id"].as_str().expect("session id").to_string();
    until_status(&daemon, &id, &["idle"]).await;

    let prompted = call(
        &daemon,
        "POST",
        &format!("/sessions/{id}/prompts"),
        Some(json!({"request_id": "live-prompt", "text": "stay busy"})),
    )
    .await;
    assert_eq!(prompted.status, 200, "{}", prompted.raw);
    until_status(&daemon, &id, &["working"]).await;

    let refused = call(
        &daemon,
        "POST",
        &format!("/sessions/{id}/settings"),
        Some(json!({"request_id": "live-settings", "model": "another-model"})),
    )
    .await;
    assert_eq!(refused.status, 409, "{}", refused.raw);
    assert!(
        refused.body["error"]
            .as_str()
            .unwrap_or_default()
            .contains("wait for this turn to finish"),
        "{}",
        refused.raw
    );
    // The change was never accepted: the stored word is still unpicked.
    let session = call(&daemon, "GET", &format!("/sessions/{id}"), None).await;
    assert!(session.body["model"].is_null(), "{}", session.raw);
    let _ = call(
        &daemon,
        "POST",
        &format!("/sessions/{id}/stop"),
        Some(json!({"request_id": "live-stop"})),
    )
    .await;
}

/// A driver that refuses a live change answers with one sentence, and that
/// sentence lands in the chat as a note: the console never claims a change
/// the harness did not make.
#[tokio::test]
async fn a_refused_live_setting_lands_as_a_note_in_the_chat() {
    let folder = tempfile::tempdir().expect("plugin folder");
    let daemon =
        start_daemon_with_plugins(&[], &[live_spec(folder.path(), &["--refuse-settings"])]).await;

    let workspace = tempfile::tempdir().expect("workspace");
    let created = call(
        &daemon,
        "POST",
        "/sessions",
        Some(json!({
            "request_id": "refuse-create",
            "harness": "fixture-harness",
            "workspace": workspace.path()
        })),
    )
    .await;
    assert_eq!(created.status, 201, "{}", created.raw);
    let id = created.body["id"].as_str().expect("session id").to_string();
    let before = until_status(&daemon, &id, &["idle"]).await;

    let accepted = call(
        &daemon,
        "POST",
        &format!("/sessions/{id}/settings"),
        Some(json!({"request_id": "refuse-settings", "model": "missing-model", "fast": true})),
    )
    .await;
    assert_eq!(accepted.status, 200, "{}", accepted.raw);
    // The live wire has no acknowledgement, so the change is stored when it
    // is written; the refusal below is what undoes it.
    assert_eq!(accepted.body["model"], "missing-model", "{}", accepted.raw);

    let events = until_kinds(
        &daemon,
        &format!("/sessions/{id}/events"),
        &["daemon.error"],
    )
    .await;
    let note = events
        .iter()
        .find(|(_, payload)| kind(payload) == "daemon.error")
        .expect("the driver's refusal lands as a note");
    assert_eq!(
        note.1["message"],
        "fixture-driver has no model by that name",
        "{}",
        dump(&events)
    );
    // The refused change is rolled back before the note lands, so the chip and
    // a later resume keep what the harness actually runs.
    let after = call(&daemon, "GET", &format!("/sessions/{id}"), None).await;
    assert_ne!(after.body["model"], "missing-model", "{}", after.raw);
    assert_eq!(after.body["model"], before["model"], "{}", after.raw);
    assert_eq!(after.body["effort"], before["effort"], "{}", after.raw);
    assert_eq!(after.body["fast"], before["fast"], "{}", after.raw);
    let restored = events
        .iter()
        .rposition(|(_, payload)| kind(payload) == "session.updated")
        .expect("the rollback is announced");
    let noted = events
        .iter()
        .position(|(_, payload)| kind(payload) == "daemon.error")
        .expect("the note");
    assert!(restored < noted, "{}", dump(&events));
    assert_ne!(
        events[restored].1["session"]["model"],
        "missing-model",
        "{}",
        dump(&events)
    );
    let _ = call(
        &daemon,
        "POST",
        &format!("/sessions/{id}/stop"),
        Some(json!({"request_id": "refuse-stop"})),
    )
    .await;
}

///
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

/// The query string's form of a folder path.
fn encode(path: &Path) -> String {
    path.to_string_lossy()
        .bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.') {
                (b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect()
}

/// The manifest's contract fields reach the harness row, and a driver that
/// declares `commands` fills the composer's menus from `--commands <folder>`.
#[tokio::test]
async fn a_plugin_harness_row_and_menus_follow_its_manifest() {
    let folder = tempfile::tempdir().expect("plugin folder");
    let declared = HarnessSpec {
        traits: HarnessTraits {
            permission_modes: vec![
                PermissionWord::new("ask", "Ask first"),
                PermissionWord::new("auto", "Run without asking"),
            ],
            default_permission: Some("ask".into()),
            fast: true,
            mcp: true,
            commands: true,
            ..HarnessTraits::default()
        },
        ..spec(folder.path())
    };
    let daemon = start_daemon_with_plugins(&[], &[declared]).await;

    let capabilities = call(&daemon, "GET", "/capabilities", None).await;
    let harness = capabilities.body["harnesses"]
        .as_array()
        .expect("harnesses")
        .iter()
        .find(|harness| harness["id"] == "fixture-harness")
        .expect("the plugin harness")
        .clone();
    assert_eq!(harness["fast_supported"], true, "{harness}");
    assert_eq!(harness["honors"]["fast"], true, "{harness}");
    assert_eq!(harness["honors"]["mcp"], true, "{harness}");
    assert_eq!(harness["honors"]["tools"], false, "{harness}");
    assert_eq!(
        harness["permission_modes"],
        json!([{"w": "ask", "l": "Ask first"}, {"w": "auto", "l": "Run without asking"}]),
        "{harness}"
    );
    assert_eq!(harness["default_permission"], "ask", "{harness}");

    let workspace = tempfile::tempdir().expect("workspace");
    let reply = call(
        &daemon,
        "GET",
        &format!(
            "/commands?harness=fixture-harness&workspace={}",
            encode(workspace.path())
        ),
        None,
    )
    .await;
    assert_eq!(reply.status, 200, "{}", reply.raw);
    assert!(reply.body.get("error").is_none(), "{}", reply.raw);
    let command = reply.body["commands"]
        .as_array()
        .expect("commands")
        .iter()
        .find(|command| command["name"] == "fixture-review")
        .expect("the driver's command")
        .clone();
    assert_eq!(command["source"], "harness", "{command}");
    assert_eq!(command["argument_hint"], "<path>", "{command}");
    assert_eq!(command["aliases"], json!(["fr"]), "{command}");
    let agent = reply.body["agents"]
        .as_array()
        .expect("agents")
        .iter()
        .find(|agent| agent["name"] == "fixture-planner")
        .expect("the driver's agent")
        .clone();
    assert_eq!(
        agent["description"],
        workspace.path().to_string_lossy().as_ref(),
        "the driver was asked about the chat's folder: {agent}"
    );
}

/// A driver that does not declare `commands` is never started for a menu.
#[tokio::test]
async fn a_plugin_harness_without_commands_lists_none_of_its_own() {
    let folder = tempfile::tempdir().expect("plugin folder");
    let daemon = start_daemon_with_plugins(&[], &[spec(folder.path())]).await;
    let workspace = tempfile::tempdir().expect("workspace");
    let reply = call(
        &daemon,
        "GET",
        &format!(
            "/commands?harness=fixture-harness&workspace={}",
            encode(workspace.path())
        ),
        None,
    )
    .await;
    assert_eq!(reply.status, 200, "{}", reply.raw);
    assert!(
        reply.body["commands"]
            .as_array()
            .expect("commands")
            .iter()
            .all(|command| command["source"] != "harness"),
        "{}",
        reply.raw
    );
}

/// This machine's id, which a hand-written MCP server is installed on.
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

/// Start a chat on an `echo-spawn` driver with these traits, after the user
/// set up one MCP server of their own (`github`), and answer what the driver
/// says its spawn command carried.
async fn what_the_spawn_carried(traits: HarnessTraits, tag: &str) -> String {
    let folder = tempfile::tempdir().expect("plugin folder");
    let echo = HarnessSpec {
        args: vec!["echo-spawn".into()],
        traits,
        ..spec(folder.path())
    };
    let daemon = start_daemon_with_plugins(&[], &[echo]).await;
    let machine = this_machine_id(&daemon).await;
    let installed = call(
        &daemon,
        "POST",
        "/mcp-servers",
        Some(json!({
            "request_id": format!("{tag}-mcp"),
            "machine": machine,
            "name": "GitHub",
            "detail": "Issues and pull requests.",
            "command": "npx -y @modelcontextprotocol/server-github",
            "needs_key": false
        })),
    )
    .await;
    assert_eq!(installed.status, 201, "raw: {}", installed.raw);

    let workspace = tempfile::tempdir().expect("workspace");
    let created = call(
        &daemon,
        "POST",
        "/sessions",
        Some(json!({
            "request_id": format!("{tag}-create"),
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
        Some(json!({"request_id": format!("{tag}-p1"), "text": "what did you get"})),
    )
    .await;
    let events = until_kinds(
        &daemon,
        &format!("/sessions/{id}/events"),
        &["message.added", "turn.completed"],
    )
    .await;
    events
        .iter()
        .find(|(_, payload)| {
            kind(payload) == "message.added" && payload["message"]["role"] == "assistant"
        })
        .and_then(|(_, payload)| payload["message"]["text"].as_str().map(String::from))
        .expect("an assistant message")
}

/// A driver that declares `mcp` and holds `agent_tools` and `read_chats` is
/// handed every server the chat has, OpenRemote's own among them, and its
/// pre-approved reading tools.
#[tokio::test]
async fn a_plugin_harness_with_mcp_and_powers_gets_the_chats_servers_and_tools() {
    let text = what_the_spawn_carried(
        HarnessTraits {
            mcp: true,
            powers: vec![PluginPower::AgentTools, PluginPower::ReadChats],
            ..HarnessTraits::default()
        },
        "mcp-full",
    )
    .await;
    assert!(
        text.contains("mcp=[github,openremote]"),
        "the user's server and OpenRemote's per-chat server: {text}"
    );
    assert!(
        text.contains("mcp__openremote__browser_") && text.contains("mcp__openremote__agents_"),
        "the browser and chat-reading tools are pre-approved: {text}"
    );
}

/// A driver that does not declare `mcp` is handed nothing: no user server
/// (whose command can carry a key), no OpenRemote server, no pre-approval.
#[tokio::test]
async fn a_plugin_harness_without_mcp_is_handed_no_servers_or_tools() {
    let text = what_the_spawn_carried(
        HarnessTraits {
            powers: vec![PluginPower::AgentTools, PluginPower::ReadChats],
            ..HarnessTraits::default()
        },
        "mcp-none",
    )
    .await;
    assert!(
        text.contains("mcp=[] allowed=[]"),
        "both lists arrive empty: {text}"
    );
}

/// `mcp` without the powers: the user's own servers ride, OpenRemote's
/// per-chat server and its pre-approved tools do not.
#[tokio::test]
async fn a_plugin_harness_with_mcp_but_no_powers_gets_only_the_users_servers() {
    let text = what_the_spawn_carried(
        HarnessTraits {
            mcp: true,
            ..HarnessTraits::default()
        },
        "mcp-bare",
    )
    .await;
    assert!(
        text.contains("mcp=[github] allowed=[]"),
        "the user's server only, nothing pre-approved: {text}"
    );
}

/// An ask that names a choice in `session_grant` records the tool for the
/// rest of the chat when that choice is picked.
#[tokio::test]
async fn a_plugin_ask_whose_choice_grants_the_tool_records_it_for_the_chat() {
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
            "request_id": "ph-grant-create",
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
        Some(json!({"request_id": "ph-grant-p1", "text": "write it"})),
    )
    .await;
    let events = until_kinds(
        &daemon,
        &format!("/sessions/{id}/events"),
        &["decision.requested"],
    )
    .await;
    let decision_id = events
        .iter()
        .find(|(_, payload)| kind(payload) == "decision.requested")
        .and_then(|(_, payload)| payload["decision"]["id"].as_str().map(String::from))
        .expect("the driver's approval arrives");

    let answered = call(
        &daemon,
        "POST",
        &format!("/decisions/{decision_id}/answer"),
        Some(json!({"request_id": "ph-grant-answer", "choice": "always"})),
    )
    .await;
    assert_eq!(answered.status, 200, "raw: {}", answered.raw);

    let session = call(&daemon, "GET", &format!("/sessions/{id}"), None).await;
    assert_eq!(
        session.body["approved_tools"],
        json!(["write_file"]),
        "{}",
        session.raw
    );
}

/// Send two prompts to a held turn on a harness with these traits, and
/// answer the second prompt's status.
async fn second_prompt_status(traits: HarnessTraits, tag: &str) -> u16 {
    let folder = tempfile::tempdir().expect("plugin folder");
    let held = HarnessSpec {
        args: vec!["--hold".into()],
        traits,
        ..spec(folder.path())
    };
    let daemon = start_daemon_with_plugins(&[], &[held]).await;
    let workspace = tempfile::tempdir().expect("workspace");
    let created = call(
        &daemon,
        "POST",
        "/sessions",
        Some(json!({
            "request_id": format!("{tag}-create"),
            "harness": "fixture-harness",
            "workspace": workspace.path()
        })),
    )
    .await;
    assert_eq!(created.status, 201, "{}", created.raw);
    let id = created.body["id"].as_str().expect("session id").to_string();
    until_status(&daemon, &id, &["idle"]).await;
    let first = call(
        &daemon,
        "POST",
        &format!("/sessions/{id}/prompts"),
        Some(json!({"request_id": format!("{tag}-p1"), "text": "stay busy"})),
    )
    .await;
    assert_eq!(first.status, 200, "{}", first.raw);
    until_status(&daemon, &id, &["working"]).await;
    let second = call(
        &daemon,
        "POST",
        &format!("/sessions/{id}/prompts"),
        Some(json!({"request_id": format!("{tag}-p2"), "text": "and this"})),
    )
    .await;
    let _ = call(
        &daemon,
        "POST",
        &format!("/sessions/{id}/stop"),
        Some(json!({"request_id": format!("{tag}-stop")})),
    )
    .await;
    second.status
}

/// The manifest decides the prompt gate: by default a second message waits
/// for the turn; a driver that steers takes it inside the running turn.
#[tokio::test]
async fn a_plugin_harness_takes_a_mid_turn_message_only_when_its_manifest_says_so() {
    assert_eq!(
        second_prompt_status(HarnessTraits::default(), "gate-default").await,
        409,
        "one prompt at a time unless the manifest says otherwise"
    );
    let steering = HarnessTraits {
        one_prompt_at_a_time: false,
        steers_mid_turn: true,
        ..HarnessTraits::default()
    };
    assert_eq!(
        second_prompt_status(steering, "gate-steer").await,
        200,
        "a steering driver takes the message mid-turn"
    );
}
