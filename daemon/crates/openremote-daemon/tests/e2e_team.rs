//! Agent teams: a chat's agent starts other chats as workers on any
//! harness, hears back from them, and stays inside the user's limits. The
//! fixture agent stands in for every harness; the tools are called as the
//! chat's MCP server calls them.

mod common;

use common::*;
use serde_json::{Value, json};

async fn chat(daemon: &TestDaemon, harness: &str, ws: &tempfile::TempDir) -> String {
    let created = call(
        daemon,
        "POST",
        "/sessions",
        Some(json!({"request_id": format!("t-create-{}", uuid::Uuid::new_v4()), "harness": harness, "workspace": ws.path()})),
    )
    .await;
    assert_eq!(created.status, 201, "raw: {}", created.raw);
    let id = created.body["id"].as_str().expect("session id").to_string();
    until_status(daemon, &id, &["idle"]).await;
    id
}

/// One team tool, as the chat's MCP server calls it: (is it an error, text).
async fn tool(daemon: &TestDaemon, chat: &str, name: &str, arguments: Value) -> (bool, String) {
    let reply = call(
        daemon,
        "POST",
        &format!("/sessions/{chat}/agents/call"),
        Some(json!({"name": name, "arguments": arguments})),
    )
    .await;
    assert_eq!(reply.status, 200, "raw: {}", reply.raw);
    (
        reply.body["isError"].as_bool().unwrap_or(false),
        reply.body["content"][0]["text"]
            .as_str()
            .unwrap_or_default()
            .to_string(),
    )
}

/// The id a spawn answers with: the first line is its JSON.
fn spawned(text: &str) -> String {
    let info: Value =
        serde_json::from_str(text.lines().next().unwrap_or("")).expect("a JSON first line");
    info["agent"].as_str().expect("agent id").to_string()
}

async fn session(daemon: &TestDaemon, id: &str) -> Value {
    call(daemon, "GET", &format!("/sessions/{id}"), None)
        .await
        .body
}

fn user_messages(events: &[(u64, Value)]) -> Vec<Value> {
    events
        .iter()
        .filter(|(_, e)| kind(e) == "message.added" && e["message"]["role"] == "user")
        .map(|(_, e)| e["message"].clone())
        .collect()
}

#[tokio::test]
async fn a_worker_on_another_harness_reports_and_wakes_an_idle_lead() {
    let daemon = start_daemon(&[]).await;
    let ws = workspace(None);
    let lead = chat(&daemon, "claude", &ws).await;

    let (error, text) = tool(
        &daemon,
        &lead,
        "agents_spawn",
        json!({"harness": "codex", "brief": "say hello", "title": "Hello worker"}),
    )
    .await;
    assert!(!error, "{text}");
    let worker = spawned(&text);

    // The worker is a chat of the tree, under the lead, on the other harness.
    let w = session(&daemon, &worker).await;
    assert_eq!(w["harness"], "codex");
    assert_eq!(w["title"], "Hello worker");
    assert_eq!(w["parent"]["chat"], lead.as_str());
    assert_eq!(w["parent"]["depth"], 1);
    let events = until_count(
        &daemon,
        &format!("/sessions/{worker}/events"),
        "message.added",
        2,
    )
    .await;
    let brief = &user_messages(&events)[0];
    assert_eq!(brief["origin"]["kind"], "agent");
    assert_eq!(brief["origin"]["chat"], lead.as_str());
    let words = brief["text"].as_str().unwrap();
    assert!(
        words.starts_with("You are a worker in an OpenRemote team"),
        "{words}"
    );
    assert!(words.ends_with("Task:\nsay hello"), "{words}");

    // Its final message reaches the idle lead at once, as a message from the
    // agent - and the lead's harness read it behind the line saying so.
    let lead_events = until_count(
        &daemon,
        &format!("/sessions/{lead}/events"),
        "message.added",
        2,
    )
    .await;
    let report = &user_messages(&lead_events)[0];
    assert_eq!(report["origin"]["chat"], worker.as_str());
    assert_eq!(report["origin"]["report"], true);
    let body = report["text"].as_str().unwrap();
    assert!(
        body.starts_with("Report from your worker \"Hello worker\""),
        "{body}"
    );
    assert!(body.contains("finished its turn"), "{body}");
    let reply = lead_events
        .iter()
        .find(|(_, e)| kind(e) == "message.added" && e["message"]["role"] == "assistant")
        .map(|(_, e)| e["message"]["text"].as_str().unwrap().to_string())
        .expect("the lead answered");
    assert!(
        reply.starts_with("done: [Message from agent \"Hello worker\", not from the user]"),
        "{reply}"
    );
}

#[tokio::test]
async fn a_wait_takes_the_report_and_nothing_is_queued_for_it() {
    let daemon = start_daemon(&[]).await;
    // The lead's turn stays open on a pending approval while its worker works.
    let ws = workspace(Some("approve"));
    let lead = chat(&daemon, "claude", &ws).await;
    let sent = call(
        &daemon,
        "POST",
        &format!("/sessions/{lead}/prompts"),
        Some(json!({"request_id": "t-lead-go", "text": "go"})),
    )
    .await;
    assert_eq!(sent.status, 200, "raw: {}", sent.raw);
    until_status(&daemon, &lead, &["waiting"]).await;
    // Each process reads its scenario as it starts: the worker's is slow.
    std::fs::write(ws.path().join("fixture-scenario"), "slow").unwrap();

    let (error, text) = tool(
        &daemon,
        &lead,
        "agents_spawn",
        json!({"harness": "claude", "brief": "say hello", "title": "Slow worker"}),
    )
    .await;
    assert!(!error, "{text}");
    let worker = spawned(&text);

    let (error, answer) = tool(&daemon, &lead, "agents_wait", json!({"timeout_s": 25})).await;
    assert!(!error, "{answer}");
    assert!(
        answer.contains("Report from your worker \"Slow worker\""),
        "{answer}"
    );
    assert!(answer.contains("say hello"), "{answer}");
    assert!(answer.contains(&worker), "{answer}");

    // The wait had it: the lead's queue holds nothing, and once its own
    // turn is answered it has had exactly that one turn.
    assert!(session(&daemon, &lead).await["queue"].is_null());
    let decisions = call(&daemon, "GET", &format!("/sessions/{lead}/decisions"), None).await;
    let decision = decisions.body[0]["id"].as_str().unwrap().to_string();
    let answered = call(
        &daemon,
        "POST",
        &format!("/decisions/{decision}/answer"),
        Some(json!({"request_id": "t-lead-allow", "choice": "allow"})),
    )
    .await;
    assert_eq!(answered.status, 200, "raw: {}", answered.raw);
    let after = until_status(&daemon, &lead, &["idle"]).await;
    assert_eq!(after["next_turn"], 1, "{after}");
    assert!(after["queue"].is_null(), "{after}");
}

#[tokio::test]
async fn the_users_limits_and_the_team_boundary_hold() {
    let daemon = start_daemon(&[]).await;
    let ws = workspace(None);
    let lead = chat(&daemon, "claude", &ws).await;
    let set = call(
        &daemon,
        "POST",
        "/agents/settings",
        Some(json!({"max_depth": 1, "max_children": 1})),
    )
    .await;
    assert_eq!(set.status, 200, "raw: {}", set.raw);

    // Refusals say what to do instead.
    let (error, text) = tool(
        &daemon,
        &lead,
        "agents_spawn",
        json!({"harness": "codex", "brief": "x", "workspace": "elsewhere"}),
    )
    .await;
    assert!(error && text.contains("inherit"), "{text}");
    let (error, text) = tool(
        &daemon,
        &lead,
        "agents_spawn",
        json!({"harness": "codex", "brief": "x", "permission": "bypassPermissions"}),
    )
    .await;
    assert!(error && text.contains("Leave permission out"), "{text}");

    let (error, text) = tool(
        &daemon,
        &lead,
        "agents_spawn",
        json!({"harness": "codex", "brief": "first", "title": "One"}),
    )
    .await;
    assert!(!error, "{text}");
    let worker = spawned(&text);
    let (error, text) = tool(
        &daemon,
        &lead,
        "agents_spawn",
        json!({"harness": "codex", "brief": "second"}),
    )
    .await;
    assert!(error && text.contains("agents_stop"), "{text}");

    // One level deep: the worker can only report up, and may not start more.
    let tools = call(
        &daemon,
        "GET",
        &format!("/sessions/{worker}/agents/tools"),
        None,
    )
    .await;
    let names: Vec<&str> = tools.body["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["agents_report"]);
    let (error, text) = tool(
        &daemon,
        &worker,
        "agents_spawn",
        json!({"harness": "codex", "brief": "more"}),
    )
    .await;
    assert!(error && text.contains("cannot start workers"), "{text}");

    // Nobody outside a team is in it: a worker cannot read its lead, and a
    // lead cannot read a chat that is not its own.
    let (error, text) = tool(&daemon, &worker, "agents_read", json!({"agent": lead})).await;
    assert!(error && text == "no such agent in your team", "{text}");
    let stranger = chat(&daemon, "claude", &ws).await;
    let (error, text) = tool(&daemon, &lead, "agents_read", json!({"agent": stranger})).await;
    assert!(error && text == "no such agent in your team", "{text}");

    // The team's own reading works: the worker's final message, by its id.
    let (error, text) = tool(
        &daemon,
        &lead,
        "agents_read",
        json!({"agent": &worker[..8], "tail": 1}),
    )
    .await;
    assert!(!error && text.contains("Chat \"One\""), "{text}");

    // Stopping the lead takes down the worker still running under it.
    until_status(&daemon, &worker, &["idle"]).await;
    let stopped = call(
        &daemon,
        "POST",
        &format!("/sessions/{lead}/stop"),
        Some(json!({"request_id": "t-stop-lead"})),
    )
    .await;
    assert_eq!(stopped.status, 200, "raw: {}", stopped.raw);
    until_status(&daemon, &worker, &["stopped"]).await;
}

/// OpenRemote's MCP server, as a chat's harness starts it, lists the team
/// tools that chat can use and runs them through the daemon.
#[tokio::test]
async fn a_chats_mcp_server_carries_only_the_team_tools_it_can_use() {
    use std::process::Stdio;
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

    let daemon = start_daemon(&[]).await;
    // The server reads the token from the data folder, as a harness's does.
    std::fs::write(daemon.data_dir().join("token"), &daemon.token).unwrap();
    let ws = workspace(None);
    let lead = chat(&daemon, "claude", &ws).await;
    let (error, text) = tool(
        &daemon,
        &lead,
        "agents_spawn",
        json!({"harness": "codex", "brief": "x", "title": "One"}),
    )
    .await;
    assert!(!error, "{text}");
    let worker = spawned(&text);

    async fn tools_of(daemon: &TestDaemon, chat: &str, call_list: bool) -> (Vec<String>, String) {
        let mut child = tokio::process::Command::new(env!("CARGO_BIN_EXE_openremote-daemon"))
            .args([
                "mcp-chat",
                "--session",
                chat,
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
        let mut rpc = async |id: u64, method: &str, params: Value| -> Value {
            let frame = json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params});
            stdin
                .write_all(format!("{frame}\n").as_bytes())
                .await
                .unwrap();
            stdin.flush().await.unwrap();
            let line = tokio::time::timeout(std::time::Duration::from_secs(60), lines.next_line())
                .await
                .expect("the server answers")
                .unwrap()
                .unwrap();
            serde_json::from_str(&line).unwrap()
        };
        rpc(1, "initialize", json!({"protocolVersion": "2025-06-18"})).await;
        let listed = rpc(2, "tools/list", json!({})).await;
        let names = listed["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|t| t["name"].as_str().map(str::to_string))
            .collect();
        let called = if call_list {
            let answer = rpc(
                3,
                "tools/call",
                json!({"name": "agents_list", "arguments": {}}),
            )
            .await;
            answer["result"]["content"][0]["text"]
                .as_str()
                .unwrap_or_default()
                .to_string()
        } else {
            String::new()
        };
        (names, called)
    }

    let (names, listing) = tools_of(&daemon, &lead, true).await;
    for wanted in [
        "agents_harnesses",
        "agents_spawn",
        "agents_list",
        "agents_read",
        "agents_wait",
        "agents_message",
        "agents_stop",
    ] {
        assert!(names.contains(&wanted.to_string()), "{names:?}");
    }
    assert!(!names.contains(&"agents_report".to_string()), "{names:?}");
    assert!(listing.contains("\"One\""), "{listing}");

    // One level down and the user's depth is 2: the worker may still start
    // workers and may report up.
    let (names, _) = tools_of(&daemon, &worker, false).await;
    assert!(names.contains(&"agents_report".to_string()), "{names:?}");
    assert!(names.contains(&"agents_spawn".to_string()), "{names:?}");

    // Turned off, nothing is listed.
    let set = call(
        &daemon,
        "POST",
        "/agents/settings",
        Some(json!({"tools": false})),
    )
    .await;
    assert_eq!(set.status, 200, "raw: {}", set.raw);
    let (names, _) = tools_of(&daemon, &lead, false).await;
    assert!(names.iter().all(|n| !n.starts_with("agents_")), "{names:?}");
}
