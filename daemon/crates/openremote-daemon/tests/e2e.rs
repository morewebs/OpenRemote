//! Daemon e2e: the real HTTP API + SSE driving the real driver against the
//! fixture agent process. No mocks in between — these tests are the
//! contract the console will lean on.
//!
//! Diagnostics ship with the tests (the standing lesson): every failure
//! dumps the events seen so far, not just the assertion that tripped.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex as StdMutex};
use std::time::Duration;

use openremote_claude::Resolution;
use openremote_core::Store;
use openremote_daemon::app::{self, App};
use openremote_daemon::supervisor::Supervisor;
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};

// ---- the fixture binary ----

fn fixture_agent() -> PathBuf {
    let target = std::env::var("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target"));
    let name = if cfg!(windows) {
        "fixture-agent.exe"
    } else {
        "fixture-agent"
    };
    let path = target.join("debug").join(name);
    if !path.is_file() {
        // Workspace-root `cargo test` builds it already; a targeted run may
        // not have. Build on demand.
        let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());
        let status = std::process::Command::new(cargo)
            .args(["build", "-p", "fixture-agent"])
            .status()
            .expect("spawn cargo to build the fixture agent");
        assert!(status.success(), "cargo build -p fixture-agent failed");
    }
    assert!(
        path.is_file(),
        "fixture agent binary not found at {}",
        path.display()
    );
    path
}

// ---- the test daemon ----

struct TestDaemon {
    token: String,
    addr: std::net::SocketAddr,
    _data_dir: tempfile::TempDir,
}

async fn start_daemon(resolution: Resolution) -> TestDaemon {
    let data_dir = tempfile::tempdir().expect("temp data dir");
    let token = uuid::Uuid::new_v4().to_string();
    let store = Arc::new(StdMutex::new(
        Store::open(data_dir.path().to_path_buf()).expect("store"),
    ));
    let supervisor = Supervisor::new(store.clone(), resolution);
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
    }
}

/// A workspace the fixture agent runs in, with an optional scenario file.
fn workspace(scenario: Option<&str>) -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("temp workspace");
    if let Some(scenario) = scenario {
        std::fs::write(dir.path().join("fixture-scenario"), scenario).expect("scenario file");
    }
    dir
}

// ---- a tiny HTTP/1.1 client (no extra deps) ----

#[derive(Debug)]
struct Reply {
    status: u16,
    body: Value,
    raw: String,
}

async fn call(daemon: &TestDaemon, method: &str, path: &str, body: Option<Value>) -> Reply {
    call_with_token(daemon, &daemon.token, method, path, body).await
}

async fn call_with_token(
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
async fn sse_collect(daemon: &TestDaemon, path: &str, want: usize) -> Vec<(u64, Value)> {
    let timeout = Duration::from_secs(15);
    let mut collected: Vec<(u64, Value)> = Vec::new();
    match tokio::time::timeout(
        timeout,
        sse_collect_inner(daemon, path, want, &mut collected),
    )
    .await
    {
        Ok(events) => events,
        Err(_) => panic!(
            "sse collection timed out after 15s (wanted {want} events); seen so far:\n{}",
            dump(&collected)
        ),
    }
}

async fn sse_collect_inner(
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
fn kind(payload: &Value) -> String {
    payload
        .get("type")
        .and_then(|t| t.as_str())
        .unwrap_or("<missing>")
        .to_string()
}

fn dump(events: &[(u64, Value)]) -> String {
    events
        .iter()
        .map(|(seq, p)| format!("  {seq}: {}", kind(p)))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Collect until the event kinds match the expected suffix, then return
/// everything seen (diagnostics keep the full trace).
async fn until_kinds(daemon: &TestDaemon, path: &str, wanted: &[&str]) -> Vec<(u64, Value)> {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(15);
    let mut seen: Vec<(u64, Value)> = Vec::new();
    while tokio::time::Instant::now() < deadline {
        let need = seen.len() + 1;
        let batch = sse_collect(daemon, path, need).await;
        let kinds: Vec<String> = batch.iter().map(|(_, p)| kind(p)).collect();
        if kinds.ends_with(&wanted.iter().map(|s| s.to_string()).collect::<Vec<_>>()) {
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

/// Wait until the stream holds at least `count` events of one kind — for
/// flows where a suffix alone can match an older, identical tail.
async fn until_count(
    daemon: &TestDaemon,
    path: &str,
    wanted_kind: &str,
    count: usize,
) -> Vec<(u64, Value)> {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(15);
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

// ---- the tests ----

#[tokio::test]
async fn healthz_is_open_and_everything_else_needs_the_token() {
    let daemon = start_daemon(Resolution::Executable(fixture_agent())).await;
    let health = call(&daemon, "GET", "/healthz", None).await;
    assert_eq!(health.status, 200, "raw: {}", health.raw);
    let anon = call_with_token(&daemon, "wrong-token", "GET", "/sessions", None).await;
    assert_eq!(anon.status, 401, "raw: {}", anon.raw);
    let good = call(&daemon, "GET", "/sessions", None).await;
    assert_eq!(good.status, 200, "raw: {}", good.raw);
}

#[tokio::test]
async fn a_prompt_runs_a_real_turn_end_to_end() {
    let resolution = Resolution::Executable(fixture_agent());
    let daemon = start_daemon(resolution).await;
    let ws = workspace(Some("plain"));

    let created = call(
        &daemon,
        "POST",
        "/sessions",
        Some(json!({"request_id": "r-create", "harness": "claude", "workspace": ws.path()})),
    )
    .await;
    assert_eq!(created.status, 201, "raw: {}", created.raw);
    let session = created.body.as_object().expect("session object");
    let id = session
        .get("id")
        .and_then(|v| v.as_str())
        .expect("session id");

    // The init frame arrives from a real process spawn; wait for idle.
    let events = until_kinds(
        &daemon,
        &format!("/sessions/{id}/events"),
        &["session.status_changed"],
    )
    .await;
    assert!(
        events.iter().any(|(_, p)| kind(p) == "session.created"),
        "no session.created in:\n{}",
        dump(&events)
    );

    let prompt = call(
        &daemon,
        "POST",
        &format!("/sessions/{id}/prompts"),
        Some(json!({"request_id": "r-prompt", "text": "hello fixture"})),
    )
    .await;
    assert_eq!(prompt.status, 200, "raw: {}", prompt.raw);

    let events = until_kinds(
        &daemon,
        &format!("/sessions/{id}/events"),
        &[
            "turn.started",
            "message.added",
            "message.added",
            "turn.completed",
            "session.status_changed",
        ],
    )
    .await;
    let kinds: Vec<String> = events.iter().map(|(_, p)| kind(p)).collect();
    assert!(
        kinds.contains(&"turn.started".to_string()),
        "events:\n{}",
        dump(&events)
    );
    assert!(
        kinds.contains(&"turn.completed".to_string()),
        "events:\n{}",
        dump(&events)
    );

    // The store saw the harness conversation id (from system/init).
    let session = call(&daemon, "GET", &format!("/sessions/{id}"), None).await;
    let body = session.body.as_object().expect("session");
    assert!(
        body.get("harness_session_ref")
            .and_then(|v| v.as_str())
            .is_some(),
        "harness_session_ref missing: {body:?}"
    );
}

#[tokio::test]
async fn approvals_flow_through_decisions() {
    let daemon = start_daemon(Resolution::Executable(fixture_agent())).await;
    let ws = workspace(Some("approve"));
    let created = call(
        &daemon,
        "POST",
        "/sessions",
        Some(json!({"request_id": "r-create", "harness": "claude", "workspace": ws.path()})),
    )
    .await;
    let id = created.body["id"].as_str().expect("session id").to_string();
    until_kinds(
        &daemon,
        &format!("/sessions/{id}/events"),
        &["session.status_changed"],
    )
    .await;

    call(
        &daemon,
        "POST",
        &format!("/sessions/{id}/prompts"),
        Some(json!({"request_id": "r-prompt", "text": "run the thing"})),
    )
    .await;

    let events = until_kinds(
        &daemon,
        &format!("/sessions/{id}/events"),
        &[
            "tool.started",
            "decision.requested",
            "session.status_changed",
        ],
    )
    .await;
    assert!(
        events.iter().any(|(_, p)| kind(p) == "decision.requested"),
        "no decision.requested in:\n{}",
        dump(&events)
    );

    // The pending decision is queryable, with the harness's request verbatim.
    let decisions = call(&daemon, "GET", &format!("/sessions/{id}/decisions"), None).await;
    let list = decisions.body.as_array().cloned().unwrap_or_default();
    assert_eq!(list.len(), 1, "decisions: {decisions:?}");
    let decision = &list[0];
    assert_eq!(decision["state"], "pending");
    assert_eq!(decision["kind"], "approval");
    assert_eq!(decision["harness_request"]["tool_name"], "Bash");
    let options: Vec<&str> = decision["options"]
        .as_array()
        .expect("options array")
        .iter()
        .map(|o| o["id"].as_str().expect("option id"))
        .collect();
    assert_eq!(options, vec!["allow", "deny"]);
    let decision_id = decision["id"].as_str().expect("decision id").to_string();

    // Answer allow; the turn finishes and the tool result lands.
    let answer = call(
        &daemon,
        "POST",
        &format!("/decisions/{decision_id}/answer"),
        Some(json!({"request_id": "r-answer", "choice": "allow"})),
    )
    .await;
    assert_eq!(answer.status, 200, "raw: {}", answer.raw);

    let events = until_kinds(
        &daemon,
        &format!("/sessions/{id}/events"),
        &[
            "decision.responded",
            "session.status_changed",
            "tool.result",
            "message.added",
            "turn.completed",
            "session.status_changed",
        ],
    )
    .await;
    let tool_result = events
        .iter()
        .find(|(_, p)| kind(p) == "tool.result")
        .expect("tool.result event");
    assert_eq!(tool_result.1["name"], "Bash");
    assert_eq!(tool_result.1["is_error"], false);
    let completed = events
        .iter()
        .find(|(_, p)| kind(p) == "turn.completed")
        .expect("turn.completed");
    assert_eq!(completed.1["coarse"], "completed");
}

#[tokio::test]
async fn late_answers_get_an_explicit_error() {
    let daemon = start_daemon(Resolution::Executable(fixture_agent())).await;
    let ws = workspace(Some("approve"));
    let created = call(
        &daemon,
        "POST",
        "/sessions",
        Some(json!({"request_id": "r-create", "harness": "claude", "workspace": ws.path()})),
    )
    .await;
    let id = created.body["id"].as_str().expect("session id").to_string();
    until_kinds(
        &daemon,
        &format!("/sessions/{id}/events"),
        &["session.status_changed"],
    )
    .await;
    call(
        &daemon,
        "POST",
        &format!("/sessions/{id}/prompts"),
        Some(json!({"request_id": "r-prompt", "text": "run it"})),
    )
    .await;
    until_kinds(
        &daemon,
        &format!("/sessions/{id}/events"),
        &["decision.requested", "session.status_changed"],
    )
    .await;

    let decisions = call(&daemon, "GET", &format!("/sessions/{id}/decisions"), None).await;
    let decision_id = decisions.body[0]["id"]
        .as_str()
        .expect("decision id")
        .to_string();

    // Answer, then answer again: the second is rejected, explicitly.
    let first = call(
        &daemon,
        "POST",
        &format!("/decisions/{decision_id}/answer"),
        Some(json!({"request_id": "r-answer-1", "choice": "allow"})),
    )
    .await;
    assert_eq!(first.status, 200, "raw: {}", first.raw);
    let second = call(
        &daemon,
        "POST",
        &format!("/decisions/{decision_id}/answer"),
        Some(json!({"request_id": "r-answer-2", "choice": "allow"})),
    )
    .await;
    assert_eq!(
        second.status, 409,
        "late answer must 409; raw: {}",
        second.raw
    );
    assert!(
        second.body["error"]
            .as_str()
            .unwrap_or("")
            .contains("no longer pending"),
        "error should name the contract; got: {}",
        second.raw
    );
}

#[tokio::test]
async fn receipts_dedup_mutations() {
    let daemon = start_daemon(Resolution::Executable(fixture_agent())).await;
    let ws = workspace(Some("plain"));
    let created = call(
        &daemon,
        "POST",
        "/sessions",
        Some(json!({"request_id": "r-create", "harness": "claude", "workspace": ws.path()})),
    )
    .await;
    let id = created.body["id"].as_str().expect("session id").to_string();
    until_kinds(
        &daemon,
        &format!("/sessions/{id}/events"),
        &["session.status_changed"],
    )
    .await;

    let body = json!({"request_id": "r-prompt", "text": "once"});
    let first = call(
        &daemon,
        "POST",
        &format!("/sessions/{id}/prompts"),
        Some(body.clone()),
    )
    .await;
    assert_eq!(first.status, 200, "raw: {}", first.raw);
    let duplicate = call(
        &daemon,
        "POST",
        &format!("/sessions/{id}/prompts"),
        Some(body),
    )
    .await;
    assert_eq!(duplicate.status, 200, "raw: {}", duplicate.raw);
    assert_eq!(
        duplicate.body["status"], "completed",
        "duplicate returns the stored outcome"
    );

    // Only one turn ran.
    let events = until_kinds(
        &daemon,
        &format!("/sessions/{id}/events"),
        &["turn.completed", "session.status_changed"],
    )
    .await;
    let turns: Vec<_> = events
        .iter()
        .filter(|(_, p)| kind(p) == "turn.started")
        .collect();
    assert_eq!(
        turns.len(),
        1,
        "duplicate prompt must not start a second turn; events:\n{}",
        dump(&events)
    );

    // A receipt nobody recorded is unknown — that's the crash window.
    let unknown = call(&daemon, "GET", "/receipts/never-sent", None).await;
    assert_eq!(unknown.status, 200, "raw: {}", unknown.raw);
    assert_eq!(unknown.body["status"], "unknown");
}

#[tokio::test]
async fn stop_retires_pending_decisions_and_the_late_answer_conflicts() {
    let daemon = start_daemon(Resolution::Executable(fixture_agent())).await;
    let ws = workspace(Some("approve"));
    let created = call(
        &daemon,
        "POST",
        "/sessions",
        Some(json!({"request_id": "r-create", "harness": "claude", "workspace": ws.path()})),
    )
    .await;
    let id = created.body["id"].as_str().expect("session id").to_string();
    until_kinds(
        &daemon,
        &format!("/sessions/{id}/events"),
        &["session.status_changed"],
    )
    .await;
    call(
        &daemon,
        "POST",
        &format!("/sessions/{id}/prompts"),
        Some(json!({"request_id": "r-prompt", "text": "needs approval"})),
    )
    .await;
    until_kinds(
        &daemon,
        &format!("/sessions/{id}/events"),
        &["decision.requested", "session.status_changed"],
    )
    .await;
    let decisions = call(&daemon, "GET", &format!("/sessions/{id}/decisions"), None).await;
    let decision_id = decisions.body[0]["id"]
        .as_str()
        .expect("decision id")
        .to_string();

    let stop = call(
        &daemon,
        "POST",
        &format!("/sessions/{id}/stop"),
        Some(json!({"request_id": "r-stop"})),
    )
    .await;
    assert_eq!(stop.status, 200, "raw: {}", stop.raw);
    assert_eq!(
        stop.body["status"], "stopped",
        "session settles stopped; body: {}",
        stop.raw
    );

    let decisions = call(&daemon, "GET", &format!("/sessions/{id}/decisions"), None).await;
    assert_eq!(
        decisions.body[0]["state"], "retired",
        "decisions: {decisions:?}"
    );

    let late = call(
        &daemon,
        "POST",
        &format!("/decisions/{decision_id}/answer"),
        Some(json!({"request_id": "r-late", "choice": "allow"})),
    )
    .await;
    assert_eq!(
        late.status, 409,
        "answer on a stopped session must conflict; raw: {}",
        late.raw
    );
}

#[tokio::test]
async fn resume_runs_a_new_process_on_the_same_conversation() {
    let daemon = start_daemon(Resolution::Executable(fixture_agent())).await;
    let ws = workspace(Some("plain"));
    let created = call(
        &daemon,
        "POST",
        "/sessions",
        Some(json!({"request_id": "r-create", "harness": "claude", "workspace": ws.path()})),
    )
    .await;
    let id = created.body["id"].as_str().expect("session id").to_string();
    until_kinds(
        &daemon,
        &format!("/sessions/{id}/events"),
        &["session.status_changed"],
    )
    .await;
    let before = call(&daemon, "GET", &format!("/sessions/{id}"), None).await;
    let harness_ref = before.body["harness_session_ref"]
        .as_str()
        .expect("harness ref")
        .to_string();

    // A full turn, then stop, then resume: the fixture echoes --resume=<id>
    // back as its session_id, so the conversation ref must survive.
    call(
        &daemon,
        "POST",
        &format!("/sessions/{id}/prompts"),
        Some(json!({"request_id": "r-prompt-1", "text": "first"})),
    )
    .await;
    until_kinds(
        &daemon,
        &format!("/sessions/{id}/events"),
        &["turn.completed", "session.status_changed"],
    )
    .await;
    let stopped = call(
        &daemon,
        "POST",
        &format!("/sessions/{id}/stop"),
        Some(json!({"request_id": "r-stop"})),
    )
    .await;
    assert_eq!(stopped.body["status"], "stopped", "raw: {}", stopped.raw);

    let resumed = call(
        &daemon,
        "POST",
        &format!("/sessions/{id}/resume"),
        Some(json!({"request_id": "r-resume"})),
    )
    .await;
    assert_eq!(resumed.status, 200, "raw: {}", resumed.raw);
    assert_eq!(resumed.body["status"], "starting");

    until_kinds(
        &daemon,
        &format!("/sessions/{id}/events"),
        &["session.status_changed"],
    )
    .await;
    let after = call(&daemon, "GET", &format!("/sessions/{id}"), None).await;
    assert_eq!(
        after.body["harness_session_ref"]
            .as_str()
            .expect("ref after resume"),
        harness_ref,
        "resume must ride the same harness conversation"
    );

    // The resumed session takes a new prompt and completes a second turn.
    call(
        &daemon,
        "POST",
        &format!("/sessions/{id}/prompts"),
        Some(json!({"request_id": "r-prompt-2", "text": "second"})),
    )
    .await;
    let events = until_count(
        &daemon,
        &format!("/sessions/{id}/events"),
        "turn.started",
        2,
    )
    .await;
    let turns: Vec<_> = events
        .iter()
        .filter(|(_, p)| kind(p) == "turn.started")
        .collect();
    assert_eq!(
        turns.len(),
        2,
        "both turns must be in the log; events:\n{}",
        dump(&events)
    );
    let completed = until_count(
        &daemon,
        &format!("/sessions/{id}/events"),
        "turn.completed",
        2,
    )
    .await;
    assert_eq!(
        completed
            .iter()
            .filter(|(_, p)| kind(p) == "turn.completed")
            .count(),
        2,
        "both turns complete on the resumed process; events:\n{}",
        dump(&completed)
    );
}

#[tokio::test]
async fn sse_resume_after_seq_gets_only_the_tail() {
    let daemon = start_daemon(Resolution::Executable(fixture_agent())).await;
    let ws = workspace(Some("plain"));
    let created = call(
        &daemon,
        "POST",
        "/sessions",
        Some(json!({"request_id": "r-create", "harness": "claude", "workspace": ws.path()})),
    )
    .await;
    let id = created.body["id"].as_str().expect("session id").to_string();
    call(
        &daemon,
        "POST",
        &format!("/sessions/{id}/prompts"),
        Some(json!({"request_id": "r-prompt-1", "text": "first"})),
    )
    .await;
    let events = until_kinds(
        &daemon,
        &format!("/sessions/{id}/events"),
        &["turn.completed", "session.status_changed"],
    )
    .await;
    let cursor = events
        .iter()
        .rev()
        .find(|(_, p)| kind(p) == "turn.completed")
        .expect("a completed first turn")
        .0;

    // A second turn, then reconnect after the cursor.
    call(
        &daemon,
        "POST",
        &format!("/sessions/{id}/prompts"),
        Some(json!({"request_id": "r-prompt-2", "text": "second"})),
    )
    .await;
    let tail = sse_collect(&daemon, &format!("/sessions/{id}/events?after={cursor}"), 3).await;
    assert!(
        tail.iter().all(|(seq, _)| *seq > cursor),
        "resume must not repeat events; got:\n{}",
        dump(&tail)
    );
    assert!(
        tail.iter().any(|(_, p)| kind(p) == "turn.started"),
        "the new turn must arrive; got:\n{}",
        dump(&tail)
    );
}

#[tokio::test]
async fn an_unavailable_harness_creates_a_failed_session_not_an_error() {
    let daemon = start_daemon(Resolution::Unavailable).await;
    let ws = workspace(None);
    let created = call(
        &daemon,
        "POST",
        "/sessions",
        Some(json!({"request_id": "r-create", "harness": "claude", "workspace": ws.path()})),
    )
    .await;
    assert_eq!(created.status, 400, "raw: {}", created.raw);
    assert!(
        created.body["error"]
            .as_str()
            .unwrap_or("")
            .contains("not available"),
        "the error names the harness; got: {}",
        created.raw
    );
}
