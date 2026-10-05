//! Claude Code e2e: the real HTTP API + SSE driving the real driver
//! against the fixture agent process. No mocks in between - these tests
//! are the contract the console leans on.
//!
//! Diagnostics ship with the tests (the standing lesson): every failure
//! dumps the events seen so far, not just the assertion that tripped.

mod common;

use common::*;
use serde_json::json;

// ---- the tests ----

#[tokio::test]
async fn healthz_is_open_and_everything_else_needs_the_token() {
    let daemon = start_daemon(&[("claude", fixture_agent())]).await;
    let health = call(&daemon, "GET", "/healthz", None).await;
    assert_eq!(health.status, 200, "raw: {}", health.raw);
    let anon = call_with_token(&daemon, "wrong-token", "GET", "/sessions", None).await;
    assert_eq!(anon.status, 401, "raw: {}", anon.raw);
    let good = call(&daemon, "GET", "/sessions", None).await;
    assert_eq!(good.status, 200, "raw: {}", good.raw);
}

#[tokio::test]
async fn a_prompt_runs_a_real_turn_end_to_end() {
    let daemon = start_daemon(&[("claude", fixture_agent())]).await;
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
            "context.used",
            "usage.cost",
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
    let daemon = start_daemon(&[("claude", fixture_agent())]).await;
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
            "context.used",
            "usage.cost",
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
    let daemon = start_daemon(&[("claude", fixture_agent())]).await;
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
    let daemon = start_daemon(&[("claude", fixture_agent())]).await;
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

    // A receipt nobody recorded is unknown - that's the crash window.
    let unknown = call(&daemon, "GET", "/receipts/never-sent", None).await;
    assert_eq!(unknown.status, 200, "raw: {}", unknown.raw);
    assert_eq!(unknown.body["status"], "unknown");
}

#[tokio::test]
async fn stop_retires_pending_decisions_and_the_late_answer_conflicts() {
    let daemon = start_daemon(&[("claude", fixture_agent())]).await;
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
    let daemon = start_daemon(&[("claude", fixture_agent())]).await;
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
    let daemon = start_daemon(&[("claude", fixture_agent())]).await;
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
    // A harness the registry knows nothing about: the same path a probe
    // that found nothing takes (backend lookup fails → explicit error, no
    // phantom session). The registry's slots don't matter - an id no slot
    // fills is unavailable everywhere.
    let daemon = start_daemon(&[]).await;
    let ws = workspace(None);
    let created = call(
        &daemon,
        "POST",
        "/sessions",
        Some(
            json!({"request_id": "r-create", "harness": "no-such-harness", "workspace": ws.path()}),
        ),
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

#[tokio::test]
async fn console_origins_can_fetch_the_daemon_and_foreign_origins_cannot() {
    // The console is a webview on another origin (vite dev on
    // http://localhost:5173; Tauri production on http://tauri.localhost /
    // tauri://localhost) - without CORS headers its fetches are blocked
    // by the webview. This is the test that would have caught the
    // first-run "connect to the daemon" dead end.
    let daemon = start_daemon(&[("claude", fixture_agent())]).await;

    // Preflight for a bearer-authed POST from the dev origin.
    let preflight = raw_http(
        &daemon,
        "OPTIONS",
        "/sessions",
        &[
            ("Origin", "http://localhost:5173"),
            ("Access-Control-Request-Method", "POST"),
            (
                "Access-Control-Request-Headers",
                "authorization, content-type",
            ),
        ],
        None,
    )
    .await;
    assert_eq!(preflight.status, 200, "raw: {}", preflight.raw);
    let allow_origin = preflight
        .headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("access-control-allow-origin"))
        .map(|(_, v)| v.clone())
        .unwrap_or_default();
    assert_eq!(
        allow_origin, "http://localhost:5173",
        "the console origin is echoed; raw: {}",
        preflight.raw
    );
    assert!(
        preflight
            .raw
            .to_ascii_lowercase()
            .contains("access-control-allow-headers"),
        "authorization must be allowed; raw: {}",
        preflight.raw
    );

    // A Tauri production origin is allowed too (any port on localhost).
    for origin in [
        "http://tauri.localhost",
        "tauri://localhost",
        "http://localhost:5174",
    ] {
        let reply = raw_http(&daemon, "GET", "/healthz", &[("Origin", origin)], None).await;
        assert_eq!(reply.status, 200, "raw: {}", reply.raw);
        let echoed = reply
            .headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case("access-control-allow-origin"))
            .map(|(_, v)| v.clone())
            .unwrap_or_default();
        assert_eq!(
            echoed, origin,
            "origin {origin} must be allowed; raw: {}",
            reply.raw
        );
    }

    // A foreign website gets no CORS grant - its reads stay blocked.
    let foreign = raw_http(
        &daemon,
        "GET",
        "/healthz",
        &[("Origin", "https://evil.example")],
        None,
    )
    .await;
    assert_eq!(foreign.status, 200, "healthz answers; raw: {}", foreign.raw);
    assert!(
        !foreign
            .headers
            .iter()
            .any(|(k, _)| k.eq_ignore_ascii_case("access-control-allow-origin")),
        "no CORS grant for foreign origins; raw: {}",
        foreign.raw
    );

    // Removing a machine rides DELETE - the preflight must allow it or the
    // webview's fetch dies (the webview-only failure raw TCP can't see).
    let preflight = raw_http(
        &daemon,
        "OPTIONS",
        "/machines",
        &[
            ("Origin", "http://localhost:5173"),
            ("Access-Control-Request-Method", "DELETE"),
            ("Access-Control-Request-Headers", "authorization"),
        ],
        None,
    )
    .await;
    assert_eq!(preflight.status, 200, "raw: {}", preflight.raw);
    let methods = preflight
        .headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("access-control-allow-methods"))
        .map(|(_, v)| v.clone())
        .unwrap_or_default();
    assert!(
        methods
            .split(',')
            .any(|m| m.trim().eq_ignore_ascii_case("DELETE")),
        "DELETE must be allowed for the console; raw: {}",
        preflight.raw
    );
}

#[tokio::test]
async fn fast_mode_rides_the_session_and_the_capability_gates_on_the_cli() {
    let daemon = start_daemon(&[("claude", fixture_agent())]).await;
    let ws = workspace(Some("plain"));

    // The fixture answers the registry's --version probe (9.9.9 ≥ claude's
    // own 2.1.205 fast-mode minimum); a harness without a fast mode never
    // advertises one, whatever its version.
    let caps = call(&daemon, "GET", "/capabilities", None).await;
    let harnesses = caps.body["harnesses"].as_array().expect("harnesses");
    let claude = harnesses
        .iter()
        .find(|h| h["id"] == "claude")
        .expect("claude in capabilities")
        .clone();
    assert_eq!(claude["version"].as_str(), Some("9.9.9"));
    assert_eq!(claude["fast_supported"], true, "raw: {}", caps.raw);
    let grok = harnesses
        .iter()
        .find(|h| h["id"] == "grok")
        .expect("grok in capabilities");
    assert_eq!(
        grok["fast_supported"], false,
        "no fast mode to offer; raw: {}",
        caps.raw
    );

    // The choice rides the session it was made on.
    let created = call(
        &daemon,
        "POST",
        "/sessions",
        Some(json!({
            "request_id": "r-create",
            "harness": "claude",
            "workspace": ws.path(),
            "fast": true
        })),
    )
    .await;
    assert_eq!(created.status, 201, "raw: {}", created.raw);
    assert_eq!(created.body["fast"], true, "raw: {}", created.raw);

    let plain = call(
        &daemon,
        "POST",
        "/sessions",
        Some(json!({"request_id": "r-plain", "harness": "claude", "workspace": ws.path()})),
    )
    .await;
    assert_eq!(plain.status, 201, "raw: {}", plain.raw);
    assert_eq!(plain.body["fast"], false, "raw: {}", plain.raw);
}

#[tokio::test]
async fn an_effort_default_rides_the_session_record_verbatim() {
    let daemon = start_daemon(&[("claude", fixture_agent())]).await;
    let ws = workspace(Some("plain"));

    // The console's new-task default: the harness's own tier word, set
    // on the session exactly as given (a word the harness doesn't know
    // is the harness's own spawn error, never ours to rename).
    let created = call(
        &daemon,
        "POST",
        "/sessions",
        Some(json!({
            "request_id": "r-effort",
            "harness": "claude",
            "workspace": ws.path(),
            "effort": "high"
        })),
    )
    .await;
    assert_eq!(created.status, 201, "raw: {}", created.raw);
    assert_eq!(created.body["effort"], "high", "raw: {}", created.raw);

    // Without a default, the session carries none - the harness's own
    // config pairing stays the harness's business.
    let plain = call(
        &daemon,
        "POST",
        "/sessions",
        Some(json!({"request_id": "r-none", "harness": "claude", "workspace": ws.path()})),
    )
    .await;
    assert_eq!(plain.status, 201, "raw: {}", plain.raw);
    assert_eq!(
        plain.body["effort"].is_null(),
        true,
        "no effort imposed; raw: {}",
        plain.raw
    );
}
