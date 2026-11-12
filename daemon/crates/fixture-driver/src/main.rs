//! A plugin harness driver fixture. It is the smallest thing that speaks the
//! seam: one `--probe` line, one `--catalog` capture, then one scripted turn
//! per prompt.
//!
//! The e2e suite points a `HarnessSpec` at this binary so the whole path runs
//! for real - the daemon starts the process, writes `spawn` and `prompt`, and
//! the supervisor translates the events. A plugin author can read it beside
//! `docs/harness-plugins.md` for the shape of a driver that needs no harness
//! of its own.
//!
//! Two flags hold a chat in a state a test needs: `--hold` leaves a turn open,
//! and `--refuse-settings` answers a settings command with a refusal.
//! `--commands <folder>` prints the composer's lists, naming the folder it
//! was asked about; the `echo-spawn` argument makes a turn's reply name the
//! MCP servers and allowed tools the spawn command carried.
//!
//! `--plugin-rpc` makes it a process plugin's program instead: OpenRemote's
//! plugin protocol (JSON-RPC lines) with two agent tools - `echo`, which
//! names what it was handed, and `ask`, which asks OpenRemote for its chats
//! and answers what came back - and one view request, `ping`.

use std::io::{BufRead, Write};

/// The capture `--catalog` prints and writes. The number is fixed so a test
/// can assert the row's capture time, and the two models differ from the
/// shipped list the fixture plugin's manifest declares.
const CATALOG: &str = r#"{"captured_at":1699999999000,"default":"fixture-model-b","models":[{"model":"fixture-model-a","display_name":"Fixture A","detail":"3x · Recommended","note":"May use data for AI training","is_default":false},{"model":"fixture-model-b","display_name":"Fixture B","is_default":true}]}"#;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|argument| argument == "--plugin-rpc") {
        plugin_rpc();
        return;
    }
    if args.iter().any(|argument| argument == "--probe") {
        // A probe is one line and an exit: no chat, no account reached.
        println!(r#"{{"available":true,"version":"0.0.1","signed_in":true,"reason":null}}"#);
        return;
    }
    if args.iter().any(|argument| argument == "--catalog") {
        // The real driver writes its capture beside itself, where the
        // daemon's next probe reads it, and answers the same line.
        let _ = std::fs::write("models.json", CATALOG);
        println!("{CATALOG}");
        return;
    }
    if let Some(index) = args.iter().position(|argument| argument == "--commands") {
        // One line and an exit, like the probe. The agent's description is
        // the folder the daemon asked about, so a test can see it arrive.
        let folder = args.get(index + 1).cloned().unwrap_or_default();
        println!(
            "{}",
            serde_json::json!({
                "commands": [{
                    "name": "fixture-review",
                    "description": "Review with the fixture",
                    "argument_hint": "<path>",
                    "aliases": ["fr"]
                }],
                "agents": [{"name": "fixture-planner", "description": folder}]
            })
        );
        return;
    }
    let hold = args.iter().any(|argument| argument == "--hold");
    let echo_spawn = args.iter().any(|argument| argument == "echo-spawn");
    let mut spawned = serde_json::Value::Null;
    let refuse_settings = args.iter().any(|argument| argument == "--refuse-settings");

    // The e2e's HarnessSpec passes args through the plugin seam. The
    // approval scenario asks once and waits; plain stays the default.
    let approval_scenario = std::env::args().any(|argument| argument == "approval");

    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&line) else {
            eprintln!("fixture-driver: not JSON: {line}");
            continue;
        };
        let kind = value
            .get("type")
            .and_then(|kind| kind.as_str())
            .unwrap_or_default();
        match kind {
            "spawn" => {
                spawned = value.clone();
                emit(
                    &mut stdout,
                    serde_json::json!({
                        "type": "initialized",
                        "session_ref": "fixture-1",
                        "model": null,
                        "permission_mode": null
                    }),
                );
            }
            "prompt" if echo_spawn => {
                let ids: Vec<&str> = spawned["mcp_servers"]
                    .as_array()
                    .map(|servers| {
                        servers
                            .iter()
                            .filter_map(|server| server["id"].as_str())
                            .collect()
                    })
                    .unwrap_or_default();
                // Each list as `[a,b]`, or `missing` when the key was absent.
                let allowed = spawned["allowed_tools"]
                    .as_array()
                    .map(|tools| {
                        let names: Vec<&str> =
                            tools.iter().filter_map(|tool| tool.as_str()).collect();
                        format!("[{}]", names.join(","))
                    })
                    .unwrap_or_else(|| "missing".into());
                emit(
                    &mut stdout,
                    serde_json::json!({
                        "type": "text",
                        "text": format!("fixture spawn: mcp=[{}] allowed={allowed}", ids.join(","))
                    }),
                );
                emit(
                    &mut stdout,
                    serde_json::json!({
                        "type": "turn_completed",
                        "subtype": "finished",
                        "coarse": "completed",
                        "is_error": false
                    }),
                );
            }
            "prompt" if approval_scenario => {
                emit(
                    &mut stdout,
                    serde_json::json!({
                        "type": "approval_requested",
                        "harness_ref": "fixture-approval-1",
                        "tool_name": "write_file",
                        "summary": "Write a.py",
                        "kind": "approval",
                        "options": [
                            {"id": "allow", "label": "Allow"},
                            {"id": "deny", "label": "Deny"},
                            {"id": "always", "label": "Allow for this chat"}
                        ],
                        "session_grant": ["always"],
                        "request": {"path": "a.py"}
                    }),
                );
            }
            "prompt" if hold => {
                // A turn that never ends, so a test can hold a session in
                // Working while it asks the daemon for a settings change.
                let text = value
                    .get("text")
                    .and_then(|text| text.as_str())
                    .unwrap_or_default();
                eprintln!("fixture-driver: holding the turn open for '{text}'");
            }
            "prompt" => {
                let text = value
                    .get("text")
                    .and_then(|text| text.as_str())
                    .unwrap_or_default();
                // Tool events first, then the reply: the console's stream
                // ends on the message the turn completed with, the way the
                // real drivers order theirs.
                emit(
                    &mut stdout,
                    serde_json::json!({
                        "type": "tool_started",
                        "tool_use_id": "call-1",
                        "name": "read_files",
                        "input": {"paths": ["a.py"]}
                    }),
                );
                emit(
                    &mut stdout,
                    serde_json::json!({
                        "type": "tool_result",
                        "tool_use_id": "call-1",
                        "name": "read_files",
                        "text": "print(1)",
                        "is_error": false
                    }),
                );
                emit(
                    &mut stdout,
                    serde_json::json!({
                        "type": "text",
                        "text": format!("fixture heard: {text}")
                    }),
                );
                emit(
                    &mut stdout,
                    serde_json::json!({
                        "type": "turn_completed",
                        "subtype": "finished",
                        "coarse": "completed",
                        "is_error": false,
                        "error_message": null
                    }),
                );
            }
            "interrupt" => emit(
                &mut stdout,
                serde_json::json!({
                    "type": "turn_completed",
                    "subtype": "interrupted",
                    "coarse": "interrupted",
                    "is_error": false
                }),
            ),
            "settings" if refuse_settings => emit(
                &mut stdout,
                serde_json::json!({
                    "type": "settings_refused",
                    "message": "fixture-driver has no model by that name"
                }),
            ),
            "settings" => eprintln!("fixture-driver: settings are not applied live"),
            "answer" if approval_scenario => {
                let choice = value
                    .get("choice")
                    .and_then(|choice| choice.as_str())
                    .unwrap_or_default();
                emit(
                    &mut stdout,
                    serde_json::json!({
                        "type": "text",
                        "text": format!("fixture answered: {choice}")
                    }),
                );
                emit(
                    &mut stdout,
                    serde_json::json!({
                        "type": "turn_completed",
                        "subtype": "finished",
                        "coarse": "completed",
                        "is_error": false,
                        "error_message": null
                    }),
                );
            }
            "answer" => eprintln!("fixture-driver: asks no approvals"),
            "shutdown" => break,
            other => eprintln!("fixture-driver: unknown command '{other}'"),
        }
    }
}

fn emit(stdout: &mut impl Write, value: serde_json::Value) {
    writeln!(stdout, "{value}").expect("write an event");
    stdout.flush().expect("flush an event");
}

/// A process plugin's program: answers `initialize`, `tools/call` and
/// `ui/request`; `ask` makes its own call to OpenRemote and waits for it.
fn plugin_rpc() {
    use serde_json::{Value, json};
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    let mut lines = stdin.lock().lines();
    let mut settings = Value::Null;
    let mut powers = Value::Null;
    while let Some(Ok(line)) = lines.next() {
        let Ok(message) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        let Some(id) = message.get("id").cloned() else {
            // settings/changed and other notifications.
            if message["method"] == "settings/changed" {
                settings = message["params"]["settings"].clone();
            }
            continue;
        };
        let params = &message["params"];
        let result = match message["method"].as_str().unwrap_or_default() {
            "initialize" => {
                settings = params["settings"].clone();
                powers = params["powers"].clone();
                json!({"protocol": 1})
            }
            "tools/call" if params["name"] == "ask" => {
                emit(
                    &mut stdout,
                    json!({"jsonrpc": "2.0", "id": "ask-1", "method": "chats/list", "params": {}}),
                );
                let mut answer = Value::Null;
                for line in lines.by_ref() {
                    let Ok(line) = line else { break };
                    let Ok(reply) = serde_json::from_str::<Value>(&line) else {
                        continue;
                    };
                    if reply["id"] == "ask-1" {
                        answer = reply;
                        break;
                    }
                }
                json!(format!("asked: {answer}"))
            }
            "tools/call" => json!({"content": [{"type": "text", "text": format!(
                "echo {} token:{} powers:{} chat:{}",
                params["arguments"],
                if settings["token"].as_str().is_some_and(|t| !t.is_empty()) { "yes" } else { "no" },
                powers,
                params["chat"]["id"],
            )}]}),
            "ui/request" => json!({"pong": params["name"]}),
            other => {
                emit(
                    &mut stdout,
                    json!({"jsonrpc": "2.0", "id": id, "error": {"code": -32601, "message": format!("no {other}")}}),
                );
                continue;
            }
        };
        emit(
            &mut stdout,
            json!({"jsonrpc": "2.0", "id": id, "result": result}),
        );
    }
}
