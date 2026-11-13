//! The fixture's Pi rpc mode: flat JSON lines exactly as the
//! openremote-pi driver expects - `{id, type, …}` calls, `{type:
//! "response", id, command, success, data}` answers, `message_update` /
//! `tool_execution_*` / `message_end` notifications, and
//! `extension_ui_request` dialogs answered with
//! `extension_ui_response` (`{value}` or `{cancelled}`).
//!
//! Scenarios: `plain` completes without dialogs; `dialog` asks a
//! select question mid-turn and reflects the answer; `mcp-tool` runs one
//! tool between two assistant messages before settling; `boot-crash`
//! dies before answering `get_state` - the broken-install shape whose
//! exit status is available in the first second.

use std::io::{BufRead, Write};

use serde_json::{Value, json};

pub fn main_pi() {
    // Where the driver told pi to keep the transcript, for the e2e to check
    // it is not the workspace. Written into the cwd, which is the workspace.
    let argv: Vec<String> = std::env::args().collect();
    if let Some(pair) = argv.windows(2).find(|pair| pair[0] == "--session-dir") {
        crate::record("fixture-pi-session-dir", &pair[1]);
    }
    let mut server = PiFixture::from_env();
    server.run();
}

struct PiFixture {
    scenario: String,
    session_id: String,
    prompt_count: u64,
    /// A turn parked on the host's dialog answer.
    pending_dialog: Option<PendingDialog>,
}

struct PendingDialog {
    dialog_id: String,
    message: String,
}

impl PiFixture {
    fn from_env() -> Self {
        let scenario = std::env::var("FIXTURE_AGENT_SCENARIO")
            .ok()
            .or_else(|| {
                std::fs::read_to_string("fixture-scenario")
                    .ok()
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
            })
            .unwrap_or_else(|| "plain".to_string());
        Self {
            scenario,
            session_id: "pi-fixture-session-1".to_string(),
            prompt_count: 0,
            pending_dialog: None,
        }
    }

    fn run(&mut self) {
        // The boot-crash knob: a CLI broken at boot never answers
        // get_state. Exiting before the first frame is the observed shape
        // of a missing DLL / expired auth crash - exit now, with the
        // reason on stderr, so the driver's death race has the
        // first-second truth to fail on.
        if self.scenario == "boot-crash" {
            eprintln!("fixture-pi: crashed at boot: the install is broken");
            return;
        }
        // `tree` is `plain` plus a helper of its own: a process this CLI
        // started and OpenRemote never saw. It starts before the first frame
        // so a test has its pid as soon as the session exists.
        if self.scenario == "tree" {
            crate::spawn_grandchild();
        }
        let stdin = std::io::stdin();
        for line in stdin.lock().lines() {
            let line = match line {
                Ok(l) => l.trim_end_matches('\r').to_string(),
                Err(_) => break,
            };
            if line.trim().is_empty() {
                continue;
            }
            let Ok(frame) = serde_json::from_str::<Value>(&line) else {
                eprintln!("fixture-pi: unparsable stdin line: {line}");
                continue;
            };
            self.handle_frame(frame);
        }
    }

    fn handle_frame(&mut self, frame: Value) {
        let frame_type = frame
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or_default();
        // A dialog answer rides extension_ui_response.
        if frame_type == "extension_ui_response" {
            self.resolve_dialog(&frame);
            return;
        }
        let id = frame
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        match frame_type {
            "get_state" => self.respond(
                &id,
                "get_state",
                json!({
                    "sessionId": self.session_id,
                    "sessionFile": "fixture.jsonl",
                    "model": {"id": "pi-fixture-model", "provider": "fixture"},
                    "thinkingLevel": "low",
                    "isStreaming": false,
                    "isCompacting": false
                }),
            ),
            "prompt" => {
                let message = frame
                    .get("message")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                self.prompt_count += 1;
                // The prompt response acknowledges acceptance; the turn
                // plays out as notifications.
                self.respond(&id, "prompt", json!({"accepted": true}));
                self.run_turn(message);
            }
            "set_thinking_level" => {
                let level = frame
                    .get("level")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                crate::record("fixture-pi-thinking", level);
                self.respond(&id, "set_thinking_level", json!({}));
            }
            "abort" => {
                self.respond(&id, "abort", json!({"aborted": true}));
                if let Some(pending) = self.pending_dialog.take() {
                    let _ = pending;
                }
                self.notify(json!({
                    "type": "message_end",
                    "message": {"role": "assistant", "stopReason": "aborted", "usage": {}}
                }));
                self.notify(json!({"type": "agent_settled", "aborted": true}));
            }
            other => {
                eprintln!("fixture-pi: unknown call {other}");
                self.respond(&id, other, json!({}));
            }
        }
    }

    fn run_turn(&mut self, message: &str) {
        self.notify(json!({"type": "agent_start"}));
        self.notify(json!({
            "type": "message_update",
            "assistantMessageEvent": {"type": "text_delta", "delta": "work"}
        }));
        if self.scenario == "mcp-tool" {
            // A run that uses a tool, in the shape pi's json event docs
            // give: the first assistant message stops for the tool, the
            // tool runs, a second message answers, and only then does the
            // run settle. A driver that ends the turn at the first
            // `message_end` files the tool and the answer after the turn.
            self.notify(json!({
                "type": "message_end",
                "message": {
                    "role": "assistant",
                    "stopReason": "toolUse",
                    "usage": {"input": 900, "output": 12, "cacheRead": 100,
                              "cacheWrite": 0, "totalTokens": 1012,
                              "cost": {"input": 0.0, "output": 0.0, "cacheRead": 0.0,
                                       "cacheWrite": 0.0, "total": 0.01}}
                }
            }));
            self.notify(json!({
                "type": "tool_execution_start", "toolCallId": "call_1",
                "toolName": "bash", "args": {"command": "ls"}
            }));
            self.notify(json!({
                "type": "tool_execution_end", "toolCallId": "call_1",
                "toolName": "bash", "isError": false,
                "result": {"content": [{"type": "text", "text": "Cargo.toml"}], "details": {}}
            }));
            self.notify(json!({
                "type": "message_update",
                "assistantMessageEvent": {"type": "text_delta", "delta": " and done"}
            }));
            self.notify(json!({
                "type": "message_end",
                "message": {
                    "role": "assistant",
                    "stopReason": "stop",
                    "usage": {"input": 1100, "output": 20, "cacheRead": 100,
                              "cacheWrite": 0, "totalTokens": 1220,
                              "cost": {"input": 0.0, "output": 0.0, "cacheRead": 0.0,
                                       "cacheWrite": 0.0, "total": 0.02}}
                }
            }));
            self.notify(json!({"type": "agent_end", "messages": [], "willRetry": false}));
            self.notify(json!({"type": "agent_settled", "aborted": false}));
            return;
        }
        if self.scenario == "dialog" && self.pending_dialog.is_none() {
            let dialog_id = format!("dialog-{}", self.prompt_count);
            self.notify(json!({
                "type": "extension_ui_request",
                "id": dialog_id,
                "method": "select",
                "title": "Which database?",
                "options": ["Postgres", "SQLite"]
            }));
            self.pending_dialog = Some(PendingDialog {
                dialog_id,
                message: message.to_string(),
            });
            return; // the turn resumes when the dialog is answered
        }
        self.finish_turn(message, "stop");
    }

    fn resolve_dialog(&mut self, frame: &Value) {
        let Some(pending) = self.pending_dialog.take() else {
            return;
        };
        if frame.get("id").and_then(Value::as_str) != Some(pending.dialog_id.as_str()) {
            return;
        }
        let chosen = frame
            .get("value")
            .and_then(Value::as_str)
            .unwrap_or("cancelled");
        self.notify(json!({
            "type": "message_update",
            "assistantMessageEvent": {"type": "text_delta", "delta": chosen}
        }));
        self.finish_turn(&pending.message, "stop");
    }

    fn finish_turn(&self, message: &str, stop_reason: &str) {
        self.notify(json!({
            "type": "message_end",
            "message": {
                "role": "assistant",
                "stopReason": stop_reason,
                "usage": {"input_tokens": 10, "output_tokens": 5},
                "text": format!("done: {message}")
            }
        }));
        self.notify(json!({"type": "agent_settled"}));
    }

    fn respond(&self, id: &str, command: &str, data: Value) {
        self.out(json!({"type": "response", "id": id, "command": command, "success": true, "data": data}));
    }

    fn notify(&self, frame: Value) {
        self.out(frame);
    }

    fn out(&self, value: Value) {
        let mut stdout = std::io::stdout().lock();
        serde_json::to_writer(&mut stdout, &value).ok();
        stdout.write_all(b"\n").ok();
        stdout.flush().ok();
    }
}
