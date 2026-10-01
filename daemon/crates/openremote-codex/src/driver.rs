//! The Codex session driver: one `codex app-server` process per session.
//!
//! Wire plumbing: client→server calls carry `id` and are answered by
//! `{id, result|error}`; notifications are `{method, params}`; the server
//! also makes REQUESTS (`{id, method, params}`) — approvals arrive that
//! way and are answered with `{id, result: {decision}}`. The turn boundary
//! is the `turn/completed` notification; never guess turn state.

use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::{Arc, Mutex as StdMutex};
use std::time::Duration;

use openremote_core::{DecisionKind, DecisionOption, TurnOutcome};
use openremote_harness::events::{ApprovalRequest, DecisionSpec};
use openremote_harness::{DriverError, DriverEvent, Resolution, SpawnOptions};
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::{Mutex as AsyncMutex, mpsc, oneshot};
use uuid::Uuid;

/// Default approval policy when the session didn't pick one — Codex's own
/// word for "ask me before running things".
const DEFAULT_APPROVAL_POLICY: &str = "on-request";
const DEFAULT_SANDBOX: &str = "workspace-write";

const CALL_TIMEOUT: Duration = Duration::from_secs(60);

struct Shared {
    child: Arc<AsyncMutex<Child>>,
    /// call id → response waiter
    pending: StdMutex<HashMap<u64, oneshot::Sender<Value>>>,
    /// The turn Codex reports as running (turn/started … turn/completed).
    active_turn: StdMutex<Option<String>>,
}

pub struct Driver {
    stdin: tokio::process::ChildStdin,
    shared: Arc<Shared>,
    thread_id: String,
    next_id: u64,
}

impl Driver {
    /// Spawn `codex app-server`, run the initialize handshake, and start
    /// (or resume) the thread. Returns once the harness conversation is
    /// named — the session ref rides out as the first `Initialized` event.
    pub async fn spawn(
        resolution: &Resolution,
        opts: SpawnOptions,
    ) -> Result<(Self, mpsc::Receiver<DriverEvent>), DriverError> {
        let (program, mut argv): (PathBuf, Vec<std::ffi::OsString>) = match resolution {
            Resolution::Executable(path) => (path.clone(), Vec::new()),
            Resolution::NodeScript { node, script } => {
                (node.clone(), vec![script.as_os_str().to_os_string()])
            }
            Resolution::Unavailable => {
                return Err(DriverError::Spawn("codex CLI not found".to_string()));
            }
        };
        argv.extend(["app-server".into(), "--listen".into(), "stdio://".into()]);

        let mut command = Command::new(&program);
        command
            .args(&argv)
            .current_dir(&opts.cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = command
            .spawn()
            .map_err(|e| DriverError::Spawn(format!("{program:?}: {e}")))?;
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
            active_turn: StdMutex::new(None),
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
            thread_id: String::new(),
            next_id: 1,
        };

        // Handshake (cloudroom lines 22-25): initialize, then the
        // `initialized` notification.
        driver
            .call(
                "initialize",
                json!({
                    "clientInfo": {"name": "openremote", "version": env!("CARGO_PKG_VERSION")},
                    "capabilities": {"experimentalApi": true}
                }),
            )
            .await?;
        driver.notify("initialized", json!({})).await?;

        // Thread: start or resume (cloudroom lines 70-99).
        let mut params = json!({
            "cwd": opts.cwd,
            "approvalPolicy": opts.permission_mode.clone().unwrap_or_else(|| DEFAULT_APPROVAL_POLICY.to_string()),
            "sandbox": DEFAULT_SANDBOX,
            "ephemeral": false,
        });
        if let Some(model) = &opts.model {
            params["model"] = json!(model);
        }
        let (method, thread_id) = if let Some(resume) = &opts.resume {
            params["threadId"] = json!(resume);
            params["excludeTurns"] = json!(true);
            ("thread/resume", resume.clone())
        } else {
            ("thread/start", String::new())
        };
        let result = driver.call(method, params).await?;
        let id = result
            .pointer("/thread/id")
            .and_then(Value::as_str)
            .ok_or_else(|| DriverError::Protocol("thread/start returned no thread id".into()))?
            .to_string();
        if !thread_id.is_empty() && id != thread_id {
            return Err(DriverError::Protocol(
                "codex resumed a different conversation".into(),
            ));
        }
        let model = result
            .get("model")
            .and_then(|m| m.as_str())
            .map(String::from);
        driver.thread_id = id.clone();
        // The harness conversation is named: hand the supervisor its
        // session ref through the same channel the reader owns.
        let _ = tx
            .send(DriverEvent::Initialized {
                session_ref: id,
                model,
                permission_mode: None,
            })
            .await;
        Ok((driver, rx))
    }

    /// Send one user message as a turn. If a turn is already running, the
    /// message steers it (Codex's own mechanism, `expectedTurnId`).
    pub async fn send_prompt(&mut self, text: &str) -> Result<(), DriverError> {
        let input = json!([{"type": "text", "text": text, "text_elements": []}]);
        let active = self.shared.active_turn.lock().unwrap().clone();
        if let Some(turn) = active {
            self.call(
                "turn/steer",
                json!({
                    "threadId": self.thread_id,
                    "expectedTurnId": turn,
                    "input": input,
                }),
            )
            .await?;
        } else {
            let result = self
                .call(
                    "turn/start",
                    json!({
                        "threadId": self.thread_id,
                        "clientUserMessageId": Uuid::new_v4().to_string(),
                        "input": input,
                    }),
                )
                .await?;
            if result.pointer("/turn/id").and_then(Value::as_str).is_none() {
                return Err(DriverError::Protocol(
                    "turn/start returned no turn id; outcome uncertain".into(),
                ));
            }
        }
        Ok(())
    }

    /// Interrupt the running turn. No active turn = nothing to do.
    pub async fn interrupt(&mut self) -> Result<(), DriverError> {
        let active = self.shared.active_turn.lock().unwrap().clone();
        if let Some(turn) = active {
            self.call(
                "turn/interrupt",
                json!({"threadId": self.thread_id, "turnId": turn}),
            )
            .await?;
        }
        Ok(())
    }

    /// Answer an approval request with Codex's own decision word
    /// (`accept | acceptForSession | decline | cancel`).
    pub async fn answer(
        &mut self,
        harness_ref: &str,
        choice: &str,
        _request: &Value,
    ) -> Result<(), DriverError> {
        let id: u64 = harness_ref
            .parse()
            .map_err(|_| DriverError::Protocol("codex approval ids are numeric".into()))?;
        self.write(json!({"id": id, "result": {"decision": choice}}))
            .await
    }

    /// SIGTERM, wait up to 5s, SIGKILL.
    pub async fn shutdown(&mut self) -> Result<Option<i32>, DriverError> {
        let child = self.shared.child.lock().await;
        kill_and_reap(child).await
    }

    // ---- plumbing ----

    async fn call(&mut self, method: &str, params: Value) -> Result<Value, DriverError> {
        let id = self.next_id;
        self.next_id += 1;
        let (tx, rx) = oneshot::channel();
        self.shared.pending.lock().unwrap().insert(id, tx);
        self.write(json!({"id": id, "method": method, "params": params}))
            .await?;
        match tokio::time::timeout(CALL_TIMEOUT, rx).await {
            Ok(Ok(value)) => {
                if let Some(error) = value.get("error") {
                    Err(DriverError::Harness(format!(
                        "codex rejected {method} (code {}): {}",
                        error.get("code").and_then(|c| c.as_i64()).unwrap_or(0),
                        error
                            .get("message")
                            .and_then(|m| m.as_str())
                            .unwrap_or("no message")
                    )))
                } else {
                    Ok(value.get("result").cloned().unwrap_or(Value::Null))
                }
            }
            Ok(Err(_)) => Err(DriverError::Gone),
            Err(_) => Err(DriverError::Protocol(format!("{method} timed out"))),
        }
    }

    async fn notify(&mut self, method: &str, params: Value) -> Result<(), DriverError> {
        self.write(json!({"method": method, "params": params}))
            .await
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

/// Codex's own decision words, in the schema's order.
fn approval_spec(tool: &str, summary: Option<String>) -> DecisionSpec {
    DecisionSpec {
        kind: DecisionKind::Approval,
        options: vec![
            DecisionOption {
                id: "accept".into(),
                label: "Accept".into(),
            },
            DecisionOption {
                id: "acceptForSession".into(),
                label: "Accept for session".into(),
            },
            DecisionOption {
                id: "decline".into(),
                label: "Decline".into(),
            },
            DecisionOption {
                id: "cancel".into(),
                label: "Cancel".into(),
            },
        ],
        tool_name: Some(tool.to_string()),
        summary,
        interrupts_turn: vec!["cancel".to_string()],
    }
}

fn coarse_outcome(status: &str) -> TurnOutcome {
    match status {
        "completed" => TurnOutcome::Completed,
        "interrupted" => TurnOutcome::Interrupted,
        _ => TurnOutcome::Failed,
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
        let has_method = frame.get("method").is_some();
        let id = frame.get("id").and_then(Value::as_u64);

        if let (true, Some(id)) = (has_method, id) {
            // A server→client request: approvals land here.
            let method = frame["method"].as_str().unwrap_or_default().to_string();
            let params = frame.get("params").cloned().unwrap_or(Value::Null);
            let Some(event) = server_request(&method, &params, id) else {
                continue;
            };
            if tx.send(event).await.is_err() {
                return;
            }
        } else if has_method {
            let method = frame["method"].as_str().unwrap_or_default().to_string();
            let params = frame.get("params").cloned().unwrap_or(Value::Null);
            if let Some(event) = notification(&method, &params, &shared) {
                if tx.send(event).await.is_err() {
                    return;
                }
            }
        } else if let Some(id) = id {
            // A response to one of our calls.
            if let Some(waiter) = shared.pending.lock().unwrap().remove(&id) {
                let _ = waiter.send(frame);
            }
        }
    }
    let _ = tx.send(DriverEvent::StdoutClosed).await;
}

/// Server→client requests. Approvals are answered later through `answer`.
fn server_request(method: &str, params: &Value, id: u64) -> Option<DriverEvent> {
    match method {
        "commandExecution/requestApproval" => Some(DriverEvent::ApprovalRequested {
            approval: ApprovalRequest {
                harness_ref: id.to_string(),
                spec: approval_spec(
                    "commandExecution",
                    params
                        .get("command")
                        .and_then(|c| c.as_str())
                        .map(String::from),
                ),
                request: params.clone(),
            },
        }),
        "applyPatch/requestApproval" => Some(DriverEvent::ApprovalRequested {
            approval: ApprovalRequest {
                harness_ref: id.to_string(),
                spec: approval_spec("applyPatch", None),
                request: params.clone(),
            },
        }),
        _ => None,
    }
}

/// Notifications → driver events. Also tracks the active turn for
/// interrupt/steer.
fn notification(method: &str, params: &Value, shared: &Shared) -> Option<DriverEvent> {
    match method {
        "turn/started" => {
            if let Some(turn) = params.pointer("/turn/id").and_then(Value::as_str) {
                *shared.active_turn.lock().unwrap() = Some(turn.to_string());
            }
            None
        }
        "turn/completed" => {
            *shared.active_turn.lock().unwrap() = None;
            let status = params
                .pointer("/turn/status")
                .and_then(Value::as_str)
                .unwrap_or("failed")
                .to_string();
            let error_message = params
                .pointer("/turn/error/message")
                .and_then(Value::as_str)
                .map(String::from);
            if let Some(message) = &error_message {
                eprintln!("codex turn error: {message}");
            }
            let is_error = params.pointer("/turn/error").is_some();
            Some(DriverEvent::TurnCompleted {
                subtype: status.clone(),
                coarse: coarse_outcome(&status),
                is_error,
                error_message,
            })
        }
        "item/agentMessage/delta" => {
            params
                .get("delta")
                .and_then(Value::as_str)
                .map(|text| DriverEvent::TextDelta {
                    text: text.to_string(),
                })
        }
        "item/commandExecution/outputDelta" => {
            params
                .get("delta")
                .and_then(Value::as_str)
                .map(|text| DriverEvent::TextDelta {
                    text: format!("\n{text}"),
                })
        }
        "item/started" => {
            let item = params.get("item")?;
            match item.get("type").and_then(Value::as_str)? {
                "commandExecution" => Some(DriverEvent::ToolStarted {
                    tool_use_id: item
                        .get("id")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string(),
                    name: "commandExecution".into(),
                    input: item.clone(),
                }),
                _ => None,
            }
        }
        "item/completed" => {
            let item = params.get("item")?;
            match item.get("type").and_then(Value::as_str)? {
                "agentMessage" => item.get("text").and_then(Value::as_str).map(|text| {
                    DriverEvent::AssistantText {
                        text: text.to_string(),
                    }
                }),
                "commandExecution" => {
                    let exit = item.get("exitCode").and_then(Value::as_i64).unwrap_or(0);
                    Some(DriverEvent::ToolResult {
                        tool_use_id: item
                            .get("id")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_string(),
                        name: "commandExecution".into(),
                        text: item
                            .get("aggregatedOutput")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_string(),
                        is_error: exit != 0,
                    })
                }
                _ => None,
            }
        }
        _ => None,
    }
}

/// The model catalog for `/harnesses/codex/models`: a short-lived
/// app-server, `initialize` + `model/list` (paginated), then kill. Every
/// model arrives with Codex's own reasoning-effort words.
pub async fn models(
    resolution: &Resolution,
) -> Result<Vec<openremote_harness::ModelDescriptor>, DriverError> {
    let opts = SpawnOptions {
        cwd: std::env::temp_dir(),
        ..Default::default()
    };
    let (mut driver, mut rx) = Driver::spawn(resolution, opts).await?;
    // Drain the handshake's channel side effects while paging models.
    let mut models = Vec::new();
    let mut cursor = Value::Null;
    loop {
        let page = driver
            .call(
                "model/list",
                json!({"includeHidden": true, "cursor": cursor}),
            )
            .await?;
        let Some(entries) = page.get("data").and_then(Value::as_array) else {
            return Err(DriverError::Protocol(
                "model/list returned no catalog".into(),
            ));
        };
        for entry in entries {
            let Some(model) = entry.get("model").and_then(Value::as_str) else {
                continue;
            };
            let efforts = entry
                .get("supportedReasoningEfforts")
                .and_then(Value::as_array)
                .map(|levels| {
                    levels
                        .iter()
                        .filter_map(|l| l.get("reasoningEffort").and_then(Value::as_str))
                        .map(String::from)
                        .collect()
                })
                .unwrap_or_default();
            models.push(openremote_harness::ModelDescriptor {
                model: model.to_string(),
                display_name: None,
                reasoning_efforts: efforts,
            });
        }
        let next = page.get("nextCursor").cloned().unwrap_or(Value::Null);
        if next.is_null() {
            break;
        }
        cursor = next;
        // keep the reader from stalling on dropped events
        while rx.try_recv().is_ok() {}
    }
    let _ = driver.shutdown().await;
    Ok(models)
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
