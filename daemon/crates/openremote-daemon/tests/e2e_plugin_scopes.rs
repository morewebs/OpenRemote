//! Who a plugin is for, end to end (plan step 5): this machine, one
//! project (outside or inside its repo), or every device of the account.
//! Three promises are held here:
//!
//! - who a plugin is for never widens what it may do;
//! - a project's plugin runs only for that project's chats;
//! - a Global plugin arriving from another device installs nothing until
//!   the user reviews and approves it here, as any install.

mod common;

use std::path::Path;

use common::cloud::{device, until};
use common::*;
use fake_cloud::FakeCloud;
use serde_json::{Value, json};

/// A WASM tool plugin (no program, so nothing to build): its one tool
/// answers "wasm". It asks for no power; reading it implies `agent_tools`.
fn wasm_plugin() -> tempfile::TempDir {
    let folder = tempfile::tempdir().expect("plugin folder");
    let answer = r#"{"content":[{"type":"text","text":"wasm"}]}"#;
    let wat = format!(
        r#"(module
            (memory (export "memory") 1)
            (data (i32.const 1024) "{}")
            (func (export "or_alloc") (param i32) (result i32) (i32.const 4096))
            (func (export "or_call") (param i32 i32) (result i64)
                (i64.or (i64.shl (i64.const 1024) (i64.const 32)) (i64.const {}))))"#,
        answer.replace('"', "\\\""),
        answer.len()
    );
    std::fs::write(
        folder.path().join("tool.wasm"),
        wat::parse_str(&wat).expect("assemble"),
    )
    .unwrap();
    std::fs::write(
        folder.path().join("openremote.json"),
        json!({
            "api": 1,
            "name": "wasm-tools",
            "runtime": "wasm",
            "entry": "tool.wasm",
            "contributes": {"tools": [{"name": "hello", "description": "Say hello"}]}
        })
        .to_string(),
    )
    .unwrap();
    folder
}

async fn stage_source(daemon: &TestDaemon, source: Value) -> Reply {
    call(
        daemon,
        "POST",
        "/plugins/stage",
        Some(json!({"request_id": uuid::Uuid::new_v4().to_string(), "source": source})),
    )
    .await
}

async fn stage_folder(daemon: &TestDaemon, folder: &Path) -> String {
    let staged = stage_source(daemon, json!({"kind": "folder", "path": folder})).await;
    assert_eq!(staged.status, 200, "{}", staged.raw);
    staged.body["result"]["stage_id"]
        .as_str()
        .expect("stage id")
        .to_string()
}

/// Approve a staged plugin with an install body of `extra` fields.
async fn approve(daemon: &TestDaemon, stage_id: &str, extra: Value) -> Reply {
    let mut body = json!({"request_id": uuid::Uuid::new_v4().to_string()});
    for (k, v) in extra.as_object().unwrap() {
        body[k] = v.clone();
    }
    call(
        daemon,
        "POST",
        &format!("/plugins/stage/{stage_id}/install"),
        Some(body),
    )
    .await
}

async fn remove(daemon: &TestDaemon, id: &str) {
    let removed = call(
        daemon,
        "POST",
        &format!("/plugins/{}/remove", id.replace('~', "%7E")),
        Some(json!({"request_id": uuid::Uuid::new_v4().to_string()})),
    )
    .await;
    assert_eq!(removed.status, 200, "{}", removed.raw);
}

async fn project(daemon: &TestDaemon, folder: &Path) -> String {
    let saved = call(
        daemon,
        "POST",
        "/projects",
        Some(json!({"request_id": uuid::Uuid::new_v4().to_string(), "folders": [folder]})),
    )
    .await;
    assert_eq!(saved.status, 201, "{}", saved.raw);
    saved.body["id"].as_str().expect("project id").to_string()
}

async fn chat_in(daemon: &TestDaemon, folder: &Path) -> String {
    let created = call(
        daemon,
        "POST",
        "/sessions",
        Some(json!({
            "request_id": uuid::Uuid::new_v4().to_string(),
            "harness": "claude",
            "workspace": folder,
        })),
    )
    .await;
    assert_eq!(created.status, 201, "{}", created.raw);
    created.body["id"].as_str().expect("chat id").to_string()
}

async fn tool_names(daemon: &TestDaemon, chat: &str) -> Vec<String> {
    let listed = call(
        daemon,
        "GET",
        &format!("/sessions/{chat}/plugins/tools"),
        None,
    )
    .await;
    assert_eq!(listed.status, 200, "{}", listed.raw);
    listed.body["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap().to_string())
        .collect()
}

async fn call_hello(daemon: &TestDaemon, chat: &str) -> Value {
    let reply = call(
        daemon,
        "POST",
        &format!("/sessions/{chat}/plugins/call"),
        Some(json!({"name": "wasm-tools__hello", "arguments": {}})),
    )
    .await;
    assert_eq!(reply.status, 200, "{}", reply.raw);
    reply.body
}

#[tokio::test]
async fn who_a_plugin_is_for_never_widens_what_it_may_do() {
    let daemon = start_daemon(&[]).await;
    let folder = wasm_plugin();
    let repo = workspace(None);
    let p = project(&daemon, repo.path()).await;
    // Every scope, each asked for with powers slipped into the request: the
    // record holds what the review read from the plugin, and nothing more.
    let greedy = json!(["run_programs", "network", "read_chats", "microphone"]);
    for (scope, project) in [
        ("machine", None),
        ("global", None),
        ("project_local", Some(p.clone())),
        ("project", Some(p.clone())),
    ] {
        let stage_id = stage_folder(&daemon, folder.path()).await;
        let mut extra = json!({"scope": scope, "powers": greedy, "not_sandboxed": false});
        if let Some(project) = &project {
            extra["project"] = json!(project);
        }
        let installed = approve(&daemon, &stage_id, extra).await;
        assert_eq!(installed.status, 200, "{scope}: {}", installed.raw);
        let record = &installed.body["result"];
        assert_eq!(record["scope"], scope);
        assert_eq!(record["powers"], json!(["agent_tools"]), "{scope}");
        let id = record["id"].as_str().unwrap().to_string();
        let listed = call(&daemon, "GET", "/plugins", None).await;
        let here = listed.body["installed"]
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p["id"] == id)
            .cloned()
            .expect("listed");
        assert_eq!(here["powers"], json!(["agent_tools"]), "{scope}");
        remove(&daemon, &id).await;
    }
    // The Global install told the account, and its removal did too.
    let records = call(&daemon, "GET", "/sync/plugins", None).await;
    assert_eq!(records.status, 200, "{}", records.raw);
    assert_eq!(records.body["plugins"][0]["name"], "wasm-tools");
    assert_eq!(records.body["plugins"][0]["removed"], true);
}

#[tokio::test]
async fn a_project_plugin_runs_only_for_its_own_projects_chats() {
    let daemon = start_daemon(&[]).await;
    let folder = wasm_plugin();
    let (repo_a, repo_b, loose) = (workspace(None), workspace(None), workspace(None));
    let a = project(&daemon, repo_a.path()).await;
    let _b = project(&daemon, repo_b.path()).await;

    let stage_id = stage_folder(&daemon, folder.path()).await;
    let installed = approve(
        &daemon,
        &stage_id,
        json!({"scope": "project", "project": a}),
    )
    .await;
    assert_eq!(installed.status, 200, "{}", installed.raw);
    let id = installed.body["result"]["id"].as_str().unwrap().to_string();
    assert_eq!(id, format!("wasm-tools@local~{a}"));
    // Its files went into the repo, with a note of where it came from.
    let in_repo = repo_a.path().join(".openremote/plugins/wasm-tools");
    assert!(in_repo.join("tool.wasm").is_file());
    let index = std::fs::read_to_string(repo_a.path().join(".openremote/plugins.json")).unwrap();
    assert!(index.contains("wasm-tools") && !index.contains(&*folder.path().to_string_lossy()));

    let (in_a, in_b, elsewhere) = (
        chat_in(&daemon, repo_a.path()).await,
        chat_in(&daemon, repo_b.path()).await,
        chat_in(&daemon, loose.path()).await,
    );
    assert_eq!(tool_names(&daemon, &in_a).await, ["wasm-tools__hello"]);
    let result = call_hello(&daemon, &in_a).await;
    assert_eq!(result["content"][0]["text"], "wasm", "{result}");
    for chat in [&in_b, &elsewhere] {
        assert!(tool_names(&daemon, chat).await.is_empty());
        let refused = call_hello(&daemon, chat).await;
        assert_eq!(refused["isError"], true, "{refused}");
    }

    // Someone changes the plugin in the repo: it stops running until this
    // machine reviews the new files.
    std::fs::write(in_repo.join("added.txt"), "changed upstream").unwrap();
    assert!(tool_names(&daemon, &in_a).await.is_empty());
    assert_eq!(call_hello(&daemon, &in_a).await["isError"], true);
    let listed = call(&daemon, "GET", "/plugins", None).await;
    assert_eq!(listed.body["found"][0]["why"], "changed", "{}", listed.raw);
    assert_eq!(listed.body["found"][0]["project"], a);
    // Reviewed and approved again, for its project: it runs again.
    let staged = stage_source(
        &daemon,
        json!({"kind": "project", "project": a, "name": "wasm-tools"}),
    )
    .await;
    assert_eq!(staged.status, 200, "{}", staged.raw);
    assert_eq!(staged.body["result"]["for_project"], a);
    assert_eq!(staged.body["result"]["id"], id);
    let stage_id = staged.body["result"]["stage_id"].as_str().unwrap();
    let again = approve(&daemon, stage_id, json!({"scope": "project", "project": a})).await;
    assert_eq!(again.status, 200, "{}", again.raw);
    assert_eq!(tool_names(&daemon, &in_a).await, ["wasm-tools__hello"]);
    let listed = call(&daemon, "GET", "/plugins", None).await;
    assert_eq!(listed.body["found"], json!([]));

    // Removed: its files leave the repo too.
    remove(&daemon, &id).await;
    assert!(!in_repo.exists());
}

#[tokio::test]
async fn a_project_install_names_its_project_and_clashes_only_where_chats_meet() {
    let daemon = start_daemon(&[]).await;
    let folder = wasm_plugin();
    let repo = workspace(None);
    let p = project(&daemon, repo.path()).await;
    for (scope, project, status) in [
        ("project", None, 400),
        ("project_local", None, 400),
        ("project", Some(uuid::Uuid::new_v4().to_string()), 404),
    ] {
        let stage_id = stage_folder(&daemon, folder.path()).await;
        let mut extra = json!({"scope": scope});
        if let Some(project) = project {
            extra["project"] = json!(project);
        }
        let refused = approve(&daemon, &stage_id, extra).await;
        assert_eq!(refused.status, status, "{scope}: {}", refused.raw);
    }
    let listed = call(&daemon, "GET", "/plugins", None).await;
    assert_eq!(listed.body["installed"], json!([]), "nothing was installed");

    // Installed for this machine, the same plugin for one project would
    // ride that project's chats twice: refused, in its own words.
    let stage_id = stage_folder(&daemon, folder.path()).await;
    let machine = approve(&daemon, &stage_id, json!({})).await;
    assert_eq!(machine.status, 200, "{}", machine.raw);
    let stage_id = stage_folder(&daemon, folder.path()).await;
    let clash = approve(
        &daemon,
        &stage_id,
        json!({"scope": "project_local", "project": p}),
    )
    .await;
    assert_eq!(clash.status, 409, "{}", clash.raw);
    assert!(
        clash.raw.contains("already installed for this machine"),
        "{}",
        clash.raw
    );
    assert!(!clash.raw.contains("from local"), "{}", clash.raw);
}

/// Two of an account's devices: a Global plugin installed on one reaches
/// the other as news to review. Nothing installs, and no power is held,
/// until the user approves it there - and then its powers are the ones read
/// from the copy on that device.
#[tokio::test]
async fn a_global_plugin_from_another_device_waits_for_this_devices_approval() {
    let cloud = FakeCloud::start().await;
    let (a, _id_a) = device(&cloud, "desk-a").await;
    let (b, id_b) = device(&cloud, "desk-b").await;
    until("a sees b online", || {
        common::cloud::sees(&a, &id_b, "online", json!(true))
    })
    .await;

    let folder = wasm_plugin();
    let stage_id = stage_folder(&a, folder.path()).await;
    let installed = approve(&a, &stage_id, json!({"scope": "global"})).await;
    assert_eq!(installed.status, 200, "{}", installed.raw);

    // b hears of it, and lists it for review - installed, it is not.
    let pending = until("b lists the account's plugin", || async {
        let listed = call(&b, "GET", "/plugins", None).await;
        let pending = listed.body["global_pending"].as_array()?.first()?.clone();
        Some((pending, listed.body["installed"].clone()))
    })
    .await;
    assert_eq!(pending.0["id"], "wasm-tools@local");
    assert_eq!(pending.0["why"], "missing");
    assert_eq!(pending.1, json!([]), "a record installs nothing");
    let chat = chat_in(&b, workspace(None).path()).await;
    assert!(tool_names(&b, &chat).await.is_empty());
    assert_eq!(call_hello(&b, &chat).await["isError"], true);

    // The user reviews it on b: the copy comes from a, exactly as approved,
    // through the same review as any install.
    let staged = stage_source(&b, json!({"kind": "account", "id": "wasm-tools@local"})).await;
    assert_eq!(staged.status, 200, "{}", staged.raw);
    let review = &staged.body["result"];
    assert_eq!(review["for_account"], true);
    assert_eq!(review["manifest"]["powers"], json!(["agent_tools"]));
    assert!(
        review["scopes"]
            .as_array()
            .unwrap()
            .contains(&json!("global"))
    );
    // Closing the review installs nothing.
    let cancelled = call(
        &b,
        "POST",
        &format!(
            "/plugins/stage/{}/cancel",
            review["stage_id"].as_str().unwrap()
        ),
        Some(json!({"request_id": uuid::Uuid::new_v4().to_string()})),
    )
    .await;
    assert_eq!(cancelled.status, 200, "{}", cancelled.raw);
    assert_eq!(
        call(&b, "GET", "/plugins", None).await.body["installed"],
        json!([])
    );

    // Approved: now it is b's, with the powers b's review read.
    let staged = stage_source(&b, json!({"kind": "account", "id": "wasm-tools@local"})).await;
    let stage_id = staged.body["result"]["stage_id"].as_str().unwrap();
    let approved = approve(&b, stage_id, json!({"scope": "global"})).await;
    assert_eq!(approved.status, 200, "{}", approved.raw);
    assert_eq!(approved.body["result"]["powers"], json!(["agent_tools"]));
    assert_eq!(tool_names(&b, &chat).await, ["wasm-tools__hello"]);
    let listed = call(&b, "GET", "/plugins", None).await;
    assert_eq!(listed.body["global_pending"], json!([]), "{}", listed.raw);

    // Removed from every device on a: b's copy goes too.
    remove(&a, "wasm-tools@local").await;
    until("b lets it go", || async {
        let listed = call(&b, "GET", "/plugins", None).await;
        (listed.body["installed"] == json!([])).then_some(())
    })
    .await;
    assert!(tool_names(&b, &chat).await.is_empty());
}

/// The account's routes are open to every device of the account, and
/// answer only with what a review on that device then checks.
#[tokio::test]
async fn a_peer_reaches_the_records_and_only_a_global_copy() {
    use std::sync::Arc;
    use tower::ServiceExt;
    let (daemon, app) = start_embedded(dead_cloud()).await;
    let folder = wasm_plugin();
    let stage_id = stage_folder(&daemon, folder.path()).await;
    let machine = approve(&daemon, &stage_id, json!({})).await;
    assert_eq!(machine.status, 200, "{}", machine.raw);
    let peer = openremote_daemon::http::peer_router(Arc::clone(&app));
    let get = |uri: String| {
        let peer = peer.clone();
        async move {
            let request = axum::http::Request::builder()
                .uri(uri)
                .body(axum::body::Body::empty())
                .unwrap();
            peer.oneshot(request).await.unwrap().status().as_u16()
        }
    };
    assert_eq!(get("/sync/plugins".into()).await, 200);
    // A plugin this device has for itself is not the account's to copy.
    let hash = "0".repeat(64);
    assert_eq!(
        get(format!(
            "/sync/plugins/wasm-tools@local/archive?resolved={hash}"
        ))
        .await,
        404
    );
}

/// The source text between `from` and the next `until`.
fn between<'a>(source: &'a str, from: &str, until: &str) -> &'a str {
    let start = source.find(from).expect("the start");
    let end = source[start..].find(until).expect("the end");
    &source[start..start + end]
}

/// Every install path asks the tool-name question twice: before it touches
/// anything, and again under the store's lock right before the record is
/// written, so a second install that slipped in between can't take the
/// same tool names. Two installs can't be raced on demand, so the order is
/// pinned in the source.
#[test]
fn an_install_asks_about_tool_names_again_under_the_lock_before_the_record() {
    let source = include_str!("../src/plugin_runtime/install.rs");
    let body = between(source, "pub async fn install(", "pub async fn remove(");
    let first = body.find("tool_name_clash").expect("asked first");
    let lock = body
        .rfind("let mut store = lock_recovering")
        .expect("the lock");
    let second = body.rfind("tool_name_clash").expect("asked again");
    let record = body.find("put_installed_plugin").expect("the record");
    assert!(first < lock && lock < second && second < record);
    // The route and the account's copy both go through this one install.
    let http = include_str!("../src/http.rs");
    let route = between(http, "async fn install_staged(", "async fn cancel_staged(");
    assert!(route.contains("plugin_runtime::install::install("));
    assert!(
        !route.contains("put_installed_plugin"),
        "no second way to write a record"
    );
}

/// A command plugin made again before a chat replaces its folder: a program
/// still running the old copy is stopped (pinned: running a command plugin
/// needs a marketplace and a harness chat).
#[test]
fn a_command_plugin_made_again_stops_its_old_program() {
    let supervisor = include_str!("../src/supervisor.rs");
    let rerun = between(
        supervisor,
        "async fn rerun_plugin_commands(",
        "pub fn remove_mcp_server(",
    );
    let record = rerun.find("put_installed_plugin").expect("the record");
    let stop = rerun.find("plugin_changed").expect("the stop");
    assert!(record < stop);
    let app = include_str!("../src/app.rs");
    let wired = between(app, "set_plugin_changed(", "});");
    assert!(wired.contains("runtime.changed(&id)"), "{wired}");
}
