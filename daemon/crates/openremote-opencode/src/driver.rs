//! The OpenCode session driver: one `opencode serve` child per session.
//!
//! Flow: spawn `opencode serve --port 0` (password generated for the
//! child, read back from `OPENCODE_SERVER_PASSWORD`), parse its listening
//! line, create the session (`POST /api/session`, body `{id}` resumes),
//! then subscribe to `GET /api/event` and filter by session id. The
//! turn boundary is `session.next.step.ended` with OpenCode's own
//! `finish` word; permissions (`permission.v2.asked`) and questions
//! (`question.v2.asked`) arrive as events answered over HTTP with
//! OpenCode's own words.

use std::collections::HashMap;
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use openremote_core::{DecisionKind, DecisionOption, TurnOutcome};
use openremote_harness::events::{ApprovalRequest, DecisionSpec};
use openremote_harness::{
    DriverError, DriverEvent, Prompt, Resolution, SessionSettings, SpawnOptions,
};
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::{Mutex as AsyncMutex, mpsc};
use uuid::Uuid;

use crate::listen::{Wire, parse_listen_line};

const START_TIMEOUT: Duration = Duration::from_secs(30);

/// A transport error on the event stream is retried while the serve
/// child stays alive: a momentary loopback hiccup must not end a healthy
/// session's stream. Five attempts with doubling
/// backoff starting at 250ms stays well inside the supervisor's stop
/// drain (3s) when the stream dies during a stop, and the child liveness
/// gate means a dead serve never pays the full ladder.
const SSE_RETRY_LIMIT: usize = 5;
const SSE_RETRY_PAUSE: Duration = Duration::from_millis(250);

pub struct Driver {
    child: Arc<AsyncMutex<Child>>,
    client: crate::http::HttpClient,
    session_id: String,
    /// OpenCode's own command names, asked once, the first time a prompt
    /// starts with `/`.
    commands: Option<Vec<String>>,
    wire: Wire,
    /// v2 publishes a step end and an execution end for one turn. The
    /// first boundary closes the turn; a prompt reopens it.
    turn_open: Arc<AtomicBool>,
}

impl Driver {
    pub async fn spawn(
        resolution: &Resolution,
        opts: SpawnOptions,
    ) -> Result<(Self, mpsc::Receiver<DriverEvent>), DriverError> {
        let program = match resolution {
            Resolution::Executable(path) => path.clone(),
            Resolution::NodeScript { node, script } => {
                // opencode's npm layout is a native binary; the node form
                // would need the script as argv - not seen in the wild.
                let mut command = Command::new(node);
                command.arg(script);
                return spawn_serve(command, opts).await;
            }
            Resolution::Unavailable => {
                return Err(DriverError::Spawn("opencode CLI not found".to_string()));
            }
        };
        let command = Command::new(program);
        spawn_serve(command, opts).await
    }

    pub async fn send_prompt(&mut self, prompt: &Prompt) -> Result<(), DriverError> {
        let text = prompt.text_only("OpenCode")?;
        self.turn_open.store(true, Ordering::SeqCst);
        // A prompt that starts with one of OpenCode's own commands runs it
        // through OpenCode's command endpoint (its template, its
        // `$ARGUMENTS`); anything else is a plain prompt.
        if let Some((name, arguments)) = self.leading_command(text).await {
            let (path, body) = command_call(self.wire, &self.session_id, &name, &arguments);
            self.client.post(&path, body).await?;
            return Ok(());
        }
        self.client
            .post(
                &format!("/api/session/{}/prompt", self.session_id),
                prompt_body(self.wire, text),
            )
            .await?;
        Ok(())
    }

    /// `/name rest` when `name` is one of OpenCode's commands.
    async fn leading_command(&mut self, text: &str) -> Option<(String, String)> {
        let rest = text.trim_start().strip_prefix('/')?;
        let (name, arguments) = rest
            .split_once(char::is_whitespace)
            .map(|(n, a)| (n, a.trim()))
            .unwrap_or((rest.trim(), ""));
        if self.commands.is_none() {
            let path = if self.wire == Wire::V2 {
                "/api/command"
            } else {
                "/command"
            };
            let listed = self.client.get(path).await.ok()?;
            self.commands = Some(
                command_list(&listed)
                    .into_iter()
                    .flatten()
                    .filter_map(|c| c.get("name").and_then(Value::as_str).map(String::from))
                    .collect(),
            );
        }
        self.commands
            .iter()
            .flatten()
            .any(|known| known == name)
            .then(|| (name.to_string(), arguments.to_string()))
    }

    /// v2 takes a model on the live session (`POST /api/session/:id/model`).
    /// Effort and fast have no wire here; v1 takes the model only at
    /// process start. Either case is the start-time error the supervisor
    /// restarts on, so the stored model is what the next process runs.
    pub async fn apply_settings(&mut self, settings: &SessionSettings) -> Result<(), DriverError> {
        if let Some(model) = settings
            .model
            .as_deref()
            .map(str::trim)
            .filter(|model| !model.is_empty())
        {
            if self.wire == Wire::V2 {
                self.client
                    .post(
                        &format!("/api/session/{}/model", self.session_id),
                        json!({"model": model_ref(model)}),
                    )
                    .await?;
            } else {
                return Err(start_time_settings());
            }
        }
        if settings.effort.is_some() || settings.fast.is_some() {
            return Err(start_time_settings());
        }
        Ok(())
    }

    pub async fn interrupt(&mut self) -> Result<(), DriverError> {
        self.client
            .post(
                &format!("/api/session/{}/interrupt", self.session_id),
                json!({}),
            )
            .await?;
        Ok(())
    }

    /// Answer with OpenCode's own words: permissions take
    /// `once | always | reject`; questions take the chosen option label.
    pub async fn answer(
        &mut self,
        harness_ref: &str,
        choice: &str,
        request: &Value,
    ) -> Result<(), DriverError> {
        let kind = request
            .get("kind")
            .and_then(Value::as_str)
            .unwrap_or("permission");
        match kind {
            "question" => {
                self.client
                    .post(
                        &format!(
                            "/api/session/{}/question/{}/reply",
                            self.session_id, harness_ref
                        ),
                        json!({"answers": [[choice]]}),
                    )
                    .await?;
            }
            "form" => {
                let field = request
                    .get("field")
                    .and_then(Value::as_str)
                    .unwrap_or("answer");
                let value = if request.get("multiple").and_then(Value::as_bool) == Some(true) {
                    json!([choice])
                } else {
                    json!(choice)
                };
                self.client
                    .post(
                        &format!(
                            "/api/session/{}/form/{}/reply",
                            self.session_id, harness_ref
                        ),
                        json!({"answer": {field: value}}),
                    )
                    .await?;
            }
            _ => {
                // v1 reads `reply`. v2's schema is strict and reads `decision`.
                let body = if self.wire == Wire::V2 {
                    json!({"decision": choice})
                } else {
                    json!({"reply": choice})
                };
                self.client
                    .post(
                        &format!(
                            "/api/session/{}/permission/{}/reply",
                            self.session_id, harness_ref
                        ),
                        body,
                    )
                    .await?;
            }
        }
        Ok(())
    }

    /// Kill the serve child (the session stays in OpenCode's own store).
    pub async fn shutdown(&mut self) -> Result<Option<i32>, DriverError> {
        let mut child = self.child.lock().await;
        child.start_kill()?;
        match tokio::time::timeout(Duration::from_secs(5), child.wait()).await {
            Ok(Ok(status)) => Ok(status.code()),
            _ => Ok(None),
        }
    }
}

/// The chat's permission word is OpenCode's own permission config (a JSON
/// object such as `{"edit":"ask","bash":"ask"}`), which `opencode serve`
/// reads from `OPENCODE_PERMISSION` when it starts and merges over the
/// user's own config: the keys it names are this chat's, every other key
/// stays as the user set it. Without this the word was recorded on the
/// chat and never reached OpenCode, so the chat ran on OpenCode's default -
/// which lets edits and commands run without asking - whatever it chose.
///
/// OpenCode silently drops a value it cannot parse, and carries one that
/// parses but breaks its schema (`{"edit":"aks"}`) into its rules unchecked,
/// so a word outside that schema refuses to start rather than run on rules
/// nobody chose. A chat with no word has the variable removed, so one set in
/// the daemon's own environment never stands in for the chat's choice.
fn apply_permission(command: &mut Command, opts: &SpawnOptions) -> Result<(), DriverError> {
    let Some(word) = opts
        .permission_mode
        .as_deref()
        .map(str::trim)
        .filter(|w| !w.is_empty())
    else {
        command.env_remove("OPENCODE_PERMISSION");
        return Ok(());
    };
    if !serde_json::from_str::<Value>(word).is_ok_and(|v| is_permission_config(&v)) {
        return Err(DriverError::Spawn(
            "opencode takes its permission setting as its own permission config: a JSON \
             object whose values are \"ask\", \"allow\" or \"deny\", or a map of patterns \
             to those, such as {\"edit\":\"ask\",\"bash\":{\"git *\":\"allow\"}}"
                .into(),
        ));
    }
    command.env("OPENCODE_PERMISSION", word);
    Ok(())
}

/// The keys OpenCode's schema takes as a bare action only, never a map of
/// patterns (its `PermissionConfig`, opencode.ai/config.json).
const ACTION_ONLY_PERMISSIONS: [&str; 5] = [
    "todowrite",
    "question",
    "webfetch",
    "websearch",
    "doom_loop",
];

/// OpenCode's permission object, as its schema states it: each key an action
/// (`ask`, `allow`, `deny`) or, outside the action-only keys, a map of
/// patterns to actions. Any other key is allowed, as the schema allows it,
/// because tools that plugins add are named by their own keys.
fn is_permission_config(config: &Value) -> bool {
    let is_action = |v: &Value| matches!(v.as_str(), Some("ask" | "allow" | "deny"));
    let Some(entries) = config.as_object() else {
        return false;
    };
    entries.iter().all(|(key, rule)| match rule {
        Value::String(_) => is_action(rule),
        Value::Object(patterns) if !ACTION_ONLY_PERMISSIONS.contains(&key.as_str()) => {
            patterns.values().all(is_action)
        }
        _ => false,
    })
}

async fn spawn_serve(
    mut command: Command,
    opts: SpawnOptions,
) -> Result<(Driver, mpsc::Receiver<DriverEvent>), DriverError> {
    apply_permission(&mut command, &opts)?;
    let password = Uuid::new_v4().to_string();
    command
        .args(["serve", "--port", "0", "--hostname", "127.0.0.1"])
        .env("OPENCODE_SERVER_PASSWORD", &password)
        .current_dir(&opts.cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        // A dropped driver never orphans a serve child - the daemon's
        // crash takes its harnesses with it.
        .kill_on_drop(true);
    // Windows: no console flash, no Ctrl-C from a shared console; the
    // why lives on openremote_harness::creation_flags.
    #[cfg(windows)]
    command.creation_flags(openremote_harness::creation_flags());
    let mut child = command
        .spawn()
        .map_err(|e| DriverError::Spawn(format!("opencode: {e}")))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| DriverError::Spawn("stdout not piped".into()))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| DriverError::Spawn("stderr not piped".into()))?;

    // v1: "opencode server listening on http://127.0.0.1:<port>".
    // v2: "server listening on http://127.0.0.1:<port>".
    let mut lines = BufReader::new(stdout);
    let mut buffer = String::new();
    let (wire, port) = loop {
        buffer.clear();
        match tokio::time::timeout(START_TIMEOUT, lines.read_line(&mut buffer)).await {
            Ok(Ok(0)) => {
                return Err(exited_before_listening(stderr).await);
            }
            Ok(Ok(_)) => {
                if let Some(heard) = parse_listen_line(&buffer) {
                    break heard;
                }
            }
            Ok(Err(e)) => return Err(e.into()),
            Err(_) => {
                return Err(DriverError::Protocol(
                    "opencode serve did not listen in time".into(),
                ));
            }
        }
    };
    // Keep draining stdout so the pipe never fills.
    tokio::spawn(async move {
        let mut lines = lines;
        loop {
            let mut scratch = String::new();
            match lines.read_line(&mut scratch).await {
                Ok(0) | Err(_) => break,
                Ok(_) => {}
            }
        }
    });
    let (tx, rx) = mpsc::channel(256);
    let child = Arc::new(AsyncMutex::new(child));
    tokio::spawn(drain_stderr(BufReader::new(stderr), tx.clone()));

    let client = crate::http::HttpClient::new(port, &password);
    // Create (or resume) the session; the id is OpenCode's own. A pinned
    // model takes `{"id", "providerID"}` (verified live against the
    // installed opencode's own 400s - `modelID` is its old shape).
    let mut body = json!({});
    if let Some(model) = &opts.model {
        body["model"] = model_ref(model);
    }
    if let Some(resume) = &opts.resume {
        body["id"] = json!(resume);
    }
    let created = client.post("/api/session", body).await?;
    let session_id = created
        .pointer("/data/id")
        .and_then(Value::as_str)
        .ok_or_else(|| DriverError::Protocol("opencode session id missing".into()))?
        .to_string();
    if let Some(expected) = &opts.resume {
        if &session_id != expected {
            return Err(DriverError::Protocol(
                "opencode resumed a different session".into(),
            ));
        }
    }
    let _ = tx
        .send(DriverEvent::Initialized {
            session_ref: session_id.clone(),
            model: opts.model.clone(),
            permission_mode: None,
        })
        .await;

    let turn_open = Arc::new(AtomicBool::new(true));
    let driver = Driver {
        child: Arc::clone(&child),
        client,
        session_id: session_id.clone(),
        commands: None,
        wire,
        turn_open: Arc::clone(&turn_open),
    };
    // The bus watcher: every event for our session rides the driver
    // channel. A clean server-side close of the event connection is a
    // real EOF for this stream: the watcher reports it (StdoutClosed) so
    // the session lands on a recorded boundary instead of Working
    // forever behind a live serve child; a transport error is retried a
    // few times while the serve child stays alive (a momentary loopback
    // hiccup must not fail a healthy session) before the same boundary
    // is reported.
    let watch_client = crate::http::HttpClient::new(port, &password);
    let watch_child = child;
    let event_tx = tx.clone();
    let end_tx = tx.clone();
    let watch_session = session_id.clone();
    let tool_names = Arc::new(Mutex::new(HashMap::<String, String>::new()));
    tokio::spawn(async move {
        let mut backoff = SSE_RETRY_PAUSE;
        for attempt in 0..SSE_RETRY_LIMIT {
            let attempt_tx = event_tx.clone();
            let attempt_session = watch_session.clone();
            let attempt_tools = Arc::clone(&tool_names);
            let attempt_open = Arc::clone(&turn_open);
            let result = watch_client
                .sse("/api/event", move |event| {
                    remember_tool(&attempt_tools, &event);
                    let event = with_remembered_tool(&attempt_tools, event);
                    let Some(event) = map_event(&event, &attempt_session) else {
                        return;
                    };
                    if matches!(event, DriverEvent::TurnCompleted { .. })
                        && !attempt_open.swap(false, Ordering::SeqCst)
                    {
                        return;
                    }
                    let tx = attempt_tx.clone();
                    tokio::spawn(async move {
                        let _ = tx.send(event).await;
                    });
                })
                .await;
            match result {
                // Clean EOF: the server closed the stream. Reconnecting
                // here would paper over a serve that stopped serving
                // events for this session; a reconnect belongs on the
                // error path, where the child is healthy and the drop is
                // momentary. Future work: reestablish the subscription
                // with a Last-Event-ID replay when the console can
                // absorb the gap.
                Ok(()) => break,
                Err(_)
                    if attempt + 1 < SSE_RETRY_LIMIT && watch_child_alive(&watch_child).await =>
                {
                    tokio::time::sleep(backoff).await;
                    backoff *= 2;
                }
                Err(_) => break,
            }
        }
        let _ = end_tx.send(DriverEvent::StdoutClosed).await;
    });
    Ok((driver, rx))
}

/// Whether the serve child is still with us. A stopped/killed child
/// means the stream's death is the process's death: the pump's own
/// boundary events cover it, and the watcher must simply end (harness
/// audit F2).
async fn watch_child_alive(child: &Arc<AsyncMutex<Child>>) -> bool {
    let mut child = child.lock().await;
    child
        .try_wait()
        .map(|status| status.is_none())
        .unwrap_or(false)
}

/// Map one bus event for our session onto a driver event.
fn map_event(event: &Value, session_id: &str) -> Option<DriverEvent> {
    // v2 frames the payload as `data`; v1 and the fixture used `properties`.
    let properties = event.get("data").or_else(|| event.get("properties"))?;
    if event_session(properties) != Some(session_id) {
        return None;
    }
    let kind = event.get("type").and_then(Value::as_str)?;
    match kind {
        // OpenCode only names its model when a turn runs - the step-start
        // carries it (`model: {id, providerID}`, observed live). The
        // first one lights the chat's model chip before the reply lands.
        "session.next.step.started" | "session.step.started" => properties
            .pointer("/model/id")
            .and_then(Value::as_str)
            .map(|model| DriverEvent::ModelReported {
                model: model.to_string(),
            }),
        "session.next.text.delta" | "session.text.delta" => properties
            .get("delta")
            .and_then(Value::as_str)
            .map(|text| DriverEvent::TextDelta {
                text: text.to_string(),
            }),
        "session.next.text.ended" | "session.text.ended" => properties
            .get("text")
            .and_then(Value::as_str)
            .map(|text| DriverEvent::AssistantText {
                text: text.to_string(),
            }),
        "session.next.tool.called" | "session.tool.called" => Some(DriverEvent::ToolStarted {
            tool_use_id: tool_id(properties),
            name: tool_name(properties),
            input: properties.get("input").cloned().unwrap_or(Value::Null),
        }),
        "session.next.tool.success"
        | "session.next.tool.failed"
        | "session.tool.success"
        | "session.tool.failed" => {
            let failed = kind.ends_with("failed");
            Some(DriverEvent::ToolResult {
                tool_use_id: tool_id(properties),
                name: tool_name(properties),
                text: tool_text(properties),
                is_error: failed,
                images: Vec::new(),
            })
        }
        "session.reasoning.delta" => properties.get("delta").and_then(Value::as_str).map(|text| {
            DriverEvent::ReasoningDelta {
                text: text.to_string(),
            }
        }),
        // A failed step ends the turn on its own - no `step.ended`
        // follows (observed live) - with OpenCode's own error words.
        "session.next.step.failed" | "session.step.failed" | "session.execution.failed" => {
            let error = properties
                .pointer("/error/message")
                .and_then(Value::as_str)
                .unwrap_or("step failed");
            Some(DriverEvent::TurnCompleted {
                subtype: "error".to_string(),
                coarse: TurnOutcome::Failed,
                is_error: true,
                error_message: Some(error.to_string()),
            })
        }
        // v2's step end is not the turn end when the model called tools.
        // `tool-calls` means another step follows. `stop` is the model's
        // own finish word for the turn.
        "session.step.ended" => {
            let finish = properties
                .get("finish")
                .and_then(Value::as_str)
                .unwrap_or("error");
            if finish == "tool-calls" {
                return None;
            }
            let coarse = match finish {
                "stop" => TurnOutcome::Completed,
                _ => TurnOutcome::Failed,
            };
            Some(DriverEvent::TurnCompleted {
                subtype: finish.to_string(),
                coarse,
                is_error: coarse == TurnOutcome::Failed,
                error_message: properties
                    .pointer("/error/message")
                    .and_then(Value::as_str)
                    .map(String::from),
            })
        }
        "session.execution.succeeded" => Some(DriverEvent::TurnCompleted {
            subtype: "stop".to_string(),
            coarse: TurnOutcome::Completed,
            is_error: false,
            error_message: None,
        }),
        "session.execution.interrupted" => Some(DriverEvent::TurnCompleted {
            subtype: "aborted".to_string(),
            coarse: TurnOutcome::Interrupted,
            is_error: false,
            error_message: None,
        }),
        "session.next.step.ended" => {
            let finish = properties
                .get("finish")
                .and_then(Value::as_str)
                .unwrap_or("error");
            let coarse = match finish {
                "stop" | "toolUse" => TurnOutcome::Completed,
                "aborted" => TurnOutcome::Interrupted,
                _ => TurnOutcome::Failed,
            };
            Some(DriverEvent::TurnCompleted {
                subtype: finish.to_string(),
                coarse,
                is_error: coarse == TurnOutcome::Failed,
                error_message: properties
                    .get("error")
                    .and_then(Value::as_str)
                    .map(String::from),
            })
        }
        "permission.v2.asked" | "permission.asked" => {
            let id = properties.get("id").and_then(Value::as_str)?.to_string();
            let action = properties
                .get("action")
                .and_then(Value::as_str)
                .unwrap_or("permission");
            let resources = properties
                .get("resources")
                .and_then(Value::as_array)
                .map(|r| {
                    r.iter()
                        .filter_map(Value::as_str)
                        .collect::<Vec<_>>()
                        .join(", ")
                });
            let mut request = properties.clone();
            request["kind"] = json!("permission");
            Some(DriverEvent::ApprovalRequested {
                approval: ApprovalRequest {
                    harness_ref: id,
                    spec: DecisionSpec {
                        kind: DecisionKind::Approval,
                        // OpenCode's own reply words.
                        options: vec![
                            DecisionOption {
                                id: "once".into(),
                                label: "Once".into(),
                            },
                            DecisionOption {
                                id: "always".into(),
                                label: "Always".into(),
                            },
                            DecisionOption {
                                id: "reject".into(),
                                label: "Reject".into(),
                            },
                        ],
                        tool_name: Some(action.to_string()),
                        summary: resources,
                        interrupts_turn: Vec::new(),
                    },
                    request,
                },
            })
        }
        "form.created" => map_form(properties),
        "question.v2.asked" => {
            let id = properties.get("id").and_then(Value::as_str)?.to_string();
            let question = properties
                .pointer("/questions/0")
                .cloned()
                .unwrap_or(Value::Null);
            let options = question
                .get("options")
                .and_then(Value::as_array)
                .map(|opts| {
                    opts.iter()
                        .filter_map(|o| {
                            let label = o.get("label").and_then(Value::as_str)?;
                            Some(DecisionOption {
                                id: label.to_string(),
                                label: label.to_string(),
                            })
                        })
                        .collect()
                })
                .unwrap_or_default();
            let mut request = properties.clone();
            request["kind"] = json!("question");
            Some(DriverEvent::ApprovalRequested {
                approval: ApprovalRequest {
                    harness_ref: id,
                    spec: DecisionSpec {
                        kind: DecisionKind::Question,
                        options,
                        tool_name: question
                            .get("header")
                            .and_then(Value::as_str)
                            .map(String::from),
                        summary: question
                            .get("question")
                            .and_then(Value::as_str)
                            .map(String::from),
                        interrupts_turn: Vec::new(),
                    },
                    request,
                },
            })
        }
        _ => None,
    }
}

fn event_session(properties: &Value) -> Option<&str> {
    properties
        .get("sessionID")
        .and_then(Value::as_str)
        .or_else(|| {
            properties
                .pointer("/form/sessionID")
                .and_then(Value::as_str)
        })
}

fn tool_id(properties: &Value) -> String {
    properties
        .get("callID")
        .or_else(|| properties.get("id"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

fn tool_name(properties: &Value) -> String {
    properties
        .get("tool")
        .or_else(|| properties.get("name"))
        .and_then(Value::as_str)
        .filter(|name| !name.is_empty())
        .unwrap_or("tool")
        .to_string()
}

fn tool_text(properties: &Value) -> String {
    if let Some(result) = properties.get("result") {
        return match result {
            Value::String(text) => text.clone(),
            other => other.to_string(),
        };
    }
    if let Some(message) = properties.pointer("/error/message").and_then(Value::as_str) {
        return message.to_string();
    }
    properties
        .get("content")
        .map(|content| content.to_string())
        .unwrap_or_default()
}

fn remember_tool(tools: &Mutex<HashMap<String, String>>, event: &Value) {
    let Some(kind) = event.get("type").and_then(Value::as_str) else {
        return;
    };
    if kind != "session.tool.input.started" && kind != "session.tool.called" {
        return;
    }
    let Some(properties) = event.get("data").or_else(|| event.get("properties")) else {
        return;
    };
    let id = tool_id(properties);
    let name = properties
        .get("name")
        .or_else(|| properties.get("tool"))
        .and_then(Value::as_str)
        .filter(|name| !name.is_empty());
    if id.is_empty() {
        return;
    }
    if let Some(name) = name {
        tools
            .lock()
            .expect("tool names")
            .insert(id, name.to_string());
    }
}

fn with_remembered_tool(tools: &Mutex<HashMap<String, String>>, mut event: Value) -> Value {
    let Some(kind) = event.get("type").and_then(Value::as_str) else {
        return event;
    };
    if !kind.starts_with("session.tool.") {
        return event;
    }
    let key = if event.get("data").is_some() {
        "data"
    } else if event.get("properties").is_some() {
        "properties"
    } else {
        return event;
    };
    let Some(properties) = event.get_mut(key) else {
        return event;
    };
    if tool_name(properties) != "tool" {
        return event;
    }
    let id = tool_id(properties);
    if let Some(name) = tools.lock().expect("tool names").get(&id) {
        properties["tool"] = json!(name);
    }
    event
}

fn map_form(properties: &Value) -> Option<DriverEvent> {
    let form = properties.get("form")?;
    let id = form.get("id").and_then(Value::as_str)?.to_string();
    let field = form
        .get("fields")
        .and_then(Value::as_array)
        .and_then(|fields| fields.iter().find(|field| field.get("options").is_some()))?;
    let key = field.get("key").and_then(Value::as_str)?.to_string();
    let multiple = field.get("type").and_then(Value::as_str) == Some("multiselect");
    let options: Vec<DecisionOption> = field
        .get("options")
        .and_then(Value::as_array)
        .map(|opts| {
            opts.iter()
                .filter_map(|option| {
                    let id = option
                        .get("value")
                        .or_else(|| option.get("label"))
                        .and_then(Value::as_str)?;
                    let label = option.get("label").and_then(Value::as_str).unwrap_or(id);
                    Some(DecisionOption {
                        id: id.to_string(),
                        label: label.to_string(),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    if options.is_empty() {
        return None;
    }
    let request = json!({"kind": "form", "field": key, "multiple": multiple});
    Some(DriverEvent::ApprovalRequested {
        approval: ApprovalRequest {
            harness_ref: id,
            spec: DecisionSpec {
                kind: DecisionKind::Question,
                options,
                tool_name: form.get("title").and_then(Value::as_str).map(String::from),
                summary: field
                    .get("title")
                    .or_else(|| field.get("description"))
                    .and_then(Value::as_str)
                    .map(String::from),
                interrupts_turn: Vec::new(),
            },
            request,
        },
    })
}

fn model_ref(model: &str) -> Value {
    let provider = model.split('/').next().unwrap_or("");
    if model.contains('/') {
        json!({"id": model.split('/').nth(1).unwrap_or(model), "providerID": provider})
    } else {
        json!({"id": model, "providerID": "opencode"})
    }
}

fn start_time_settings() -> DriverError {
    DriverError::Harness(
        "this harness applies model and effort when the chat starts - stop it and resume to change them"
            .into(),
    )
}

fn prompt_body(wire: Wire, text: &str) -> Value {
    match wire {
        // v1's prompt route reads `prompt.text` (the fixture still does).
        Wire::V1 => json!({"prompt": {"text": text}}),
        // v2's schema is the prompt fields themselves, and it rejects extras.
        Wire::V2 => json!({"text": text}),
    }
}

fn command_call(wire: Wire, session_id: &str, name: &str, arguments: &str) -> (String, Value) {
    match wire {
        Wire::V1 => (
            format!("/session/{session_id}/command"),
            json!({"command": name, "arguments": arguments}),
        ),
        Wire::V2 => (
            format!("/api/session/{session_id}/command"),
            json!({"name": name, "text": arguments}),
        ),
    }
}

fn command_list(value: &Value) -> Option<&Vec<Value>> {
    value
        .as_array()
        .or_else(|| value.get("data").and_then(Value::as_array))
}

async fn exited_before_listening(stderr: tokio::process::ChildStderr) -> DriverError {
    let detail = stderr_excerpt(stderr).await;
    let message = if detail.is_empty() {
        "opencode exited before listening".to_string()
    } else {
        format!("opencode exited before listening: {detail}")
    };
    DriverError::Protocol(message)
}

pub(crate) async fn stderr_excerpt(stderr: tokio::process::ChildStderr) -> String {
    let mut lines = BufReader::new(stderr).lines();
    let mut out = String::new();
    let deadline = tokio::time::Instant::now() + Duration::from_millis(800);
    while tokio::time::Instant::now() < deadline && out.len() < 240 {
        match tokio::time::timeout(Duration::from_millis(200), lines.next_line()).await {
            Ok(Ok(Some(line))) => {
                let line = line.trim();
                if line.is_empty() {
                    continue;
                }
                if !out.is_empty() {
                    out.push_str("; ");
                }
                out.push_str(line);
            }
            _ => break,
        }
    }
    out
}

async fn drain_stderr(
    mut reader: BufReader<tokio::process::ChildStderr>,
    tx: mpsc::Sender<DriverEvent>,
) {
    let mut buffer = String::new();
    loop {
        buffer.clear();
        match reader.read_line(&mut buffer).await {
            Ok(0) | Err(_) => break,
            Ok(_) => {}
        }
        let line = buffer.trim_end_matches(['\r', '\n']).to_string();
        if line.is_empty() {
            continue;
        }
        if tx.send(DriverEvent::Stderr { line }).await.is_err() {
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn permission_env(command: &Command) -> Option<Option<String>> {
        command
            .as_std()
            .get_envs()
            .find(|(key, _)| *key == "OPENCODE_PERMISSION")
            .map(|(_, value)| value.map(|v| v.to_string_lossy().into_owned()))
    }

    fn opts_with(permission: Option<&str>) -> SpawnOptions {
        SpawnOptions {
            cwd: std::path::PathBuf::from("/w"),
            permission_mode: permission.map(String::from),
            ..Default::default()
        }
    }

    #[test]
    fn a_chats_permission_config_rides_opencode_permission() {
        // The chat's word is OpenCode's own permission config; the serve
        // child reads it from OPENCODE_PERMISSION, merged over the user's.
        let mut command = Command::new("opencode");
        apply_permission(
            &mut command,
            &opts_with(Some(r#"{"edit":"ask","bash":"ask"}"#)),
        )
        .expect("an object is OpenCode's config");
        assert_eq!(
            permission_env(&command),
            Some(Some(r#"{"edit":"ask","bash":"ask"}"#.to_string()))
        );
    }

    #[test]
    fn no_word_leaves_opencodes_own_config_alone() {
        for word in [None, Some(""), Some("  ")] {
            let mut command = Command::new("opencode");
            apply_permission(&mut command, &opts_with(word)).expect("nothing to pass");
            // Removed, not merely unset: an OPENCODE_PERMISSION in the
            // daemon's own environment never reaches a chat that chose none.
            assert_eq!(permission_env(&command), Some(None), "{word:?}");
        }
    }

    #[test]
    fn a_config_outside_opencodes_schema_refuses_to_start() {
        // Valid JSON that OpenCode would carry into its rules unchecked: a
        // misspelt action, a number, a pattern map under a key that takes
        // only an action, a pattern map with a bad action, a map nested
        // twice, and a top-level array inside the object.
        for word in [
            r#"{"edit":"aks"}"#,
            r#"{"bash":5}"#,
            r#"{"bash":null}"#,
            r#"{"bash":["allow"]}"#,
            r#"{"webfetch":{"*":"allow"}}"#,
            r#"{"doom_loop":{"*":"allow"}}"#,
            r#"{"bash":{"git *":"nope"}}"#,
            r#"{"bash":{"git *":{"x":"allow"}}}"#,
            r#"{"bash":{"git *":true}}"#,
        ] {
            let mut command = Command::new("opencode");
            let refused = apply_permission(&mut command, &opts_with(Some(word)));
            assert!(refused.is_err(), "{word} must not start");
            assert_eq!(permission_env(&command), None, "{word}");
        }
    }

    #[test]
    fn opencodes_own_shapes_pass_verbatim() {
        // An action per key, a pattern map of actions (order kept - OpenCode
        // lets the last matching pattern win), the action-only keys as plain
        // actions, an empty object, and a key OpenCode does not list (its
        // schema takes any key as a rule, for tools that plugins add).
        for word in [
            r#"{"bash":{"git *":"allow","*":"ask"}}"#,
            r#"{"*":"ask","read":"allow","edit":"deny"}"#,
            r#"{"webfetch":"ask","websearch":"deny","doom_loop":"ask","todowrite":"allow","question":"allow"}"#,
            r#"{"external_directory":{"/srv/work/*":"allow"}}"#,
            r#"{}"#,
            r#"{"custom_tool":"deny"}"#,
        ] {
            let mut command = Command::new("opencode");
            apply_permission(&mut command, &opts_with(Some(word))).expect(word);
            assert_eq!(permission_env(&command), Some(Some(word.to_string())));
        }
    }

    #[test]
    fn the_refusal_reads_as_one_sentence() {
        let mut command = Command::new("opencode");
        let refused = apply_permission(&mut command, &opts_with(Some("ask")))
            .expect_err("not a config")
            .to_string();
        assert!(!refused.contains("  "), "stray spaces in {refused:?}");
    }

    #[test]
    fn a_word_opencode_would_ignore_refuses_to_start() {
        // OpenCode drops an OPENCODE_PERMISSION it cannot parse without a
        // word, so passing one would run the chat on looser rules than it
        // chose. It does not start instead.
        for word in ["ask", "never", "[\"ask\"]", "\"ask\"", "{\"edit\":"] {
            let mut command = Command::new("opencode");
            let refused = apply_permission(&mut command, &opts_with(Some(word)));
            assert!(refused.is_err(), "{word} must not start");
            assert_eq!(permission_env(&command), None, "{word}");
        }
    }

    fn event(kind: &str, data: Value) -> Value {
        json!({"id": "evt_1", "type": kind, "data": data})
    }

    #[test]
    fn v2_text_and_stop_end_the_turn() {
        let delta = map_event(
            &event(
                "session.text.delta",
                json!({"sessionID": "ses_1", "delta": "hi"}),
            ),
            "ses_1",
        );
        assert!(matches!(delta, Some(DriverEvent::TextDelta { text }) if text == "hi"));

        let done = map_event(
            &event(
                "session.step.ended",
                json!({"sessionID": "ses_1", "finish": "stop"}),
            ),
            "ses_1",
        );
        assert!(matches!(
            done,
            Some(DriverEvent::TurnCompleted {
                coarse: TurnOutcome::Completed,
                ..
            })
        ));
    }

    #[test]
    fn v2_tool_calls_do_not_end_the_turn() {
        let mid = map_event(
            &event(
                "session.step.ended",
                json!({"sessionID": "ses_1", "finish": "tool-calls"}),
            ),
            "ses_1",
        );
        assert!(mid.is_none());
    }

    #[test]
    fn v1_fixture_events_still_map() {
        let asked = map_event(
            &json!({
                "type": "permission.v2.asked",
                "properties": {
                    "id": "per_1",
                    "sessionID": "ses_1",
                    "action": "bash",
                    "resources": ["echo"]
                }
            }),
            "ses_1",
        );
        assert!(matches!(asked, Some(DriverEvent::ApprovalRequested { .. })));
    }

    #[test]
    fn v2_prompt_body_is_flat() {
        assert_eq!(
            prompt_body(Wire::V1, "hi"),
            json!({"prompt": {"text": "hi"}})
        );
        assert_eq!(prompt_body(Wire::V2, "hi"), json!({"text": "hi"}));
    }

    #[test]
    fn remembered_tool_name_fills_a_v2_result() {
        let tools = Mutex::new(HashMap::new());
        let started = event(
            "session.tool.input.started",
            json!({"sessionID": "ses_1", "id": "call_1", "name": "bash"}),
        );
        remember_tool(&tools, &started);
        let result = with_remembered_tool(
            &tools,
            event(
                "session.tool.success",
                json!({"sessionID": "ses_1", "id": "call_1", "content": ["ok"]}),
            ),
        );
        let mapped = map_event(&result, "ses_1").expect("mapped");
        match mapped {
            DriverEvent::ToolResult {
                name,
                text,
                is_error,
                ..
            } => {
                assert_eq!(name, "bash");
                assert!(!is_error);
                assert!(text.contains("ok"));
            }
            other => panic!("unexpected {other:?}"),
        }
    }
}
