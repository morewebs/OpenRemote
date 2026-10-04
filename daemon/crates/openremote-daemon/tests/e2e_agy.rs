//! Antigravity e2e: the real HTTP API + SSE driving the real
//! openremote-agy driver against the fixture's print mode - plus the
//! real TSV model catalog backing the console's model slot.

mod common;

use common::*;
use serde_json::json;

async fn agy_daemon(scenario: Option<&str>) -> (TestDaemon, tempfile::TempDir) {
    let daemon = start_daemon(&[
        ("claude", fixture_agent()),
        ("codex", fixture_agent()),
        ("grok", fixture_agent()),
        ("pi", fixture_agent()),
        ("opencode", fixture_agent()),
        ("agy", fixture_agent()),
    ])
    .await;
    let ws = workspace(scenario);
    (daemon, ws)
}

async fn create_agy(daemon: &TestDaemon, ws: &tempfile::TempDir) -> String {
    let created = call(
        daemon,
        "POST",
        "/sessions",
        Some(json!({"request_id": "a-create", "harness": "agy", "workspace": ws.path()})),
    )
    .await;
    assert_eq!(created.status, 201, "raw: {}", created.raw);
    created.body["id"].as_str().expect("session id").to_string()
}

#[tokio::test]
async fn an_agy_prompt_runs_a_real_turn_with_antigravity_words() {
    let (daemon, ws) = agy_daemon(Some("plain")).await;
    let id = create_agy(&daemon, &ws).await;

    call(
        &daemon,
        "POST",
        &format!("/sessions/{id}/prompts"),
        Some(json!({"request_id": "a-p1", "text": "ship the notes"})),
    )
    .await;
    let events = until_kinds(
        &daemon,
        &format!("/sessions/{id}/events"),
        &[
            "message.delta",
            "message.added",
            "turn.completed",
            "session.status_changed",
        ],
    )
    .await;
    let completed = events
        .iter()
        .find(|(_, p)| kind(p) == "turn.completed")
        .expect("completed");
    assert_eq!(completed.1["outcome"], "SUCCESS", "antigravity's own word");
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
    let session = call(&daemon, "GET", &format!("/sessions/{id}"), None).await;
    assert_eq!(
        session.body["harness_session_ref"].as_str(),
        Some("agy-fixture-conversation-1"),
        "the conversation id is the ref: {}",
        session.raw
    );
}

#[tokio::test]
async fn agy_continuations_resume_and_models_speak_antigravity_words() {
    let (daemon, ws) = agy_daemon(Some("plain")).await;
    let id = create_agy(&daemon, &ws).await;
    call(
        &daemon,
        "POST",
        &format!("/sessions/{id}/prompts"),
        Some(json!({"request_id": "a-p1", "text": "first"})),
    )
    .await;
    until_count(
        &daemon,
        &format!("/sessions/{id}/events"),
        "turn.completed",
        1,
    )
    .await;
    call(
        &daemon,
        "POST",
        &format!("/sessions/{id}/prompts"),
        Some(json!({"request_id": "a-p2", "text": "second"})),
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
        Some("agy-fixture-conversation-1"),
        "resumed prompts ride the same conversation"
    );

    // The catalog: Antigravity's own ids and display names.
    let models = call(&daemon, "GET", "/harnesses/agy/models", None).await;
    assert_eq!(models.status, 200, "raw: {}", models.raw);
    let list = models.body.as_array().expect("models array");
    let ids: Vec<&str> = list
        .iter()
        .map(|m| m["model"].as_str().expect("id"))
        .collect();
    assert_eq!(
        ids,
        vec![
            "gemini-3.8-flash-high",
            "gemini-3.8-flash-medium",
            "claude-sonnet-4-6"
        ]
    );
    assert_eq!(
        list[0]["display_name"].as_str(),
        Some("Gemini 3.8 Flash (High)")
    );
}

#[tokio::test]
async fn a_dead_agy_process_fails_the_turn_with_a_note() {
    let (daemon, ws) = agy_daemon(Some("die")).await;
    let id = create_agy(&daemon, &ws).await;
    call(
        &daemon,
        "POST",
        &format!("/sessions/{id}/prompts"),
        Some(json!({"request_id": "a-p1", "text": "anything"})),
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
    assert_eq!(completed.1["outcome"], "FAILURE", "antigravity's own word");
    let note = events
        .iter()
        .find(|(_, p)| kind(p) == "daemon.error")
        .expect("the note");
    assert_eq!(
        note.1["message"],
        "antigravity exited before the turn finished"
    );
}
