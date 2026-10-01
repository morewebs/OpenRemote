//! Grok e2e: the real HTTP API + SSE driving the real openremote-grok
//! driver against the fixture's print mode. Grok is one process per
//! prompt — the conversation rides `--resume <grok session id>`.

mod common;

use common::*;
use serde_json::json;

async fn grok_daemon(scenario: Option<&str>) -> (TestDaemon, tempfile::TempDir) {
    let daemon = start_daemon(&[
        ("claude", fixture_agent()),
        ("codex", fixture_agent()),
        ("grok", fixture_agent()),
    ])
    .await;
    let ws = workspace(scenario);
    (daemon, ws)
}

async fn create_grok(daemon: &TestDaemon, ws: &tempfile::TempDir) -> String {
    let created = call(
        daemon,
        "POST",
        "/sessions",
        Some(json!({"request_id": "g-create", "harness": "grok", "workspace": ws.path()})),
    )
    .await;
    assert_eq!(created.status, 201, "raw: {}", created.raw);
    created.body["id"].as_str().expect("session id").to_string()
}

#[tokio::test]
async fn a_grok_prompt_runs_a_real_turn_and_names_the_conversation() {
    let (daemon, ws) = grok_daemon(Some("plain")).await;
    let id = create_grok(&daemon, &ws).await;

    // A fresh grok session honestly shows starting until the first
    // prompt names the conversation.
    let session = call(&daemon, "GET", &format!("/sessions/{id}"), None).await;
    assert_eq!(
        session.body["harness_session_ref"],
        serde_json::Value::Null,
        "no conversation ref before the first prompt: {}",
        session.raw
    );

    call(
        &daemon,
        "POST",
        &format!("/sessions/{id}/prompts"),
        Some(json!({"request_id": "g-p1", "text": "ship the notes"})),
    )
    .await;

    let events = until_kinds(
        &daemon,
        &format!("/sessions/{id}/events"),
        &[
            "message.added",
            "message.added",
            "turn.completed",
            "session.status_changed",
        ],
    )
    .await;
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
    let completed = events
        .iter()
        .find(|(_, p)| kind(p) == "turn.completed")
        .expect("completed");
    assert_eq!(completed.1["outcome"], "success");
    assert_eq!(completed.1["coarse"], "completed");

    // The first prompt's init frame named grok's own session.
    let session = call(&daemon, "GET", &format!("/sessions/{id}"), None).await;
    assert_eq!(
        session.body["harness_session_ref"].as_str(),
        Some("grok-fixture-1"),
        "the grok session id is the ref: {}",
        session.raw
    );
}

#[tokio::test]
async fn grok_continuations_resume_the_same_conversation() {
    let (daemon, ws) = grok_daemon(Some("plain")).await;
    let id = create_grok(&daemon, &ws).await;
    call(
        &daemon,
        "POST",
        &format!("/sessions/{id}/prompts"),
        Some(json!({"request_id": "g-p1", "text": "first"})),
    )
    .await;
    until_count(
        &daemon,
        &format!("/sessions/{id}/events"),
        "turn.completed",
        1,
    )
    .await;

    // The second prompt respawns with --resume grok-fixture-1; the
    // fixture echoes the same id, and the ref must stay stable.
    call(
        &daemon,
        "POST",
        &format!("/sessions/{id}/prompts"),
        Some(json!({"request_id": "g-p2", "text": "second"})),
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
    let session = call(&daemon, "GET", &format!("/sessions/{id}"), None).await;
    assert_eq!(
        session.body["harness_session_ref"].as_str(),
        Some("grok-fixture-1")
    );
}

#[tokio::test]
async fn grok_sessions_stop_between_prompts_and_resume() {
    let (daemon, ws) = grok_daemon(Some("plain")).await;
    let id = create_grok(&daemon, &ws).await;
    // One prompt names the conversation; then stop while no process is
    // live (between prompts) — no StdoutClosed will ever come, so the
    // supervisor settles it.
    call(
        &daemon,
        "POST",
        &format!("/sessions/{id}/prompts"),
        Some(json!({"request_id": "g-p1", "text": "first"})),
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
        Some(json!({"request_id": "g-stop"})),
    )
    .await;
    assert_eq!(stopped.status, 200, "raw: {}", stopped.raw);
    assert_eq!(stopped.body["status"], "stopped", "body: {}", stopped.raw);

    // Resumed continuations ride the same grok conversation.
    let resumed = call(
        &daemon,
        "POST",
        &format!("/sessions/{id}/resume"),
        Some(json!({"request_id": "g-resume"})),
    )
    .await;
    assert_eq!(resumed.status, 200, "raw: {}", resumed.raw);
    call(
        &daemon,
        "POST",
        &format!("/sessions/{id}/prompts"),
        Some(json!({"request_id": "g-p2", "text": "second"})),
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
        "both turns in the log; events:
{}",
        dump(&events)
    );
}

#[tokio::test]
async fn a_grok_process_dying_mid_turn_fails_the_turn_with_a_note() {
    let (daemon, ws) = grok_daemon(Some("die")).await;
    let id = create_grok(&daemon, &ws).await;
    call(
        &daemon,
        "POST",
        &format!("/sessions/{id}/prompts"),
        Some(json!({"request_id": "g-p1", "text": "anything"})),
    )
    .await;
    let events = until_kinds(
        &daemon,
        &format!("/sessions/{id}/events"),
        &["turn.completed", "session.status_changed", "daemon.error"],
    )
    .await;
    let completed = events
        .iter()
        .find(|(_, p)| kind(p) == "turn.completed")
        .expect("completed");
    assert_eq!(completed.1["outcome"], "failed");
    assert_eq!(completed.1["coarse"], "failed");
    let note = events
        .iter()
        .find(|(_, p)| kind(p) == "daemon.error")
        .expect("the note");
    assert_eq!(note.1["message"], "grok exited before the turn finished");
}
