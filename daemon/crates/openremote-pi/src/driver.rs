//! The Pi session driver: one persistent `pi --mode rpc` process.
//!
//! Wire (flat JSON lines): our calls `{id, type, …params}` are answered
//! by `{type: "response", id, command, success, data|error}`;
//! notifications carry the stream (`message_update`,
//! `tool_execution_start|update|end`, `message_end`, `agent_settled`)
//! and the dialogs (`extension_ui_request` — answered with
//! `extension_ui_response` carrying `{value}` or `{cancelled}`). The
//! turn boundary is `message_end` with the assistant message's
//! `stopReason` — pi's own words (`stop`/`toolUse` → completed,
//! `aborted` → interrupted, `error`/`length` → failed).

use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::{Arc, Mutex as StdMutex};
use std::time::Duration;

use openremote_core::{DecisionKind, DecisionOption, TurnOutcome};
use openremote_harness::events::{ApprovalRequest, DecisionSpec};
use openremote_harness::{DriverError, DriverEvent, Resolution, SessionSettings, SpawnOptions};
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::{Mutex as AsyncMutex, mpsc, oneshot};

const CALL_TIMEOUT: Duration = Duration::from_secs(60);

struct Shared {
    child: Arc<AsyncMutex<Child>>,
    /// call id (string) → response waiter
    pending: StdMutex<HashMap<String, oneshot::Sender<Value>>>,
}

pub struct Driver {
    stdin: tokio::process::ChildStdin,
    shared: Arc<Shared>,
    next_id: u64,
}

impl Driver {
    /// Spawn `pi --mode rpc`, run `get_state`, and hand the supervisor
    /// pi's own session id as the conversation ref.
    pub async fn spawn(
        resolution: &Resolution,
        opts: SpawnOptions,
    ) -> Result<(Self, mpsc::Receiver<DriverEvent>), DriverError> {
        let (program, prefix): (PathBuf, Vec<std::ffi::OsString>) = match resolution {
            Resolution::Executable(path) => (path.clone(), Vec::new()),
            Resolution::NodeScript { node, script } => {
                (node.clone(), vec![script.as_os_str().to_os_string()])
            }
            Resolution::Unavailable => {
                return Err(DriverError::Spawn("pi CLI not found".to_string()));
            }
        };
        let mut command = Command::new(program);
        command.args(&prefix);
        command
            .arg("--mode")
            .arg("rpc")
            .arg("--session-dir")
            .arg(".pi-sessions")
            .current_dir(&opts.cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if let Some(model) = &opts.model {
            command.arg("--model").arg(model);
        }
        // Resume pi's own conversation by id (pi accepts path|id).
        if let Some(resume) = &opts.resume {
            command.arg("--session").arg(resume);
        }

        let mut child = command
            .spawn()
            .map_err(|e| DriverError::Spawn(format!("pi: {e}")))?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| DriverError::Spawn("stdin not piped".into()))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| DriverError::Spawn("stdout not piped".into()))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| DriverError::Spawn("stderr not piped".into()))?;

        let (tx, rx) = mpsc::channel(256);
        let shared = Arc::new(Shared {
            child: Arc::new(AsyncMutex::new(child)),
            pending: StdMutex::new(HashMap::new()),
        });
        tokio::spawn(read_stdout(
            BufReader::new(stdout),
            tx.clone(),
            Arc::clone(&shared),
        ));
        tokio::spawn(read_stderr(BufReader::new(stderr), tx.clone()));

        let mut driver = Self {
            stdin,
            shared,
            next_id: 1,
        };

        // get_state names the conversation (cloudroom lines 126-151).
        let state = driver
            .call("get_state", json!({}))
            .await
            .map_err(|e| DriverError::Protocol(format!("pi get_state failed: {e}")))?;
        let session_id = state
            .get("sessionId")
            .and_then(Value::as_str)
            .ok_or_else(|| DriverError::Protocol("pi reported no session id".into()))?
            .to_string();
        if let Some(expected) = &opts.resume {
            if &session_id != expected {
                return Err(DriverError::Protocol(
                    "pi resumed a different conversation".into(),
                ));
            }
        }
        let model = state
            .pointer("/model/id")
            .and_then(Value::as_str)
            .map(String::from);
        let _ = tx
            .send(DriverEvent::Initialized {
                session_ref: session_id,
                model,
                permission_mode: None,
            })
            .await;
        Ok((driver, rx))
    }

    /// Fire one prompt; pi's turn boundary arrives as `message_end`.
    pub async fn send_prompt(&mut self, text: &str) -> Result<(), DriverError> {
        self.write(json!({"type": "prompt", "message": text})).await
    }

    /// Pi's own rpc: `set_model` and `set_thinking_level` apply to the
    /// running process. Pi has no fast mode.
    pub async fn apply_settings(&mut self, settings: &SessionSettings) -> Result<(), DriverError> {
        if settings.fast.is_some() {
            return Err(DriverError::Harness("Pi Agent has no fast mode".into()));
        }
        if let Some(model) = &settings.model {
            self.call("set_model", json!({"model": model})).await?;
        }
        if let Some(effort) = &settings.effort {
            self.call("set_thinking_level", json!({"level": effort}))
                .await?;
        }
        Ok(())
    }

    /// Abort the running turn (`message_end` with `stopReason: aborted`
    /// follows).
    pub async fn interrupt(&mut self) -> Result<(), DriverError> {
        self.write(json!({"type": "abort"})).await
    }

    /// Answer a dialog: the choice id is pi's own value (a select option
    /// or a confirm boolean) — routed with the dialog's id.
    pub async fn answer(
        &mut self,
        harness_ref: &str,
        choice: &str,
        request: &Value,
    ) -> Result<(), DriverError> {
        let method = request.get("method").and_then(Value::as_str).unwrap_or("");
        let response = match method {
            // Free-text dialogs are not button-answerable tonight; pi gets
            // its own cancelled shape (cloudroom's behavior for all dialogs
            // — ours only where the console can't answer yet).
            "input" | "editor" => json!({"cancelled": true}),
            "confirm" => json!({"value": choice == "true"}),
            _ => json!({"value": choice}),
        };
        let mut frame = json!({"type": "extension_ui_response", "id": harness_ref});
        if let Some(fields) = response.as_object() {
            for (key, value) in fields {
                frame[key] = value.clone();
            }
        }
        self.write(frame).await
    }

    /// SIGTERM, wait up to 5s, SIGKILL.
    pub async fn shutdown(&mut self) -> Result<Option<i32>, DriverError> {
        let child = self.shared.child.lock().await;
        kill_and_reap(child).await
    }

    // ---- plumbing ----

    /// Pi folds id and method into one flat object; responses come back
    /// as `{type: "response", id, command, success, …}`.
    async fn call(&mut self, method: &str, params: Value) -> Result<Value, DriverError> {
        let id = self.next_id.to_string();
        self.next_id += 1;
        let (tx, rx) = oneshot::channel();
        self.shared.pending.lock().unwrap().insert(id.clone(), tx);
        let mut frame = json!({"id": id, "type": method});
        if let Some(map) = params.as_object() {
            for (key, value) in map {
                frame[key] = value.clone();
            }
        }
        self.write(frame).await?;
        match tokio::time::timeout(CALL_TIMEOUT, rx).await {
            Ok(Ok(response)) => {
                if response.get("success") == Some(&json!(true)) {
                    Ok(response.get("data").cloned().unwrap_or(Value::Null))
                } else {
                    Err(DriverError::Harness(format!(
                        "pi rejected {method}: {}",
                        response
                            .get("error")
                            .and_then(Value::as_str)
                            .unwrap_or("no message")
                    )))
                }
            }
            Ok(Err(_)) => Err(DriverError::Gone),
            Err(_) => Err(DriverError::Protocol(format!("{method} timed out"))),
        }
    }

    async fn write(&mut self, value: Value) -> Result<(), DriverError> {
        let mut line = serde_json::to_string(&value).expect("envelope serializes");
        line.push('\n');
        self.stdin.write_all(line.as_bytes()).await?;
        self.stdin.flush().await?;
        Ok(())
    }
}

async fn kill_and_reap(
    mut child: tokio::sync::MutexGuard<'_, Child>,
) -> Result<Option<i32>, DriverError> {
    child.start_kill()?;
    match tokio::time::timeout(Duration::from_secs(5), child.wait()).await {
        Ok(Ok(status)) => Ok(status.code()),
        Ok(Err(e)) => Err(e.into()),
        Err(_) => {
            child.start_kill()?;
            let status = child.wait().await?;
            Ok(status.code())
        }
    }
}

/// pi's own outcome words from `message_end`.
fn coarse_outcome(stop_reason: &str) -> TurnOutcome {
    match stop_reason {
        "stop" | "toolUse" => TurnOutcome::Completed,
        "aborted" => TurnOutcome::Interrupted,
        _ => TurnOutcome::Failed,
    }
}

/// The decision spec for a dialog, in pi's own words.
fn dialog_spec(request: &Value) -> DecisionSpec {
    let method = request
        .get("method")
        .and_then(Value::as_str)
        .unwrap_or("select");
    let title = request
        .get("title")
        .and_then(Value::as_str)
        .map(String::from);
    match method {
        "confirm" => DecisionSpec {
            kind: DecisionKind::Question,
            options: vec![
                DecisionOption {
                    id: "true".into(),
                    label: "Confirm".into(),
                },
                DecisionOption {
                    id: "false".into(),
                    label: "Cancel".into(),
                },
            ],
            tool_name: Some("confirm".into()),
            summary: title,
            interrupts_turn: Vec::new(),
        },
        "input" | "editor" => DecisionSpec {
            kind: DecisionKind::Question,
            options: Vec::new(),
            tool_name: Some(method.to_string()),
            summary: title,
            interrupts_turn: Vec::new(),
        },
        _ => {
            let options = request
                .get("options")
                .and_then(Value::as_array)
                .map(|opts| {
                    opts.iter()
                        .filter_map(|o| {
                            let label = o.as_str().map(String::from).unwrap_or_default();
                            (!label.is_empty()).then_some(DecisionOption {
                                id: label.clone(),
                                label,
                            })
                        })
                        .collect()
                })
                .unwrap_or_default();
            DecisionSpec {
                kind: DecisionKind::Question,
                options,
                tool_name: Some(method.to_string()),
                summary: title,
                interrupts_turn: Vec::new(),
            }
        }
    }
}

async fn read_stdout(
    mut reader: BufReader<tokio::process::ChildStdout>,
    tx: mpsc::Sender<DriverEvent>,
    shared: Arc<Shared>,
) {
    let mut buffer = String::new();
    loop {
        buffer.clear();
        // ICRNL lesson: both delimiters are legal.
        match reader.read_line(&mut buffer).await {
            Ok(0) => break,
            Ok(_) => {}
            Err(e) => {
                let _ = tx
                    .send(DriverEvent::Stderr {
                        line: format!("stdout read error: {e}"),
                    })
                    .await;
                break;
            }
        }
        let line = buffer.trim_end_matches(['\r', '\n']);
        if line.trim().is_empty() {
            continue;
        }
        let Ok(frame) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        let frame_type = frame
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let event = match frame_type {
            "response" => {
                if let Some(id) = frame.get("id").and_then(Value::as_str) {
                    if let Some(waiter) = shared.pending.lock().unwrap().remove(id) {
                        let _ = waiter.send(frame.clone());
                    }
                }
                // A failed prompt response surfaces as a note.
                if frame.get("command") == Some(&json!("prompt"))
                    && frame.get("success") != Some(&json!(true))
                {
                    Some(DriverEvent::Stderr {
                        line: format!(
                            "pi rejected the prompt: {}",
                            frame
                                .get("error")
                                .and_then(Value::as_str)
                                .unwrap_or("no message")
                        ),
                    })
                } else {
                    None
                }
            }
            "message_update" => {
                let delta_type = frame
                    .pointer("/assistantMessageEvent/type")
                    .and_then(Value::as_str);
                if delta_type == Some("text_delta") {
                    frame
                        .pointer("/assistantMessageEvent/delta")
                        .and_then(Value::as_str)
                        .map(|text| DriverEvent::TextDelta {
                            text: text.to_string(),
                        })
                } else {
                    None
                }
            }
            "message_end" => {
                if frame.pointer("/message/role") == Some(&json!("assistant")) {
                    let stop_reason = frame
                        .pointer("/message/stopReason")
                        .and_then(Value::as_str)
                        .unwrap_or("error");
                    let is_error = stop_reason == "error" || stop_reason == "length";
                    Some(DriverEvent::TurnCompleted {
                        subtype: stop_reason.to_string(),
                        coarse: coarse_outcome(stop_reason),
                        is_error,
                        error_message: frame
                            .pointer("/message/errorMessage")
                            .and_then(Value::as_str)
                            .map(String::from),
                    })
                } else {
                    None
                }
            }
            "tool_execution_start" => Some(DriverEvent::ToolStarted {
                tool_use_id: frame
                    .get("toolCallId")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
                name: frame
                    .get("toolName")
                    .and_then(Value::as_str)
                    .unwrap_or("tool")
                    .to_string(),
                input: frame.clone(),
            }),
            "tool_execution_end" => Some(DriverEvent::ToolResult {
                tool_use_id: frame
                    .get("toolCallId")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
                name: frame
                    .get("toolName")
                    .and_then(Value::as_str)
                    .unwrap_or("tool")
                    .to_string(),
                text: frame
                    .get("result")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
                is_error: false,
            }),
            "extension_ui_request" => {
                let id = frame
                    .get("id")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string();
                Some(DriverEvent::ApprovalRequested {
                    approval: ApprovalRequest {
                        harness_ref: id,
                        spec: dialog_spec(&frame),
                        request: frame.clone(),
                    },
                })
            }
            _ => None, // agent_settled, thinking deltas, future types
        };
        if let Some(event) = event {
            if tx.send(event).await.is_err() {
                return;
            }
        }
    }
    let _ = tx.send(DriverEvent::StdoutClosed).await;
}

async fn read_stderr(
    mut reader: BufReader<tokio::process::ChildStderr>,
    tx: mpsc::Sender<DriverEvent>,
) {
    let mut buffer = String::new();
    loop {
        buffer.clear();
        match reader.read_line(&mut buffer).await {
            Ok(0) => break,
            Ok(_) => {}
            Err(_) => break,
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
