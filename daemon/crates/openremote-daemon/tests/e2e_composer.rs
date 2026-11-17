//! The composer's / and @: what a chat's harness can run (its own list,
//! asked once per workspace and asked again when the plugins change or a
//! running chat reports a different one), the installed plugins' commands
//! (dimmed on a harness that can't run them), Codex's skills and mentions
//! sent as its own input items, @ file suggestions that respect
//! .gitignore, and the read-a-chat tool that reads only the chats a chat
//! was pointed at.

mod common;

use std::path::Path;
use std::process::Stdio;

use common::*;
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

fn write(dir: &Path, files: &[(&str, &str)]) {
    for (path, body) in files {
        let p = dir.join(path);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, body).unwrap();
    }
}

async fn post(daemon: &TestDaemon, path: &str, rid: &str, body: Value) -> Reply {
    let mut body = body;
    body["request_id"] = json!(rid);
    call(daemon, "POST", path, Some(body)).await
}

/// A Claude plugin with a skill, a command file, an agent: installed
/// directly, as notes@local.
async fn install_notes(daemon: &TestDaemon) -> tempfile::TempDir {
    let folder = tempfile::tempdir().unwrap();
    write(
        folder.path(),
        &[
            (".claude-plugin/plugin.json", r#"{"name": "notes"}"#),
            (
                "skills/jot/SKILL.md",
                "---\nname: jot\ndescription: Jot a note\n---\n# jot",
            ),
            ("commands/clean.md", "---\ndescription: Clean up\n---\nbody"),
            (
                "agents/scribe.md",
                "---\nname: scribe\ndescription: Writes things down\n---\n",
            ),
        ],
    );
    let staged = post(
        daemon,
        "/plugins/stage",
        "n-stage",
        json!({"source": {"kind": "folder", "path": folder.path()}}),
    )
    .await;
    assert_eq!(staged.status, 200, "{}", staged.raw);
    let id = staged.body["result"]["stage_id"].as_str().unwrap();
    let done = post(
        daemon,
        &format!("/plugins/stage/{id}/install"),
        "n-install",
        json!({}),
    )
    .await;
    assert_eq!(done.status, 200, "{}", done.raw);
    folder
}

async fn commands(daemon: &TestDaemon, query: &str) -> Value {
    let reply = call(daemon, "GET", &format!("/commands?{query}"), None).await;
    assert_eq!(reply.status, 200, "{}", reply.raw);
    reply.body
}

fn names(view: &Value) -> Vec<String> {
    view["commands"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["name"].as_str().unwrap().to_string())
        .collect()
}

fn entry<'a>(view: &'a Value, list: &str, name: &str) -> &'a Value {
    view[list]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["name"] == name)
        .unwrap_or_else(|| panic!("no {name} in {view}"))
}

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

async fn start_chat(daemon: &TestDaemon, rid: &str, harness: &str, ws: &Path) -> String {
    let created = post(
        daemon,
        "/sessions",
        rid,
        json!({"harness": harness, "workspace": ws}),
    )
    .await;
    assert_eq!(created.status, 201, "{}", created.raw);
    created.body["id"].as_str().unwrap().to_string()
}

async fn prompt(daemon: &TestDaemon, id: &str, rid: &str, body: Value, turns: usize) {
    let sent = post(daemon, &format!("/sessions/{id}/prompts"), rid, body).await;
    assert_eq!(sent.status, 200, "{}", sent.raw);
    until_count(
        daemon,
        &format!("/sessions/{id}/events"),
        "turn.completed",
        turns,
    )
    .await;
}

#[tokio::test]
async fn claudes_own_list_carries_the_plugins_and_is_asked_again_when_it_changes() {
    let daemon = start_daemon(&[("claude", fixture_agent())]).await;
    let _plugin = install_notes(&daemon).await;
    let ws = workspace(Some("plain"));
    let query = format!("harness=claude&workspace={}", encode(ws.path()));

    let view = commands(&daemon, &query).await;
    assert_eq!(view["runs_plugins"], true);
    // claude lists /compact; OpenRemote's table knows it as a built-in.
    assert_eq!(entry(&view, "commands", "compact")["source"], "builtin");
    assert_eq!(entry(&view, "commands", "compact")["route"], "native");
    // claude lists the plugin's skill itself (loaded by --plugin-dir) ...
    let jot = entry(&view, "commands", "notes:jot");
    assert_eq!(jot["source"], "plugin");
    assert_eq!(jot["plugin"], "notes@local");
    assert_eq!(jot["available"], true);
    // ... and the plugin's other parts fill in from its folder.
    assert_eq!(entry(&view, "commands", "notes:clean")["available"], true);
    assert_eq!(entry(&view, "agents", "Explore")["source"], "harness");
    assert_eq!(entry(&view, "agents", "notes:scribe")["source"], "plugin");

    // Cached: a command that appears later isn't seen ...
    std::fs::write(ws.path().join("fixture-extra-command"), "fresh").unwrap();
    assert!(!names(&commands(&daemon, &query).await).contains(&"fresh".to_string()));
    // ... until a running chat lists it as it starts.
    let id = start_chat(&daemon, "c-create", "claude", ws.path()).await;
    prompt(&daemon, &id, "c-p1", json!({"text": "hi"}), 1).await;
    assert!(names(&commands(&daemon, &query).await).contains(&"fresh".to_string()));

    // A plugin turned off is gone from the next list.
    let off = post(
        &daemon,
        "/plugins/notes@local/enabled",
        "c-off",
        json!({"enabled": false}),
    )
    .await;
    assert_eq!(off.status, 200, "{}", off.raw);
    let view = commands(&daemon, &query).await;
    assert!(
        !names(&view).iter().any(|n| n.starts_with("notes:")),
        "{view}"
    );

    // A chat's own query names its harness and workspace.
    let by_chat = commands(&daemon, &format!("session={id}")).await;
    assert_eq!(by_chat["harness"], "claude");
}

#[tokio::test]
async fn codex_runs_no_plugins_yet_and_gets_its_skills_and_files_as_its_own_items() {
    let daemon = start_daemon(&[("claude", fixture_agent()), ("codex", fixture_agent())]).await;
    let _plugin = install_notes(&daemon).await;
    let ws = workspace(Some("plain"));
    write(ws.path(), &[("README.md", "# readme")]);
    let query = format!("harness=codex&workspace={}", encode(ws.path()));

    let view = commands(&daemon, &query).await;
    assert_eq!(view["runs_plugins"], false);
    assert_eq!(
        entry(&view, "commands", "fixture-skill")["source"],
        "harness"
    );
    let jot = entry(&view, "commands", "notes:jot");
    assert_eq!(jot["available"], false);
    assert_eq!(jot["why"], "needs a harness that runs plugins");

    let id = start_chat(&daemon, "x-create", "codex", ws.path()).await;
    prompt(
        &daemon,
        &id,
        "x-p1",
        json!({"text": "/fixture-skill then read @README.md please"}),
        1,
    )
    .await;
    let recorded = std::fs::read_to_string(ws.path().join("fixture-codex-input")).unwrap();
    let input: Value = serde_json::from_str(recorded.lines().last().unwrap()).unwrap();
    let kinds: Vec<&str> = input
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["type"].as_str().unwrap())
        .collect();
    assert_eq!(kinds, vec!["text", "skill", "mention"], "{input}");
    assert_eq!(
        input[0]["text"],
        "/fixture-skill then read @README.md please"
    );
    assert_eq!(input[1]["name"], "fixture-skill");
    assert_eq!(input[2]["name"], "README.md");
}

#[tokio::test]
async fn file_suggestions_follow_git_and_leave_ignored_files_out() {
    let daemon = start_daemon(&[("claude", fixture_agent())]).await;
    let ws = workspace(Some("plain"));
    write(
        ws.path(),
        &[
            ("src/app.js", "a"),
            ("src/util.js", "u"),
            ("node_modules/lib/index.js", "x"),
            (".gitignore", "node_modules/\nfixture-*\n"),
            ("notes.md", "n"),
        ],
    );
    let git = |args: &[&str]| {
        let out = std::process::Command::new("git")
            .args(["-c", "user.email=t@example.com", "-c", "user.name=t"])
            .args(args)
            .current_dir(ws.path())
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
    };
    git(&["init", "--quiet"]);
    git(&["add", "src", ".gitignore"]);
    git(&["commit", "--quiet", "-m", "x"]);

    let files = |q: &str| {
        let path = format!("/files?workspace={}&q={q}", encode(ws.path()));
        let daemon = &daemon;
        async move {
            let reply = call(daemon, "GET", &path, None).await;
            assert_eq!(reply.status, 200, "{}", reply.raw);
            reply.body["entries"]
                .as_array()
                .unwrap()
                .iter()
                .map(|e| e["path"].as_str().unwrap().to_string())
                .collect::<Vec<_>>()
        }
    };
    assert_eq!(
        files("app").await.first().map(String::as_str),
        Some("src/app.js")
    );
    // untracked but not ignored: offered
    assert!(files("notes").await.contains(&"notes.md".to_string()));
    // ignored: never offered
    assert!(
        files("index").await.is_empty(),
        "{:?}",
        files("index").await
    );
    let top = files("").await;
    assert_eq!(top.first().map(String::as_str), Some("src"), "{top:?}");

    // A chat's own workspace stands in for a named one.
    let id = start_chat(&daemon, "f-create", "claude", ws.path()).await;
    let reply = call(&daemon, "GET", &format!("/files?session={id}&q=util"), None).await;
    assert_eq!(
        reply.body["entries"][0]["path"], "src/util.js",
        "{}",
        reply.raw
    );
}

/// One MCP exchange with the read-a-chat server: a request, its answer.
async fn rpc(
    stdin: &mut tokio::process::ChildStdin,
    lines: &mut tokio::io::Lines<BufReader<tokio::process::ChildStdout>>,
    id: u64,
    method: &str,
    params: Value,
) -> Value {
    let frame = json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params});
    stdin
        .write_all(format!("{frame}\n").as_bytes())
        .await
        .unwrap();
    stdin.flush().await.unwrap();
    let line = tokio::time::timeout(std::time::Duration::from_secs(20), lines.next_line())
        .await
        .expect("an answer in time")
        .unwrap()
        .expect("a line");
    serde_json::from_str(&line).unwrap()
}

#[tokio::test]
async fn a_chat_reads_only_the_chats_it_was_pointed_at() {
    let daemon = start_daemon(&[("claude", fixture_agent())]).await;
    let ws = workspace(Some("plain"));
    let first = start_chat(&daemon, "r-a", "claude", ws.path()).await;
    prompt(
        &daemon,
        &first,
        "r-a1",
        json!({"text": "remember the word kumquat"}),
        1,
    )
    .await;
    let other = start_chat(&daemon, "r-c", "claude", ws.path()).await;
    let reader = start_chat(&daemon, "r-b", "claude", ws.path()).await;

    // Not pointed at yet: refused.
    let refused = call(
        &daemon,
        "GET",
        &format!("/sessions/{reader}/mentions/{first}"),
        None,
    )
    .await;
    assert_eq!(refused.status, 409, "{}", refused.raw);

    prompt(
        &daemon,
        &reader,
        "r-b1",
        json!({"text": "what did that chat say?", "mentioned_chats": [first]}),
        1,
    )
    .await;
    let read = call(
        &daemon,
        "GET",
        &format!("/sessions/{reader}/mentions/{}", &first[..8]),
        None,
    )
    .await;
    assert_eq!(read.status, 200, "{}", read.raw);
    assert!(
        read.body["text"].as_str().unwrap().contains("kumquat"),
        "{}",
        read.raw
    );

    // The chat's harness was handed the read-a-chat server.
    let argv = std::fs::read_to_string(ws.path().join("fixture-claude-argv")).unwrap();
    assert!(argv.contains("mcp-chat"), "{argv}");
    assert!(argv.contains("openremote"), "{argv}");

    // The server itself, as the harness runs it: MCP over stdio.
    std::fs::write(daemon.data_dir().join("token"), &daemon.token).unwrap();
    let mut child = tokio::process::Command::new(env!("CARGO_BIN_EXE_openremote-daemon"))
        .args([
            "mcp-chat",
            "--session",
            &reader,
            "--port",
            &daemon.addr.port().to_string(),
            "--data-dir",
        ])
        .arg(daemon.data_dir())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let mut lines = BufReader::new(child.stdout.take().unwrap()).lines();
    let hello = rpc(
        &mut stdin,
        &mut lines,
        1,
        "initialize",
        json!({"protocolVersion": "2025-06-18"}),
    )
    .await;
    assert_eq!(
        hello["result"]["serverInfo"]["name"], "openremote",
        "{hello}"
    );
    let tools = rpc(&mut stdin, &mut lines, 2, "tools/list", json!({})).await;
    assert_eq!(tools["result"]["tools"][0]["name"], "read_chat", "{tools}");
    let answer = rpc(
        &mut stdin,
        &mut lines,
        3,
        "tools/call",
        json!({"name": "read_chat", "arguments": {"chat": first}}),
    )
    .await;
    let text = answer["result"]["content"][0]["text"]
        .as_str()
        .unwrap_or_default();
    assert!(text.contains("kumquat"), "{answer}");
    let denied = rpc(
        &mut stdin,
        &mut lines,
        4,
        "tools/call",
        json!({"name": "read_chat", "arguments": {"chat": other}}),
    )
    .await;
    assert_eq!(denied["result"]["isError"], true, "{denied}");
}

#[tokio::test]
async fn rename_gives_a_chat_the_name_every_device_shows() {
    let daemon = start_daemon(&[("claude", fixture_agent())]).await;
    let ws = workspace(Some("plain"));
    let id = start_chat(&daemon, "t-create", "claude", ws.path()).await;
    let renamed = post(
        &daemon,
        &format!("/sessions/{id}/title"),
        "t-rename",
        json!({"title": "  Login work  "}),
    )
    .await;
    assert_eq!(renamed.status, 200, "{}", renamed.raw);
    let session = call(&daemon, "GET", &format!("/sessions/{id}"), None).await;
    assert_eq!(session.body["title"], "Login work", "{}", session.raw);
    let empty = post(
        &daemon,
        &format!("/sessions/{id}/title"),
        "t-empty",
        json!({"title": "   "}),
    )
    .await;
    assert_eq!(empty.status, 409, "{}", empty.raw);
}

#[tokio::test]
async fn every_apps_built_ins_are_listed_routed_and_refused_where_they_cannot_run() {
    let daemon = start_daemon(&[("claude", fixture_agent()), ("codex", fixture_agent())]).await;
    let ws = workspace(Some("plain"));
    let claude = commands(
        &daemon,
        &format!("harness=claude&workspace={}", encode(ws.path())),
    )
    .await;
    // A terminal-only built-in is in the menu, dimmed, with its reason.
    let theme = entry(&claude, "commands", "theme");
    assert_eq!(theme["source"], "builtin");
    assert_eq!(theme["route"], "refuse");
    assert_eq!(theme["available"], false);
    assert!(
        theme["why"].as_str().unwrap().contains("terminal"),
        "{theme}"
    );
    // One OpenRemote does itself names the action.
    let copy = entry(&claude, "commands", "copy");
    assert_eq!(copy["route"], "console");
    assert_eq!(copy["action"], "copy");

    let codex = commands(
        &daemon,
        &format!("harness=codex&workspace={}", encode(ws.path())),
    )
    .await;
    assert_eq!(entry(&codex, "commands", "compact")["route"], "driver");
    assert_eq!(entry(&codex, "commands", "vim")["route"], "refuse");

    // A refused command opens no turn and says why.
    let id = start_chat(&daemon, "s-create", "claude", ws.path()).await;
    let refused = call(
        &daemon,
        "POST",
        &format!("/sessions/{id}/prompts"),
        Some(json!({"request_id": "s-p1", "text": "/theme"})),
    )
    .await;
    assert_eq!(refused.status, 409, "{}", refused.raw);
    let message = refused.body["error"].as_str().unwrap_or("");
    assert!(message.starts_with("/theme "), "{message}");
    let chat = call(&daemon, "GET", &format!("/sessions/{id}"), None).await;
    assert_ne!(chat.body["status"], "working", "{}", chat.raw);

    // Codex's /compact goes through its own call, never as words.
    let cx = start_chat(&daemon, "s-codex", "codex", ws.path()).await;
    prompt(&daemon, &cx, "s-p2", json!({"text": "/compact"}), 1).await;
    let calls = std::fs::read_to_string(ws.path().join("fixture-codex-calls")).unwrap_or_default();
    assert!(calls.contains("thread/compact/start"), "{calls}");
    let input = std::fs::read_to_string(ws.path().join("fixture-codex-input")).unwrap_or_default();
    assert!(!input.contains("/compact"), "{input}");
}

#[tokio::test]
async fn a_new_command_file_reaches_the_menu_without_a_restart() {
    let daemon = start_daemon(&[("claude", fixture_agent())]).await;
    let ws = workspace(Some("plain"));
    let query = format!("harness=claude&workspace={}", encode(ws.path()));
    let before = commands(&daemon, &query).await["rev"].as_u64().unwrap();
    // The folder does not exist yet: the first file in it still counts.
    write(
        ws.path(),
        &[(
            ".claude/commands/ship.md",
            "---
description: Ship it
---
Ship.",
        )],
    );
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
    loop {
        let caps = call(&daemon, "GET", "/capabilities", None).await;
        if caps.body["commands_rev"].as_u64().unwrap_or(0) > before {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "the new file was never heard"
        );
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    let after = commands(&daemon, &query).await;
    assert!(after["rev"].as_u64().unwrap() > before, "{after}");
}
