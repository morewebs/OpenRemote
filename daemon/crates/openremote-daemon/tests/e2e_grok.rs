//! Grok e2e: the real HTTP API + SSE driving the real openremote-grok
//! driver against the fixture's print mode. Grok is one process per
//! prompt - the conversation rides `--resume <grok session id>`.

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
            // the init's model echo rides as session.updated
            "session.updated",
            "message.added",
            // the result frame's usage and cost are the turn's own facts
            "context.used",
            "usage.cost",
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
    assert!(
        session.body["harness_session_ref"]
            .as_str()
            .unwrap_or_default()
            .starts_with("grok-fixture-"),
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

    // The second prompt respawns with --resume <the first run's id>; the
    // fixture echoes it back, and the ref must stay that id.
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
    let conversation = session.body["harness_session_ref"]
        .as_str()
        .unwrap_or_default();
    assert!(conversation.starts_with("grok-fixture-"), "{}", session.raw);
}

#[tokio::test]
async fn grok_sessions_stop_between_prompts_and_resume() {
    let (daemon, ws) = grok_daemon(Some("plain")).await;
    let id = create_grok(&daemon, &ws).await;
    // One prompt names the conversation; then stop while no process is
    // live (between prompts) - no StdoutClosed will ever come, so the
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

/// A dash-leading prompt is text, never flags (harness audit F6): the
/// driver writes it to a prompt file the CLI reads, the fixture reads that
/// file, and the round-trip is the echo. An argv slot would have fed
/// "--help" to the CLI's own parser and died with the generic
/// exited-before-the-turn note instead of an answer.
#[tokio::test]
async fn a_dash_leading_grok_prompt_roundtrips_as_text() {
    let (daemon, ws) = grok_daemon(Some("plain")).await;
    let id = create_grok(&daemon, &ws).await;
    call(
        &daemon,
        "POST",
        &format!("/sessions/{id}/prompts"),
        Some(json!({"request_id": "g-dash", "text": "--help me ship the notes"})),
    )
    .await;
    let events = until_count(
        &daemon,
        &format!("/sessions/{id}/events"),
        "turn.completed",
        1,
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
            .contains("--help me ship the notes"),
        "the prompt round-tripped through the CLI's own parser; events:\n{}",
        dump(&events)
    );
    let completed = events
        .iter()
        .find(|(_, p)| kind(p) == "turn.completed")
        .expect("completed");
    assert_eq!(
        completed.1["outcome"],
        "success",
        "the turn ran to success, it did not die on a flag misparse; events:\n{}",
        dump(&events)
    );
}

/// A second prompt while the turn runs is refused, not raced (harness
/// audit F8): grok is one process per prompt, so an overlapping prompt
/// would spawn a second process, orphan the first child, and interleave
/// two turns' frames under the newest turn number. The gate rides the
/// driver's own per-prompt-process flag, so the answer is 409 with a
/// plain-words message, exactly one process ever ran (the fixture's run
/// ledger), and the gate re-opens once the turn completes.
#[tokio::test]
async fn a_second_grok_prompt_while_working_answers_409_with_one_process() {
    let (daemon, ws) = grok_daemon(Some("slow")).await;
    let id = create_grok(&daemon, &ws).await;
    let first = call(
        &daemon,
        "POST",
        &format!("/sessions/{id}/prompts"),
        Some(json!({"request_id": "g-p1", "text": "first"})),
    )
    .await;
    assert_eq!(first.status, 200, "raw: {}", first.raw);

    // The slow scenario holds the turn open, so the second prompt lands
    // inside the Working window.
    let refused = call(
        &daemon,
        "POST",
        &format!("/sessions/{id}/prompts"),
        Some(json!({"request_id": "g-p2", "text": "second"})),
    )
    .await;
    assert_eq!(
        refused.status, 409,
        "the overlapping prompt conflicts; raw: {}",
        refused.raw
    );
    let message = refused.body["error"].as_str().unwrap_or("");
    assert!(
        message.contains("one prompt at a time"),
        "the refusal says why in plain words: {message}"
    );

    // One turn, one process, one frame stream: the refused prompt bumped
    // no turn, spawned nothing, and its text never landed.
    let events = until_count(
        &daemon,
        &format!("/sessions/{id}/events"),
        "turn.completed",
        1,
    )
    .await;
    assert_eq!(
        events
            .iter()
            .filter(|(_, p)| kind(p) == "turn.started")
            .count(),
        1,
        "only the first turn ever started; events:\n{}",
        dump(&events)
    );
    let user_texts: Vec<&str> = events
        .iter()
        .filter(|(_, p)| kind(p) == "message.added" && p["message"]["role"] == "user")
        .map(|(_, p)| p["message"]["text"].as_str().unwrap_or(""))
        .collect();
    assert_eq!(
        user_texts,
        vec!["first"],
        "the refused prompt never landed as a message; events:\n{}",
        dump(&events)
    );
    let runs = std::fs::read_to_string(ws.path().join("fixture-grok-runs"))
        .expect("the fixture's run ledger");
    assert_eq!(
        runs.lines().count(),
        1,
        "one prompt process ran, never two; ledger:\n{runs}"
    );

    // The gate re-opens: once the turn completed, the next prompt runs a
    // real turn through one more process.
    let third = call(
        &daemon,
        "POST",
        &format!("/sessions/{id}/prompts"),
        Some(json!({"request_id": "g-p3", "text": "third"})),
    )
    .await;
    assert_eq!(third.status, 200, "raw: {}", third.raw);
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
        "the gate re-opened after the turn; events:\n{}",
        dump(&events)
    );
    let runs = std::fs::read_to_string(ws.path().join("fixture-grok-runs"))
        .expect("the fixture's run ledger");
    assert_eq!(
        runs.lines().count(),
        2,
        "the re-opened gate ran exactly one more process; ledger:\n{runs}"
    );
}

/// A Grok turn's usage is the conversation's context, and its result frame
/// carries the turn's own cost. Both were parsed and dropped before.
#[tokio::test]
async fn a_grok_turn_reports_context_and_cost() {
    let (daemon, ws) = grok_daemon(Some("plain")).await;
    let id = create_grok(&daemon, &ws).await;
    call(
        &daemon,
        "POST",
        &format!("/sessions/{id}/prompts"),
        Some(json!({"request_id": "g-usage", "text": "hi"})),
    )
    .await;
    let events = until_count(
        &daemon,
        &format!("/sessions/{id}/events"),
        "turn.completed",
        1,
    )
    .await;
    let context = events
        .iter()
        .find(|(_, payload)| kind(payload) == "context.used")
        .expect("grok reports the context it saw");
    assert_eq!(context.1["used"], 10, "events:\n{}", dump(&events));
    let cost = events
        .iter()
        .find(|(_, payload)| kind(payload) == "usage.cost")
        .expect("grok reports the turn's cost");
    assert_eq!(cost.1["cost_usd"], 0.02, "events:\n{}", dump(&events));
}
