//! A scriptable fake harness CLI for daemon e2e tests.
//!
//! One binary, many wire protocols — chosen by the argv the real driver
//! builds, so e2e never smuggles fake flags:
//!
//! - Claude Code (`--output-format stream-json --input-format
//!   stream-json …`): user envelopes on stdin, NDJSON frames on stdout,
//!   `control_request`/`control_response` for approvals and interrupts,
//!   `result` as the turn boundary.
//! - Codex (`app-server --listen stdio://`): JSON-lines RPC — calls,
//!   notifications, and server→client approval requests answered with
//!   `{id, result: {decision}}`.
//!
//! The scenario is chosen with `FIXTURE_AGENT_SCENARIO` (or a
//! `fixture-scenario` file in the cwd).
//!
//! One reader owns stdin for the whole run — a turn that waits for an
//! approval parks in `pending_approval` and the main loop keeps reading;
//! nesting a second `stdin().lock()` deadlocks (Stdin's mutex is not
//! reentrant), which is exactly how the first e2e run hung.
//!
//! Lines are accepted with either `\r\n` or `\n` delimiters — the ICRNL
//! lesson, paid for once in a PTY and never again.

mod agy;
mod codex;
mod grok;
mod opencode;
mod pi;

use std::io::{BufRead, Write};

fn main() {
    // Protocol inference from the real driver's argv.
    let argv: Vec<String> = std::env::args().skip(1).collect();
    if argv.iter().any(|a| a == "app-server") {
        codex::main_codex();
        return;
    }
    if argv.iter().any(|a| a == "--single") {
        grok::main_grok(&argv);
        return;
    }
    if argv.iter().any(|a| a == "--mode") {
        pi::main_pi();
        return;
    }
    if argv.iter().any(|a| a == "serve") {
        opencode::main_opencode();
        return;
    }
    if argv.first().is_some_and(|a| a == "models")
        || argv.iter().any(|a| a == "--print" || a == "-p")
    {
        agy::main_agy(&argv);
        return;
    }
    let mut agent = FixtureAgent::from_env();
    agent.run();
}

/// A turn parked waiting for the host's approval answer.
struct PendingApproval {
    tool_id: String,
    prompt: String,
}

struct FixtureAgent {
    session_id: String,
    scenario: String,
    next_request_id: u64,
    next_tool_id: u64,
    prompt_count: u64,
    pending_approval: Option<PendingApproval>,
}

impl FixtureAgent {
    fn from_env() -> Self {
        let mut session_id = std::env::var("FIXTURE_AGENT_SESSION_ID")
            .unwrap_or_else(|_| format!("fixture-session-{}", std::process::id()));
        for arg in std::env::args().skip(1) {
            if let Some(id) = arg.strip_prefix("--resume=") {
                session_id = id.to_string();
            }
        }
        // Scenario resolution: env first, then a `fixture-scenario` file in
        // the cwd — the per-session channel the e2e suite uses without
        // smuggling fake flags into the argv the daemon builds.
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
            session_id,
            scenario,
            next_request_id: 1,
            next_tool_id: 1,
            prompt_count: 0,
            pending_approval: None,
        }
    }

    fn run(&mut self) {
        self.emit_init();
        let stdin = std::io::stdin();
        // ICRNL lesson: split on \n, trim a trailing \r — both delimiters are legal.
        for line in stdin.lock().lines() {
            let line = match line {
                Ok(l) => l.trim_end_matches('\r').to_string(),
                Err(_) => break,
            };
            if line.trim().is_empty() {
                continue;
            }
            let frame: serde_json::Value = match serde_json::from_str(&line) {
                Ok(v) => v,
                Err(e) => {
                    eprintln!("fixture-agent: unparsable stdin line ({e}): {line}");
                    continue;
                }
            };
            self.handle_frame(frame);
        }
    }

    fn handle_frame(&mut self, frame: serde_json::Value) {
        // A parked turn answers only its approval (or an interrupt).
        if self.pending_approval.is_some() {
            match frame.get("type").and_then(|t| t.as_str()) {
                Some("control_response") => self.resolve_approval(&frame),
                Some("control_request") => {
                    if frame["request"]["subtype"] == "interrupt" {
                        self.handle_control_request(&frame);
                        let pending = self.pending_approval.take().expect("parked turn");
                        self.result(
                            "error_during_execution",
                            "aborted while waiting",
                            "aborted_tools",
                        );
                        let _ = pending;
                    } else {
                        self.handle_control_request(&frame);
                    }
                }
                _ => eprintln!("fixture-agent: while awaiting approval, ignored: {frame}"),
            }
            return;
        }
        match frame.get("type").and_then(|t| t.as_str()) {
            Some("user") => self.handle_user_message(&frame),
            Some("control_request") => self.handle_control_request(&frame),
            Some("control_response") => eprintln!("fixture-agent: stray control_response ignored"),
            _ => eprintln!("fixture-agent: ignored frame: {frame}"),
        }
    }

    // ---- wire output helpers ----

    fn out(&self, value: serde_json::Value) {
        let mut stdout = std::io::stdout().lock();
        serde_json::to_writer(&mut stdout, &value).ok();
        stdout.write_all(b"\n").ok();
        stdout.flush().ok();
    }

    fn emit_init(&self) {
        self.out(serde_json::json!({
            "type": "system",
            "subtype": "init",
            "session_id": self.session_id,
            "model": "fixture-sonnet",
            "permissionMode": "default",
            "tools": ["Read", "Bash", "Edit"],
            "cwd": "."
        }));
    }

    fn assistant_text(&self, text: &str) {
        self.out(serde_json::json!({
            "type": "assistant",
            "session_id": self.session_id,
            "uuid": format!("asst-{}", self.prompt_count),
            "parent_tool_use_id": null,
            "message": {
                "id": format!("msg_{}", self.prompt_count),
                "role": "assistant",
                "model": "fixture-sonnet",
                "content": [{"type": "text", "text": text}],
                "stop_reason": null,
                "usage": {"input_tokens": 10, "output_tokens": 5}
            }
        }));
    }

    fn assistant_tool_use(&self, tool_id: &str, name: &str, input: serde_json::Value) {
        self.out(serde_json::json!({
            "type": "assistant",
            "session_id": self.session_id,
            "uuid": format!("asst-tool-{}", self.prompt_count),
            "parent_tool_use_id": null,
            "message": {
                "id": format!("msg_tool_{}", self.prompt_count),
                "role": "assistant",
                "model": "fixture-sonnet",
                "content": [{"type": "tool_use", "id": tool_id, "name": name, "input": input}],
                "stop_reason": null,
                "usage": {"input_tokens": 10, "output_tokens": 5}
            }
        }));
    }

    fn tool_result(&self, tool_id: &str, text: &str, is_error: bool) {
        self.out(serde_json::json!({
            "type": "user",
            "session_id": self.session_id,
            "parent_tool_use_id": null,
            "tool_use_result": text,
            "message": {
                "role": "user",
                "content": [{"type": "tool_result", "tool_use_id": tool_id, "is_error": is_error,
                             "content": [{"type": "text", "text": text}]}]
            }
        }));
    }

    fn result(&self, subtype: &str, text: &str, terminal_reason: &str) {
        self.out(serde_json::json!({
            "type": "result",
            "subtype": subtype,
            "session_id": self.session_id,
            "result": text,
            "is_error": subtype != "success",
            "num_turns": self.prompt_count,
            "terminal_reason": terminal_reason,
            "duration_ms": 7,
            "total_cost_usd": 0,
            "usage": {"input_tokens": 10, "output_tokens": 5}
        }));
    }

    fn can_use_tool_request(
        &mut self,
        tool_id: &str,
        name: &str,
        input: serde_json::Value,
    ) -> String {
        let request_id = format!("cr-{}", self.next_request_id);
        self.next_request_id += 1;
        self.out(serde_json::json!({
            "type": "control_request",
            "request_id": request_id,
            "request": {
                "subtype": "can_use_tool",
                "tool_name": name,
                "tool_use_id": tool_id,
                "input": input
            }
        }));
        request_id
    }

    fn control_response_ok(&self, request_id: &str, response: serde_json::Value) {
        self.out(serde_json::json!({
            "type": "control_response",
            "response": {"subtype": "success", "request_id": request_id, "response": response}
        }));
    }

    // ---- frame handling ----

    fn prompt_text(frame: &serde_json::Value) -> String {
        let content = &frame["message"]["content"];
        if let Some(text) = content.as_str() {
            return text.to_string();
        }
        if let Some(blocks) = content.as_array() {
            let texts: Vec<String> = blocks
                .iter()
                .filter_map(|b| b.get("text").and_then(|t| t.as_str()).map(String::from))
                .collect();
            return texts.join("\n");
        }
        String::new()
    }

    fn handle_user_message(&mut self, frame: &serde_json::Value) {
        self.prompt_count += 1;
        let prompt = Self::prompt_text(frame);
        match self.scenario.as_str() {
            "approve" | "deny" => self.scenario_approve(&prompt),
            "fail" => {
                self.assistant_text("the fixture is about to fail");
                self.result("error_during_execution", "fixture failure", "api_error");
            }
            "stream" => {
                for chunk in ["Hel", "lo ", "wor", "ld"] {
                    self.out(serde_json::json!({
                        "type": "stream_event",
                        "parent_tool_use_id": null,
                        "event": {"type": "content_block_delta",
                                  "delta": {"type": "text_delta", "text": chunk}}
                    }));
                }
                self.assistant_text("Hello world");
                self.result("success", "Hello world", "completed");
            }
            _ => {
                self.assistant_text(&format!("done: {prompt}"));
                self.result("success", &format!("done: {prompt}"), "completed");
            }
        }
    }

    /// Emits a tool_use and asks the host to approve it, then parks the
    /// turn; the main loop resumes reading and `resolve_approval` finishes
    /// it when the answer arrives.
    fn scenario_approve(&mut self, prompt: &str) {
        let tool_id = format!("toolu_{}", self.next_tool_id);
        self.next_tool_id += 1;
        let input =
            serde_json::json!({"command": "echo fixture", "description": "a fixture command"});
        self.assistant_tool_use(&tool_id, "Bash", input.clone());
        self.can_use_tool_request(&tool_id, "Bash", input);
        self.pending_approval = Some(PendingApproval {
            tool_id,
            prompt: prompt.to_string(),
        });
    }

    fn resolve_approval(&mut self, frame: &serde_json::Value) {
        let Some(pending) = self.pending_approval.take() else {
            return;
        };
        let behavior = frame["response"]["response"]["behavior"]
            .as_str()
            .unwrap_or("deny");
        let allowed = behavior == "allow";
        self.tool_result(
            &pending.tool_id,
            if allowed {
                "fixture output"
            } else {
                "permission denied"
            },
            !allowed,
        );
        let closing = if allowed {
            format!("ran the command for: {}", pending.prompt)
        } else {
            format!("permission denied for: {}", pending.prompt)
        };
        self.assistant_text(&closing);
        self.result("success", &closing, "completed");
    }

    fn handle_control_request(&mut self, frame: &serde_json::Value) {
        let request_id = frame["request_id"].as_str().unwrap_or("").to_string();
        match frame["request"]["subtype"].as_str() {
            Some("interrupt") => {
                self.control_response_ok(
                    &request_id,
                    serde_json::json!({"still_queued": [], "cancelled": []}),
                );
            }
            Some(other) => {
                eprintln!("fixture-agent: control request {other} acknowledged, no-op");
                self.control_response_ok(&request_id, serde_json::json!({}));
            }
            None => eprintln!("fixture-agent: control request without subtype"),
        }
    }
}
