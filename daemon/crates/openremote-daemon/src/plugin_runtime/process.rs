//! Process plugins: one program per plugin per machine, speaking
//! OpenRemote's plugin protocol (newline-delimited JSON-RPC 2.0 over its
//! stdin and stdout). A plugin starts the first time something needs it -
//! an agent's tool call, a view's request - and never before. (A harness
//! plugin's driver is main's `plugin_driver`, one child per chat; it is
//! not this program.) A plugin that stops is started again
//! on the next need; one that keeps stopping waits longer each time (1 s,
//! 2 s, 4 s ... up to a minute), so a broken plugin can't spin the daemon.
//!
//! A process plugin runs as the user, not sandboxed: the user approved
//! that at install (`run_programs`), and its program is a file inside its
//! own folder. Its settings, secrets included, reach it in the
//! `initialize` call and nowhere else - never its command line or its
//! environment, which other programs can read, and never a log.

use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex as StdMutex, OnceLock, Weak};
use std::time::{Duration, Instant};

use futures_util::future::BoxFuture;
use openremote_core::{PluginPower, PluginScope};
use serde_json::{Map, Value, json};
use tokio::io::{AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin};
use tokio::sync::{Mutex as AsyncMutex, oneshot};

/// The protocol version this daemon speaks.
pub const PROTOCOL: u64 = 1;

/// One line from a plugin may carry a tool result with pictures in it.
const MAX_LINE: usize = 16 * 1024 * 1024;
/// How long a plugin has to answer `initialize`.
const HANDSHAKE: Duration = Duration::from_secs(20);
/// A plugin that ran this long before it stopped was working: its next
/// start waits only the first step again.
const STABLE: Duration = Duration::from_secs(60);
const MAX_BACKOFF: Duration = Duration::from_secs(60);
/// A plugin's log line is cut here on the daemon's stderr.
const LOG_LINE: usize = 2000;

/// A JSON-RPC error: its code and its words.
#[derive(Debug, Clone)]
pub struct RpcError {
    pub code: i64,
    pub message: String,
}

impl RpcError {
    pub fn new(code: i64, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

impl std::fmt::Display for RpcError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

/// Everything needed to start one plugin's program.
pub struct StartSpec {
    pub id: String,
    pub name: String,
    pub version: Option<String>,
    pub root: PathBuf,
    /// A folder of its own for whatever it keeps.
    pub data: PathBuf,
    pub program: PathBuf,
    pub args: Vec<String>,
    /// The user's values, secrets included.
    pub settings: Map<String, Value>,
    pub powers: Vec<PluginPower>,
    pub scope: PluginScope,
}

/// What a plugin may ask OpenRemote (its own calls the other way), and
/// who answers: the runtime, which checks the plugin's powers.
pub type Requests =
    Arc<dyn Fn(String, String, Value) -> BoxFuture<'static, Result<Value, RpcError>> + Send + Sync>;

/// One running plugin program.
pub struct Conn {
    id: String,
    stdin: AsyncMutex<ChildStdin>,
    child: AsyncMutex<Child>,
    pending: StdMutex<HashMap<u64, oneshot::Sender<Result<Value, RpcError>>>>,
    next_id: AtomicU64,
    alive: AtomicBool,
    /// Stopped on purpose (turned off, updated, removed): not a crash.
    stopping: AtomicBool,
    started: Instant,
}

impl Conn {
    pub fn is_alive(&self) -> bool {
        self.alive.load(Ordering::SeqCst)
    }

    async fn write(&self, frame: &Value) -> Result<(), RpcError> {
        let mut line = serde_json::to_string(frame).expect("a frame serializes");
        line.push('\n');
        let mut stdin = self.stdin.lock().await;
        stdin
            .write_all(line.as_bytes())
            .await
            .map_err(|_| RpcError::new(-32000, "the plugin stopped"))?;
        stdin
            .flush()
            .await
            .map_err(|_| RpcError::new(-32000, "the plugin stopped"))
    }

    /// Call one of the plugin's methods and wait for its answer.
    pub async fn call(
        &self,
        method: &str,
        params: Value,
        timeout: Duration,
    ) -> Result<Value, RpcError> {
        if !self.is_alive() {
            return Err(RpcError::new(-32000, "the plugin stopped"));
        }
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let (tx, rx) = oneshot::channel();
        self.pending
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(id, tx);
        let frame = json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params});
        if let Err(e) = self.write(&frame).await {
            self.abandon(id);
            return Err(e);
        }
        match tokio::time::timeout(timeout, rx).await {
            Ok(Ok(answer)) => answer,
            Ok(Err(_)) => Err(RpcError::new(-32000, "the plugin stopped")),
            Err(_) => {
                self.abandon(id);
                Err(RpcError::new(
                    -32001,
                    format!(
                        "the plugin didn't answer {method} within {}s",
                        timeout.as_secs()
                    ),
                ))
            }
        }
    }

    /// Tell the plugin something that wants no answer.
    pub async fn notify(&self, method: &str, params: Value) -> Result<(), RpcError> {
        self.write(&json!({"jsonrpc": "2.0", "method": method, "params": params}))
            .await
    }

    fn abandon(&self, id: u64) {
        self.pending
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&id);
    }

    async fn kill(&self) {
        self.stopping.store(true, Ordering::SeqCst);
        let mut child = self.child.lock().await;
        let _ = child.start_kill();
        let _ = tokio::time::timeout(Duration::from_secs(5), child.wait()).await;
    }
}

#[derive(Default)]
struct SlotState {
    conn: Option<Arc<Conn>>,
    /// Stops in a row that came before the plugin settled.
    crashes: u32,
    /// No start before this, after a crash.
    retry_at: Option<Instant>,
    last_error: Option<String>,
}

#[derive(Default)]
struct Slot {
    state: AsyncMutex<SlotState>,
}

/// How a plugin's program stands, for the Plugins page.
#[derive(serde::Serialize, Debug)]
pub struct ProcessStatus {
    /// `running`, `stopped` (not started yet, or stopped on purpose) or
    /// `waiting` (it stopped by itself and starts again after a pause).
    pub state: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// The plugin programs on this machine.
#[derive(Default)]
pub struct ProcessHost {
    slots: StdMutex<HashMap<String, Arc<Slot>>>,
    requests: OnceLock<Requests>,
}

impl ProcessHost {
    pub fn set_requests(&self, requests: Requests) {
        let _ = self.requests.set(requests);
    }

    fn slot(&self, id: &str) -> Arc<Slot> {
        Arc::clone(
            self.slots
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .entry(id.to_string())
                .or_default(),
        )
    }

    /// The plugin's running program, started now if it isn't - unless it
    /// stopped by itself moments ago and its pause isn't over.
    pub async fn connection(
        &self,
        id: &str,
        spec: impl FnOnce() -> Result<StartSpec, String>,
    ) -> Result<Arc<Conn>, String> {
        let slot = self.slot(id);
        let mut state = slot.state.lock().await;
        if let Some(conn) = state.conn.as_ref().filter(|c| c.is_alive()) {
            return Ok(Arc::clone(conn));
        }
        if let Some(at) = state.retry_at {
            let now = Instant::now();
            if at > now {
                return Err(format!(
                    "the plugin stopped {} time{} in a row ({}); it starts again in {}s",
                    state.crashes,
                    if state.crashes == 1 { "" } else { "s" },
                    state.last_error.as_deref().unwrap_or("no reason given"),
                    (at - now).as_secs().max(1)
                ));
            }
        }
        let spec = spec()?;
        match start(spec, self.requests.get().cloned(), Arc::downgrade(&slot)).await {
            Ok(conn) => {
                state.conn = Some(Arc::clone(&conn));
                state.last_error = None;
                Ok(conn)
            }
            Err(e) => {
                state.crashes += 1;
                state.retry_at = Some(Instant::now() + backoff(state.crashes));
                state.last_error = Some(e.clone());
                Err(e)
            }
        }
    }

    /// The program if it is running now; never starts one.
    pub async fn running(&self, id: &str) -> Option<Arc<Conn>> {
        let slot = self
            .slots
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(id)
            .cloned()?;
        let state = slot.state.lock().await;
        state.conn.as_ref().filter(|c| c.is_alive()).cloned()
    }

    /// Stop a plugin's program (turned off, updated, removed, or its
    /// settings need a fresh start). Its next need starts it again, with
    /// no pause: this was no crash.
    pub async fn stop(&self, id: &str) {
        let slot = self
            .slots
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(id)
            .cloned();
        let Some(slot) = slot else {
            return;
        };
        let conn = {
            let mut state = slot.state.lock().await;
            state.crashes = 0;
            state.retry_at = None;
            state.last_error = None;
            state.conn.take()
        };
        if let Some(conn) = conn {
            conn.kill().await;
        }
    }

    /// The daemon is exiting: no plugin program outlives it.
    pub async fn stop_all(&self) {
        let ids: Vec<String> = self
            .slots
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .keys()
            .cloned()
            .collect();
        for id in ids {
            self.stop(&id).await;
        }
    }

    pub fn status(&self, id: &str) -> ProcessStatus {
        let slot = self
            .slots
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(id)
            .cloned();
        let Some(slot) = slot else {
            return ProcessStatus {
                state: "stopped",
                error: None,
            };
        };
        // Never wait on a start in progress: the page asks often.
        let Ok(state) = slot.state.try_lock() else {
            return ProcessStatus {
                state: "running",
                error: None,
            };
        };
        if state.conn.as_ref().is_some_and(|c| c.is_alive()) {
            return ProcessStatus {
                state: "running",
                error: None,
            };
        }
        ProcessStatus {
            state: if state.retry_at.is_some_and(|at| at > Instant::now()) {
                "waiting"
            } else {
                "stopped"
            },
            error: state.last_error.clone(),
        }
    }
}

fn backoff(crashes: u32) -> Duration {
    Duration::from_secs(1u64 << crashes.saturating_sub(1).min(6)).min(MAX_BACKOFF)
}

async fn start(
    spec: StartSpec,
    requests: Option<Requests>,
    slot: Weak<Slot>,
) -> Result<Arc<Conn>, String> {
    let _ = openremote_core::fsx::create_private_dir(&spec.data);
    let mut command = tokio::process::Command::new(&spec.program);
    command
        .args(&spec.args)
        .current_dir(&spec.root)
        .env("OPENREMOTE_PLUGIN_ID", &spec.id)
        .env("OPENREMOTE_PLUGIN_ROOT", &spec.root)
        .env("OPENREMOTE_PLUGIN_DATA", &spec.data)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    // Windows: no console window per plugin, and no Ctrl-C from a shared
    // console; the why lives on openremote_harness::creation_flags.
    #[cfg(windows)]
    command.creation_flags(openremote_harness::creation_flags());
    let mut child = command
        .spawn()
        .map_err(|e| format!("could not start {}: {e}", spec.program.display()))?;
    let (Some(stdin), Some(stdout), Some(stderr)) =
        (child.stdin.take(), child.stdout.take(), child.stderr.take())
    else {
        let _ = child.start_kill();
        return Err("the plugin's pipes did not open".into());
    };
    let conn = Arc::new(Conn {
        id: spec.id.clone(),
        stdin: AsyncMutex::new(stdin),
        child: AsyncMutex::new(child),
        pending: StdMutex::new(HashMap::new()),
        next_id: AtomicU64::new(1),
        alive: AtomicBool::new(true),
        stopping: AtomicBool::new(false),
        started: Instant::now(),
    });
    tokio::spawn(read_stdout(Arc::clone(&conn), stdout, requests, slot));
    tokio::spawn(read_stderr(spec.id.clone(), stderr));
    let hello = json!({
        "protocol": PROTOCOL,
        "openremote": env!("CARGO_PKG_VERSION"),
        "plugin": {
            "id": spec.id,
            "name": spec.name,
            "version": spec.version,
            "root": spec.root,
            "data": spec.data,
            "scope": spec.scope,
        },
        "powers": spec.powers,
        "settings": spec.settings,
        "platform": super::manifest::this_system(),
    });
    let answer = conn.call("initialize", hello, HANDSHAKE).await;
    let spoken = match answer {
        Ok(reply) => reply.get("protocol").and_then(Value::as_u64),
        Err(e) => {
            conn.kill().await;
            return Err(format!("the plugin did not start: {e}"));
        }
    };
    if spoken != Some(PROTOCOL) {
        conn.kill().await;
        return Err(format!(
            "the plugin speaks protocol {}, this OpenRemote speaks {PROTOCOL}",
            spoken.map_or("(none named)".into(), |v| v.to_string())
        ));
    }
    eprintln!("plugin {}: started", spec.id);
    Ok(conn)
}

async fn read_stdout(
    conn: Arc<Conn>,
    stdout: tokio::process::ChildStdout,
    requests: Option<Requests>,
    slot: Weak<Slot>,
) {
    let mut reader = openremote_harness::LineReader::with_cap(BufReader::new(stdout), MAX_LINE);
    while let Ok(Some(line)) = reader.next_line().await {
        if line.trim().is_empty() {
            continue;
        }
        let Ok(message) = serde_json::from_str::<Value>(&line) else {
            eprintln!(
                "plugin {}: a line that is not JSON-RPC was ignored ({} bytes)",
                conn.id,
                line.len()
            );
            continue;
        };
        let method = message.get("method").and_then(Value::as_str);
        let params = message.get("params").cloned().unwrap_or(Value::Null);
        match (method, message.get("id").cloned()) {
            // The plugin asks OpenRemote something.
            (Some(method), Some(id)) => {
                let conn = Arc::clone(&conn);
                let method = method.to_string();
                let requests = requests.clone();
                tokio::spawn(async move {
                    let answer = match requests {
                        Some(answer) => answer(conn.id.clone(), method, params).await,
                        None => Err(RpcError::new(-32601, "OpenRemote answers nothing yet")),
                    };
                    let frame = match answer {
                        Ok(result) => json!({"jsonrpc": "2.0", "id": id, "result": result}),
                        Err(e) => {
                            json!({"jsonrpc": "2.0", "id": id, "error": {"code": e.code, "message": e.message}})
                        }
                    };
                    let _ = conn.write(&frame).await;
                });
            }
            (Some(method), None) => notification(&conn, method, &params),
            // An answer to one of OpenRemote's calls.
            (None, Some(id)) => {
                let Some(id) = id.as_u64() else {
                    continue;
                };
                let answer = match message.get("error") {
                    Some(error) => Err(RpcError::new(
                        error.get("code").and_then(Value::as_i64).unwrap_or(-32000),
                        error
                            .get("message")
                            .and_then(Value::as_str)
                            .unwrap_or("the plugin gave no reason"),
                    )),
                    None => Ok(message.get("result").cloned().unwrap_or(Value::Null)),
                };
                let waiter = conn
                    .pending
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .remove(&id);
                if let Some(waiter) = waiter {
                    let _ = waiter.send(answer);
                }
            }
            (None, None) => {}
        }
    }
    gone(conn, slot).await;
}

/// The program ended: every call in flight fails, and - unless it was stopped on purpose - the slot remembers the
/// crash for the next start's pause.
async fn gone(conn: Arc<Conn>, slot: Weak<Slot>) {
    conn.alive.store(false, Ordering::SeqCst);
    let waiters: Vec<_> = conn
        .pending
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .drain()
        .collect();
    for (_, waiter) in waiters {
        let _ = waiter.send(Err(RpcError::new(-32000, "the plugin stopped")));
    }
    let status = {
        let mut child = conn.child.lock().await;
        match tokio::time::timeout(Duration::from_secs(5), child.wait()).await {
            Ok(Ok(status)) => status.to_string(),
            _ => "no exit status".into(),
        }
    };
    let on_purpose = conn.stopping.load(Ordering::SeqCst);
    eprintln!(
        "plugin {}: stopped ({status}){}",
        conn.id,
        if on_purpose { "" } else { " by itself" }
    );
    if on_purpose {
        return;
    }
    if let Some(slot) = slot.upgrade() {
        let mut state = slot.state.lock().await;
        if state.conn.as_ref().is_some_and(|c| Arc::ptr_eq(c, &conn)) {
            state.conn = None;
            state.crashes = if conn.started.elapsed() >= STABLE {
                1
            } else {
                state.crashes + 1
            };
            state.retry_at = Some(Instant::now() + backoff(state.crashes));
            state.last_error = Some(format!("it exited with {status}"));
        }
    }
}

fn notification(conn: &Conn, method: &str, params: &Value) {
    match method {
        "log" => {
            let level = params
                .get("level")
                .and_then(Value::as_str)
                .unwrap_or("info");
            let message = params
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or_default();
            eprintln!("plugin {} [{level}]: {}", conn.id, cut(message));
        }
        other => eprintln!(
            "plugin {}: sent '{other}', which this OpenRemote doesn't know",
            conn.id
        ),
    }
}

async fn read_stderr(id: String, stderr: tokio::process::ChildStderr) {
    let mut reader = openremote_harness::LineReader::new(BufReader::new(stderr));
    while let Ok(Some(line)) = reader.next_line().await {
        if !line.trim().is_empty() {
            eprintln!("plugin {id}: {}", cut(&line));
        }
    }
}

fn cut(line: &str) -> String {
    if line.chars().count() > LOG_LINE {
        let mut out: String = line.chars().take(LOG_LINE).collect();
        out.push_str(" ...");
        out
    } else {
        line.to_string()
    }
}
