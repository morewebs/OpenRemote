//! Plugins written in a sibling plugin layout - the one Claude Code's was
//! the pattern for - install rewritten for Claude Code, and the review says
//! what changed and what was left out. A catalog keeps its marketplace.json
//! at the top of its folder with a pluginRoot above its relative sources; a
//! plugin keeps its manifest in a folder of its own, may carry program
//! hooks and servers that sign in at its publisher, and may come as a zip
//! pinned by sha256 with the plugin in one folder of it.

mod common;

use std::path::Path;

use common::*;
use serde_json::{Value, json};
use sha2::Digest;

const MARKET: &str = "layout-tools";

fn write(dir: &Path, files: &[(&str, &str)]) {
    for (path, body) in files {
        let p = dir.join(path);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, body).unwrap();
    }
}

async fn post(daemon: &TestDaemon, path: &str, rid: &str, mut body: Value) -> Reply {
    body["request_id"] = json!(rid);
    call(daemon, "POST", path, Some(body)).await
}

async fn stage(daemon: &TestDaemon, rid: &str, plugin: &str) -> Reply {
    let source = json!({"kind": "marketplace", "marketplace": MARKET, "plugin": plugin});
    post(daemon, "/plugins/stage", rid, json!({"source": source})).await
}

async fn install(daemon: &TestDaemon, rid: &str, staged: &Reply) {
    assert_eq!(staged.status, 200, "{}", staged.raw);
    let id = staged.body["result"]["stage_id"]
        .as_str()
        .unwrap_or_else(|| panic!("staged: {}", staged.raw));
    let done = post(
        daemon,
        &format!("/plugins/stage/{id}/install"),
        rid,
        json!({}),
    )
    .await;
    assert_eq!(done.status, 200, "{}", done.raw);
}

fn find<'a>(list: &'a [Value], id: &str) -> &'a Value {
    list.iter()
        .find(|e| e["id"] == id)
        .unwrap_or_else(|| panic!("no {id} in {list:?}"))
}

fn json_at(dir: &Path, path: &str) -> Value {
    serde_json::from_slice(&std::fs::read(dir.join(path)).unwrap()).unwrap()
}

/// A zip with each file under `top/`, plus one file outside it.
fn zip_with(top: &str, files: &[(&str, &str)], outside: (&str, &str)) -> Vec<u8> {
    let mut bytes = Vec::new();
    {
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(&mut bytes));
        let options = zip::write::SimpleFileOptions::default();
        for (path, body) in files {
            zip.start_file(format!("{top}/{path}"), options).unwrap();
            std::io::Write::write_all(&mut zip, body.as_bytes()).unwrap();
        }
        zip.start_file(outside.0, options).unwrap();
        std::io::Write::write_all(&mut zip, outside.1.as_bytes()).unwrap();
        zip.finish().unwrap();
    }
    bytes
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Serve one file at `/<name>` on a listener the test bound, so the
/// catalog can name its own address.
async fn serve(listener: tokio::net::TcpListener, name: &'static str, bytes: Vec<u8>) {
    let app = axum::Router::new().route(
        &format!("/{name}"),
        axum::routing::get(move || {
            let bytes = bytes.clone();
            async move { bytes }
        }),
    );
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
}

#[tokio::test]
async fn a_plugin_in_the_sibling_layout_installs_rewritten_and_says_what_it_left_out() {
    let daemon = start_daemon(&[("claude", fixture_agent())]).await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let zip = zip_with(
        "zipped",
        &[
            (
                ".zcode-plugin/plugin.json",
                r#"{"name": "zipped", "version": "1.0.0", "skills": "skills"}"#,
            ),
            ("skills/x/SKILL.md", "# x"),
        ],
        ("elsewhere/leftover.txt", "not the plugin"),
    );
    let digest = hex(&sha2::Sha256::digest(&zip));
    serve(listener, "zipped.zip", zip).await;

    let market = tempfile::tempdir().unwrap();
    // The catalog sits at the top of its folder, with no .claude-plugin.
    write(
        market.path(),
        &[(
            "marketplace.json",
            &json!({
                "name": MARKET,
                "owner": {"name": "Test"},
                "pluginRoot": "plugins",
                "plugins": [
                    {"name": "greeter", "source": "./greeter", "version": "0.2.0",
                     "description": "Says hello", "description_i18n": {"en": "Says hello in English"}},
                    {"name": "accounts-only", "source": "./accounts-only"},
                    {"name": "zipped", "source": {"source": "url", "type": "zip",
                        "url": format!("{base}/zipped.zip"), "sha256": digest, "path": "zipped"}},
                    {"name": "tampered", "source": {"source": "url", "type": "zip",
                        "url": format!("{base}/zipped.zip"), "sha256": "0".repeat(64), "path": "zipped"}},
                    {"name": "escaping", "source": {"source": "url", "type": "zip",
                        "url": format!("{base}/zipped.zip"), "sha256": digest, "path": "../zipped"}},
                    {"name": "unpinned", "source": {"source": "url", "type": "zip",
                        "url": format!("{base}/zipped.zip")}},
                    {"name": "plain-http", "source": {"source": "url", "type": "zip",
                        "url": "http://example.com/x.zip", "sha256": digest}},
                ],
            })
            .to_string(),
        )],
    );
    write(
        &market.path().join("plugins/greeter"),
        &[
            (
                ".zcode-plugin/plugin.json",
                r#"{"name": "greeter", "version": "0.2.0", "author": "Someone",
                    "description_i18n": {"en": "Says hello in English", "fr": "Dit bonjour"},
                    "skills": "skills"}"#,
            ),
            (
                "skills/hello/SKILL.md",
                "---\nname: hello\ndescription: say hello\n---\nhello",
            ),
            (
                "hooks/hooks.json",
                r#"{"hooks": {
                    "PreToolUse": [{"matcher": "Bash", "hooks": [
                        {"type": "process", "command": "node",
                         "args": ["${ZCODE_PLUGIN_ROOT}/hooks/check.mjs"], "timeoutMs": 4500}]}],
                    "Stop": [{"hooks": [{"type": "command", "command": "echo done", "timeout": 7}]}]}}"#,
            ),
            ("hooks/check.mjs", ""),
            (
                ".mcp.json",
                r#"{"mcpServers": {
                    "data": {"type": "http", "url": "${ZCODE_BASE_URL}/mcp",
                             "auth": {"type": "zcode_official"}},
                    "local": {"command": "node", "args": ["${ZCODE_PLUGIN_ROOT}/server.mjs"],
                              "cwd": "${ZCODE_PROJECT_DIR}", "timeoutMs": 30000}}}"#,
            ),
            ("server.mjs", ""),
        ],
    );
    write(
        &market.path().join("plugins/accounts-only"),
        &[
            (
                ".zcode-plugin/plugin.json",
                r#"{"name": "accounts-only", "version": "0.1.0"}"#,
            ),
            (
                ".mcp.json",
                r#"{"mcpServers": {"data": {"type": "http", "url": "${ZCODE_BASE_URL}/mcp",
                                            "auth": {"type": "zcode_official"}}}}"#,
            ),
        ],
    );

    let added = post(
        &daemon,
        "/marketplaces",
        "s-add",
        json!({"source": market.path()}),
    )
    .await;
    assert_eq!(added.status, 200, "{}", added.raw);
    assert_eq!(added.body["result"]["plugin_count"], 7);

    // Discover reads the catalog and says what it can before install.
    let list = call(&daemon, "GET", "/marketplaces/catalog", None)
        .await
        .body["entries"]
        .as_array()
        .cloned()
        .unwrap();
    let greeter = find(&list, "greeter@layout-tools");
    assert_eq!(greeter["installable"], true, "{greeter}");
    assert_eq!(greeter["description"], "Says hello in English");
    assert_eq!(greeter["claude_parts"], json!(["skills", "hooks", "mcp"]));
    let accounts = find(&list, "accounts-only@layout-tools");
    assert_eq!(accounts["installable"], false);
    assert!(
        accounts["why_not"]
            .as_str()
            .unwrap()
            .contains("account with this plugin's publisher"),
        "{accounts}"
    );
    for (name, says) in [
        ("escaping", "leaves the download"),
        ("unpinned", "sha256"),
        ("plain-http", "https"),
    ] {
        let entry = find(&list, &format!("{name}@layout-tools"));
        assert_eq!(entry["installable"], false, "{entry}");
        assert!(entry["why_not"].as_str().unwrap().contains(says), "{entry}");
    }

    // The review: what was rewritten, and the one server left out.
    let staged = stage(&daemon, "s-greeter", "greeter").await;
    let manifest = &staged.body["result"]["manifest"];
    assert_eq!(manifest["description"], "Says hello in English");
    assert_eq!(manifest["powers"], json!(["run_programs"]));
    assert_eq!(
        manifest["left_out"],
        json!(["The MCP server data needs an account with this plugin's publisher"]),
        "{manifest}"
    );
    let adapted = manifest["adapted"].to_string();
    assert!(
        adapted.contains("hook runs as a Claude Code hook"),
        "{adapted}"
    );
    install(&daemon, "s-i-greeter", &staged).await;

    // On disk: Claude Code's manifest, its exec form with seconds, and
    // only the server that can run.
    let dir = daemon
        .data_dir()
        .join("plugins")
        .join(MARKET)
        .join("greeter");
    let claude = json_at(&dir, ".claude-plugin/plugin.json");
    assert_eq!(claude["name"], "greeter");
    assert_eq!(claude["author"], json!({"name": "Someone"}));
    assert!(
        claude.get("skills").is_none(),
        "the default folder is scanned anyway"
    );
    let hooks = json_at(&dir, "hooks/hooks.json");
    assert_eq!(
        hooks["hooks"]["PreToolUse"][0]["hooks"][0],
        json!({"type": "command", "command": "node",
               "args": ["${CLAUDE_PLUGIN_ROOT}/hooks/check.mjs"], "timeout": 5})
    );
    assert_eq!(hooks["hooks"]["Stop"][0]["hooks"][0]["timeout"], 7);
    let servers = json_at(&dir, ".mcp.json");
    assert_eq!(
        servers,
        json!({"mcpServers": {"local": {"command": "node",
            "args": ["${CLAUDE_PLUGIN_ROOT}/server.mjs"], "timeout": 30000}}})
    );

    // Nothing left to run is refused, with the reason.
    let refused = stage(&daemon, "s-accounts", "accounts-only").await;
    assert_eq!(refused.status, 400, "{}", refused.raw);
    assert!(
        refused.raw.contains("nothing in it can run here"),
        "{}",
        refused.raw
    );

    // A pinned zip unpacks only its folder; a changed one is refused.
    let zipped = stage(&daemon, "s-zipped", "zipped").await;
    install(&daemon, "s-i-zipped", &zipped).await;
    let dir = daemon
        .data_dir()
        .join("plugins")
        .join(MARKET)
        .join("zipped");
    assert!(dir.join("skills/x/SKILL.md").is_file());
    assert!(!dir.join("leftover.txt").exists());
    assert!(!dir.join("elsewhere").exists());
    let changed = stage(&daemon, "s-tampered", "tampered").await;
    assert_eq!(changed.status, 400, "{}", changed.raw);
    assert!(changed.raw.contains("sha256"), "{}", changed.raw);
}
