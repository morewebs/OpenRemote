//! Shared e2e plumbing: the fixture binary, a real daemon on a loopback
//! port, a raw HTTP client, SSE collection with diagnostics-in-panics.
//! Every harness's e2e suite builds on this.

#![allow(dead_code)]

pub mod cloud;

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex as StdMutex, OnceLock};
use std::time::Duration;

use openremote_core::Store;
use openremote_daemon::app::{self, App, AppOptions};
use openremote_daemon::plugins::HarnessSpec;
use openremote_daemon::registry::HarnessRegistry;
use openremote_daemon::supervisor::Supervisor;
use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};

// ---- the fixture binary ----

pub fn fixture_agent() -> PathBuf {
    // One build per test process: e2e tests run in parallel, and a
    // nested cargo per test once piled forty lock-waiting builds onto
    // the outer cargo's package-cache and build-dir locks on CI - one
    // lost the race and its "successful" build left no binary behind.
    static FIXTURE: OnceLock<PathBuf> = OnceLock::new();
    FIXTURE
        .get_or_init(|| {
            let target = std::env::var("CARGO_TARGET_DIR")
                .map(PathBuf::from)
                .unwrap_or_else(|_| Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target"));
            let name = if cfg!(windows) {
                "fixture-agent.exe"
            } else {
                "fixture-agent"
            };
            let path = target.join("debug").join(name);
            // CI builds the fixture up front; present means done, and no
            // nested cargo ever runs. Missing means a targeted local run:
            // build it (the rescue), then give the relink a moment. The
            // PATH cargo, not $CARGO: an env-derived executable is
            // command-injection-shaped, and this path is local-only.
            if !path.is_file() {
                let status = std::process::Command::new("cargo")
                    .args(["build", "-p", "fixture-agent"])
                    .status()
                    .expect("spawn cargo to build the fixture agent");
                assert!(status.success(), "cargo build -p fixture-agent failed");
                for _ in 0..20 {
                    if path.is_file() {
                        break;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(100));
                }
            }
            assert!(
                path.is_file(),
                "fixture agent binary not found at {}",
                path.display()
            );
            path
        })
        .clone()
}

// ---- the test daemon ----

pub struct TestDaemon {
    pub token: String,
    pub addr: std::net::SocketAddr,
    _data_dir: tempfile::TempDir,
    _child: Option<Box<DaemonChild>>,
}

impl TestDaemon {
    /// The daemon's data folder - where it keeps its state and plugins.
    pub fn data_dir(&self) -> &std::path::Path {
        self._data_dir.path()
    }

    /// The address-and-token view over a daemon this test spawned and
    /// reaps itself (the exit-sweep shape: the child never lives here,
    /// so dropping this kills nothing).
    pub fn from_parts(
        addr: std::net::SocketAddr,
        token: String,
        data_dir: tempfile::TempDir,
    ) -> Self {
        Self {
            token,
            addr,
            _data_dir: data_dir,
            _child: None,
        }
    }
}

/// A daemon spawned as its own process (the real binary, its own env).
/// Dropping kills it; its stdin closes first, which the daemon's own
/// watchdog honors anyway.
pub struct DaemonChild(std::process::Child, Option<std::process::ChildStdin>);

impl Drop for DaemonChild {
    fn drop(&mut self) {
        let _ = self.1.take();
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// The daemon binary cargo built for this test run.
pub fn daemon_binary() -> PathBuf {
    let target = std::env::var("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target"));
    let name = if cfg!(windows) {
        "openremote-daemon.exe"
    } else {
        "openremote-daemon"
    };
    let bin = target.join("debug").join(name);
    assert!(
        bin.is_file(),
        "daemon binary not found at {}",
        bin.display()
    );
    bin
}

/// Spawn the daemon binary as a real process with `extra_env` - for the
/// flows that ride process env (the install command override). The token
/// is pre-written into a fresh data dir; the address comes from the
/// binary's own `READY` line.
pub async fn spawn_daemon_process(extra_env: &[(&str, String)]) -> TestDaemon {
    let bin = daemon_binary();

    let data_dir = tempfile::tempdir().expect("temp data dir");
    let token = uuid::Uuid::new_v4().to_string();
    std::fs::create_dir_all(data_dir.path()).expect("data dir");
    std::fs::write(data_dir.path().join("token"), &token).expect("token file");

    let mut command = std::process::Command::new(&bin);
    command
        .env("OPENREMOTE_DATA_DIR", data_dir.path())
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null());
    for (key, value) in extra_env {
        command.env(key, value);
    }
    let mut child = command.spawn().expect("spawn daemon process");
    let stdout = child.stdout.take().expect("daemon stdout piped");
    let stdin = child.stdin.take();

    // The binary announces itself: `READY 127.0.0.1:<port>`, then
    // `DATA_DIR <path>`. Read the second line too - dropping the stdout
    // reader with the pipe still holding unread output is what kills the
    // daemon (the next println! hits EPIPE and panics the process).
    // The reads block with no bound of their own, so they run on a
    // thread and this side waits with a deadline: a daemon that never
    // finishes its handshake fails the test fast instead of hanging it
    // for the rest of the job.
    use std::io::BufRead;
    let (tx, rx) = std::sync::mpsc::channel::<Result<std::net::SocketAddr, String>>();
    let handshake = std::thread::spawn(move || {
        let result = (|| -> Result<std::net::SocketAddr, String> {
            let mut addr = None;
            let mut lines = std::io::BufReader::new(stdout).lines();
            for line in lines.by_ref() {
                let line = match line {
                    Ok(line) => line,
                    Err(err) => return Err(format!("daemon stdout: {err}")),
                };
                if let Some(rest) = line.strip_prefix("READY ") {
                    addr = rest.parse::<std::net::SocketAddr>().ok();
                    break;
                }
            }
            let addr = addr.ok_or("stdout closed before READY")?;
            // Drain exactly the DATA_DIR line: empty the pipe before the
            // reader drops. Reading past it would block - the daemon prints
            // nothing more on stdout until it exits (its token note goes to
            // stderr).
            for line in lines.by_ref() {
                match line {
                    Ok(line) if line.starts_with("DATA_DIR ") => break,
                    Ok(_) => continue,
                    Err(err) => return Err(format!("daemon stdout: {err}")),
                }
            }
            Ok(addr)
        })();
        let _ = tx.send(result);
    });
    let addr = match rx.recv_timeout(Duration::from_secs(30)) {
        Ok(Ok(addr)) => addr,
        Ok(Err(what)) => panic!("daemon handshake failed: {what}"),
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
            panic!("daemon never finished its READY/DATA_DIR handshake within 30s")
        }
        Err(_) => panic!("daemon handshake thread died"),
    };
    let _ = handshake.join();
    TestDaemon {
        token,
        addr,
        _data_dir: data_dir,
        _child: Some(Box::new(DaemonChild(child, stdin))),
    }
}

pub async fn start_daemon(overrides: &[(&str, PathBuf)]) -> TestDaemon {
    start_cloud_daemon(overrides, None).await
}

/// A daemon whose Cloud mode talks to `cloud` (the stand-in), or to a dead
/// address when there is none.
/// The Cloud settings the suites use when the feature under test is not Cloud
/// at all: an address that answers nothing, so a stray attempt fails fast and
/// loudly instead of reaching a real one.
pub fn dead_cloud() -> openremote_cloud::CloudConfig {
    openremote_cloud::CloudConfig {
        api_base: "http://127.0.0.1:9/v1/openremote".into(),
        auth_issuer: "http://127.0.0.1:9/v1/auth".into(),
        client_id: openremote_cloud::config::CLIENT_ID.into(),
        device_name: "test-device".into(),
        platform: std::env::consts::OS.into(),
        redirect_uri: None,
    }
}

/// The daemon plus a phone-pairing host whose LAN listener binds `bind`, with
/// the pairing control routes merged into the loopback router the way
/// `main.rs` merges them.
///
/// Only the pairing suite wants a second socket, but the bootstrap is the
/// shared part: the data folder, the owner token, the probe-to-serve handoff
/// of the loopback port. The registry is deliberately empty (a machine with no
/// harnesses), because the phone's home projection is asserted against exactly
/// that, and it is also why this path needs no fixture binary.
pub async fn start_pairing_daemon_over(
    data_dir: tempfile::TempDir,
    bind: std::net::SocketAddr,
    token: String,
    code_ttl_ms: i64,
) -> (
    TestDaemon,
    Arc<App>,
    Arc<openremote_daemon::pairing::PairingHost>,
) {
    let store = Arc::new(StdMutex::new(
        Store::open(data_dir.path().to_path_buf()).expect("store"),
    ));
    let supervisor = Supervisor::new(store.clone(), HarnessRegistry::none());
    let app = App::assemble(
        AppOptions {
            data_dir: data_dir.path().to_path_buf(),
            token: token.clone(),
            cloud: dead_cloud(),
        },
        store,
        supervisor,
    );
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("addr");
    app.supervisor.set_controls(
        PathBuf::from(env!("CARGO_BIN_EXE_openremote-daemon")),
        addr.port(),
    );
    let host =
        openremote_daemon::pairing::PairingHost::open_on(Arc::clone(&app), bind, code_ttl_ms);
    let router = openremote_daemon::http::router(Arc::clone(&app)).merge(host.control_router());
    tokio::spawn(async move {
        let _ = axum::serve(listener, router).await;
    });
    (
        TestDaemon {
            token,
            addr,
            _data_dir: data_dir,
            _child: None,
        },
        app,
        host,
    )
}

pub async fn start_cloud_daemon(
    overrides: &[(&str, PathBuf)],
    cloud: Option<openremote_cloud::CloudConfig>,
) -> TestDaemon {
    start_daemon_over(overrides, cloud, &[]).await
}

/// The daemon with the harnesses an installed plugin declares. The specs are
/// built by the test the way `App::new` builds them from the store; everything
/// after that is the real daemon, the real supervisor and the real driver
/// process.
pub async fn start_daemon_with_plugins(
    overrides: &[(&str, PathBuf)],
    plugins: &[HarnessSpec],
) -> TestDaemon {
    start_daemon_over(overrides, None, plugins).await
}

async fn start_daemon_over(
    overrides: &[(&str, PathBuf)],
    cloud: Option<openremote_cloud::CloudConfig>,
    plugins: &[HarnessSpec],
) -> TestDaemon {
    // Every harness slot defaults to the fixture agent, so the registry's
    // probes stay hermetic and cheap (no real CLIs get spawned from a
    // suite); a suite overrides only the harness it drives. The
    // "no such harness" path is still reachable - an id no slot fills.
    let fixture = fixture_agent();
    let mut filled: Vec<(&str, PathBuf)> = Vec::new();
    for id in ["claude", "codex", "grok", "pi", "opencode", "agy"] {
        if !overrides.iter().any(|(k, _)| *k == id) {
            filled.push((id, fixture.clone()));
        }
    }
    let overrides: std::collections::HashMap<String, PathBuf> = overrides
        .iter()
        .chain(filled.iter())
        .map(|(k, v)| ((*k).to_string(), v.clone()))
        .collect();
    let data_dir = tempfile::tempdir().expect("temp data dir");
    let token = uuid::Uuid::new_v4().to_string();
    let store = Arc::new(StdMutex::new(
        Store::open(data_dir.path().to_path_buf()).expect("store"),
    ));
    let supervisor = Supervisor::new(
        store.clone(),
        HarnessRegistry::probe(&overrides, plugins).await,
    );
    let app = App::assemble(
        AppOptions {
            data_dir: data_dir.path().to_path_buf(),
            token: token.clone(),
            cloud: cloud.unwrap_or_else(dead_cloud),
        },
        store,
        supervisor,
    );
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("addr");
    // Chats get the read-a-chat MCP server from this package's own daemon
    // binary, as a real daemon hands it its own path.
    app.supervisor.set_controls(
        PathBuf::from(env!("CARGO_BIN_EXE_openremote-daemon")),
        addr.port(),
    );
    tokio::spawn(app::serve(app, listener));
    TestDaemon {
        token,
        addr,
        _data_dir: data_dir,
        _child: None,
    }
}

/// The daemon as the Android app runs it: in-process, no harnesses. The
/// app itself is handed back too, for what the OS does to it (a sign-in's
/// return address, coming back to the front).
pub async fn start_embedded(cloud: openremote_cloud::CloudConfig) -> (TestDaemon, Arc<App>) {
    let data_dir = tempfile::tempdir().expect("temp data dir");
    let embedded = openremote_daemon::embedded::start(data_dir.path().to_path_buf(), cloud)
        .await
        .expect("embedded daemon");
    let addr = embedded
        .url
        .trim_start_matches("http://")
        .parse()
        .expect("loopback address");
    let daemon = TestDaemon {
        token: embedded.token,
        addr,
        _data_dir: data_dir,
        _child: None,
    };
    (daemon, embedded.app)
}

/// The scenario names the fixture binary on disk implements. Asked once per
/// test process: `cargo test` never rebuilds that binary, so the answer can be
/// older than the test asking.
fn fixture_scenarios(bin: &Path) -> Vec<String> {
    static KNOWN: OnceLock<Vec<String>> = OnceLock::new();
    KNOWN
        .get_or_init(|| {
            let out = std::process::Command::new(bin)
                .arg("--scenarios")
                .output()
                .unwrap_or_else(|e| panic!("ask {} for its scenarios: {e}", bin.display()));
            assert!(
                out.status.success(),
                "the fixture agent at {} could not list its scenarios ({}): {}",
                bin.display(),
                out.status,
                String::from_utf8_lossy(&out.stderr).trim()
            );
            String::from_utf8_lossy(&out.stdout)
                .lines()
                .map(str::trim)
                .filter(|line| !line.is_empty())
                .map(str::to_string)
                .collect()
        })
        .clone()
}

/// Refuse to drive a scenario the fixture binary on disk does not implement.
/// Without this check a test written against a newer scenario drives a turn the
/// old binary treats as plain, the event the test waits for never arrives, and
/// its own timeout reports a daemon regression that is not there.
fn require_scenario(bin: &Path, scenario: &str) {
    let known = fixture_scenarios(bin);
    assert!(
        known.iter().any(|name| name == scenario),
        "stale fixture-agent.exe at {}: it does not implement the '{scenario}' \
         scenario these tests drive (it lists {}). Run `cargo build -p \
         fixture-agent`; `cargo test` does not rebuild the fixture, and an old \
         one turns a scenario it never heard of into a plain turn that makes \
         this test wait out its timeout.",
        bin.display(),
        if known.is_empty() {
            "nothing, so it predates the --scenarios answer".to_string()
        } else {
            format!("only {}", known.join(", "))
        }
    );
}

/// A workspace the fixture agent runs in, with an optional scenario file.
pub fn workspace(scenario: Option<&str>) -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("temp workspace");
    if let Some(scenario) = scenario {
        require_scenario(&fixture_agent(), scenario);
        std::fs::write(dir.path().join("fixture-scenario"), scenario).expect("scenario file");
    }
    dir
}

// ---- a tiny HTTP/1.1 client (no extra deps) ----

#[derive(Debug)]
pub struct Reply {
    pub status: u16,
    pub body: Value,
    pub raw: String,
}

pub async fn call(daemon: &TestDaemon, method: &str, path: &str, body: Option<Value>) -> Reply {
    call_with_token(daemon, &daemon.token, method, path, body).await
}

/// A connection that died while the request was in flight - the class of
/// failure a replay survives (the peer closed before or while answering).
fn died_mid_flight(err: &std::io::Error) -> bool {
    matches!(
        err.kind(),
        std::io::ErrorKind::ConnectionReset
            | std::io::ErrorKind::ConnectionAborted
            | std::io::ErrorKind::BrokenPipe
    )
}

pub async fn call_with_token(
    daemon: &TestDaemon,
    token: &str,
    method: &str,
    path: &str,
    body: Option<Value>,
) -> Reply {
    let body = body.map(|b| b.to_string()).unwrap_or_default();
    let request = format!(
        "{method} {path} HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer {token}\r\n\
         Content-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );

    // A daemon that accepts but never answers fails the test in 30s
    // instead of hanging it for the rest of the job; a connection that
    // died mid-flight (a process daemon exiting under the test, the
    // runner's RST-on-close quirk) is retried. Safe to replay: every
    // mutating e2e call carries a request_id, so the receipt contract
    // dedups whatever did arrive.
    let mut resets = 0;
    let raw = loop {
        let shot = tokio::time::timeout(Duration::from_secs(30), async {
            let mut stream = tokio::net::TcpStream::connect(daemon.addr)
                .await
                .expect("connect");
            stream.write_all(request.as_bytes()).await?;
            let mut raw = Vec::new();
            stream.read_to_end(&mut raw).await?;
            Ok::<Vec<u8>, std::io::Error>(raw)
        })
        .await;
        match shot {
            Ok(Ok(raw)) => break raw,
            Ok(Err(err)) if died_mid_flight(&err) => {
                resets += 1;
                if resets >= 3 {
                    panic!("{method} {path}: the connection kept dying mid-flight ({err})");
                }
            }
            Ok(Err(err)) => panic!("{method} {path}: {err}"),
            Err(_) => panic!("{method} {path}: the daemon never answered within 30s"),
        }
    };
    let raw = String::from_utf8_lossy(&raw).to_string();
    let (head, rest) = raw
        .split_once("\r\n\r\n")
        .expect("response has a header block");
    let status: u16 = head
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|code| code.parse().ok())
        .unwrap_or(0);
    let body = serde_json::from_str(rest.trim_end_matches(['\r', '\n', '\0']))
        .unwrap_or(Value::String(rest.to_string()));
    Reply { status, body, raw }
}

// ---- SSE collection ----

/// Open an SSE subscription and collect events until `want` data frames
/// arrive or the timeout trips. Returns `(seq, payload)` pairs; on timeout
/// it panics with everything it did see (diagnostics ride with tests).
pub async fn sse_collect(daemon: &TestDaemon, path: &str, want: usize) -> Vec<(u64, Value)> {
    let timeout = Duration::from_secs(60);
    let mut collected: Vec<(u64, Value)> = Vec::new();
    match tokio::time::timeout(
        timeout,
        sse_collect_inner(daemon, path, want, &mut collected),
    )
    .await
    {
        Ok(events) => events,
        Err(_) => panic!(
            "sse collection timed out after {timeout:?} (wanted {want} events); seen so far:\n{}",
            dump(&collected)
        ),
    }
}

pub async fn sse_collect_inner(
    daemon: &TestDaemon,
    path: &str,
    want: usize,
    collected: &mut Vec<(u64, Value)>,
) -> Vec<(u64, Value)> {
    let mut stream = tokio::net::TcpStream::connect(daemon.addr)
        .await
        .expect("sse connect");
    let request = format!(
        "GET {path} HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer {}\r\nConnection: close\r\n\r\n",
        daemon.token
    );
    stream
        .write_all(request.as_bytes())
        .await
        .expect("sse request write");
    let (rx, _) = stream.split();
    let mut lines = BufReader::new(rx).lines();
    let mut in_body = false;
    let mut last_id: Option<u64> = None;
    while let Ok(Some(line)) = lines.next_line().await {
        if !in_body {
            if line.is_empty() {
                in_body = true;
            }
            continue;
        }
        if let Some(id) = line.strip_prefix("id: ") {
            last_id = id.trim().parse().ok();
        } else if line.starts_with("data: ") {
            let payload: Value =
                serde_json::from_str(line.trim_start_matches("data: ")).expect("sse data is json");
            let seq = last_id.expect("id precedes data");
            collected.push((seq, payload.clone()));
            if collected.len() >= want {
                return std::mem::take(collected);
            }
        }
    }
    std::mem::take(collected)
}

/// The event kind of a collected SSE payload.
pub fn kind(payload: &Value) -> String {
    payload
        .get("type")
        .and_then(|t| t.as_str())
        .unwrap_or("<missing>")
        .to_string()
}

pub fn dump(events: &[(u64, Value)]) -> String {
    events
        .iter()
        .map(|(seq, p)| format!("  {seq}: {}", kind(p)))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Collect until the event kinds match the expected suffix, then return
/// everything seen (diagnostics keep the full trace).
///
/// The match steps over incidental `session.updated` events: the
/// harness's model echo and settings fold ride as session.updated
/// whenever the driver reports them (mid-turn, timing loose from the
/// message stream), so an exact-suffix match races them on a loaded
/// runner. Positional expectations (codex's tool grant) list their own
/// session.updated and still hold - a match is always preferred over
/// stepping over one.
pub async fn until_kinds(daemon: &TestDaemon, path: &str, wanted: &[&str]) -> Vec<(u64, Value)> {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(60);
    let mut seen: Vec<(u64, Value)> = Vec::new();
    while tokio::time::Instant::now() < deadline {
        let need = seen.len() + 1;
        let batch = sse_collect(daemon, path, need).await;
        let kinds: Vec<String> = batch.iter().map(|(_, p)| kind(p)).collect();
        if suffix_with_slack(&kinds, wanted) {
            return batch;
        }
        if batch.len() == seen.len() {
            // no progress; wait a beat before re-reading
            tokio::time::sleep(Duration::from_millis(150)).await;
        }
        seen = batch;
        if seen.len() > 64 {
            panic!(
                "event stream never reached {wanted:?}; seen:\n{}",
                dump(&seen)
            );
        }
    }
    panic!(
        "timed out waiting for {wanted:?}; last seen:\n{}",
        dump(&seen)
    );
}

/// Whether `kinds` ends with `wanted`, stepping over any incidental
/// `session.updated` in the matched window.
fn suffix_with_slack(kinds: &[String], wanted: &[&str]) -> bool {
    // Walk both ends, matching from the tail; an unmatched
    // session.updated in `kinds` is stepped over, everything else fails.
    let mut ki = kinds.len();
    let mut wi = wanted.len();
    while wi > 0 {
        if ki == 0 {
            return false;
        }
        if kinds[ki - 1] == wanted[wi - 1] {
            ki -= 1;
            wi -= 1;
        } else if kinds[ki - 1] == "session.updated" {
            ki -= 1;
        } else {
            return false;
        }
    }
    true
}

/// Wait until the stream holds at least `count` events of one kind - for
/// flows where a suffix alone can match an older, identical tail.
pub async fn until_count(
    daemon: &TestDaemon,
    path: &str,
    wanted_kind: &str,
    count: usize,
) -> Vec<(u64, Value)> {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(60);
    let mut seen: Vec<(u64, Value)> = Vec::new();
    while tokio::time::Instant::now() < deadline {
        let need = seen.len() + 1;
        seen = sse_collect(daemon, path, need).await;
        let have = seen.iter().filter(|(_, p)| kind(p) == wanted_kind).count();
        if have >= count {
            return seen;
        }
        if seen.len() > 64 {
            panic!("never saw {count} × {wanted_kind}; seen:\n{}", dump(&seen));
        }
    }
    panic!(
        "timed out waiting for {count} × {wanted_kind}; last seen:\n{}",
        dump(&seen)
    );
}

/// Wait until the session's status settles to one of `wanted` - a poll of
/// the session itself, for the transitions the event stream cannot name
/// deterministically (a settle racing the drain window).
pub async fn until_status(daemon: &TestDaemon, id: &str, wanted: &[&str]) -> Value {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
    let mut last = Value::Null;
    while tokio::time::Instant::now() < deadline {
        let reply = call(daemon, "GET", &format!("/sessions/{id}"), None).await;
        last = reply.body.clone();
        if let Some(status) = reply.body["status"].as_str() {
            if wanted.contains(&status) {
                return reply.body;
            }
        }
        tokio::time::sleep(Duration::from_millis(150)).await;
    }
    panic!("session {id} never settled to {wanted:?}; last: {last}");
}

/// A raw HTTP request with arbitrary headers (CORS probes), returning
/// the parsed status plus every response header line.
pub struct RawReply {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub raw: String,
}

impl RawReply {
    /// The answer's body as text, with the trailing line endings and NULs
    /// trimmed: what a reader of the response would call its content.
    pub fn text(&self) -> &str {
        self.raw
            .split_once("\r\n\r\n")
            .map(|(_, body)| body.trim_end_matches(['\r', '\n', '\0']))
            .unwrap_or_default()
    }

    /// The body as JSON. A body that is not JSON becomes `Null` rather than a
    /// panic: what a caller is asserting about is the field it then indexes.
    pub fn json(&self) -> Value {
        serde_json::from_str(self.text()).unwrap_or(Value::Null)
    }

    /// One response header, case-insensitively, as a reader would ask for it.
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }
}

pub async fn raw_http(
    daemon: &TestDaemon,
    method: &str,
    path: &str,
    headers: &[(&str, &str)],
    body: Option<String>,
) -> RawReply {
    try_raw_http(
        daemon.addr,
        method,
        path,
        Some(&daemon.token),
        headers,
        body.as_deref(),
    )
    .await
    .unwrap_or_else(|| panic!("{method} {path}: the daemon never answered"))
}

/// A raw request to any address, with no bearer token when `bearer` is `None`.
///
/// `None` back means the peer did not answer at all, which is exactly what the
/// assertions about a socket that should be closed are about; a plain
/// refused connection is not a test failure there, it is the expected result.
/// The timeout still panics: a peer that accepts the connection and then says
/// nothing is a wedged daemon, not a closed port.
pub async fn try_raw_http(
    addr: std::net::SocketAddr,
    method: &str,
    path: &str,
    bearer: Option<&str>,
    headers: &[(&str, &str)],
    body: Option<&str>,
) -> Option<RawReply> {
    let body = body.unwrap_or_default();
    let mut request = format!("{method} {path} HTTP/1.1\r\nHost: localhost\r\n");
    if let Some(token) = bearer {
        request.push_str(&format!("Authorization: Bearer {token}\r\n"));
    }
    for (key, value) in headers {
        request.push_str(&format!("{key}: {value}\r\n"));
    }
    request.push_str(&format!(
        "Content-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    ));
    let mut stream = tokio::net::TcpStream::connect(addr).await.ok()?;
    // A peer that drops the connection before or while we write has not
    // answered, which is what a socket with nothing behind it looks like on
    // the way out.
    stream.write_all(request.as_bytes()).await.ok()?;
    let mut raw = Vec::new();
    // Bounded like call_with_token: a wedged daemon fails the test in
    // 30s instead of hanging it for the rest of the job.
    let read = stream.read_to_end(&mut raw);
    match tokio::time::timeout(Duration::from_secs(30), read).await {
        Ok(result) => {
            result.ok()?;
        }
        Err(_) => panic!("{method} {path}: the daemon never answered within 30s"),
    }
    let raw = String::from_utf8_lossy(&raw).to_string();
    let (head, _) = raw.split_once("\r\n\r\n").expect("header block");
    let status: u16 = head
        .lines()
        .next()
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|c| c.parse().ok())
        .unwrap_or(0);
    let headers = head
        .lines()
        .skip(1)
        .filter_map(|l| l.split_once(':'))
        .map(|(k, v)| (k.trim().to_string(), v.trim().to_string()))
        .collect();
    Some(RawReply {
        status,
        headers,
        raw,
    })
}
