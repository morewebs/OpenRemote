//! The Pi session driver: one persistent `pi --mode rpc` process.
//!
//! Wire (flat JSON lines): our calls `{id, type, …params}` are answered
//! by `{type: "response", id, command, success, data|error}`;
//! notifications carry the stream (`message_update`,
//! `tool_execution_start|update|end`, `message_end`, `agent_settled`)
//! and the dialogs (`extension_ui_request` - answered with
//! `extension_ui_response` carrying `{value}` or `{cancelled}`). The
//! turn boundary is `agent_settled`, Pi's word for "no more work without
//! you": a run holds one assistant message per model call, so a
//! `message_end` is never the end on its own. The last assistant
//! message's `stopReason` gives the outcome in pi's own words
//! (`stop`/`toolUse` → completed, `aborted` → interrupted,
//! `error`/`length` → failed).

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
/// its own entry, or the map pins dead senders for the session's life.
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
    /// left or a waiter that timed out, so the entry must not linger.
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
    /// returning the error, never after.
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
        let args = build_args(&opts, &session_dir(&opts)?)?;
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
        // that can never come.
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

    /// Fire one prompt; pi's turn boundary arrives as `agent_settled`.
    ///
    /// Every prompt says `streamingBehavior: "steer"`. Pi reads that field
    /// only while a run is going, where it queues the message into the run
    /// (delivered after the current tool calls, before the next model
    /// call); an idle Pi starts a run as usual. Without it, a message
    /// typed during a run is refused by Pi and never reaches the agent.
    ///
    /// Pictures ride as Pi's own `ImageContent` blocks.
    pub async fn send_prompt(&mut self, prompt: &Prompt) -> Result<(), DriverError> {
        self.write(prompt_frame(prompt)).await
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

    /// Kill the harness (its whole process tree on Windows), wait up to 5s
    /// for it to be reaped, and kill again if it was not. No polite signal
    /// goes first: nothing here sends one.
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
            // so the waiter must not linger in the map.
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

/// Where chats used to keep their transcripts: a folder inside the workspace,
/// relative to the chat's cwd. A chat that already has its transcript there
/// keeps resuming from it; new chats never write there.
const WORKSPACE_SESSIONS: &str = ".pi-sessions";

/// The folder pi keeps this chat's transcript in, as `--session-dir`.
///
/// New chats get one folder per workspace inside OpenRemote's data, so the
/// user's repository never gains an untracked folder. One per workspace, not
/// one for everything, because pi looks a resume id up among the sessions of
/// the chat's own folder first and, finding it under another folder, asks on
/// stdin whether to fork it, which in rpc mode reads the next command
/// instead. A chat whose transcript is already in the workspace folder is
/// resumed from there, untouched. With no data folder (`state_dir` None) the
/// workspace folder is still used.
fn session_dir(opts: &SpawnOptions) -> Result<std::path::PathBuf, DriverError> {
    let legacy = std::path::PathBuf::from(WORKSPACE_SESSIONS);
    let Some(state_dir) = &opts.state_dir else {
        return Ok(legacy);
    };
    if let Some(resume) = &opts.resume {
        if holds_session(&opts.cwd.join(WORKSPACE_SESSIONS), resume) {
            return Ok(legacy);
        }
    }
    let dir = state_dir.join("sessions").join(workspace_key(&opts.cwd));
    openremote_core::fsx::create_private_dir(&dir).map_err(|e| {
        DriverError::Spawn(format!(
            "pi's transcript folder could not be made at {}: {e}",
            dir.display()
        ))
    })?;
    Ok(dir)
}

/// Whether `dir` holds pi's transcript for session `id`. Pi itself matches
/// by the id in each file's first line (its session header), so this does
/// too, reading no more of a file than that line.
fn holds_session(dir: &std::path::Path, id: &str) -> bool {
    use std::io::{BufRead, Read};
    let Ok(entries) = std::fs::read_dir(dir) else {
        return false;
    };
    entries.flatten().any(|entry| {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
            return false;
        }
        let Ok(file) = std::fs::File::open(&path) else {
            return false;
        };
        let mut first = String::new();
        if std::io::BufReader::new(file.take(4096))
            .read_line(&mut first)
            .is_err()
        {
            return false;
        }
        serde_json::from_str::<Value>(&first)
            .ok()
            .is_some_and(|header| {
                header.get("type") == Some(&json!("session"))
                    && header.get("id").and_then(Value::as_str) == Some(id)
            })
    })
}

/// A folder name for a workspace: its own last name, kept readable, and a
/// hash of the whole path, so two folders with the same name stay apart.
/// The hash is FNV-1a, written out here because the name must not change
/// between builds of the daemon (std's hasher promises no such thing), or a
/// chat would stop finding its transcript after an update. Two paths that
/// still met in one folder would be told apart by pi, which reads the
/// working folder recorded in each transcript.
fn workspace_key(cwd: &std::path::Path) -> String {
    let path = cwd.to_string_lossy();
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in path.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    let name: String = cwd
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') {
                c
            } else {
                '-'
            }
        })
        .take(40)
        .collect();
    let name = name.trim_matches(|c| c == '.' || c == '-');
    if name.is_empty() {
        format!("{hash:016x}")
    } else {
        format!("{name}-{hash:016x}")
    }
}

/// The argv after the executable, in pi's rpc shape.
///
/// Every word a chat chooses arrives as the value of a flag, so a value that is
/// empty or dash-leading would change what the command means instead of naming
/// one. The session folder is OpenRemote's own path, never a chat's word.
fn build_args(
    opts: &SpawnOptions,
    session_dir: &std::path::Path,
) -> Result<Vec<std::ffi::OsString>, DriverError> {
    let mut args: Vec<std::ffi::OsString> = vec![
        "--mode".into(),
        "rpc".into(),
        "--session-dir".into(),
        session_dir.as_os_str().to_owned(),
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
    let mut run = Run::default();
    loop {
        // Bounded, lossy reads: an over-long line is
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
        if frame.get("type") == Some(&json!("response")) {
            if let Some(id) = frame.get("id").and_then(Value::as_str) {
                shared.pending.deliver(id, frame.clone());
            }
        }
        for event in run.on_frame(&frame) {
            if tx.send(event).await.is_err() {
                return;
            }
        }
    }
    let _ = tx.send(DriverEvent::StdoutClosed).await;
}

/// What one run has said so far, kept until Pi settles. Pi's run can hold
/// several assistant messages (one per model call, with tool calls between
/// them), so no single message is the end of the turn: `agent_settled` is,
/// Pi's own word for "nothing more will happen without you".
#[derive(Default)]
struct Run {
    /// Between `agent_start` and `agent_settled`.
    running: bool,
    /// The last assistant message's stop reason and error, in Pi's words.
    stop_reason: Option<String>,
    error_message: Option<String>,
    /// The run's spend, summed over its assistant messages.
    cost_usd: f64,
}

impl Run {
    /// The events one stdout frame means. Responses to our own calls are
    /// routed to their waiters by the reader; this sees them too, for the
    /// prompt's, which can decide a turn.
    fn on_frame(&mut self, frame: &Value) -> Vec<DriverEvent> {
        let frame_type = frame
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or_default();
        match frame_type {
            "response" if frame.get("command") == Some(&json!("prompt")) => {
                self.on_prompt_response(frame)
            }
            "agent_start" => {
                self.running = true;
                Vec::new()
            }
            "message_update" => {
                let delta = frame
                    .pointer("/assistantMessageEvent/delta")
                    .and_then(Value::as_str)
                    .map(String::from);
                match (
                    frame
                        .pointer("/assistantMessageEvent/type")
                        .and_then(Value::as_str),
                    delta,
                ) {
                    (Some("text_delta"), Some(text)) => vec![DriverEvent::TextDelta { text }],
                    (Some("thinking_delta"), Some(text)) => {
                        vec![DriverEvent::ReasoningDelta { text }]
                    }
                    _ => Vec::new(),
                }
            }
            "message_end" if frame.pointer("/message/role") == Some(&json!("assistant")) => {
                self.on_assistant_end(frame)
            }
            "agent_settled" => self.settle(frame.get("aborted") == Some(&json!(true))),
            "compaction_end" if frame.get("result").is_some_and(|r| !r.is_null()) => {
                vec![DriverEvent::Compacted]
            }
            "tool_execution_start" => vec![DriverEvent::ToolStarted {
                tool_use_id: tool_call_id(frame),
                name: tool_name(frame),
                // Pi's `args` are the call's input; a frame without them is
                // passed whole, as it always was.
                input: frame.get("args").cloned().unwrap_or_else(|| frame.clone()),
            }],
            "tool_execution_end" => {
                let result = frame.get("result");
                // Pi's result is a tool result body: `content` blocks of
                // text and pictures. A bare string is read as the text.
                let blocks = result
                    .and_then(|r| r.get("content"))
                    .and_then(Value::as_array)
                    .map(Vec::as_slice)
                    .unwrap_or_default();
                let text = match result.and_then(Value::as_str) {
                    Some(text) => text.to_string(),
                    None => openremote_harness::anthropic_wire::content_text(blocks),
                };
                vec![DriverEvent::ToolResult {
                    tool_use_id: tool_call_id(frame),
                    name: tool_name(frame),
                    text,
                    is_error: frame.get("isError") == Some(&json!(true)),
                    images: openremote_harness::anthropic_wire::content_images(blocks),
                }]
            }
            "extension_ui_request" => {
                let id = frame
                    .get("id")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string();
                vec![DriverEvent::ApprovalRequested {
                    approval: ApprovalRequest {
                        harness_ref: id,
                        spec: dialog_spec(frame),
                        request: frame.clone(),
                    },
                }]
            }
            _ => Vec::new(), // turn and queue bookkeeping, future types
        }
    }

    /// A prompt that starts no run ends its turn here, since no
    /// `agent_settled` will follow: one an extension handled outright, and
    /// one Pi refused while idle (no model, a compaction in progress). A
    /// refusal during a run is a steer that did not land; the run's turn
    /// goes on, and the refusal is said as a note.
    fn on_prompt_response(&mut self, frame: &Value) -> Vec<DriverEvent> {
        if frame.get("success") == Some(&json!(true)) {
            if frame.pointer("/data/disposition") == Some(&json!("handled")) && !self.running {
                return vec![DriverEvent::TurnCompleted {
                    subtype: "handled".into(),
                    coarse: TurnOutcome::Completed,
                    is_error: false,
                    error_message: None,
                }];
            }
            return Vec::new();
        }
        let why = frame
            .get("error")
            .and_then(Value::as_str)
            .unwrap_or("no message")
            .to_string();
        let note = DriverEvent::Stderr {
            line: format!("pi rejected the prompt: {why}"),
        };
        if self.running {
            vec![note]
        } else {
            vec![
                note,
                DriverEvent::TurnCompleted {
                    subtype: "rejected".into(),
                    coarse: TurnOutcome::Failed,
                    is_error: true,
                    error_message: Some(format!("Pi did not take the message: {why}")),
                },
            ]
        }
    }

    /// One model call finished. Its usage is the context the model saw on
    /// that call (Pi's input buckets together), and its cost adds to the
    /// run's; the turn itself goes on until Pi settles.
    fn on_assistant_end(&mut self, frame: &Value) -> Vec<DriverEvent> {
        self.stop_reason = frame
            .pointer("/message/stopReason")
            .and_then(Value::as_str)
            .map(String::from);
        self.error_message = frame
            .pointer("/message/errorMessage")
            .and_then(Value::as_str)
            .map(String::from);
        let usage = frame.pointer("/message/usage");
        if let Some(cost) = usage
            .and_then(|u| u.pointer("/cost/total"))
            .and_then(Value::as_f64)
        {
            self.cost_usd += cost;
        }
        let field = |name: &str| {
            usage
                .and_then(|u| u.get(name))
                .and_then(Value::as_u64)
                .unwrap_or(0)
        };
        let used = field("input") + field("cacheRead") + field("cacheWrite");
        if used > 0 {
            vec![DriverEvent::ContextUsed { used, window: None }]
        } else {
            Vec::new()
        }
    }

    /// Pi will not continue on its own: the turn ends with the last
    /// assistant message's outcome, and the run's spend when it had one.
    fn settle(&mut self, aborted: bool) -> Vec<DriverEvent> {
        let run = std::mem::take(self);
        let mut events = Vec::new();
        // An all-zero spend is a provider that reports none, not a free
        // turn; it is left unsaid.
        if run.cost_usd > 0.0 {
            events.push(DriverEvent::TurnCost {
                cost_usd: run.cost_usd,
            });
        }
        let stop_reason = if aborted {
            "aborted".to_string()
        } else {
            run.stop_reason.unwrap_or_else(|| "stop".to_string())
        };
        let coarse = coarse_outcome(&stop_reason);
        events.push(DriverEvent::TurnCompleted {
            is_error: stop_reason == "error" || stop_reason == "length",
            subtype: stop_reason,
            coarse,
            error_message: run.error_message,
        });
        events
    }
}

/// One message as pi's rpc `prompt`: the words, the steer behavior that
/// lets it land in a running turn, and the pictures as `ImageContent`.
fn prompt_frame(prompt: &Prompt) -> Value {
    let mut frame = json!({
        "type": "prompt",
        "message": prompt.text,
        "streamingBehavior": "steer",
    });
    if !prompt.images.is_empty() {
        frame["images"] = Value::Array(
            prompt
                .images
                .iter()
                .map(|image| json!({"type": "image", "data": image.data, "mimeType": image.mime}))
                .collect(),
        );
    }
    frame
}

fn tool_call_id(frame: &Value) -> String {
    frame
        .get("toolCallId")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

fn tool_name(frame: &Value) -> String {
    frame
        .get("toolName")
        .and_then(Value::as_str)
        .unwrap_or("tool")
        .to_string()
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

    /// A session folder for argv tests, which are about the words.
    fn dir() -> &'static std::path::Path {
        std::path::Path::new("/data/harness-state/pi/sessions/w-0")
    }

    #[test]
    fn a_dash_leading_word_is_refused_before_it_reaches_argv() {
        let opts = SpawnOptions {
            cwd: std::path::PathBuf::from("/w"),
            model: Some("--dangerously-skip-permissions".into()),
            ..Default::default()
        };
        assert!(
            build_args(&opts, dir()).is_err(),
            "a dash-leading model is refused, never passed"
        );

        let argv = build_args(
            &SpawnOptions {
                cwd: std::path::PathBuf::from("/w"),
                model: Some("pi-model-1".into()),
                ..Default::default()
            },
            dir(),
        )
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
            assert!(build_args(&opts, dir()).is_err(), "{bad:?} must not pass");
        }
        let opts = SpawnOptions {
            cwd: std::path::PathBuf::from("/w"),
            resume: Some("01J8Z6Q4X9R7M2K3".into()),
            ..Default::default()
        };
        let argv = build_args(&opts, dir()).expect("argv builds");
        let at = argv
            .iter()
            .position(|word| *word == std::ffi::OsStr::new("--session"))
            .expect("--session rides argv for a real id");
        assert_eq!(argv[at + 1].to_string_lossy(), "01J8Z6Q4X9R7M2K3");
    }

    /// The kinds a list of events names, in order, for compact asserts.
    fn kinds(events: &[DriverEvent]) -> Vec<&'static str> {
        events
            .iter()
            .map(|event| match event {
                DriverEvent::TextDelta { .. } => "text_delta",
                DriverEvent::ReasoningDelta { .. } => "reasoning_delta",
                DriverEvent::ToolStarted { .. } => "tool_started",
                DriverEvent::ToolResult { .. } => "tool_result",
                DriverEvent::ContextUsed { .. } => "context_used",
                DriverEvent::TurnCost { .. } => "turn_cost",
                DriverEvent::TurnCompleted { .. } => "turn_completed",
                DriverEvent::Compacted => "compacted",
                DriverEvent::Stderr { .. } => "stderr",
                _ => "other",
            })
            .collect()
    }

    fn assistant_end(stop_reason: &str, input: u64, cost: f64) -> Value {
        json!({
            "type": "message_end",
            "message": {
                "role": "assistant",
                "stopReason": stop_reason,
                "usage": {
                    "input": input, "output": 20, "cacheRead": 100, "cacheWrite": 0,
                    "totalTokens": input + 120,
                    "cost": {"input": 0.0, "output": 0.0, "cacheRead": 0.0,
                             "cacheWrite": 0.0, "total": cost}
                }
            }
        })
    }

    #[test]
    fn a_run_with_tools_ends_when_pi_settles_not_at_the_first_message() {
        // The documented shape of a run that uses a tool: one assistant
        // message asks for it (`toolUse`), the tool runs, a second message
        // answers. The first `message_end` is the middle of the turn.
        let mut run = Run::default();
        let mut events = Vec::new();
        for frame in [
            json!({"type": "agent_start"}),
            assistant_end("toolUse", 1000, 0.01),
            json!({"type": "tool_execution_start", "toolCallId": "call_1",
                   "toolName": "bash", "args": {"command": "ls"}}),
            json!({"type": "tool_execution_end", "toolCallId": "call_1",
                   "toolName": "bash", "isError": false,
                   "result": {"content": [{"type": "text", "text": "Cargo.toml"}],
                              "details": {}}}),
            assistant_end("stop", 1200, 0.02),
            json!({"type": "agent_end", "messages": [], "willRetry": false}),
            json!({"type": "agent_settled", "aborted": false}),
        ] {
            events.extend(run.on_frame(&frame));
        }
        assert_eq!(
            kinds(&events),
            [
                "context_used",
                "tool_started",
                "tool_result",
                "context_used",
                "turn_cost",
                "turn_completed"
            ]
        );
        let DriverEvent::ToolStarted { input, name, .. } = &events[1] else {
            unreachable!()
        };
        assert_eq!(name, "bash");
        assert_eq!(input, &json!({"command": "ls"}), "the call's own args");
        let DriverEvent::ToolResult {
            text,
            is_error,
            tool_use_id,
            ..
        } = &events[2]
        else {
            unreachable!()
        };
        assert_eq!(text, "Cargo.toml", "the result's content blocks are read");
        assert!(!is_error);
        assert_eq!(tool_use_id, "call_1");
        let DriverEvent::ContextUsed { used, .. } = &events[3] else {
            unreachable!()
        };
        assert_eq!(*used, 1300, "input plus cache, as the model saw it");
        let DriverEvent::TurnCost { cost_usd } = &events[4] else {
            unreachable!()
        };
        assert!(
            (cost_usd - 0.03).abs() < 1e-9,
            "the run's calls summed: {cost_usd}"
        );
        let DriverEvent::TurnCompleted {
            coarse, subtype, ..
        } = &events[5]
        else {
            unreachable!()
        };
        assert_eq!(*coarse, TurnOutcome::Completed);
        assert_eq!(subtype, "stop");
    }

    #[test]
    fn a_failed_tool_says_so() {
        let mut run = Run::default();
        let events = run.on_frame(&json!({
            "type": "tool_execution_end", "toolCallId": "c", "toolName": "read",
            "isError": true,
            "result": {"content": [{"type": "text", "text": "no such file"}]}
        }));
        let [DriverEvent::ToolResult { is_error, text, .. }] = events.as_slice() else {
            panic!("one tool result: {events:?}")
        };
        assert!(*is_error);
        assert_eq!(text, "no such file");
    }

    #[test]
    fn an_aborted_run_is_interrupted_and_an_errored_one_failed() {
        let mut run = Run::default();
        run.on_frame(&json!({"type": "agent_start"}));
        run.on_frame(&assistant_end("toolUse", 10, 0.0));
        let events = run.on_frame(&json!({"type": "agent_settled", "aborted": true}));
        let [DriverEvent::TurnCompleted { coarse, .. }] = events.as_slice() else {
            panic!("a free run states no cost: {events:?}")
        };
        assert_eq!(*coarse, TurnOutcome::Interrupted);

        // Settling resets the run: the next one's outcome is its own.
        run.on_frame(&json!({"type": "agent_start"}));
        run.on_frame(&json!({"type": "message_end", "message": {
            "role": "assistant", "stopReason": "error", "errorMessage": "529 overloaded"}}));
        let events = run.on_frame(&json!({"type": "agent_settled", "aborted": false}));
        let [
            DriverEvent::TurnCompleted {
                coarse,
                is_error,
                error_message,
                ..
            },
        ] = events.as_slice()
        else {
            panic!("{events:?}")
        };
        assert_eq!(*coarse, TurnOutcome::Failed);
        assert!(*is_error);
        assert_eq!(error_message.as_deref(), Some("529 overloaded"));
    }

    #[test]
    fn a_prompt_that_starts_no_run_ends_its_turn_and_a_refused_steer_does_not() {
        // Refused while idle: no run will settle, so the turn ends here.
        let mut run = Run::default();
        let refused = json!({"type": "response", "command": "prompt", "success": false,
                             "error": "No model selected"});
        let events = run.on_frame(&refused);
        assert_eq!(kinds(&events), ["stderr", "turn_completed"]);
        let DriverEvent::TurnCompleted { coarse, .. } = &events[1] else {
            unreachable!()
        };
        assert_eq!(*coarse, TurnOutcome::Failed);

        // Handled by an extension while idle: done, nothing failed.
        let handled = json!({"type": "response", "command": "prompt", "success": true,
                             "data": {"disposition": "handled"}});
        let events = run.on_frame(&handled);
        let [DriverEvent::TurnCompleted { coarse, .. }] = events.as_slice() else {
            panic!("{events:?}")
        };
        assert_eq!(*coarse, TurnOutcome::Completed);

        // During a run, a refusal is a steer that did not land: a note, and
        // the run's own turn goes on.
        run.on_frame(&json!({"type": "agent_start"}));
        assert_eq!(kinds(&run.on_frame(&refused)), ["stderr"]);
        assert!(run.on_frame(&handled).is_empty());
        let started = json!({"type": "response", "command": "prompt", "success": true,
                             "data": {"disposition": "queued"}});
        assert!(run.on_frame(&started).is_empty());
    }

    #[test]
    fn a_finished_compaction_is_marked_and_a_failed_one_is_not() {
        let mut run = Run::default();
        let done = json!({"type": "compaction_end", "reason": "threshold",
                          "result": {"summary": "s", "tokensBefore": 10},
                          "aborted": false, "willRetry": false});
        assert_eq!(kinds(&run.on_frame(&done)), ["compacted"]);
        let failed = json!({"type": "compaction_end", "reason": "threshold",
                            "aborted": false, "errorMessage": "terminated"});
        assert!(run.on_frame(&failed).is_empty());
    }

    #[test]
    fn thinking_streams_beside_the_reply() {
        let mut run = Run::default();
        let events = run.on_frame(&json!({"type": "message_update",
            "assistantMessageEvent": {"type": "thinking_delta", "contentIndex": 0,
                                      "delta": "hmm"}}));
        assert_eq!(kinds(&events), ["reasoning_delta"]);
    }

    #[test]
    fn every_prompt_can_steer_and_carries_its_pictures() {
        let plain = prompt_frame(&Prompt::text("hi"));
        assert_eq!(
            plain,
            json!({"type": "prompt", "message": "hi", "streamingBehavior": "steer"})
        );
        let mut with_picture = Prompt::text("what is this");
        with_picture.images.push(openremote_harness::Image {
            mime: "image/png".into(),
            data: "iVBORw0K".into(),
            path: None,
        });
        assert_eq!(
            prompt_frame(&with_picture)["images"],
            json!([{"type": "image", "data": "iVBORw0K", "mimeType": "image/png"}])
        );
    }

    /// A fresh folder under the system temp dir, removed when dropped. This
    /// crate has no temp-dir dependency, and these tests need nothing more.
    struct Scratch(std::path::PathBuf);

    impl Scratch {
        fn new(label: &str) -> Self {
            use std::sync::atomic::{AtomicU64, Ordering};
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let path = std::env::temp_dir().join(format!(
                "openremote-pi-{label}-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::SeqCst)
            ));
            std::fs::create_dir_all(&path).expect("scratch folder");
            Self(path)
        }

        fn path(&self) -> &std::path::Path {
            &self.0
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// A workspace and a data folder, each its own temp dir.
    fn folders() -> (Scratch, Scratch) {
        (Scratch::new("workspace"), Scratch::new("data"))
    }

    fn write_transcript(dir: &std::path::Path, file: &str, id: &str) {
        std::fs::create_dir_all(dir).expect("session folder");
        let header = json!({"type": "session", "version": 3, "id": id,
                            "timestamp": "2026-11-01T10:00:00.000Z", "cwd": "/w"});
        std::fs::write(
            dir.join(file),
            format!("{header}\n{{\"type\":\"message\"}}\n"),
        )
        .expect("transcript");
    }

    #[test]
    fn a_new_chat_keeps_its_transcript_in_the_data_folder_not_the_workspace() {
        let (workspace, data) = folders();
        let opts = SpawnOptions {
            cwd: workspace.path().to_path_buf(),
            state_dir: Some(data.path().join("harness-state").join("pi")),
            ..Default::default()
        };
        let dir = session_dir(&opts).expect("a folder");
        assert!(dir.is_absolute(), "{dir:?}");
        assert!(
            dir.starts_with(data.path()),
            "{dir:?} is in the data folder"
        );
        assert!(dir.is_dir(), "made before pi starts");
        assert!(
            !workspace.path().join(WORKSPACE_SESSIONS).exists(),
            "the workspace gains nothing"
        );
        let argv = build_args(&opts, &dir).expect("argv builds");
        let at = argv
            .iter()
            .position(|word| *word == std::ffi::OsStr::new("--session-dir"))
            .expect("--session-dir rides argv");
        assert_eq!(std::path::Path::new(&argv[at + 1]), dir);
    }

    #[test]
    fn a_chat_whose_transcript_is_in_the_workspace_resumes_from_there() {
        let (workspace, data) = folders();
        let legacy = workspace.path().join(WORKSPACE_SESSIONS);
        write_transcript(&legacy, "2026-11-01T10-00-00_abc123.jsonl", "abc123");
        let state_dir = Some(data.path().join("pi"));
        let resumed = SpawnOptions {
            cwd: workspace.path().to_path_buf(),
            resume: Some("abc123".into()),
            state_dir: state_dir.clone(),
            ..Default::default()
        };
        assert_eq!(
            session_dir(&resumed).expect("a folder"),
            std::path::PathBuf::from(WORKSPACE_SESSIONS),
            "the old place, spelled as before, so pi finds the same file"
        );
        // Another chat in the same workspace, whose transcript is not there,
        // moves on to the data folder.
        let other = SpawnOptions {
            resume: Some("def456".into()),
            ..resumed.clone()
        };
        assert!(
            session_dir(&other)
                .expect("a folder")
                .starts_with(data.path())
        );
        // A file whose name suggests the id but whose header names another
        // session is not that session: pi matches by the header.
        write_transcript(&legacy, "2026-11-01T10-00-00_def456.jsonl", "zzz999");
        assert!(
            session_dir(&other)
                .expect("a folder")
                .starts_with(data.path())
        );
    }

    #[test]
    fn with_no_data_folder_the_workspace_folder_is_still_used() {
        let (workspace, _data) = folders();
        let opts = SpawnOptions {
            cwd: workspace.path().to_path_buf(),
            ..Default::default()
        };
        assert_eq!(
            session_dir(&opts).expect("a folder"),
            std::path::PathBuf::from(WORKSPACE_SESSIONS)
        );
    }

    #[test]
    fn a_workspace_key_is_readable_and_never_changes_between_builds() {
        // Pinned: a different key after an update would lose every chat's
        // transcript. Change this only with a migration.
        assert_eq!(
            workspace_key(std::path::Path::new("/home/me/repo")),
            "repo-4b7c312454f21bd5"
        );
        let a = workspace_key(std::path::Path::new("/work/one/app"));
        let b = workspace_key(std::path::Path::new("/work/two/app"));
        assert!(a.starts_with("app-") && b.starts_with("app-"));
        assert_ne!(a, b, "two folders with one name stay apart");
        let odd = workspace_key(std::path::Path::new("/w/my repo: v2 (copy)"));
        assert!(
            odd.chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-')),
            "{odd}"
        );
        assert!(!odd.starts_with('-'), "{odd}");
    }
}
