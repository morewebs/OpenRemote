//! The fixture's Codex app-server: JSON-lines RPC exactly as the
//! openremote-codex driver expects it - `initialize`/`initialized`,
//! `thread/start`|`thread/resume`, `turn/start` with text items,
//! `turn/interrupt`, paginated `model/list`, and approvals as
//! server→client `commandExecution/requestApproval` requests answered
//! with `{id, result: {decision}}`.
//!
//! Scenarios (via `FIXTURE_AGENT_SCENARIO` / `fixture-scenario`):
//! `plain` completes without tools; `approve` asks before a
//! `commandExecution` and reflects the decision (`cancel` interrupts the
//! turn, `decline` continues it with a denied result).

use std::io::{BufRead, Write};

use serde_json::{Value, json};

const THREAD_ID: &str = "thread-fixture-1";
const APPROVAL_REQUEST_ID: u64 = 5001;

pub fn main_codex() {
    let mut server = CodexFixture::from_env();
    server.run();
}

struct CodexFixture {
    scenario: String,
    turn_count: u64,
    thread_id: Option<String>,
    active_turn: Option<String>,
    /// The model the host last applied through thread/settings/update -
    /// echoed into turn answers so e2e can see it land.
    applied_model: Option<String>,
    /// A turn parked on the host's approval answer.
    pending_approval: Option<PendingTurn>,
}

struct PendingTurn {
    turn: String,
    item_id: String,
    prompt: String,
}

impl CodexFixture {
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
            turn_count: 0,
            thread_id: None,
            active_turn: None,
            applied_model: None,
            pending_approval: None,
        }
    }

    fn run(&mut self) {
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
                eprintln!("fixture-codex: unparsable stdin line: {line}");
                continue;
            };
            if self.handle_frame(frame) {
                break;
            }
        }
    }

    /// Returns true when the process should exit.
    fn handle_frame(&mut self, frame: Value) -> bool {
        // A response to one of our server requests (the approval answer).
        if frame.get("id").is_some()
            && frame.get("method").is_none()
            && frame.get("result").is_some()
        {
            self.resolve_approval(&frame);
            return false;
        }
        let Some(method) = frame.get("method").and_then(Value::as_str) else {
            eprintln!("fixture-codex: ignored frame without method: {frame}");
            return false;
        };
        let id = frame.get("id").and_then(Value::as_u64);
        let params = frame.get("params").cloned().unwrap_or(Value::Null);
        match (method, id) {
            ("initialize", Some(id)) => self.respond(
                id,
                json!({
                    "serverInfo": {"name": "fixture-codex", "version": "0.148.0"},
                    "capabilities": {}
                }),
            ),
            ("initialized", _) => {}
            ("thread/start", Some(id)) => {
                self.thread_id = Some(THREAD_ID.to_string());
                self.notify(
                    "thread/status/changed",
                    json!({
                        "threadId": THREAD_ID,
                        "status": {"type": "active", "activeFlags": []}
                    }),
                );
                let model = params
                    .get("model")
                    .and_then(Value::as_str)
                    .unwrap_or("gpt-5.1-codex");
                self.respond(
                    id,
                    json!({
                        "thread": {"id": THREAD_ID, "path": "/fixture/thread-1"},
                        "model": model
                    }),
                );
            }
            ("thread/resume", Some(id)) => {
                let resumed = params
                    .get("threadId")
                    .and_then(Value::as_str)
                    .unwrap_or(THREAD_ID);
                self.thread_id = Some(resumed.to_string());
                self.notify(
                    "thread/status/changed",
                    json!({
                        "threadId": resumed,
                        "status": {"type": "active", "activeFlags": []}
                    }),
                );
                self.respond(
                    id,
                    json!({
                        "thread": {"id": resumed, "path": "/fixture/thread-1"},
                        "model": "gpt-5.1-codex"
                    }),
                );
            }
            ("turn/start", Some(id)) => {
                self.turn_count += 1;
                let turn = format!("turn-fixture-{}", self.turn_count);
                self.active_turn = Some(turn.clone());
                self.respond(id, json!({"turn": {"id": turn}}));
                self.notify(
                    "turn/started",
                    json!({
                        "threadId": self.thread(),
                        "turn": {"id": turn}
                    }),
                );
                let prompt = params
                    .pointer("/input/0/text")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string();
                match self.scenario.as_str() {
                    "approve" => {
                        let item_id = format!("item-{}", self.turn_count);
                        self.notify("item/started", json!({
                            "threadId": self.thread(),
                            "turnId": turn,
                            "itemId": item_id,
                            "item": {"type": "commandExecution", "id": item_id, "command": "echo fixture"}
                        }));
                        // The approval: a server→client request.
                        self.out(json!({
                            "id": APPROVAL_REQUEST_ID,
                            "method": "commandExecution/requestApproval",
                            "params": {
                                "itemId": item_id,
                                "threadId": self.thread(),
                                "turnId": turn,
                                "command": "echo fixture",
                                "cwd": ".",
                                "reason": "fixture needs approval",
                                "startedAtMs": 1
                            }
                        }));
                        self.pending_approval = Some(PendingTurn {
                            turn,
                            item_id,
                            prompt,
                        });
                    }
                    _ => {
                        let item_id = format!("msg-{}", self.turn_count);
                        let model = self.applied_model.as_deref().unwrap_or("gpt-5.1-codex");
                        self.notify("item/completed", json!({
                            "threadId": self.thread(),
                            "turnId": turn,
                            "itemId": item_id,
                            "item": {"type": "agentMessage", "id": item_id,
                                     "text": format!("done: {prompt} on {model}")}
                        }));
                        // The thread's own context numbers - the harness
                        // reports both the used tokens and the window.
                        self.notify(
                            "thread/tokenUsage/updated",
                            json!({
                                "threadId": self.thread(),
                                "turnId": turn,
                                "tokenUsage": {
                                    "total": {"totalTokens": 1250, "inputTokens": 1000,
                                              "cachedInputTokens": 200, "cacheWriteInputTokens": 50,
                                              "outputTokens": 250, "reasoningOutputTokens": 0},
                                    "last": {"totalTokens": 300, "inputTokens": 250,
                                             "cachedInputTokens": 0, "cacheWriteInputTokens": 0,
                                             "outputTokens": 50, "reasoningOutputTokens": 0},
                                    "modelContextWindow": 200000
                                }
                            }),
                        );
                        // The compact scenario: the conversation outgrew
                        // its window first.
                        if self.scenario == "compact" {
                            self.notify(
                                "thread/compacted",
                                json!({"threadId": self.thread(), "turnId": turn}),
                            );
                        }
                        self.finish_turn(&turn, "completed");
                    }
                }
            }
            ("turn/steer", Some(id)) => {
                self.respond(id, json!({"steered": true}));
            }
            ("turn/interrupt", Some(id)) => {
                self.respond(id, json!({}));
                if let Some(turn) = self.active_turn.clone() {
                    self.finish_turn(&turn, "interrupted");
                }
                self.pending_approval = None;
            }
            ("model/list", Some(id)) => {
                let cursor = params.get("cursor").and_then(Value::as_str);
                match cursor {
                    None | Some("") => self.respond(
                        id,
                        json!({
                            "data": [
                                {"model": "gpt-5.1-codex", "supportedReasoningEfforts": [
                                    {"reasoningEffort": "low"},
                                    {"reasoningEffort": "medium"},
                                    {"reasoningEffort": "high"}
                                ]},
                                {"model": "gpt-5.1", "supportedReasoningEfforts": [
                                    {"reasoningEffort": "low"},
                                    {"reasoningEffort": "high"}
                                ]}
                            ],
                            "nextCursor": "page-2"
                        }),
                    ),
                    Some("page-2") => self.respond(
                        id,
                        json!({
                            "data": [
                                {"model": "o4-mini", "supportedReasoningEfforts": [
                                    {"reasoningEffort": "medium"}
                                ]}
                            ],
                            "nextCursor": null
                        }),
                    ),
                    Some(other) => {
                        self.respond(id, json!({"data": [], "nextCursor": null}));
                        eprintln!("fixture-codex: unknown model cursor {other}");
                    }
                }
            }
            ("thread/settings/update", Some(id)) => {
                // The host's live settings change: record the model and
                // echo it back - the real app-server answers the thread's
                // settings and notifies; the e2e asserts the call carried
                // the right words.
                if let Some(model) = params.get("model").and_then(Value::as_str) {
                    self.applied_model = Some(model.to_string());
                }
                let model = self.applied_model.as_deref().unwrap_or("gpt-5.1-codex");
                self.notify(
                    "thread/settings/updated",
                    json!({"threadId": self.thread(), "model": model}),
                );
                self.respond(id, json!({"model": model}));
            }
            (method, Some(id)) => {
                eprintln!("fixture-codex: unknown call {method}");
                self.respond(id, json!({}));
            }
            (method, None) => eprintln!("fixture-codex: unknown notification {method}"),
        }
        false
    }

    fn resolve_approval(&mut self, frame: &Value) {
        let id = frame.get("id").and_then(Value::as_u64).unwrap_or(0);
        let decision = frame
            .pointer("/result/decision")
            .and_then(Value::as_str)
            .unwrap_or("decline");
        if id != APPROVAL_REQUEST_ID {
            eprintln!("fixture-codex: response to unknown request {id}");
            return;
        }
        let Some(pending) = self.pending_approval.take() else {
            return;
        };
        let allowed = decision == "accept" || decision == "acceptForSession";
        self.notify(
            "item/completed",
            json!({
                "threadId": self.thread(),
                "turnId": pending.turn,
                "itemId": pending.item_id,
                "item": {
                    "type": "commandExecution",
                    "id": pending.item_id,
                    "command": "echo fixture",
                    "aggregatedOutput": if allowed { "fixture output" } else { "user declined" },
                    "exitCode": if allowed { 0 } else { 1 }
                }
            }),
        );
        if decision == "cancel" {
            // The schema: cancel denies AND interrupts the turn.
            self.finish_turn(&pending.turn, "interrupted");
            return;
        }
        let item_id = format!("msg-done-{}", self.turn_count);
        let text = if allowed {
            format!("ran the command for: {}", pending.prompt)
        } else {
            format!("the command was declined for: {}", pending.prompt)
        };
        self.notify(
            "item/completed",
            json!({
                "threadId": self.thread(),
                "turnId": pending.turn,
                "itemId": item_id,
                "item": {"type": "agentMessage", "id": item_id, "text": text}
            }),
        );
        self.finish_turn(&pending.turn, "completed");
    }

    fn finish_turn(&mut self, turn: &str, status: &str) {
        self.notify(
            "turn/completed",
            json!({
                "threadId": self.thread(),
                "turn": {"id": turn, "status": status}
            }),
        );
        self.notify(
            "thread/status/changed",
            json!({
                "threadId": self.thread(),
                "status": {"type": "idle"}
            }),
        );
        self.active_turn = None;
    }

    fn thread(&self) -> &str {
        self.thread_id.as_deref().unwrap_or(THREAD_ID)
    }

    fn respond(&mut self, id: u64, result: Value) {
        self.out(json!({"id": id, "result": result}));
    }

    fn notify(&self, method: &str, params: Value) {
        self.out(json!({"method": method, "params": params}));
    }

    fn out(&self, value: Value) {
        let mut stdout = std::io::stdout().lock();
        serde_json::to_writer(&mut stdout, &value).ok();
        stdout.write_all(b"\n").ok();
        stdout.flush().ok();
    }
}
