//! The example plugins in this repository's `examples/plugins`, installed
//! the way a person installs them and used the way a chat or a plugin's
//! frame uses them, under the rules a plugin meets here:
//!
//! - dictation installs from a copy of OpenRemote's own catalog, and its
//!   requests reach the internet, or a server on this computer only at an
//!   address the user typed into its settings;
//! - toy-harness's compiled program (no node or python from PATH) answers
//!   its agent tools from a chat with the settings and secret it was given,
//!   and its driver runs a turn of the "Toy CLI" harness;
//! - text-tools' WebAssembly module answers its tools from a chat, and its
//!   network reaches public addresses only.
//!
//! The examples are built from their own sources here, as their build
//! scripts build them, into this workspace's target folder.

mod common;

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use base64::Engine;
use common::*;
use serde_json::{Value, json};

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..")
}

fn examples_target() -> PathBuf {
    std::env::var("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target"))
        .join("examples")
}

/// Build an example's own crate (its own workspace and lock) into this
/// workspace's target folder; false when cargo could not.
fn build_example(manifest: &Path, extra: &[&str]) -> bool {
    std::process::Command::new("cargo")
        .args(["build", "--locked", "--manifest-path"])
        .arg(manifest)
        .arg("--target-dir")
        .arg(examples_target())
        .args(extra)
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn copy_tree(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        let name = entry.file_name();
        if name == "target" || name == "bin" {
            continue;
        }
        if path.is_dir() {
            copy_tree(&path, &to.join(&name));
        } else {
            std::fs::copy(&path, to.join(&name)).unwrap();
        }
    }
}

fn rid() -> String {
    uuid::Uuid::new_v4().to_string()
}

async fn post(daemon: &TestDaemon, path: &str, mut body: Value) -> Reply {
    body["request_id"] = json!(rid());
    call(daemon, "POST", path, Some(body)).await
}

/// Approve a staged install; the install record comes back.
async fn approve(daemon: &TestDaemon, staged: &Reply) -> Value {
    assert_eq!(staged.status, 200, "{}", staged.raw);
    let stage_id = staged.body["result"]["stage_id"]
        .as_str()
        .expect("stage id");
    let installed = post(
        daemon,
        &format!("/plugins/stage/{stage_id}/install"),
        json!({}),
    )
    .await;
    assert_eq!(installed.status, 200, "{}", installed.raw);
    installed.body["result"].clone()
}

async fn stage_folder(daemon: &TestDaemon, folder: &Path) -> Reply {
    post(
        daemon,
        "/plugins/stage",
        json!({"source": {"kind": "folder", "path": folder}}),
    )
    .await
}

async fn save_settings(daemon: &TestDaemon, id: &str, values: Value) {
    let saved = call(
        daemon,
        "POST",
        &format!("/plugins/{id}/settings"),
        Some(json!({"values": values})),
    )
    .await;
    assert_eq!(saved.status, 200, "{}", saved.raw);
}

async fn chat(daemon: &TestDaemon, harness: &str, workspace: &Path) -> String {
    let created = post(
        daemon,
        "/sessions",
        json!({"harness": harness, "workspace": workspace}),
    )
    .await;
    assert_eq!(created.status, 201, "{}", created.raw);
    created.body["id"].as_str().expect("chat id").to_string()
}

/// A tool call the way a chat's agent makes one, through the chat's MCP
/// route.
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

/// One route on a listener the test bound; each request's Authorization
/// header is kept for the test to read.
async fn transcription_server(heard: Arc<Mutex<Vec<String>>>) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}/v1", listener.local_addr().unwrap());
    let app = axum::Router::new().route(
        "/v1/audio/transcriptions",
        axum::routing::post(move |headers: axum::http::HeaderMap| {
            let heard = Arc::clone(&heard);
            async move {
                let auth = headers
                    .get("authorization")
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or_default()
                    .to_string();
                heard.lock().unwrap().push(auth);
                axum::Json(json!({"text": "words from this computer"}))
            }
        }),
    );
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    base
}

#[tokio::test]
async fn dictation_installs_from_openremotes_catalog_and_reaches_only_a_server_the_user_typed() {
    let daemon = start_daemon(&[]).await;

    // OpenRemote's own catalog as it ships at the repository's top, with
    // the dictation folder its entry names. The name `openremote` is held
    // for the catalog's own repository, so this local copy is added under
    // another name; the entry and the folder are the real ones.
    let market = tempfile::tempdir().unwrap();
    let mut catalog: Value = serde_json::from_slice(
        &std::fs::read(repo().join(".claude-plugin/marketplace.json")).unwrap(),
    )
    .unwrap();
    catalog["name"] = json!("openremote-local");
    let entry = catalog["plugins"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["name"] == "dictation")
        .expect("dictation in the catalog")
        .clone();
    let source = entry["source"]
        .as_str()
        .expect("a folder in the repository");
    std::fs::create_dir_all(market.path().join(".claude-plugin")).unwrap();
    std::fs::write(
        market.path().join(".claude-plugin/marketplace.json"),
        catalog.to_string(),
    )
    .unwrap();
    copy_tree(
        &repo().join(source.trim_start_matches("./")),
        &market.path().join(source.trim_start_matches("./")),
    );
    let added = post(&daemon, "/marketplaces", json!({"source": market.path()})).await;
    assert_eq!(added.status, 200, "{}", added.raw);

    // Discover reads the folder and says what it asks for before install.
    let listed = call(&daemon, "GET", "/marketplaces/catalog", None).await;
    let shown = listed.body["entries"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["id"] == "dictation@openremote-local")
        .cloned()
        .unwrap_or_else(|| panic!("{}", listed.raw));
    assert_eq!(shown["installable"], true, "{shown}");
    assert_eq!(shown["runtime"], "js", "{shown}");
    assert_eq!(shown["powers"], json!(["network", "microphone"]), "{shown}");

    // The review shows the button and the settings; the install keeps
    // exactly the reviewed powers.
    let staged = post(
        &daemon,
        "/plugins/stage",
        json!({"source": {"kind": "marketplace", "marketplace": "openremote-local", "plugin": "dictation"}}),
    )
    .await;
    let manifest = &staged.body["result"]["manifest"];
    assert_eq!(manifest["entry"], "dictation.js", "{}", staged.raw);
    assert_eq!(
        manifest["contributes"]["composer_inputs"][0]["id"], "dictate",
        "{}",
        staged.raw
    );
    let record = approve(&daemon, &staged).await;
    let id = record["id"].as_str().unwrap().to_string();
    assert_eq!(id, "dictation@openremote-local");
    assert_eq!(record["powers"], json!(["network", "microphone"]));

    // A whisper server on this computer. Its address is the plugin's
    // choice until the user types it: refused, and nothing reaches it.
    let heard = Arc::new(Mutex::new(Vec::new()));
    let base = transcription_server(Arc::clone(&heard)).await;
    let audio = base64::engine::general_purpose::STANDARD.encode(b"not really opus");
    let request = json!({
        "url": format!("{base}/audio/transcriptions"),
        "method": "POST",
        "headers": [["authorization", "Bearer {{secret:api_key}}"], ["content-type", "audio/webm"]],
        "body": audio,
    });
    let refused = call(
        &daemon,
        "POST",
        &format!("/plugins/{id}/fetch"),
        Some(request.clone()),
    )
    .await;
    assert_eq!(refused.status, 403, "{}", refused.raw);
    assert!(heard.lock().unwrap().is_empty());

    // Typed into Service address, with a key: the request goes there, with
    // the key written in on the way, and the words come back.
    save_settings(
        &daemon,
        &id,
        json!({"base_url": base, "api_key": "sk-typed-by-the-user"}),
    )
    .await;
    let answered = call(
        &daemon,
        "POST",
        &format!("/plugins/{id}/fetch"),
        Some(request),
    )
    .await;
    assert_eq!(answered.status, 200, "{}", answered.raw);
    assert_eq!(answered.body["status"], 200, "{}", answered.raw);
    let body = base64::engine::general_purpose::STANDARD
        .decode(answered.body["body"].as_str().unwrap())
        .unwrap();
    let words: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(words["text"], "words from this computer");
    assert_eq!(
        heard.lock().unwrap().as_slice(),
        ["Bearer sk-typed-by-the-user"]
    );
    assert!(!answered.raw.contains("sk-typed-by-the-user"));
}

#[tokio::test]
async fn the_toy_examples_program_answers_its_tools_from_a_chat_and_drives_its_harness() {
    let source = repo().join("examples/plugins/toy-harness");
    assert!(
        build_example(&source.join("program/Cargo.toml"), &[]),
        "cargo build of the toy example failed"
    );
    // The plugin as build.sh leaves it: its manifest and bin/toy.
    let folder = tempfile::tempdir().unwrap();
    copy_tree(&source, folder.path());
    let program = if cfg!(windows) { "toy.exe" } else { "toy" };
    std::fs::create_dir_all(folder.path().join("bin")).unwrap();
    std::fs::copy(
        examples_target().join("debug").join(program),
        folder.path().join("bin").join(program),
    )
    .unwrap();

    let daemon = start_daemon(&[("claude", fixture_agent())]).await;
    let staged = stage_folder(&daemon, folder.path()).await;
    let manifest = &staged.body["result"]["manifest"];
    assert!(
        manifest["entry"]
            .as_str()
            .unwrap_or_default()
            .contains("--tools"),
        "{}",
        staged.raw
    );
    assert_eq!(manifest["harness"]["id"], "toy", "{}", staged.raw);
    assert_eq!(
        manifest["powers"],
        json!(["run_programs", "agent_tools"]),
        "{}",
        staged.raw
    );
    let record = approve(&daemon, &staged).await;
    let id = record["id"].as_str().unwrap().to_string();
    assert_eq!(id, "toy-harness@local");

    // The tools run from a chat, with the settings and the secret the
    // program was handed (the toy only says whether one is set).
    save_settings(&daemon, &id, json!({"greeting": "Hey", "token": "pretend"})).await;
    let workspace = workspace(None);
    let in_chat = chat(&daemon, "claude", workspace.path()).await;
    let counted = tool(
        &daemon,
        &in_chat,
        "toy-harness__word_count",
        json!({"text": "one two three"}),
    )
    .await;
    assert_eq!(text(&counted), "3 words, 1 lines, 13 characters");
    let greeted = tool(
        &daemon,
        &in_chat,
        "toy-harness__greet",
        json!({"name": "Ann"}),
    )
    .await;
    assert_eq!(text(&greeted), "Hey, Ann! (a token is set)");

    // The same program is the harness driver: Toy CLI is in the machine's
    // list, and a chat on it runs a turn.
    let capabilities = call(&daemon, "GET", "/capabilities", None).await;
    let toy = capabilities.body["harnesses"]
        .as_array()
        .expect("harnesses")
        .iter()
        .find(|h| h["id"] == "toy")
        .cloned()
        .unwrap_or_else(|| panic!("{}", capabilities.raw));
    assert_eq!(toy["available"], true, "{toy}");
    assert_eq!(toy["name"], "Toy CLI");
    let on_toy = chat(&daemon, "toy", workspace.path()).await;
    let sent = post(
        &daemon,
        &format!("/sessions/{on_toy}/prompts"),
        json!({"text": "hello toy"}),
    )
    .await;
    assert!(sent.status < 300, "{}", sent.raw);
    let events = until_kinds(
        &daemon,
        &format!("/sessions/{on_toy}/events"),
        &["message.added", "turn.completed"],
    )
    .await;
    let said = events
        .iter()
        .find(|(_, p)| kind(p) == "message.added" && p["message"]["role"] == "assistant")
        .map(|(_, p)| {
            p["message"]["text"]
                .as_str()
                .unwrap_or_default()
                .to_string()
        })
        .unwrap_or_default();
    assert_eq!(said, "hello toy", "{}", dump(&events));
}

/// Whether this toolchain can build for WebAssembly.
fn has_wasm_target() -> bool {
    std::process::Command::new("rustc")
        .args(["--print", "sysroot"])
        .output()
        .ok()
        .map(|out| String::from_utf8_lossy(&out.stdout).trim().to_string())
        .is_some_and(|sysroot| {
            Path::new(&sysroot)
                .join("lib/rustlib/wasm32-unknown-unknown")
                .is_dir()
        })
}

#[tokio::test]
async fn the_text_tools_module_answers_from_a_chat_and_reaches_public_addresses_only() {
    if !has_wasm_target() {
        eprintln!(
            "skipped: this toolchain has no wasm32-unknown-unknown target (rustup target add wasm32-unknown-unknown)"
        );
        return;
    }
    let source = repo().join("examples/plugins/text-tools");
    assert!(
        build_example(
            &source.join("Cargo.toml"),
            &["--release", "--target", "wasm32-unknown-unknown"]
        ),
        "cargo build of the text-tools example failed"
    );
    // The plugin as build.sh leaves it: its manifest and text_tools.wasm.
    let folder = tempfile::tempdir().unwrap();
    std::fs::copy(
        source.join("openremote.json"),
        folder.path().join("openremote.json"),
    )
    .unwrap();
    std::fs::copy(
        examples_target().join("wasm32-unknown-unknown/release/text_tools.wasm"),
        folder.path().join("text_tools.wasm"),
    )
    .unwrap();

    let daemon = start_daemon(&[("claude", fixture_agent())]).await;
    let staged = stage_folder(&daemon, folder.path()).await;
    assert_eq!(
        staged.body["result"]["manifest"]["powers"],
        json!(["agent_tools", "network"]),
        "{}",
        staged.raw
    );
    let record = approve(&daemon, &staged).await;
    let id = record["id"].as_str().unwrap().to_string();

    let workspace = workspace(None);
    let in_chat = chat(&daemon, "claude", workspace.path()).await;
    let slug = tool(
        &daemon,
        &in_chat,
        "text-tools__slugify",
        json!({"text": "Hello, Plugin World"}),
    )
    .await;
    assert_eq!(text(&slug), "hello-plugin-world", "{slug}");
    // Its settings reach the module on every call.
    save_settings(&daemon, &id, json!({"separator": "_"})).await;
    let slug = tool(
        &daemon,
        &in_chat,
        "text-tools__slugify",
        json!({"text": "Hello, Plugin World"}),
    )
    .await;
    assert_eq!(text(&slug), "hello_plugin_world", "{slug}");
    let stats = tool(
        &daemon,
        &in_chat,
        "text-tools__stats",
        json!({"text": "One. Two!"}),
    )
    .await;
    assert_eq!(
        text(&stats),
        "2 words, 1 lines, 2 sentences, 9 characters",
        "{stats}"
    );

    // The network power reaches the internet, never this machine: the
    // daemon's own address is refused before anything connects.
    let mine = tool(
        &daemon,
        &in_chat,
        "text-tools__page_title",
        json!({"url": format!("http://{}/plugins", daemon.addr)}),
    )
    .await;
    assert_eq!(mine["isError"], true, "{mine}");
    assert!(text(&mine).starts_with("could not fetch"), "{mine}");
}
