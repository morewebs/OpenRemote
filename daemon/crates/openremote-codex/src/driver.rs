//! The Codex session driver: one `codex app-server` process per session.
//!
//! Wire plumbing: client→server calls carry `id` and are answered by
//! `{id, result|error}`; notifications are `{method, params}`; the server
//! also makes REQUESTS (`{id, method, params}`) - approvals arrive that
//! way and are answered with `{id, result: {decision}}`. The turn boundary
//! is the `turn/completed` notification; never guess turn state.

use std::collections::HashMap;
use std::future::Future;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::{Arc, Mutex as StdMutex};
use std::time::Duration;

use openremote_core::{DecisionKind, DecisionOption, TurnOutcome};
use openremote_harness::events::{ApprovalRequest, DecisionSpec};
use openremote_harness::{
    DriverError, DriverEvent, Resolution, SessionSettings, SpawnOptions, kill_tree,
};
use serde_json::{Value, json};
use tokio::io::{AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::{Mutex as AsyncMutex, mpsc, oneshot};
use uuid::Uuid;

/// Default approval policy when the session didn't pick one - Codex's own
/// word for "ask me before running things".
const DEFAULT_APPROVAL_POLICY: &str = "on-request";
const DEFAULT_SANDBOX: &str = "workspace-write";

const CALL_TIMEOUT: Duration = Duration::from_secs(60);

/// Where host call ids begin. The server numbers its own requests from
/// small integers (1, 2, 3, ...), and our approval answers carry those
/// ids back; a method-less frame holding a server id must never match a
/// host call's waiter. Host ids therefore start far above any id a real
/// app-server plausibly mints (harness audit F9).
const HOST_CALL_ID_BASE: u64 = 1 << 32;

/// Host calls in flight: our call id → response waiter. A call that
/// gives up (failed write, timeout) removes its own entry, or the map
/// pins dead senders for the session's life (harness audit F15).
struct PendingCalls {
    map: StdMutex<HashMap<u64, oneshot::Sender<Value>>>,
}

impl PendingCalls {
    fn new() -> Self {
        Self {
            map: StdMutex::new(HashMap::new()),
        }
    }

    /// Register the waiter for one call id.
    fn register(&self, id: u64, waiter: oneshot::Sender<Value>) {
        self.map.lock().unwrap().insert(id, waiter);
    }

    /// Give up on a call. No response can arrive for an id whose frame
    /// never left or whose waiter timed out, so the entry must not
    /// linger (harness audit F15).
    fn abandon(&self, id: u64) {
        self.map.lock().unwrap().remove(&id);
    }

    /// Hand a method-less stdout frame to the waiter registered for its
    /// id, if any. False means the frame matched no call: host ids live
    /// far above the server's, so a frame carrying a server request id
    /// (an approval answer echo, a late reply) drops instead of waking
    /// the wrong waiter (harness audit F9).
    fn deliver(&self, id: u64, frame: Value) -> bool {
        match self.map.lock().unwrap().remove(&id) {
            Some(waiter) => waiter.send(frame).is_ok(),
            None => false,
        }
    }

    /// Wait one call out. The timeout abandons the call's entry before
    /// returning the error, never after (harness audit F15).
    async fn await_response(
        &self,
        id: u64,
        rx: oneshot::Receiver<Value>,
        method: &str,
        timeout: Duration,
    ) -> Result<Value, DriverError> {
        match tokio::time::timeout(timeout, rx).await {
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
    /// Host calls in flight, keyed by call id.
    pending: PendingCalls,
    /// The turn Codex reports as running (turn/started … turn/completed).
    active_turn: StdMutex<Option<String>>,
}

pub struct Driver {
    stdin: tokio::process::ChildStdin,
    shared: Arc<Shared>,
    thread_id: String,
    next_id: u64,
    /// The workspace: an `@path` names a file under it.
    cwd: PathBuf,
    /// Codex's own skills here (name, SKILL.md path), asked once, the
    /// first time a prompt names one with `/`.
    skills: Option<Vec<(String, String)>>,
}

impl Driver {
    /// Spawn `codex app-server`, run the initialize handshake, and start
    /// (or resume) the thread. Returns once the harness conversation is
    /// named - the session ref rides out as the first `Initialized` event.
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
        if opts.endpoint.is_some() {
            return Err(DriverError::Harness(
                "custom endpoints are not available for this harness yet".to_string(),
            ));
        }
        argv.extend(["app-server".into(), "--listen".into(), "stdio://".into()]);

        let mut command = Command::new(&program);
        command
            .args(&argv)
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

        let (tx, mut rx) = mpsc::channel(256);
        let shared = Arc::new(Shared {
            child: Arc::new(AsyncMutex::new(child)),
            pending: PendingCalls::new(),
            active_turn: StdMutex::new(None),
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
            thread_id: String::new(),
            next_id: HOST_CALL_ID_BASE,
            cwd: opts.cwd.clone(),
            skills: None,
        };

        // Handshake (cloudroom lines 22-25): initialize, then the
        // `initialized` notification, then the thread. Each call is
        // raced against the death of the CLI process: a CLI that crashes
        // at boot holds its exit status in the first second, and failing
        // there beats burning the 60s call timeout on an answer that can
        // never come (harness audit F13).
        let boot = async {
            race_child_death(
                &shared,
                "initialize",
                driver.call(
                    "initialize",
                    json!({
                        "clientInfo": {"name": "openremote", "version": env!("CARGO_PKG_VERSION")},
                        "capabilities": {"experimentalApi": true}
                    }),
                ),
            )
            .await?;
            driver.notify("initialized", json!({})).await?;

            // Thread: start or resume (cloudroom lines 70-99).
            let mut params = thread_params(&opts);
            let (method, thread_id) = if let Some(resume) = &opts.resume {
                params["threadId"] = json!(resume);
                params["excludeTurns"] = json!(true);
                ("thread/resume", resume.clone())
            } else {
                ("thread/start", String::new())
            };
            let result = race_child_death(&shared, method, driver.call(method, params)).await?;
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
            Ok(())
        };
        if let Err(e) = boot.await {
            // The CLI's dying words ride stderr; splice the tail into the
            // failure so a broken install explains itself (harness audit
            // F13), and a child that survived a rejected handshake must
            // not outlive the session that never was.
            let tail = stderr_tail(&mut rx).await;
            discard_child(&shared).await;
            return Err(splice_tail(e, &tail));
        }
        Ok((driver, rx))
    }

    /// Send one user message as a turn. If a turn is already running, the
    /// message steers it (Codex's own mechanism, `expectedTurnId`).
    pub async fn send_prompt(&mut self, text: &str) -> Result<(), DriverError> {
        let input = self.user_input(text).await;
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
            let params = json!({
                "threadId": self.thread_id,
                "clientUserMessageId": Uuid::new_v4().to_string(),
                "input": input,
            });
            let result = self.call("turn/start", params).await?;
            if result.pointer("/turn/id").and_then(Value::as_str).is_none() {
                return Err(DriverError::Protocol(
                    "turn/start returned no turn id; outcome uncertain".into(),
                ));
            }
        }
        Ok(())
    }

    /// Apply model, effort, or fast to this thread now. Codex's own wire
    /// is `thread/settings/update` with flat params: a `turn/start`
    /// override only works before the thread has adopted a model - after
    /// that it is silently ignored, so the settings call is the only
    /// honest path for a live change. Verified against the real
    /// app-server (0.160.0): effort and serviceTier ride the same flat
    /// shape, and the fast tier is codex's own `fast` word.
    pub async fn apply_settings(&mut self, settings: &SessionSettings) -> Result<(), DriverError> {
        if settings.model.is_none() && settings.effort.is_none() && settings.fast.is_none() {
            return Ok(());
        }
        let mut params = json!({"threadId": self.thread_id});
        if let Some(model) = &settings.model {
            params["model"] = json!(model);
        }
        if let Some(effort) = &settings.effort {
            params["effort"] = json!(effort);
        }
        if let Some(fast) = settings.fast {
            params["serviceTier"] = json!(if fast { "fast" } else { "default" });
        }
        self.call("thread/settings/update", params).await?;
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
        self.shared.pending.register(id, tx);
        if let Err(e) = self
            .write(json!({"id": id, "method": method, "params": params}))
            .await
        {
            // The write failed: no response can ever arrive for this id,
            // so the waiter must not linger in the map (harness audit F15).
            self.shared.pending.abandon(id);
            return Err(e);
        }
        self.shared
            .pending
            .await_response(id, rx, method, CALL_TIMEOUT)
            .await
    }

    /// The turn's input in Codex's own items: the text as typed, plus a
    /// `skill` item for each `/name` that names one of Codex's skills and a
    /// `mention` item for each `@path` that names a file in the workspace.
    /// Codex runs skills and reads mentioned files through those items;
    /// the text alone would leave both to the model.
    async fn user_input(&mut self, text: &str) -> Value {
        let mut items = vec![json!({"type": "text", "text": text, "text_elements": []})];
        let words = mention_words(text);
        if self.skills.is_none() && words.iter().any(|w| w.starts_with('/')) {
            let cwd = self.cwd.to_string_lossy().into_owned();
            let listed = self
                .call("skills/list", json!({"cwds": [cwd]}))
                .await
                .map(|r| skills_from(&r))
                .unwrap_or_default();
            self.skills = Some(listed.into_iter().map(|s| (s.0, s.2)).collect());
        }
        for word in words {
            if let Some(name) = word.strip_prefix('/') {
                if let Some((name, path)) = self
                    .skills
                    .iter()
                    .flatten()
                    .find(|(skill, _)| skill == name)
                {
                    items.push(json!({"type": "skill", "name": name, "path": path}));
                }
            } else if let Some(relative) = word.strip_prefix('@') {
                let path = self.cwd.join(relative);
                if path.exists() {
                    let name = path
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_else(|| relative.to_string());
                    items.push(json!({
                        "type": "mention",
                        "name": name,
                        "path": path.to_string_lossy(),
                    }));
                }
            }
        }
        Value::Array(items)
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

/// The `/name` and `@path` words of a prompt: each at the start or after
/// whitespace; an `@"quoted path"` keeps its spaces.
pub fn mention_words(text: &str) -> Vec<String> {
    let mut words = Vec::new();
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let at_start = i == 0 || chars[i - 1].is_whitespace();
        if at_start && (chars[i] == '/' || chars[i] == '@') {
            let sigil = chars[i];
            let mut j = i + 1;
            let word: String = if sigil == '@' && chars.get(j) == Some(&'"') {
                j += 1;
                let start = j;
                while j < chars.len() && chars[j] != '"' {
                    j += 1;
                }
                let inner: String = chars[start..j].iter().collect();
                j += 1;
                inner
            } else {
                let start = j;
                while j < chars.len() && !chars[j].is_whitespace() {
                    j += 1;
                }
                chars[start..j].iter().collect()
            };
            if !word.is_empty() {
                words.push(format!("{sigil}{word}"));
            }
            i = j;
        } else {
            i += 1;
        }
    }
    words
}

/// `skills/list`'s answer as (name, description, SKILL.md path), enabled
/// skills only.
fn skills_from(result: &Value) -> Vec<(String, Option<String>, String)> {
    let mut out = Vec::new();
    for entry in result
        .get("data")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        for skill in entry
            .get("skills")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            if skill.get("enabled").and_then(Value::as_bool) == Some(false) {
                continue;
            }
            let (Some(name), Some(path)) = (
                skill.get("name").and_then(Value::as_str),
                skill.get("path").and_then(Value::as_str),
            ) else {
                continue;
            };
            let description = skill
                .get("shortDescription")
                .and_then(Value::as_str)
                .or_else(|| skill.get("description").and_then(Value::as_str))
                .map(String::from);
            out.push((name.to_string(), description, path.to_string()));
        }
    }
    out
}

/// The skills a Codex chat in `cwd` can run, as commands: a short-lived
/// app-server, its handshake, and `skills/list` - no thread is started
/// and nothing is sent to a model. Codex runs a skill named in a prompt
/// as a `skill` input item, which the driver adds for each `/name`.
pub async fn commands(
    resolution: &Resolution,
    cwd: &std::path::Path,
) -> Result<openremote_harness::CommandCatalog, DriverError> {
    use tokio::io::AsyncBufReadExt;
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
        .current_dir(cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    #[cfg(windows)]
    command.creation_flags(openremote_harness::creation_flags());
    let mut child = command
        .spawn()
        .map_err(|e| DriverError::Spawn(format!("codex commands: {e}")))?;
    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| DriverError::Spawn("stdin not piped".into()))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| DriverError::Spawn("stdout not piped".into()))?;
    let cwd_text = cwd.to_string_lossy().into_owned();
    let exchange = async {
        let frames = [
            json!({"id": 1, "method": "initialize", "params": {
                "clientInfo": {"name": "openremote", "version": env!("CARGO_PKG_VERSION")},
                "capabilities": {"experimentalApi": true}
            }}),
            json!({"method": "initialized", "params": {}}),
            json!({"id": 2, "method": "skills/list", "params": {"cwds": [cwd_text]}}),
        ];
        for frame in frames {
            let mut line = serde_json::to_string(&frame).expect("frame serializes");
            line.push('\n');
            stdin.write_all(line.as_bytes()).await?;
        }
        stdin.flush().await?;
        let mut lines = BufReader::new(stdout).lines();
        while let Some(line) = lines.next_line().await? {
            let Ok(frame) = serde_json::from_str::<Value>(&line) else {
                continue;
            };
            if frame.get("id").and_then(Value::as_u64) == Some(2) {
                return Ok::<_, std::io::Error>(Some(frame));
            }
        }
        Ok(None)
    };
    let answer = tokio::time::timeout(Duration::from_secs(30), exchange).await;
    kill_tree(&mut child).await;
    let frame = match answer {
        Ok(Ok(Some(frame))) => frame,
        Ok(Ok(None)) => {
            return Err(DriverError::Protocol(
                "codex closed before skills/list".into(),
            ));
        }
        Ok(Err(e)) => return Err(e.into()),
        Err(_) => return Err(DriverError::Protocol("codex skills/list timed out".into())),
    };
    if let Some(error) = frame.get("error") {
        return Err(DriverError::Harness(format!(
            "codex rejected skills/list: {error}"
        )));
    }
    let result = frame.get("result").cloned().unwrap_or(Value::Null);
    Ok(openremote_harness::CommandCatalog {
        commands: skills_from(&result)
            .into_iter()
            .map(
                |(name, description, _)| openremote_harness::CommandDescriptor {
                    name,
                    description,
                    argument_hint: None,
                    aliases: Vec::new(),
                },
            )
            .collect(),
        agents: Vec::new(),
    })
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
            Ok(status) => format!("codex exited during {step} ({status})"),
            Err(e) => format!("codex exited during {step} (exit unreadable: {e})"),
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

/// Splice the CLI's dying words onto a handshake failure so a broken
/// install explains itself (harness audit F13).
fn splice_tail(error: DriverError, tail: &str) -> DriverError {
    if tail.is_empty() {
        return error;
    }
    let message = match error {
        DriverError::Protocol(message) => message,
        other => other.to_string(),
    };
    DriverError::Protocol(format!("{message}; {tail}"))
}

/// A child that outlived a failed handshake must not outlive the session
/// that never was: kill and reap, best effort, leaving the original
/// failure in place.
async fn discard_child(shared: &Arc<Shared>) {
    let mut child = shared.child.lock().await;
    kill_tree(&mut child).await;
    let _ = tokio::time::timeout(Duration::from_secs(5), child.wait()).await;
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

/// The `thread/start` (and `thread/resume`) params. Fast mode rides
/// Codex's own service-tier word on the thread - `serviceTier: "fast"`
/// (v0.110.0+; a model that doesn't advertise the tier drops it with a
/// warning, so it is safe to always pass when asked).
fn thread_params(opts: &SpawnOptions) -> Value {
    let mut params = json!({
        "cwd": opts.cwd,
        "approvalPolicy": opts.permission_mode.clone().unwrap_or_else(|| DEFAULT_APPROVAL_POLICY.to_string()),
        "sandbox": DEFAULT_SANDBOX,
        "ephemeral": false,
    });
    if let Some(model) = &opts.model {
        params["model"] = json!(model);
    }
    if opts.fast {
        params["serviceTier"] = json!("fast");
    }
    // Codex's own wire for start-time settings: the config-override map,
    // dotted keyPaths (config.toml keys).
    let mut config = json!({});
    // `mcp_servers.<id>.command` / `.args` (verified against the real
    // app-server: the injected server shows up connected on the thread).
    for server in &opts.mcp_servers {
        config[format!("mcp_servers.{}.command", server.id)] = json!(server.command);
        config[format!("mcp_servers.{}.args", server.id)] = json!(server.args);
    }
    // The chat's effort, on every start and resume.
    if let Some(effort) = &opts.effort {
        config["model_reasoning_effort"] = json!(effort);
    }
    if config.as_object().is_some_and(|c| !c.is_empty()) {
        params["config"] = config;
    }
    params
}

fn coarse_outcome(status: &str) -> TurnOutcome {
    match status {
        "completed" => TurnOutcome::Completed,
        "interrupted" => TurnOutcome::Interrupted,
        _ => TurnOutcome::Failed,
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
            // A response to one of our calls. Host ids live far above the
            // server's own request ids, so a frame carrying a server id
            // finds no waiter and drops instead of waking the wrong call
            // (harness audit F9).
            shared.pending.deliver(id, frame);
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
        // The thread's own context numbers: total tokens against the
        // model's window, both reported by the harness itself.
        "thread/tokenUsage/updated" => {
            let used = params
                .pointer("/tokenUsage/total/totalTokens")
                .and_then(Value::as_u64)?;
            let window = params
                .get("tokenUsage")
                .and_then(|u| u.get("modelContextWindow"))
                .and_then(Value::as_u64);
            Some(DriverEvent::ContextUsed { used, window })
        }
        "thread/compacted" => Some(DriverEvent::Compacted),
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
                // Codex's own reasoning item, on models that emit them -
                // its words, surfaced dim, never mixed into the reply.
                "reasoning" => item
                    .get("text")
                    .and_then(Value::as_str)
                    .filter(|t| !t.is_empty())
                    .map(|text| DriverEvent::ReasoningText {
                        text: text.to_string(),
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
                // Codex's own display name rides its catalog entry.
                display_name: entry
                    .get("displayName")
                    .and_then(Value::as_str)
                    .map(String::from),
                resolved_model: None,
                reasoning_efforts: efforts,
                // Its own marker for the entry a fresh thread runs -
                // the pre-send fact its config.toml never says.
                is_default: entry.get("isDefault").and_then(Value::as_bool) == Some(true),
                // Codex's catalog carries no cost line and no notice.
                detail: None,
                note: None,
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

    #[test]
    fn fast_mode_rides_codex_service_tier_word() {
        let mut opts = SpawnOptions {
            cwd: PathBuf::from("/w"),
            model: Some("gpt-6.1-sol".into()),
            ..Default::default()
        };
        let params = thread_params(&opts);
        assert_eq!(params["model"].as_str(), Some("gpt-6.1-sol"));
        assert!(params.get("serviceTier").is_none());

        opts.fast = true;
        let params = thread_params(&opts);
        assert_eq!(params["serviceTier"].as_str(), Some("fast"));
        assert_eq!(params["approvalPolicy"].as_str(), Some("on-request"));
    }

    #[test]
    fn a_picked_permission_word_rides_the_thread_verbatim() {
        let opts = SpawnOptions {
            cwd: PathBuf::from("/w"),
            permission_mode: Some("never".into()),
            ..Default::default()
        };
        let params = thread_params(&opts);
        assert_eq!(params["approvalPolicy"].as_str(), Some("never"));
    }

    #[test]
    fn plugins_ride_codexs_own_config_overrides() {
        let opts = SpawnOptions {
            cwd: PathBuf::from("/w"),
            mcp_servers: vec![openremote_harness::McpServer {
                id: "github".into(),
                command: "npx".into(),
                args: vec!["-y".into(), "@modelcontextprotocol/server-github".into()],
            }],
            ..Default::default()
        };
        let params = thread_params(&opts);
        // Codex's own shape: dotted keyPaths into the config map.
        assert_eq!(
            params["config"]["mcp_servers.github.command"].as_str(),
            Some("npx")
        );
        assert_eq!(
            params["config"]["mcp_servers.github.args"],
            json!(["-y", "@modelcontextprotocol/server-github"])
        );

        // No plugins, no config map.
        let plain = thread_params(&SpawnOptions {
            cwd: PathBuf::from("/w"),
            ..Default::default()
        });
        assert!(plain.get("config").is_none());
    }

    #[test]
    fn host_call_ids_live_above_any_server_request_id() {
        // F9's fix is the base itself: host ids must start outside the
        // small range a real app-server numbers its own requests from.
        // Const-checked so the check rides the build, not the run.
        const _: () = assert!(
            HOST_CALL_ID_BASE > 1_000_000,
            "host call ids must live far above the server's own id space"
        );
    }

    #[test]
    fn a_server_request_id_cannot_wake_a_host_call() {
        // F9: a response frame carrying a server request id (the range
        // the old 1..n host ids occupied) must find no waiter; the host
        // call keeps waiting for its real answer.
        let pending = PendingCalls::new();
        let call_id = HOST_CALL_ID_BASE + 3;
        let (tx, rx) = oneshot::channel();
        pending.register(call_id, tx);
        let misrouted = pending.deliver(2, json!({"id": 2, "result": {"decision": "accept"}}));
        assert!(!misrouted, "a server id must not match a host call");
        assert!(!pending.is_empty(), "the host call must still be waiting");
        // The real response still routes, with the right body.
        assert!(pending.deliver(
            call_id,
            json!({"id": call_id, "result": {"turn": {"id": "t-1"}}}),
        ));
        let frame = rx.blocking_recv().expect("the waiter got its response");
        let expected = json!("t-1");
        assert_eq!(frame.pointer("/result/turn/id"), Some(&expected));
    }

    #[tokio::test]
    async fn a_timed_out_call_leaves_no_waiter_behind() {
        // F15: the entry must go when the timeout fires, not when (if
        // ever) a response arrives for it.
        let pending = PendingCalls::new();
        let (tx, rx) = oneshot::channel();
        pending.register(HOST_CALL_ID_BASE, tx);
        let result = pending
            .await_response(
                HOST_CALL_ID_BASE,
                rx,
                "model/list",
                Duration::from_millis(50),
            )
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
        pending.register(HOST_CALL_ID_BASE, tx);
        pending.abandon(HOST_CALL_ID_BASE);
        assert!(pending.is_empty());
        assert!(!pending.deliver(
            HOST_CALL_ID_BASE,
            json!({"id": HOST_CALL_ID_BASE, "result": {}}),
        ));
        drop(rx);
    }
}
