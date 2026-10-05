//! Shared e2e plumbing: the fixture binary, a real daemon on a loopback
//! port, a raw HTTP client, SSE collection with diagnostics-in-panics.
//! Every harness's e2e suite builds on this.

#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex as StdMutex};
use std::time::Duration;

use openremote_core::Store;
use openremote_daemon::app::{self, App};
use openremote_daemon::registry::HarnessRegistry;
use openremote_daemon::supervisor::Supervisor;
use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};

// ---- the fixture binary ----

pub fn fixture_agent() -> PathBuf {
    let target = std::env::var("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target"));
    let name = if cfg!(windows) {
        "fixture-agent.exe"
    } else {
        "fixture-agent"
    };
    let path = target.join("debug").join(name);
    // Always build: a no-op when fresh, a rescue when a targeted test run
    // would otherwise drive a stale fixture from an earlier build.
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());
    let status = std::process::Command::new(cargo)
        .args(["build", "-p", "fixture-agent"])
        .status()
        .expect("spawn cargo to build the fixture agent");
    assert!(status.success(), "cargo build -p fixture-agent failed");
    assert!(
        path.is_file(),
        "fixture agent binary not found at {}",
        path.display()
    );
    path
}

// ---- the test daemon ----

pub struct TestDaemon {
    pub token: String,
    pub addr: std::net::SocketAddr,
    _data_dir: tempfile::TempDir,
    _child: Option<Box<DaemonChild>>,
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

/// Spawn the daemon binary as a real process with `extra_env` - for the
/// flows that ride process env (the install command override). The token
/// is pre-written into a fresh data dir; the address comes from the
/// binary's own `READY` line.
pub async fn spawn_daemon_process(extra_env: &[(&str, String)]) -> TestDaemon {
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

    // The binary announces itself: `READY 127.0.0.1:<port>`.
    use std::io::BufRead;
    let mut addr = None;
    for line in std::io::BufReader::new(stdout).lines() {
        let line = line.expect("daemon stdout");
        if let Some(rest) = line.strip_prefix("READY ") {
            addr = rest.parse::<std::net::SocketAddr>().ok();
            break;
        }
    }
    let addr = addr.expect("daemon printed READY with an address");
    TestDaemon {
        token,
        addr,
        _data_dir: data_dir,
        _child: Some(Box::new(DaemonChild(child, stdin))),
    }
}

pub async fn start_daemon(overrides: &[(&str, PathBuf)]) -> TestDaemon {
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
    let supervisor = Supervisor::new(store.clone(), HarnessRegistry::probe(&overrides).await);
    let app = Arc::new(App {
        token: token.clone(),
        data_dir: data_dir.path().to_path_buf(),
        store,
        supervisor,
    });
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("addr");
    tokio::spawn(app::serve(app, listener));
    TestDaemon {
        token,
        addr,
        _data_dir: data_dir,
        _child: None,
    }
}

/// A workspace the fixture agent runs in, with an optional scenario file.
pub fn workspace(scenario: Option<&str>) -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("temp workspace");
    if let Some(scenario) = scenario {
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

pub async fn call_with_token(
    daemon: &TestDaemon,
    token: &str,
    method: &str,
    path: &str,
    body: Option<Value>,
) -> Reply {
    let mut stream = tokio::net::TcpStream::connect(daemon.addr)
        .await
        .expect("connect");
    let body = body.map(|b| b.to_string()).unwrap_or_default();
    let request = format!(
        "{method} {path} HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer {token}\r\n\
         Content-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream
        .write_all(request.as_bytes())
        .await
        .expect("write request");
    let mut raw = Vec::new();
    stream.read_to_end(&mut raw).await.expect("read response");
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

/// A raw HTTP request with arbitrary headers (CORS probes), returning
/// the parsed status plus every response header line.
pub struct RawReply {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub raw: String,
}

pub async fn raw_http(
    daemon: &TestDaemon,
    method: &str,
    path: &str,
    headers: &[(&str, &str)],
    body: Option<String>,
) -> RawReply {
    let mut stream = tokio::net::TcpStream::connect(daemon.addr)
        .await
        .expect("connect");
    let body = body.unwrap_or_default();
    let mut request = format!(
        "{method} {path} HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer {}\r\n",
        daemon.token
    );
    for (key, value) in headers {
        request.push_str(&format!("{key}: {value}\r\n"));
    }
    request.push_str(&format!(
        "Content-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    ));
    stream.write_all(request.as_bytes()).await.expect("write");
    let mut raw = Vec::new();
    stream.read_to_end(&mut raw).await.expect("read");
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
    RawReply {
        status,
        headers,
        raw,
    }
}
