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
    DriverError, DriverEvent, Prompt, Resolution, SessionSettings, SpawnOptions, kill_tree,
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
    /// The child's stdin. The reader writes too - it refuses the requests
    /// no one here can answer - so a whole line goes out under this lock
    /// and two writers never interleave one.
    stdin: AsyncMutex<tokio::process::ChildStdin>,
    /// Host calls in flight, keyed by call id.
    pending: PendingCalls,
    /// The turn Codex reports as running (turn/started … turn/completed).
    active_turn: StdMutex<Option<String>>,
}

pub struct Driver {
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
            stdin: AsyncMutex::new(stdin),
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
            let (method, params, thread_id) = thread_request(&opts);
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
    pub async fn send_prompt(&mut self, prompt: &Prompt) -> Result<(), DriverError> {
        let mut input = self.user_input(&prompt.text).await;
        if let Value::Array(items) = &mut input {
            // A message of pictures alone needs no empty text item.
            if prompt.text.is_empty() && !prompt.images.is_empty() {
                items.retain(|item| item.get("type").and_then(Value::as_str) != Some("text"));
            }
            items.extend(prompt.images.iter().map(image_item));
        }
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

    /// Answer one of Codex's requests with the choice picked. Each kind of
    /// request has its own answer shape; the request as stored names the
    /// kind, and `answer_result` speaks that kind's words.
    pub async fn answer(
        &mut self,
        harness_ref: &str,
        choice: &str,
        request: &Value,
    ) -> Result<(), DriverError> {
        // The reference is the request id as its own JSON: `7`, or `"7"`
        // where the app-server numbers its requests with strings.
        let id: Value = serde_json::from_str(harness_ref)
            .ok()
            .filter(|id: &Value| id.is_u64() || id.is_string())
            .ok_or_else(|| {
                DriverError::Protocol("codex request ids are numbers or strings".into())
            })?;
        self.write(json!({"id": id, "result": answer_result(choice, request)}))
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
        write_frame(&self.shared, &value).await
    }
}

/// One frame to the app-server, whole, under the stdin lock.
async fn write_frame(shared: &Shared, value: &Value) -> Result<(), DriverError> {
    let mut line = serde_json::to_string(value).expect("envelope serializes");
    line.push('\n');
    let mut stdin = shared.stdin.lock().await;
    stdin.write_all(line.as_bytes()).await?;
    stdin.flush().await?;
    Ok(())
}

/// A picture as Codex's own input item: a `localImage` it reads from disk
/// where the daemon kept one as a file, else an `image` carrying the bytes
/// as a data URL - the two picture forms its turn input takes.
fn image_item(image: &openremote_harness::Image) -> Value {
    match &image.path {
        Some(path) => json!({"type": "localImage", "path": path.to_string_lossy()}),
        None => json!({
            "type": "image",
            "url": format!("data:{};base64,{}", image.mime, image.data),
        }),
    }
}

/// An MCP tool call's name in the shape claude gives one
/// (`mcp__<server>__<tool>`), so a tool reads the same in every chat
/// whichever harness called it.
fn mcp_tool_name(item: &Value) -> String {
    let field = |key: &str| item.get(key).and_then(Value::as_str).unwrap_or_default();
    format!("mcp__{}__{}", field("server"), field("tool"))
}

/// A finished MCP tool call as a tool result: the words and pictures of
/// its content, or its error's words. A call that failed, or answered
/// with MCP's own error flag, is an error.
fn mcp_tool_result(item: &Value) -> DriverEvent {
    let content = item
        .pointer("/result/content")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default();
    let error = item
        .pointer("/error/message")
        .and_then(Value::as_str)
        .map(String::from);
    let flagged = item.pointer("/result/isError").and_then(Value::as_bool) == Some(true);
    let failed = item.get("status").and_then(Value::as_str) == Some("failed");
    let words = openremote_harness::anthropic_wire::content_text(content);
    DriverEvent::ToolResult {
        tool_use_id: item
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        name: mcp_tool_name(item),
        text: match (&error, words.is_empty()) {
            (Some(error), true) => error.clone(),
            (Some(error), false) => format!("{words}\n{error}"),
            (None, _) => words,
        },
        is_error: failed || flagged || error.is_some(),
        images: openremote_harness::anthropic_wire::content_images(content),
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

/// An approval with only the choices that mean something for it: each is
/// a decision word and the label it wears. None of them ends the turn.
fn choice_spec(tool: &str, summary: Option<String>, choices: &[(&str, &str)]) -> DecisionSpec {
    DecisionSpec {
        kind: DecisionKind::Approval,
        options: choices
            .iter()
            .map(|(id, label)| DecisionOption {
                id: (*id).into(),
                label: (*label).into(),
            })
            .collect(),
        tool_name: Some(tool.to_string()),
        summary,
        interrupts_turn: Vec::new(),
    }
}

/// The thread call a spawn makes: `thread/start`, or `thread/resume` on the
/// chat's own thread - with the same params, the chat's folder as `cwd`
/// among them, so a chat that moved to another project resumes its thread
/// working in the new folder.
fn thread_request(opts: &SpawnOptions) -> (&'static str, Value, String) {
    let mut params = thread_params(opts);
    match &opts.resume {
        Some(resume) => {
            params["threadId"] = json!(resume);
            params["excludeTurns"] = json!(true);
            ("thread/resume", params, resume.clone())
        }
        None => ("thread/start", params, String::new()),
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
        // A request id is a number or a string; the app-server uses numbers
        // today, but a request whose id is a string is still a request.
        let request_id = frame
            .get("id")
            .filter(|id| id.is_u64() || id.is_string())
            .cloned();

        if let (true, Some(request_id)) = (has_method, request_id) {
            // A server→client request: approvals land here.
            let method = frame["method"].as_str().unwrap_or_default().to_string();
            let params = frame.get("params").cloned().unwrap_or(Value::Null);
            match server_request(&method, &params, &request_id) {
                Incoming::Ask(event) => {
                    if tx.send(event).await.is_err() {
                        return;
                    }
                }
                // Nobody here can answer it. Say so now: a request left
                // unanswered holds the turn open for as long as the
                // process lives.
                Incoming::Refuse { code, message } => {
                    let refusal = json!({
                        "id": request_id,
                        "error": {"code": code, "message": message},
                    });
                    // A dead stdin ends the stdout read on its own.
                    let _ = write_frame(&shared, &refusal).await;
                }
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

/// What Codex asks of the host, by the reader's reckoning.
enum Incoming {
    /// A person decides. It rides out as a decision and is answered later
    /// through `answer`.
    Ask(DriverEvent),
    /// Nobody here can serve it, and a request left unanswered holds the
    /// turn open: it is refused on the spot with a JSON-RPC error.
    Refuse { code: i64, message: String },
}

/// JSON-RPC's "method not found".
const METHOD_NOT_FOUND: i64 = -32601;
/// JSON-RPC's server-defined error range, for "cannot be answered here".
const CANNOT_ANSWER: i64 = -32000;

/// The server→client requests Codex makes, by the names the app-server
/// speaks (checked against `codex app-server generate-json-schema`, 0.162):
/// - `item/commandExecution/requestApproval`, `item/fileChange/requestApproval`
///   and `item/permissions/requestApproval` - a command, a file change, and
///   a grant of more sandbox, each asking before it runs;
/// - `mcpServer/elicitation/request` - how an MCP tool call asks before it
///   runs (the browser's tools and the team tools are those), and how an
///   MCP server asks for anything else;
/// - `item/tool/requestUserInput` - Codex's own question to the user;
/// - `execCommandApproval` and `applyPatchApproval` - the first protocol's
///   spellings, and `commandExecution/requestApproval` and
///   `applyPatch/requestApproval`, the bare names older builds used.
///
/// Each decision keeps Codex's own params, plus the wire `method` its answer
/// needs and the `tool_name` and `input` the console reads off any approval.
fn server_request(method: &str, params: &Value, id: &Value) -> Incoming {
    let text = |key: &str| {
        params
            .get(key)
            .and_then(Value::as_str)
            .filter(|t| !t.is_empty())
            .map(String::from)
    };
    match method {
        "item/commandExecution/requestApproval" | "commandExecution/requestApproval" => {
            let command = text("command");
            // The reason leads the terminal tile as its caption, the way a
            // command's description does for claude.
            let input = input_of([
                ("command", command.clone().map(Value::from)),
                ("description", text("reason").map(Value::from)),
            ]);
            ask(
                id,
                approval_spec("commandExecution", command),
                stored(method, params, "commandExecution", input),
            )
        }
        "item/fileChange/requestApproval" | "applyPatch/requestApproval" => {
            let input = input_of([
                ("reason", text("reason").map(Value::from)),
                ("grantRoot", text("grantRoot").map(Value::from)),
            ]);
            ask(
                id,
                approval_spec("fileChange", text("reason")),
                stored(method, params, "fileChange", input),
            )
        }
        "item/permissions/requestApproval" => {
            let input = input_of([
                ("reason", text("reason").map(Value::from)),
                (
                    "permissions",
                    params.get("permissions").filter(|p| !p.is_null()).cloned(),
                ),
            ]);
            ask(
                id,
                choice_spec(
                    "permissions",
                    text("reason"),
                    &[
                        ("accept", "Accept"),
                        ("acceptForSession", "Accept for session"),
                        ("decline", "Decline"),
                    ],
                ),
                stored(method, params, "permissions", input),
            )
        }
        // The first protocol: a command as an argv, a patch as a map of
        // paths. Answered in its own review words.
        "execCommandApproval" => {
            let command = params
                .get("command")
                .and_then(Value::as_array)
                .map(|argv| {
                    argv.iter()
                        .filter_map(Value::as_str)
                        .collect::<Vec<_>>()
                        .join(" ")
                })
                .filter(|c| !c.is_empty());
            let input = input_of([
                ("command", command.clone().map(Value::from)),
                ("description", text("reason").map(Value::from)),
            ]);
            ask(
                id,
                approval_spec("commandExecution", command),
                stored(method, params, "commandExecution", input),
            )
        }
        "applyPatchApproval" => {
            let files = params
                .get("fileChanges")
                .and_then(Value::as_object)
                .map(|changes| changes.keys().cloned().collect::<Vec<_>>());
            let input = input_of([
                ("files", files.map(Value::from)),
                ("reason", text("reason").map(Value::from)),
            ]);
            ask(
                id,
                approval_spec("fileChange", text("reason")),
                stored(method, params, "fileChange", input),
            )
        }
        "mcpServer/elicitation/request" => elicitation(id, method, params),
        "item/tool/requestUserInput" => user_input(id, method, params),
        other => Incoming::Refuse {
            code: METHOD_NOT_FOUND,
            message: format!("OpenRemote does not serve {other}"),
        },
    }
}

/// An MCP server's ask. Codex asks before an MCP tool runs this way (its
/// `_meta` says `codex_approval_kind`, names the tool's arguments, and lists
/// how long a yes may last), so the tool shows as the card claude's tool
/// approvals have: `mcp__<server>__<tool>` and its arguments. Anything else
/// an MCP server asks is a form or a link, and the console cannot fill one
/// in or open one for it: it shows the message and offers a no.
fn elicitation(id: &Value, method: &str, params: &Value) -> Incoming {
    let server = params
        .get("serverName")
        .and_then(Value::as_str)
        .unwrap_or("an MCP server");
    let message = params
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let meta = params.get("_meta").filter(|m| m.is_object());
    if let Some(meta) = meta.filter(|m| m.get("codex_approval_kind").is_some()) {
        // The tool as the message names it ("... to run tool \"name\"?");
        // its display title is the fallback.
        let tool = message
            .split_once(" to run tool \"")
            .and_then(|(_, rest)| rest.split_once('"'))
            .map(|(name, _)| name.to_string())
            .or_else(|| {
                meta.get("tool_title")
                    .and_then(Value::as_str)
                    .map(String::from)
            })
            .unwrap_or_else(|| "tool".into());
        let tool_name = format!("mcp__{server}__{tool}");
        // `persist` is a word or a list of words: "session" and "always".
        // Only the session one is offered here - "always" would write a
        // standing grant into Codex's own config from a click in a chat.
        let remembers = match meta.get("persist") {
            Some(Value::String(word)) => word == "session",
            Some(Value::Array(words)) => words.iter().any(|w| w.as_str() == Some("session")),
            _ => false,
        };
        let mut choices = vec![("accept", "Allow")];
        if remembers {
            choices.push(("acceptForSession", "Allow for this session"));
        }
        choices.push(("decline", "Decline"));
        let input = meta.get("tool_params").filter(|p| !p.is_null()).cloned();
        return ask(
            id,
            choice_spec(&tool_name, Some(message.to_string()), &choices),
            stored(method, params, &tool_name, input.unwrap_or(Value::Null)),
        );
    }
    let confirm_only = params.get("mode").and_then(Value::as_str) == Some("form")
        && params
            .pointer("/requestedSchema/properties")
            .and_then(Value::as_object)
            .is_none_or(|fields| fields.is_empty());
    let mut choices = Vec::new();
    if confirm_only {
        choices.push(("accept", "Continue"));
    }
    choices.push(("decline", "Decline"));
    let input = input_of([
        ("message", Some(Value::from(message))),
        ("url", params.get("url").filter(|u| u.is_string()).cloned()),
    ]);
    let tool_name = format!("{server} asks");
    ask(
        id,
        choice_spec(&tool_name, Some(message.to_string()), &choices),
        stored(method, params, &tool_name, input),
    )
}

/// Codex's own question to the user. One question with options is a choice
/// the console can put, in the shape claude's questions have so it renders
/// the same way; a question that wants typed words, or several answers at
/// once, cannot be put here, so Codex is told so and goes on without.
fn user_input(id: &Value, method: &str, params: &Value) -> Incoming {
    let refusal = || Incoming::Refuse {
        code: CANNOT_ANSWER,
        message: "OpenRemote can only put a question that has options to choose from; \
                  the user was not asked"
            .into(),
    };
    let Some([question]) = params
        .get("questions")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
    else {
        return refusal();
    };
    let secret = question.get("isSecret").and_then(Value::as_bool) == Some(true);
    let options: Vec<&Value> = question
        .get("options")
        .and_then(Value::as_array)
        .map(|options| options.iter().collect())
        .unwrap_or_default();
    if secret || options.is_empty() {
        return refusal();
    }
    let asked = question
        .get("question")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let labels: Vec<&str> = options
        .iter()
        .filter_map(|o| o.get("label").and_then(Value::as_str))
        .collect();
    let input = json!({"questions": [{
        "question": asked,
        "header": question.get("header").and_then(Value::as_str).unwrap_or_default(),
        "options": options,
    }]});
    Incoming::Ask(DriverEvent::ApprovalRequested {
        approval: ApprovalRequest {
            harness_ref: id.to_string(),
            spec: DecisionSpec {
                kind: DecisionKind::Question,
                // The choice is the option's own label, as claude's
                // questions answer.
                options: labels
                    .iter()
                    .map(|label| DecisionOption {
                        id: (*label).to_string(),
                        label: (*label).to_string(),
                    })
                    .collect(),
                tool_name: Some("request_user_input".into()),
                summary: Some(asked.to_string()),
                interrupts_turn: Vec::new(),
            },
            request: stored(method, params, "request_user_input", input),
        },
    })
}

/// A request that a person answers: the decision as the supervisor takes it.
fn ask(id: &Value, spec: DecisionSpec, request: Value) -> Incoming {
    Incoming::Ask(DriverEvent::ApprovalRequested {
        approval: ApprovalRequest {
            // The id as its own JSON, so `answer` can send back exactly it.
            harness_ref: id.to_string(),
            spec,
            request,
        },
    })
}

/// The request as the decision keeps it: Codex's params as they came, plus
/// the wire name its answer needs and what the console reads off an
/// approval (`tool_name`, `input`).
fn stored(method: &str, params: &Value, tool_name: &str, input: Value) -> Value {
    let mut request = params.as_object().cloned().unwrap_or_default();
    request.insert("method".into(), json!(method));
    request.insert("tool_name".into(), json!(tool_name));
    request.insert("input".into(), input);
    Value::Object(request)
}

/// An approval's input: the facts that are there, or nothing at all (the
/// console shows no body for no input).
fn input_of<const N: usize>(facts: [(&str, Option<Value>); N]) -> Value {
    let facts: serde_json::Map<String, Value> = facts
        .into_iter()
        .filter_map(|(key, value)| Some((key.to_string(), value?)))
        .collect();
    if facts.is_empty() {
        Value::Null
    } else {
        Value::Object(facts)
    }
}

/// What to send back for a choice, in the words of the request's own kind.
/// A request stored without its `method` is answered as the first
/// approvals were: a bare decision word.
fn answer_result(choice: &str, request: &Value) -> Value {
    match request.get("method").and_then(Value::as_str) {
        Some("mcpServer/elicitation/request") => match choice {
            "accept" => json!({"action": "accept"}),
            // Codex's own spelling of "remember this for the session".
            "acceptForSession" => json!({"action": "accept", "_meta": {"persist": "session"}}),
            "cancel" => json!({"action": "cancel"}),
            _ => json!({"action": "decline"}),
        },
        Some("item/permissions/requestApproval") => {
            let granted = matches!(choice, "accept" | "acceptForSession");
            json!({
                // Granting is the whole of what was asked for; declining
                // grants nothing.
                "permissions": if granted {
                    request.get("permissions").cloned().unwrap_or_else(|| json!({}))
                } else {
                    json!({})
                },
                "scope": if choice == "acceptForSession" { "session" } else { "turn" },
            })
        }
        Some("item/tool/requestUserInput") => {
            let question = request
                .pointer("/questions/0/id")
                .and_then(Value::as_str)
                .unwrap_or_default();
            json!({"answers": {question: {"answers": [choice]}}})
        }
        Some("execCommandApproval" | "applyPatchApproval") => {
            let decision = match choice {
                "accept" => json!("approved"),
                "acceptForSession" => json!("approved_for_session"),
                "cancel" => json!("abort"),
                _ => json!({"denied": {"rejection": "Declined from OpenRemote"}}),
            };
            json!({"decision": decision})
        }
        _ => json!({"decision": choice}),
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
                // A call to one of the chat's MCP servers (OpenRemote's
                // read-a-chat, agent-team and browser tools among them):
                // its arguments are the card's input, as claude's tool_use
                // input is.
                "mcpToolCall" => Some(DriverEvent::ToolStarted {
                    tool_use_id: item
                        .get("id")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string(),
                    name: mcp_tool_name(item),
                    input: item.get("arguments").cloned().unwrap_or(Value::Null),
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
                        images: Vec::new(),
                    })
                }
                "mcpToolCall" => Some(mcp_tool_result(item)),
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
    fn an_mcp_tool_call_reads_like_claudes_with_its_words_and_pictures() {
        let item = json!({
            "type": "mcpToolCall", "id": "call-1", "server": "openremote",
            "tool": "browser_screenshot", "status": "completed",
            "arguments": {"tab": 1},
            "result": {"content": [
                {"type": "text", "text": "captured"},
                {"type": "image", "mimeType": "image/png", "data": "iVBORw0K"}
            ]}
        });
        assert_eq!(mcp_tool_name(&item), "mcp__openremote__browser_screenshot");
        let DriverEvent::ToolResult {
            tool_use_id,
            name,
            text,
            is_error,
            images,
        } = mcp_tool_result(&item)
        else {
            panic!("a tool result");
        };
        assert_eq!(tool_use_id, "call-1");
        assert_eq!(name, "mcp__openremote__browser_screenshot");
        assert_eq!(text, "captured");
        assert!(!is_error);
        assert_eq!(images.len(), 1);
        assert_eq!(images[0].mime, "image/png");
        assert_eq!(images[0].data, "iVBORw0K");
    }

    #[test]
    fn a_failed_or_flagged_mcp_call_is_an_error_with_its_words() {
        let failed = json!({
            "type": "mcpToolCall", "id": "c", "server": "s", "tool": "t",
            "status": "failed", "error": {"message": "no such tab"}
        });
        let DriverEvent::ToolResult { text, is_error, .. } = mcp_tool_result(&failed) else {
            panic!("a tool result");
        };
        assert_eq!(text, "no such tab");
        assert!(is_error);

        let flagged = json!({
            "type": "mcpToolCall", "id": "c", "server": "s", "tool": "t",
            "status": "completed",
            "result": {"isError": true, "content": [{"type": "text", "text": "denied"}]}
        });
        let DriverEvent::ToolResult { text, is_error, .. } = mcp_tool_result(&flagged) else {
            panic!("a tool result");
        };
        assert_eq!(text, "denied");
        assert!(is_error);
    }

    #[test]
    fn a_resumed_thread_works_in_the_chats_folder_as_it_is_now() {
        let moved = SpawnOptions {
            cwd: PathBuf::from("/projects/new"),
            resume: Some("thr_1".into()),
            ..Default::default()
        };
        let (method, params, thread) = thread_request(&moved);
        assert_eq!(method, "thread/resume");
        assert_eq!(thread, "thr_1");
        assert_eq!(params["threadId"].as_str(), Some("thr_1"));
        assert_eq!(params["cwd"], json!(PathBuf::from("/projects/new")));
        let (method, params, thread) = thread_request(&SpawnOptions {
            cwd: PathBuf::from("/w"),
            ..Default::default()
        });
        assert_eq!(method, "thread/start");
        assert!(thread.is_empty() && params.get("threadId").is_none());
    }

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

    fn asked(incoming: Incoming) -> ApprovalRequest {
        match incoming {
            Incoming::Ask(DriverEvent::ApprovalRequested { approval }) => approval,
            _ => panic!("expected a decision for a person"),
        }
    }

    fn ids(approval: &ApprovalRequest) -> Vec<&str> {
        approval
            .spec
            .options
            .iter()
            .map(|o| o.id.as_str())
            .collect()
    }

    #[test]
    fn the_approvals_codex_sends_today_reach_the_console_and_answer_in_their_words() {
        // 0.162 spells them with the item/ prefix; the bare names older
        // builds used still answer.
        for method in [
            "item/commandExecution/requestApproval",
            "commandExecution/requestApproval",
        ] {
            let params = json!({"itemId": "i1", "threadId": "t", "turnId": "u",
                "command": "npm test", "reason": "run the suite", "startedAtMs": 1});
            let approval = asked(server_request(method, &params, &json!(7)));
            assert_eq!(approval.harness_ref, "7");
            assert_eq!(approval.spec.tool_name.as_deref(), Some("commandExecution"));
            assert_eq!(
                ids(&approval),
                ["accept", "acceptForSession", "decline", "cancel"]
            );
            // Codex's params ride as they came, with what the card reads.
            assert_eq!(approval.request["command"], "npm test");
            assert_eq!(approval.request["input"]["command"], "npm test");
            assert_eq!(approval.request["input"]["description"], "run the suite");
            assert_eq!(
                answer_result("acceptForSession", &approval.request),
                json!({"decision": "acceptForSession"})
            );
        }
        let patch = asked(server_request(
            "item/fileChange/requestApproval",
            &json!({"itemId": "i2", "threadId": "t", "turnId": "u", "grantRoot": "/w", "startedAtMs": 1}),
            &json!(8),
        ));
        assert_eq!(patch.request["tool_name"], "fileChange");
        assert_eq!(patch.request["input"]["grantRoot"], "/w");
        assert_eq!(
            answer_result("decline", &patch.request),
            json!({"decision": "decline"})
        );
    }

    #[test]
    fn an_mcp_tool_asks_through_an_elicitation_and_a_yes_can_last_the_session() {
        let params = json!({
            "serverName": "openremote", "threadId": "t", "turnId": "u", "mode": "form",
            "message": "Allow the openremote MCP server to run tool \"agents_spawn\"?",
            "requestedSchema": {"type": "object", "properties": {}},
            "_meta": {"codex_approval_kind": "mcp_tool_call",
                "persist": ["session", "always"], "tool_params": {"harness": "codex"}},
        });
        let approval = asked(server_request(
            "mcpServer/elicitation/request",
            &params,
            &json!(3),
        ));
        // Named as claude names an MCP tool, with its arguments as the input,
        // so the console's tool cards and approvals read it the same way.
        assert_eq!(
            approval.spec.tool_name.as_deref(),
            Some("mcp__openremote__agents_spawn")
        );
        assert_eq!(approval.request["input"], json!({"harness": "codex"}));
        assert_eq!(ids(&approval), ["accept", "acceptForSession", "decline"]);
        assert_eq!(
            answer_result("accept", &approval.request),
            json!({"action": "accept"})
        );
        assert_eq!(
            answer_result("acceptForSession", &approval.request),
            json!({"action": "accept", "_meta": {"persist": "session"}})
        );
        assert_eq!(
            answer_result("decline", &approval.request),
            json!({"action": "decline"})
        );

        // A server's own form cannot be filled in here: a no is all it gets.
        let form = asked(server_request(
            "mcpServer/elicitation/request",
            &json!({"serverName": "docs", "threadId": "t", "mode": "form", "message": "Your API key?",
                "requestedSchema": {"type": "object", "properties": {"key": {"type": "string"}}}}),
            &json!(4),
        ));
        assert_eq!(ids(&form), ["decline"]);
    }

    #[test]
    fn permissions_and_questions_answer_in_their_own_shape() {
        let permissions = json!({"fileSystem": {"write": ["/w/out"]}, "network": null});
        let ask_for = asked(server_request(
            "item/permissions/requestApproval",
            &json!({"cwd": "/w", "itemId": "i", "threadId": "t", "turnId": "u",
                "permissions": permissions, "reason": "write the build output", "startedAtMs": 1}),
            &json!(5),
        ));
        assert_eq!(
            answer_result("accept", &ask_for.request),
            json!({"permissions": permissions, "scope": "turn"})
        );
        assert_eq!(
            answer_result("decline", &ask_for.request),
            json!({"permissions": {}, "scope": "turn"})
        );

        let question = json!({"itemId": "i", "threadId": "t", "turnId": "u", "isBlocking": true,
            "questions": [{"id": "q1", "header": "DB", "question": "Which database?",
                "options": [{"label": "Postgres", "description": "x"}, {"label": "SQLite", "description": "y"}]}]});
        let approval = asked(server_request(
            "item/tool/requestUserInput",
            &question,
            &json!(6),
        ));
        assert_eq!(approval.spec.kind, DecisionKind::Question);
        assert_eq!(ids(&approval), ["Postgres", "SQLite"]);
        assert_eq!(
            answer_result("SQLite", &approval.request),
            json!({"answers": {"q1": {"answers": ["SQLite"]}}})
        );
        // Several questions, or one wanting typed words, cannot be put:
        // Codex is told at once rather than left waiting.
        let two = json!({"questions": [question["questions"][0].clone(), question["questions"][0].clone()]});
        assert!(matches!(
            server_request("item/tool/requestUserInput", &two, &json!(9)),
            Incoming::Refuse { .. }
        ));
    }

    #[test]
    fn a_request_nobody_here_serves_is_refused_not_left_waiting() {
        assert!(matches!(
            server_request("attestation/generate", &json!({}), &json!(2)),
            Incoming::Refuse {
                code: METHOD_NOT_FOUND,
                ..
            }
        ));
        // The first protocol's words, for the builds that still speak it.
        let argv = asked(server_request(
            "execCommandApproval",
            &json!({"callId": "c", "command": ["git", "status"], "cwd": "/w", "parsedCmd": []}),
            &json!("r-1"),
        ));
        assert_eq!(argv.harness_ref, "\"r-1\"");
        assert_eq!(argv.request["input"]["command"], "git status");
        assert_eq!(
            answer_result("accept", &argv.request),
            json!({"decision": "approved"})
        );
        assert_eq!(
            answer_result("cancel", &argv.request),
            json!({"decision": "abort"})
        );
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
