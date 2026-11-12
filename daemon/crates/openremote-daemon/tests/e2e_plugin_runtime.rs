//! The plugin runtime end to end (plan step 4): a process plugin's agent
//! tools, view request and settings, and a WASM plugin's tool - each
//! installed the way a person installs it (stage a folder, approve it),
//! each reached the way a chat's MCP server and the console reach it.

mod common;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use common::*;
use serde_json::{Value, json};

/// The driver fixture doubles as a process plugin's program
/// (`--plugin-rpc`); built once per test process, as the harness e2e does.
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
            let status = std::process::Command::new("cargo")
                .args(["build", "-p", "fixture-driver"])
                .current_dir(env!("CARGO_MANIFEST_DIR"))
                .status()
                .expect("spawn cargo to build the fixture driver");
            assert!(status.success(), "cargo build -p fixture-driver failed");
            target.join("debug").join(name)
        })
        .clone()
}

/// Stage `folder` and approve it; the install record comes back.
async fn install(daemon: &TestDaemon, folder: &Path, tag: &str) -> Value {
    let staged = stage(daemon, folder, tag).await;
    assert_eq!(staged.status, 200, "{}", staged.raw);
    let stage_id = staged.body["result"]["stage_id"]
        .as_str()
        .expect("stage id")
        .to_string();
    let installed = call(
        daemon,
        "POST",
        &format!("/plugins/stage/{stage_id}/install"),
        Some(json!({"request_id": format!("{tag}-install")})),
    )
    .await;
    assert_eq!(installed.status, 200, "{}", installed.raw);
    installed.body["result"].clone()
}

async fn stage(daemon: &TestDaemon, folder: &Path, tag: &str) -> Reply {
    call(
        daemon,
        "POST",
        "/plugins/stage",
        Some(json!({
            "request_id": format!("{tag}-stage"),
            "source": {"kind": "folder", "path": folder}
        })),
    )
    .await
}

async fn new_chat(daemon: &TestDaemon, tag: &str) -> (String, tempfile::TempDir) {
    let workspace = workspace(None);
    let created = call(
        daemon,
        "POST",
        "/sessions",
        Some(json!({
            "request_id": format!("{tag}-chat"),
            "harness": "claude",
            "workspace": workspace.path(),
        })),
    )
    .await;
    assert_eq!(created.status, 201, "{}", created.raw);
    (
        created.body["id"].as_str().expect("chat id").to_string(),
        workspace,
    )
}

async fn tool(daemon: &TestDaemon, chat: &str, name: &str, arguments: Value) -> Value {
    let reply = call(
        daemon,
        "POST",
        &format!("/sessions/{chat}/plugins/call"),
        Some(json!({"name": name, "arguments": arguments})),
    )
    .await;
    assert_eq!(reply.status, 200, "{}", reply.raw);
    reply.body
}

fn text(result: &Value) -> String {
    result["content"][0]["text"]
        .as_str()
        .unwrap_or_default()
        .to_string()
}

/// A process plugin whose program is the driver fixture, shipped in its own
/// folder.
fn process_plugin() -> tempfile::TempDir {
    let folder = tempfile::tempdir().expect("plugin folder");
    let program = if cfg!(windows) {
        "bin/plugin.exe"
    } else {
        "bin/plugin"
    };
    std::fs::create_dir_all(folder.path().join("bin")).unwrap();
    std::fs::create_dir_all(folder.path().join("ui")).unwrap();
    std::fs::copy(fixture_driver(), folder.path().join(program)).unwrap();
    std::fs::write(folder.path().join("ui/index.html"), "<p>tools</p>").unwrap();
    std::fs::write(
        folder.path().join("openremote.json"),
        json!({
            "api": 1,
            "name": "fixture-tools",
            "version": "0.1.0",
            "runtime": "process",
            "entry": {"command": format!("${{OPENREMOTE_PLUGIN_ROOT}}/{program}"), "args": ["--plugin-rpc"]},
            "contributes": {
                "tools": [
                    {"name": "echo", "description": "Echo what it was handed", "input_schema": {"type": "object"}},
                    {"name": "ask", "description": "Ask OpenRemote for its chats"},
                    {"name": "env", "description": "Say what reached it from the environment"}
                ],
                "views": [{"id": "home", "title": "Home", "place": "sidebar", "entry": "ui/index.html"}],
                "settings": [{"key": "token", "type": "secret", "label": "Token", "required": true}]
            }
        })
        .to_string(),
    )
    .unwrap();
    folder
}

#[tokio::test]
async fn a_process_plugins_tools_run_in_its_program_with_only_the_approved_powers() {
    let daemon = start_daemon(&[]).await;
    let folder = process_plugin();

    // The review shows what it runs and adds, and the power its tools imply.
    let staged = stage(&daemon, folder.path(), "pp-review").await;
    let manifest = &staged.body["result"]["manifest"];
    assert!(
        manifest["entry"]
            .as_str()
            .unwrap_or_default()
            .contains("--plugin-rpc"),
        "{}",
        staged.raw
    );
    assert_eq!(manifest["contributes"]["tools"][0]["name"], "echo");
    let powers = manifest["powers"].as_array().expect("powers");
    assert!(powers.contains(&json!("agent_tools")), "{}", staged.raw);
    assert!(powers.contains(&json!("run_programs")), "{}", staged.raw);
    assert!(!powers.contains(&json!("read_chats")), "{}", staged.raw);

    let record = install(&daemon, folder.path(), "pp").await;
    let id = record["id"].as_str().expect("plugin id").to_string();
    assert_eq!(id, "fixture-tools@local");
    assert_eq!(record["contributes"]["tools"][1]["name"], "ask");
    assert_eq!(record["contributes"]["views"][0]["id"], "home");
    assert_eq!(
        record["process"]["state"], "stopped",
        "nothing runs before a need"
    );

    let (chat, _workspace) = new_chat(&daemon, "pp").await;
    let listed = call(
        &daemon,
        "GET",
        &format!("/sessions/{chat}/plugins/tools"),
        None,
    )
    .await;
    let names: Vec<&str> = listed.body["tools"]
        .as_array()
        .expect("tools")
        .iter()
        .filter_map(|t| t["name"].as_str())
        .collect();
    assert_eq!(
        names,
        [
            "fixture-tools__echo",
            "fixture-tools__ask",
            "fixture-tools__env"
        ]
    );

    // A required setting unset: the program doesn't start.
    let refused = tool(&daemon, &chat, "fixture-tools__echo", json!({"x": 1})).await;
    assert_eq!(refused["isError"], true);
    assert!(
        text(&refused).contains("needs Token set first"),
        "{refused}"
    );

    // The secret is stored and answered as set, never shown again.
    let set = call(
        &daemon,
        "POST",
        &format!("/plugins/{id}/settings"),
        Some(json!({"values": {"token": "s3cret-value"}})),
    )
    .await;
    assert_eq!(set.status, 200, "{}", set.raw);
    assert_eq!(set.body["secrets"]["token"], "set");
    assert!(!set.raw.contains("s3cret-value"), "{}", set.raw);
    let read = call(&daemon, "GET", &format!("/plugins/{id}/settings"), None).await;
    assert!(!read.raw.contains("s3cret-value"), "{}", read.raw);
    let bad = call(
        &daemon,
        "POST",
        &format!("/plugins/{id}/settings"),
        Some(json!({"values": {"undeclared": 1}})),
    )
    .await;
    assert_eq!(bad.status, 400, "{}", bad.raw);

    // Now it runs, and the program is handed the secret and its powers.
    let echoed = tool(&daemon, &chat, "fixture-tools__echo", json!({"x": 1})).await;
    let said = text(&echoed);
    assert!(said.contains(r#"echo {"x":1}"#), "{echoed}");
    assert!(said.contains("token:yes"), "{echoed}");
    assert!(said.contains(&chat), "{echoed}");
    assert!(
        said.contains("agent_tools") && said.contains("run_programs"),
        "{echoed}"
    );
    assert!(!said.contains("read_chats"), "{echoed}");

    // The daemon's environment stays the daemon's. This test's process is
    // the daemon, and cargo set CARGO_PKG_NAME on it (standing in for any
    // token or key the daemon was handed): it never reaches the program,
    // while PATH does.
    assert!(std::env::var_os("CARGO_PKG_NAME").is_some());
    let env = text(
        &tool(
            &daemon,
            &chat,
            "fixture-tools__env",
            json!({"name": "CARGO_PKG_NAME"}),
        )
        .await,
    );
    assert_eq!(env, "CARGO_PKG_NAME:false path:true");

    // Its own call to OpenRemote needs a power it wasn't given.
    let asked = tool(&daemon, &chat, "fixture-tools__ask", json!({})).await;
    let said = text(&asked);
    assert!(said.contains("-32003"), "{asked}");
    assert!(said.contains("power this plugin wasn't given"), "{asked}");

    // A tool it doesn't declare isn't run.
    let missing = tool(&daemon, &chat, "fixture-tools__rm_rf", json!({})).await;
    assert_eq!(missing["isError"], true);

    // Its view asks its program.
    let asked = call(
        &daemon,
        "POST",
        &format!("/plugins/{id}/request"),
        Some(json!({"name": "ping", "params": {}})),
    )
    .await;
    assert_eq!(asked.status, 200, "{}", asked.raw);
    assert_eq!(asked.body["result"]["pong"], "ping");

    // Its view's page, from inside its folder only, as a sandboxed document.
    let page = raw_http(
        &daemon,
        "GET",
        &format!("/plugins/{id}/files/ui/index.html"),
        &[],
        None,
    )
    .await;
    assert!(page.text().contains("<p>tools</p>"), "{}", page.text());
    assert_eq!(
        page.header("content-security-policy"),
        Some("sandbox allow-scripts")
    );
    for escape in [
        "..%2Fopenremote.json",
        "ui/..%2F..%2F..%2Fstate.json",
        "%2E%2E/x",
    ] {
        let reply = call(
            &daemon,
            "GET",
            &format!("/plugins/{id}/files/{escape}"),
            None,
        )
        .await;
        assert_ne!(reply.status, 200, "{escape}: {}", reply.raw);
    }

    let plugins = call(&daemon, "GET", "/plugins", None).await;
    assert_eq!(plugins.body["installed"][0]["process"]["state"], "running");

    // Off: its tools go from chats and its program stops.
    let off = call(
        &daemon,
        "POST",
        &format!("/plugins/{id}/enabled"),
        Some(json!({"request_id": "pp-off", "enabled": false})),
    )
    .await;
    assert_eq!(off.status, 200, "{}", off.raw);
    assert_eq!(off.body["result"]["process"]["state"], "stopped");
    let listed = call(
        &daemon,
        "GET",
        &format!("/sessions/{chat}/plugins/tools"),
        None,
    )
    .await;
    assert_eq!(listed.body["tools"], json!([]));
    let refused = tool(&daemon, &chat, "fixture-tools__echo", json!({})).await;
    assert_eq!(refused["isError"], true);
    let page = call(
        &daemon,
        "GET",
        &format!("/plugins/{id}/files/ui/index.html"),
        None,
    )
    .await;
    assert_eq!(page.status, 404);

    // Removed: its settings go with it, secrets included.
    let settings_file = daemon
        .data_dir()
        .join("plugin-settings")
        .join(format!("{id}.json"));
    assert!(settings_file.is_file());
    let removed = call(
        &daemon,
        "POST",
        &format!("/plugins/{id}/remove"),
        Some(json!({"request_id": "pp-remove"})),
    )
    .await;
    assert_eq!(removed.status, 200, "{}", removed.raw);
    assert!(!settings_file.exists());
}

#[tokio::test]
async fn a_process_plugin_whose_program_leaves_its_folder_is_refused() {
    let daemon = start_daemon(&[]).await;
    let folder = tempfile::tempdir().expect("plugin folder");
    let outside = fixture_driver();
    for (n, command) in [
        outside.to_string_lossy().into_owned(),
        "../outside".to_string(),
        "node".to_string(),
    ]
    .into_iter()
    .enumerate()
    {
        std::fs::write(
            folder.path().join("openremote.json"),
            json!({
                "api": 1,
                "name": "leaver",
                "runtime": "process",
                "entry": {"command": command, "args": ["--plugin-rpc"]},
                "contributes": {"tools": [{"name": "t", "description": "d"}]}
            })
            .to_string(),
        )
        .unwrap();
        let staged = stage(&daemon, folder.path(), &format!("leaver-{n}")).await;
        assert_eq!(staged.status, 400, "{command}: {}", staged.raw);
        assert!(staged.raw.contains("its program"), "{}", staged.raw);
    }
}

/// A WASM tool module in the text format, around the contract's exports;
/// `imports` go first. Its answer is `{"content":[{"type":"text","text":"wasm"}]}`.
fn wasm_plugin(imports: &str, powers: &[&str]) -> tempfile::TempDir {
    let folder = tempfile::tempdir().expect("plugin folder");
    let answer = r#"{"content":[{"type":"text","text":"wasm"}]}"#;
    let wat = format!(
        r#"(module
            {imports}
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
            "powers": powers,
            "entry": "tool.wasm",
            "contributes": {"tools": [{"name": "hello", "description": "Say hello"}]}
        })
        .to_string(),
    )
    .unwrap();
    folder
}

#[tokio::test]
async fn a_wasm_plugins_tool_runs_sandboxed_and_its_imports_are_checked_at_review() {
    let daemon = start_daemon(&[]).await;

    // Asking for the network's functions without the network power: refused.
    let asks = wasm_plugin(
        r#"(import "openremote" "http" (func (param i32 i32) (result i32)))"#,
        &[],
    );
    let staged = stage(&daemon, asks.path(), "wasm-net").await;
    assert_eq!(staged.status, 400, "{}", staged.raw);
    assert!(staged.raw.contains("network"), "{}", staged.raw);
    // Asking for WASI: refused whatever the powers.
    let wasi = wasm_plugin(
        r#"(import "wasi_snapshot_preview1" "fd_write" (func (param i32 i32 i32 i32) (result i32)))"#,
        &["network"],
    );
    let staged = stage(&daemon, wasi.path(), "wasm-wasi").await;
    assert_eq!(staged.status, 400, "{}", staged.raw);

    let folder = wasm_plugin("", &[]);
    let record = install(&daemon, folder.path(), "wasm").await;
    assert_eq!(record["runtime"], "wasm");
    assert_eq!(
        record["powers"],
        json!(["agent_tools"]),
        "WASM runs no program"
    );
    let (chat, _workspace) = new_chat(&daemon, "wasm").await;
    let result = tool(&daemon, &chat, "wasm-tools__hello", json!({})).await;
    assert_eq!(text(&result), "wasm", "{result}");
}

/// The runtime's routes act for the user on this device: another of the
/// user's devices never reaches them through the peer API.
#[tokio::test]
async fn the_plugin_routes_answer_this_devices_console_only() {
    use tower::ServiceExt;
    let (_daemon, app) = start_embedded(dead_cloud()).await;
    let peer = openremote_daemon::http::peer_router(Arc::clone(&app));
    for (method, path) in [
        ("GET", "/plugins/x@local/settings"),
        ("POST", "/plugins/x@local/settings"),
        ("GET", "/plugins/x@local/files/ui/index.html"),
        ("POST", "/plugins/x@local/request"),
        (
            "GET",
            "/sessions/00000000-0000-0000-0000-000000000000/plugins/tools",
        ),
        (
            "POST",
            "/sessions/00000000-0000-0000-0000-000000000000/plugins/call",
        ),
    ] {
        let request = axum::http::Request::builder()
            .method(method)
            .uri(path)
            .header("content-type", "application/json")
            .body(axum::body::Body::from("{}"))
            .unwrap();
        let response = peer.clone().oneshot(request).await.unwrap();
        assert!(
            matches!(response.status().as_u16(), 403 | 404 | 405),
            "{method} {path}: {}",
            response.status()
        );
    }
    // A daemon-token request without the token is refused too.
    let daemon = start_daemon(&[]).await;
    let reply = call_with_token(&daemon, "wrong", "GET", "/plugins/x@local/settings", None).await;
    assert_eq!(reply.status, 401, "{}", reply.raw);
}

/// An update stops the plugin's program before its folder is replaced and
/// again once the new folder and record are in place, so a tool call that
/// started the old program during the swap can't keep it running. The
/// window can't be hit on demand, so the order is pinned in the source.
#[test]
fn an_update_stops_the_program_on_both_sides_of_the_swap() {
    let source = include_str!("../src/plugin_runtime/install.rs");
    let start = source.find("pub async fn install(").expect("install");
    let body = &source[start
        ..start
            + source[start..]
                .find("pub async fn remove(")
                .expect("the next fn")];
    let first = body.find("plugin_runtime.changed").expect("a stop before");
    let install = body
        .find("app.plugins.install(staged)")
        .expect("the install");
    let record = body.find("put_installed_plugin").expect("the record");
    let second = body.rfind("plugin_runtime.changed").expect("a stop after");
    assert!(first < install && install < record && record < second);
    // The 409 naming live chats still comes first.
    assert!(body.find("plugin_chats").expect("the 409") < first);
}

/// A project's process plugin whose files in the repo change after its
/// review: its running program is stopped, and a view's next call does not
/// reach it.
#[tokio::test]
async fn a_project_process_plugin_stops_when_its_repo_copy_changes() {
    let daemon = start_daemon(&[]).await;
    let folder = process_plugin();
    let repo = workspace(None);
    let saved = call(
        &daemon,
        "POST",
        "/projects",
        Some(json!({"request_id": "proj", "folders": [repo.path()]})),
    )
    .await;
    assert_eq!(saved.status, 201, "{}", saved.raw);
    let project = saved.body["id"].as_str().unwrap().to_string();
    let staged = stage(&daemon, folder.path(), "proj-pp").await;
    let stage_id = staged.body["result"]["stage_id"].as_str().unwrap();
    let installed = call(
        &daemon,
        "POST",
        &format!("/plugins/stage/{stage_id}/install"),
        Some(json!({"request_id": "proj-pp-install", "scope": "project", "project": project})),
    )
    .await;
    assert_eq!(installed.status, 200, "{}", installed.raw);
    let id = installed.body["result"]["id"].as_str().unwrap().to_string();
    let route = |tail: &str| format!("/plugins/{}/{tail}", id.replace('~', "%7E"));
    let set = call(
        &daemon,
        "POST",
        &route("settings"),
        Some(json!({"values": {"token": "t"}})),
    )
    .await;
    assert_eq!(set.status, 200, "{}", set.raw);
    let request = route("request");
    let ask = || {
        call(
            &daemon,
            "POST",
            &request,
            Some(json!({"name": "ping", "params": {}})),
        )
    };
    let asked = ask().await;
    assert_eq!(asked.status, 200, "{}", asked.raw);
    let state = |body: &Value| body["installed"][0]["process"]["state"].clone();
    assert_eq!(
        state(&call(&daemon, "GET", "/plugins", None).await.body),
        "running"
    );

    std::fs::write(
        repo.path()
            .join(".openremote/plugins/fixture-tools/ui/index.html"),
        "<p>changed upstream</p>",
    )
    .unwrap();
    let asked = ask().await;
    assert_ne!(asked.status, 200, "{}", asked.raw);
    assert_eq!(
        state(&call(&daemon, "GET", "/plugins", None).await.body),
        "stopped"
    );
}
