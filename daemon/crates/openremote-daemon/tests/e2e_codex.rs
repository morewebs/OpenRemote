//! Codex e2e: the real HTTP API + SSE driving the real openremote-codex
//! driver against the fixture's app-server mode. Every test drives the
//! same spawn path production takes (`codex app-server --listen stdio://`).

mod common;

use common::*;
use serde_json::{Value, json};

async fn codex_daemon(scenario: Option<&str>) -> (TestDaemon, tempfile::TempDir) {
    let daemon = start_daemon(&[("claude", fixture_agent()), ("codex", fixture_agent())]).await;
    let ws = workspace(scenario);
    (daemon, ws)
}

async fn create_codex(daemon: &TestDaemon, ws: &tempfile::TempDir) -> String {
    let created = call(
        daemon,
        "POST",
        "/sessions",
        Some(json!({"request_id": "c-create", "harness": "codex", "workspace": ws.path()})),
    )
    .await;
    assert_eq!(created.status, 201, "raw: {}", created.raw);
    created.body["id"].as_str().expect("session id").to_string()
}

#[tokio::test]
async fn a_codex_prompt_runs_a_real_turn_end_to_end() {
    let (daemon, ws) = codex_daemon(Some("plain")).await;
    let id = create_codex(&daemon, &ws).await;

    // The app-server handshake names the conversation.
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
    let session = call(&daemon, "GET", &format!("/sessions/{id}"), None).await;
    assert_eq!(
        session.body["harness_session_ref"].as_str(),
        Some("thread-fixture-1"),
        "the codex thread id is the session ref: {}",
        session.raw
    );

    let prompt = call(
        &daemon,
        "POST",
        &format!("/sessions/{id}/prompts"),
        Some(json!({"request_id": "c-prompt", "text": "ship the notes"})),
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
    let completed = events
        .iter()
        .find(|(_, p)| kind(p) == "turn.completed")
        .expect("turn.completed");
    assert_eq!(
        completed.1["outcome"],
        "completed",
        "codex's own word; events:\n{}",
        dump(&events)
    );
    assert_eq!(completed.1["coarse"], "completed");
    let agent = events
        .iter()
        .find(|(_, p)| kind(p) == "message.added" && p["message"]["role"] == "assistant")
        .expect("agent message");
    assert!(
        agent.1["message"]["text"]
            .as_str()
            .unwrap_or("")
            .contains("ship the notes"),
        "events:\n{}",
        dump(&events)
    );
}

#[tokio::test]
async fn codex_approvals_flow_through_decisions_with_codex_words() {
    let (daemon, ws) = codex_daemon(Some("approve")).await;
    let id = create_codex(&daemon, &ws).await;
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
        Some(json!({"request_id": "c-prompt", "text": "run the thing"})),
    )
    .await;

    let events = until_kinds(
        &daemon,
        &format!("/sessions/{id}/events"),
        &["decision.requested", "session.status_changed"],
    )
    .await;
    let decision_event = events
        .iter()
        .find(|(_, p)| kind(p) == "decision.requested")
        .expect("decision.requested");
    let decision = &decision_event.1["decision"];
    assert_eq!(decision["kind"], "approval");
    assert_eq!(
        decision["harness_request"]["command"], "echo fixture",
        "the codex request rides verbatim"
    );
    let options: Vec<&str> = decision["options"]
        .as_array()
        .expect("options")
        .iter()
        .map(|o| o["id"].as_str().expect("option id"))
        .collect();
    // Codex's own decision words, in the schema's order.
    assert_eq!(
        options,
        vec!["accept", "acceptForSession", "decline", "cancel"]
    );
    let decision_id = decision["id"].as_str().expect("decision id").to_string();

    // Answer accept; the turn continues and the tool result lands.
    let answer = call(
        &daemon,
        "POST",
        &format!("/decisions/{decision_id}/answer"),
        Some(json!({"request_id": "c-answer", "choice": "accept"})),
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
    let tool = events
        .iter()
        .find(|(_, p)| kind(p) == "tool.result")
        .expect("tool.result");
    assert_eq!(tool.1["name"], "commandExecution");
    assert_eq!(tool.1["is_error"], false);
    let completed = events
        .iter()
        .find(|(_, p)| kind(p) == "turn.completed")
        .expect("completed");
    assert_eq!(completed.1["outcome"], "completed");
}

#[tokio::test]
async fn codex_cancel_interrupts_the_turn_instead_of_continuing() {
    let (daemon, ws) = codex_daemon(Some("approve")).await;
    let id = create_codex(&daemon, &ws).await;
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
        Some(json!({"request_id": "c-prompt", "text": "needs approval"})),
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

    let answer = call(
        &daemon,
        "POST",
        &format!("/decisions/{decision_id}/answer"),
        Some(json!({"request_id": "c-cancel", "choice": "cancel"})),
    )
    .await;
    assert_eq!(answer.status, 200, "raw: {}", answer.raw);

    // The cancel answer must NOT move the session to working (its own
    // interrupt settles it); the turn completes as interrupted.
    let events = until_kinds(
        &daemon,
        &format!("/sessions/{id}/events"),
        &[
            "decision.responded",
            "tool.result",
            "turn.completed",
            "session.status_changed",
        ],
    )
    .await;
    let responded_at = events
        .iter()
        .position(|(_, p)| kind(p) == "decision.responded")
        .expect("decision.responded");
    let post_answer: Vec<Value> = events[responded_at + 1..]
        .iter()
        .filter(|(_, p)| kind(p) == "session.status_changed")
        .map(|(_, p)| p["status"].clone())
        .collect();
    assert!(
        !post_answer.iter().any(|s| s == "working"),
        "cancel interrupts, it does not resume work; post-answer statuses: {post_answer:?}; events:\n{}",
        dump(&events)
    );
    let completed = events
        .iter()
        .find(|(_, p)| kind(p) == "turn.completed")
        .expect("completed");
    assert_eq!(completed.1["outcome"], "interrupted", "codex's own word");
    assert_eq!(completed.1["coarse"], "interrupted");
}

#[tokio::test]
async fn codex_models_endpoint_speaks_codex_words() {
    let (daemon, _ws) = codex_daemon(None).await;
    let models = call(&daemon, "GET", "/harnesses/codex/models", None).await;
    assert_eq!(models.status, 200, "raw: {}", models.raw);
    let list = models.body.as_array().expect("models array");
    let ids: Vec<&str> = list
        .iter()
        .map(|m| m["model"].as_str().expect("model id"))
        .collect();
    assert_eq!(
        ids,
        vec!["gpt-5.1-codex", "gpt-5.1", "o4-mini"],
        "models: {models:?}"
    );
    let efforts: Vec<&str> = list[0]["reasoning_efforts"]
        .as_array()
        .expect("efforts")
        .iter()
        .map(|e| e.as_str().expect("effort"))
        .collect();
    assert_eq!(efforts, vec!["low", "medium", "high"]);

    // Claude advertises nothing through us — an empty list is a real
    // answer (the slot stays reserved).
    let claude_models = call(&daemon, "GET", "/harnesses/claude/models", None).await;
    assert_eq!(claude_models.status, 200, "raw: {}", claude_models.raw);
    assert_eq!(claude_models.body.as_array().map(Vec::len), Some(0));
}

#[tokio::test]
async fn codex_sessions_resume_on_the_same_thread() {
    let (daemon, ws) = codex_daemon(Some("plain")).await;
    let id = create_codex(&daemon, &ws).await;
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
        Some(json!({"request_id": "c-p1", "text": "first"})),
    )
    .await;
    until_count(
        &daemon,
        &format!("/sessions/{id}/events"),
        "turn.completed",
        1,
    )
    .await;

    let stopped = call(
        &daemon,
        "POST",
        &format!("/sessions/{id}/stop"),
        Some(json!({"request_id": "c-stop"})),
    )
    .await;
    assert_eq!(stopped.body["status"], "stopped", "raw: {}", stopped.raw);

    let resumed = call(
        &daemon,
        "POST",
        &format!("/sessions/{id}/resume"),
        Some(json!({"request_id": "c-resume"})),
    )
    .await;
    assert_eq!(resumed.status, 200, "raw: {}", resumed.raw);

    // The resumed process answers on the same thread id.
    until_kinds(
        &daemon,
        &format!("/sessions/{id}/events"),
        &["session.status_changed"],
    )
    .await;
    let session = call(&daemon, "GET", &format!("/sessions/{id}"), None).await;
    assert_eq!(
        session.body["harness_session_ref"].as_str(),
        Some("thread-fixture-1"),
        "resume rides the same codex thread"
    );

    call(
        &daemon,
        "POST",
        &format!("/sessions/{id}/prompts"),
        Some(json!({"request_id": "c-p2", "text": "second"})),
    )
    .await;
    let events = until_count(
        &daemon,
        &format!("/sessions/{id}/events"),
        "turn.completed",
        2,
    )
    .await;
    assert_eq!(
        events
            .iter()
            .filter(|(_, p)| kind(p) == "turn.started")
            .count(),
        2,
        "both turns in the log; events:\n{}",
        dump(&events)
    );
}
