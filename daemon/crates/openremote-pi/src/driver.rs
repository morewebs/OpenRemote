//! The Pi session driver: one persistent `pi --mode rpc` process.
//!
//! Wire (flat JSON lines): our calls `{id, type, …params}` are answered
//! by `{type: "response", id, command, success, data|error}`;
//! notifications carry the stream (`message_update`,
//! `tool_execution_start|update|end`, `message_end`, `agent_settled`)
//! and the dialogs (`extension_ui_request` - answered with
//! `extension_ui_response` carrying `{value}` or `{cancelled}`). The
//! turn boundary is `message_end` with the assistant message's
//! `stopReason` - pi's own words (`stop`/`toolUse` → completed,
//! `aborted` → interrupted, `error`/`length` → failed).

use std::collections::HashMap;
use std::future::Future;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::{Arc, Mutex as StdMutex};
use std::time::Duration;

use openremote_core::{DecisionKind, DecisionOption, TurnOutcome};
use openremote_harness::events::{ApprovalRequest, DecisionSpec};
use openremote_harness::{
    DriverError, DriverEvent, Prompt, Resolution, SessionSettings, SpawnOptions, kill_tree,
};
use serde_json::{Value, json};
use tokio::io::{AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::{Mutex as AsyncMutex, mpsc, oneshot};

const CALL_TIMEOUT: Duration = Duration::from_secs(60);

/// Host calls in flight: our call id (a string, pi's own shape) →
/// response waiter. A call that gives up (failed write, timeout) removes
/// its own entry, or the map pins dead senders for the session's life
/// (harness audit F15).
struct PendingCalls {
    map: StdMutex<HashMap<String, oneshot::Sender<Value>>>,
}

impl PendingCalls {
    fn new() -> Self {
        Self {
            map: StdMutex::new(HashMap::new()),
        }
    }

    fn register(&self, id: String, waiter: oneshot::Sender<Value>) {
        self.map.lock().unwrap().insert(id, waiter);
    }

    /// Give up on a call: no response can arrive for a frame that never
    /// left or a waiter that timed out, so the entry must not linger
    /// (harness audit F15).
    fn abandon(&self, id: &str) {
        self.map.lock().unwrap().remove(id);
    }

    /// Hand a `response` frame to the waiter registered for its id, if
    /// any.
    fn deliver(&self, id: &str, frame: Value) -> bool {
        match self.map.lock().unwrap().remove(id) {
            Some(waiter) => waiter.send(frame).is_ok(),
            None => false,
        }
    }

    /// Wait one call out. The timeout abandons the call's entry before
    /// returning the error, never after (harness audit F15).
    async fn await_response(
        &self,
        id: &str,
        rx: oneshot::Receiver<Value>,
        method: &str,
        timeout: Duration,
    ) -> Result<Value, DriverError> {
        match tokio::time::timeout(timeout, rx).await {
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
            Err(_) => {
                self.abandon(id);
                Err(DriverError::Protocol(format!("{method} timed out")))
            }
        }
    }

    #[cfg(test)]
    fn is_empty(&self) -> bool {
        self.map.lock().unwrap().is_empty()
    }
}

struct Shared {
    child: Arc<AsyncMutex<Child>>,
    /// Host calls in flight, keyed by pi's own string call id.
    pending: PendingCalls,
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
        let args = build_args(&opts)?;
        let mut command = Command::new(program);
        command
            .args(&prefix)
            .args(&args)
            .current_dir(&opts.cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        // Windows: no console flash, no Ctrl-C from a shared console; the
        // why lives on openremote_harness::creation_flags.
        #[cfg(windows)]
        command.creation_flags(openremote_harness::creation_flags());

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

        let (tx, mut rx) = mpsc::channel(256);
        let shared = Arc::new(Shared {
            child: Arc::new(AsyncMutex::new(child)),
            pending: PendingCalls::new(),
        });
        tokio::spawn(read_stdout(
            openremote_harness::LineReader::new(BufReader::new(stdout)),
            tx.clone(),
            Arc::clone(&shared),
        ));
        tokio::spawn(read_stderr(
            openremote_harness::LineReader::new(BufReader::new(stderr)),
            tx.clone(),
        ));

        let mut driver = Self {
            stdin,
            shared: Arc::clone(&shared),
            next_id: 1,
        };

        // get_state names the conversation (cloudroom lines 126-151),
        // raced against the death of the CLI process: a CLI that
        // crashes at boot holds its exit status in the first second, and
        // failing there beats burning the 60s call timeout on an answer
        // that can never come (harness audit F13).
        let boot = async {
            let state = race_child_death(&shared, "get_state", driver.call("get_state", json!({})))
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
            // The chat's effort rides every start: pi takes it over rpc,
            // its own thinking level.
            if let Some(effort) = &opts.effort {
                race_child_death(
                    &shared,
                    "set_thinking_level",
                    driver.call("set_thinking_level", json!({"level": effort})),
                )
                .await
                .map_err(|e| DriverError::Protocol(format!("pi set_thinking_level failed: {e}")))?;
            }
            Ok(())
        };
        if let Err(e) = boot.await {
            // The CLI's dying words ride stderr; splice the tail into
            // the failure so a broken install explains itself (harness
            // audit F13), and a child that survived a rejected handshake
            // must not outlive the session that never was.
            let tail = stderr_tail(&mut rx).await;
            discard_child(&shared).await;
            let message = match e {
                DriverError::Protocol(message) => message,
                other => other.to_string(),
            };
            return Err(if tail.is_empty() {
                DriverError::Protocol(message)
            } else {
                DriverError::Protocol(format!("{message}; {tail}"))
            });
        }
        Ok((driver, rx))
    }

    /// Fire one prompt; pi's turn boundary arrives as `message_end`.
    pub async fn send_prompt(&mut self, prompt: &Prompt) -> Result<(), DriverError> {
        let text = prompt.text_only("Pi Agent")?;
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
    /// or a confirm boolean) - routed with the dialog's id.
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
            // - ours only where the console can't answer yet).
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
        self.shared.pending.register(id.clone(), tx);
        let mut frame = json!({"id": id, "type": method});
        if let Some(map) = params.as_object() {
            for (key, value) in map {
                frame[key] = value.clone();
            }
        }
        if let Err(e) = self.write(frame).await {
            // The write failed: no response can ever arrive for this id,
            // so the waiter must not linger in the map (harness audit F15).
            self.shared.pending.abandon(&id);
            return Err(e);
        }
        self.shared
            .pending
            .await_response(&id, rx, method, CALL_TIMEOUT)
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
    kill_tree(&mut child).await;
    match tokio::time::timeout(Duration::from_secs(5), child.wait()).await {
        Ok(Ok(status)) => Ok(status.code()),
        Ok(Err(e)) => Err(e.into()),
        Err(_) => {
            kill_tree(&mut child).await;
            let status = child.wait().await?;
            Ok(status.code())
        }
    }
}

/// Race one handshake call against the death of the CLI process (harness
/// audit F13). Holding the child lock for the race is safe here: nothing
/// else takes it until spawn returns. When the call wins, the wait is
/// simply dropped; when the process dies first, the exit status it
/// already holds becomes the failure, in the first second instead of
/// after the call timeout.
async fn race_child_death<T>(
    shared: &Arc<Shared>,
    step: &str,
    call: impl Future<Output = Result<T, DriverError>>,
) -> Result<T, DriverError> {
    let mut child = shared.child.lock().await;
    tokio::select! {
        result = call => result,
        waited = child.wait() => Err(DriverError::Protocol(match waited {
            Ok(status) => format!("pi exited during {step} ({status})"),
            Err(e) => format!("pi exited during {step} (exit unreadable: {e})"),
        })),
    }
}

/// The CLI's dying words: the last stderr lines queued around the
/// process's exit. Best effort with a short grace, since the stderr
/// reader may still be draining when the failure is decided.
async fn stderr_tail(rx: &mut mpsc::Receiver<DriverEvent>) -> String {
    let deadline = tokio::time::Instant::now() + Duration::from_millis(500);
    let mut lines: Vec<String> = Vec::new();
    while tokio::time::Instant::now() < deadline {
        match rx.try_recv() {
            Ok(DriverEvent::Stderr { line }) => {
                lines.push(line);
                if lines.len() > 4 {
                    lines.remove(0);
                }
            }
            Ok(_) => {}
            Err(_) => tokio::time::sleep(Duration::from_millis(10)).await,
        }
    }
    lines.join("; ")
}

/// A child that outlived a failed handshake must not outlive the session
/// that never was: kill and reap, best effort, leaving the original
/// failure in place.
async fn discard_child(shared: &Arc<Shared>) {
    let mut child = shared.child.lock().await;
    kill_tree(&mut child).await;
    let _ = tokio::time::timeout(Duration::from_secs(5), child.wait()).await;
}

/// The argv after the executable, in pi's rpc shape.
///
/// Every word a chat chooses arrives as the value of a flag, so a value that is
/// empty or dash-leading would change what the command means instead of naming
/// one.
fn build_args(opts: &SpawnOptions) -> Result<Vec<std::ffi::OsString>, DriverError> {
    let mut args: Vec<std::ffi::OsString> = vec![
        "--mode".into(),
        "rpc".into(),
        "--session-dir".into(),
        ".pi-sessions".into(),
    ];
    if let Some(model) = &opts.model {
        push_word(&mut args, "--model", model)?;
    }
    // Resume pi's own conversation. pi accepts `path|id` here, and only an id
    // may pass: a path-shaped ref would make the CLI open a transcript that is
    // not this chat's.
    if let Some(resume) = &opts.resume {
        if !is_session_ref(resume) {
            return Err(DriverError::Spawn(format!(
                "--session {resume:?}: only one of pi's own session ids can resume a chat"
            )));
        }
        args.push("--session".into());
        args.push(resume.into());
    }
    Ok(args)
}

/// Whether a resume ref is shaped like an id rather than a path.
fn is_session_ref(value: &str) -> bool {
    !value.is_empty()
        && !value.starts_with('-')
        && !value.starts_with('.')
        && !value.chars().any(|c| matches!(c, '/' | '\\' | ':' | '\0'))
}

/// One flag and its value, refused when the value could be read as grammar
/// instead of as a name.
fn push_word(
    args: &mut Vec<std::ffi::OsString>,
    flag: &str,
    value: &str,
) -> Result<(), DriverError> {
    openremote_core::validate_argv_word(value)
        .map_err(|why| DriverError::Spawn(format!("{flag} {value:?}: {why}")))?;
    args.push(flag.into());
    args.push(value.into());
    Ok(())
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
    mut reader: openremote_harness::LineReader<BufReader<tokio::process::ChildStdout>>,
    tx: mpsc::Sender<DriverEvent>,
    shared: Arc<Shared>,
) {
    loop {
        // Bounded, lossy reads (harness audit F11): an over-long line is
        // dropped and invalid UTF-8 decodes lossily, never a fatal break.
        let line = match reader.next_line().await {
            Ok(None) => break,
            Ok(Some(line)) => line,
            Err(e) => {
                let _ = tx
                    .send(DriverEvent::Stderr {
                        line: format!("stdout read error: {e}"),
                    })
                    .await;
                break;
            }
        };
        if line.trim().is_empty() {
            continue;
        }
        let Ok(frame) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        let frame_type = frame
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let event = match frame_type {
            "response" => {
                if let Some(id) = frame.get("id").and_then(Value::as_str) {
                    shared.pending.deliver(id, frame.clone());
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
                images: Vec::new(),
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
    mut reader: openremote_harness::LineReader<BufReader<tokio::process::ChildStderr>>,
    tx: mpsc::Sender<DriverEvent>,
) {
    loop {
        let line = match reader.next_line().await {
            Ok(None) | Err(_) => break,
            Ok(Some(line)) => line,
        };
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

    #[tokio::test]
    async fn a_timed_out_call_leaves_no_waiter_behind() {
        // F15: the entry must go when the timeout fires, not when (if
        // ever) a response arrives for it.
        let pending = PendingCalls::new();
        let (tx, rx) = oneshot::channel();
        pending.register("1".to_string(), tx);
        let result = pending
            .await_response("1", rx, "get_state", Duration::from_millis(50))
            .await;
        assert!(result.is_err());
        assert!(pending.is_empty(), "the timed-out call leaked its waiter");
    }

    #[test]
    fn an_abandoned_call_cannot_be_answered_late() {
        // F15's write-failure path: the abandoned id drops any late
        // response instead of waking a waiter that no longer exists.
        let pending = PendingCalls::new();
        let (tx, rx) = oneshot::channel();
        pending.register("1".to_string(), tx);
        pending.abandon("1");
        assert!(pending.is_empty());
        assert!(!pending.deliver(
            "1",
            json!({"type": "response", "id": "1", "command": "get_state", "success": true}),
        ));
        drop(rx);
    }

    #[test]
    fn a_response_routes_to_its_own_waiter() {
        let pending = PendingCalls::new();
        let (tx, rx) = oneshot::channel();
        pending.register("1".to_string(), tx);
        assert!(pending.deliver(
            "1",
            json!({"type": "response", "id": "1", "command": "get_state",
                   "success": true, "data": {"sessionId": "s-1"}}),
        ));
        let response = rx.blocking_recv().expect("the waiter got its response");
        let expected = json!("s-1");
        assert_eq!(response.pointer("/data/sessionId"), Some(&expected));
    }

    #[test]
    fn a_dash_leading_word_is_refused_before_it_reaches_argv() {
        let opts = SpawnOptions {
            cwd: std::path::PathBuf::from("/w"),
            model: Some("--dangerously-skip-permissions".into()),
            ..Default::default()
        };
        assert!(
            build_args(&opts).is_err(),
            "a dash-leading model is refused, never passed"
        );

        let argv = build_args(&SpawnOptions {
            cwd: std::path::PathBuf::from("/w"),
            model: Some("pi-model-1".into()),
            ..Default::default()
        })
        .expect("argv builds");
        assert!(!argv.iter().any(|word| {
            word.to_string_lossy()
                .contains("--dangerously-skip-permissions")
        }));
    }

    #[test]
    fn a_path_shaped_resume_ref_is_refused() {
        // pi reads `--session path|id`, so a path-shaped ref would open a
        // transcript that is not this chat's.
        for bad in [
            "../other-chat.jsonl",
            "C:\\Users\\me\\.pi-sessions\\x.jsonl",
            "/home/me/.pi-sessions/x",
            ".pi-sessions/x",
            "--dangerously-skip-permissions",
            "",
        ] {
            let opts = SpawnOptions {
                cwd: std::path::PathBuf::from("/w"),
                resume: Some(bad.to_string()),
                ..Default::default()
            };
            assert!(build_args(&opts).is_err(), "{bad:?} must not pass");
        }
        let opts = SpawnOptions {
            cwd: std::path::PathBuf::from("/w"),
            resume: Some("01J8Z6Q4X9R7M2K3".into()),
            ..Default::default()
        };
        let argv = build_args(&opts).expect("argv builds");
        let at = argv
            .iter()
            .position(|word| *word == std::ffi::OsStr::new("--session"))
            .expect("--session rides argv for a real id");
        assert_eq!(argv[at + 1].to_string_lossy(), "01J8Z6Q4X9R7M2K3");
    }
}
