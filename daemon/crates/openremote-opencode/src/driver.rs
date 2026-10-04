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

use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use openremote_core::{DecisionKind, DecisionOption, TurnOutcome};
use openremote_harness::events::{ApprovalRequest, DecisionSpec};
use openremote_harness::{DriverError, DriverEvent, Resolution, SpawnOptions};
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::{Mutex as AsyncMutex, mpsc};
use uuid::Uuid;

const START_TIMEOUT: Duration = Duration::from_secs(30);

pub struct Driver {
    child: Arc<AsyncMutex<Child>>,
    client: crate::http::HttpClient,
    session_id: String,
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
                // would need the script as argv — not seen in the wild.
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

    pub async fn send_prompt(&mut self, text: &str) -> Result<(), DriverError> {
        self.client
            .post(
                &format!("/api/session/{}/prompt", self.session_id),
                json!({"prompt": {"text": text}}),
            )
            .await?;
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
            _ => {
                self.client
                    .post(
                        &format!(
                            "/api/session/{}/permission/{}/reply",
                            self.session_id, harness_ref
                        ),
                        json!({"reply": choice}),
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

async fn spawn_serve(
    mut command: Command,
    opts: SpawnOptions,
) -> Result<(Driver, mpsc::Receiver<DriverEvent>), DriverError> {
    let password = Uuid::new_v4().to_string();
    command
        .args(["serve", "--port", "0", "--hostname", "127.0.0.1"])
        .env("OPENCODE_SERVER_PASSWORD", &password)
        .current_dir(&opts.cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        // A dropped driver never orphans a serve child — the daemon's
        // crash takes its harnesses with it.
        .kill_on_drop(true);
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

    // The listening line names the port: "opencode server listening on
    // http://127.0.0.1:<port>".
    let mut lines = BufReader::new(stdout);
    let mut buffer = String::new();
    let port = loop {
        buffer.clear();
        match tokio::time::timeout(START_TIMEOUT, lines.read_line(&mut buffer)).await {
            Ok(Ok(0)) => {
                return Err(DriverError::Protocol(
                    "opencode exited before listening".into(),
                ));
            }
            Ok(Ok(_)) => {
                let line = buffer.trim_end_matches(['\r', '\n']);
                if let Some(url) = line.strip_prefix("opencode server listening on ") {
                    let port: u16 = url
                        .rsplit(':')
                        .next()
                        .and_then(|p| p.parse().ok())
                        .ok_or_else(|| {
                            DriverError::Protocol(format!("bad listening line: {line}"))
                        })?;
                    break port;
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
    // installed opencode's own 400s — `modelID` is its old shape).
    let mut body = json!({});
    if let Some(model) = &opts.model {
        let provider = model.split('/').next().unwrap_or("");
        body["model"] = if model.contains('/') {
            json!({"id": model.split('/').nth(1).unwrap_or(model), "providerID": provider})
        } else {
            json!({"id": model, "providerID": "opencode"})
        };
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

    let driver = Driver {
        child,
        client,
        session_id: session_id.clone(),
    };
    // The bus watcher: every event for our session rides the driver
    // channel; a dropped subscription ends the session's stream.
    let watch_client = crate::http::HttpClient::new(port, &password);
    let event_tx = tx.clone();
    let end_tx = tx.clone();
    tokio::spawn(async move {
        let result = watch_client
            .sse("/api/event", move |event| {
                let Some(event) = map_event(&event, &session_id) else {
                    return;
                };
                let tx = event_tx.clone();
                tokio::spawn(async move {
                    let _ = tx.send(event).await;
                });
            })
            .await;
        if result.is_err() {
            let _ = end_tx.send(DriverEvent::StdoutClosed).await;
        }
    });
    Ok((driver, rx))
}

/// Map one bus event for our session onto a driver event.
fn map_event(event: &Value, session_id: &str) -> Option<DriverEvent> {
    // The real serve speaks `data`; the fixture (and older serves) spoke
    // `properties` — accept either (observed live against both).
    let properties = event
        .get("data")
        .or_else(|| event.get("properties"))?;
    if properties.get("sessionID").and_then(Value::as_str) != Some(session_id) {
        return None;
    }
    let kind = event.get("type").and_then(Value::as_str)?;
    match kind {
        // OpenCode only names its model when a turn runs — the step-start
        // carries it (`model: {id, providerID}`, observed live). The
        // first one lights the chat's model chip before the reply lands.
        "session.next.step.started" => properties
            .pointer("/model/id")
            .and_then(Value::as_str)
            .map(|model| DriverEvent::ModelReported {
                model: model.to_string(),
            }),
        "session.next.text.delta" => {
            properties
                .get("delta")
                .and_then(Value::as_str)
                .map(|text| DriverEvent::TextDelta {
                    text: text.to_string(),
                })
        }
        "session.next.text.ended" => {
            properties
                .get("text")
                .and_then(Value::as_str)
                .map(|text| DriverEvent::AssistantText {
                    text: text.to_string(),
                })
        }
        "session.next.tool.called" => Some(DriverEvent::ToolStarted {
            tool_use_id: properties
                .get("callID")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            name: properties
                .get("tool")
                .and_then(Value::as_str)
                .unwrap_or("tool")
                .to_string(),
            input: properties.get("input").cloned().unwrap_or(Value::Null),
        }),
        "session.next.tool.success" | "session.next.tool.failed" => {
            let failed = kind == "session.next.tool.failed";
            Some(DriverEvent::ToolResult {
                tool_use_id: properties
                    .get("callID")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
                name: properties
                    .get("tool")
                    .and_then(Value::as_str)
                    .unwrap_or("tool")
                    .to_string(),
                text: properties
                    .get("result")
                    .map(|r| r.to_string())
                    .unwrap_or_default(),
                is_error: failed,
            })
        }
        // A failed step ends the turn on its own — no `step.ended`
        // follows (observed live) — with OpenCode's own error words.
        "session.next.step.failed" => {
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
        "permission.v2.asked" => {
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
