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
            // the result frame's usage is now the ring's fact
            "context.used",
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
    assert!(
        session.body["harness_session_ref"]
            .as_str()
            .unwrap_or_default()
            .starts_with("agy-fixture-conversation-"),
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
    assert!(
        session.body["harness_session_ref"]
            .as_str()
            .unwrap_or_default()
            .starts_with("agy-fixture-conversation-"),
        "resumed prompts ride the same conversation: {}",
        session.raw
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

/// A dash-leading prompt is text, never flags: the
/// driver sends it as one stream-json message on stdin, the fixture reads
/// that message, and the round-trip is the echo. An argv value slot would
/// have fed "-v" to the CLI's own parser and died with the generic
/// exited-before-the-turn note instead of an answer.
#[tokio::test]
async fn a_dash_leading_agy_prompt_roundtrips_as_text() {
    let (daemon, ws) = agy_daemon(Some("plain")).await;
    let id = create_agy(&daemon, &ws).await;
    call(
        &daemon,
        "POST",
        &format!("/sessions/{id}/prompts"),
        Some(json!({"request_id": "a-dash", "text": "-v the notes please"})),
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
            .contains("-v the notes please"),
        "the prompt round-tripped through the CLI's own parser; events:\n{}",
        dump(&events)
    );
    let completed = events
        .iter()
        .find(|(_, p)| kind(p) == "turn.completed")
        .expect("completed");
    assert_eq!(
        completed.1["outcome"],
        "SUCCESS",
        "the turn ran to success, it did not die on a flag misparse; events:\n{}",
        dump(&events)
    );
}

/// A second prompt while the turn runs is refused, not raced (harness
/// audit F8): Antigravity print mode is one process per prompt, so an
/// overlapping prompt would spawn a second process, orphan the first
/// child, and interleave two turns' frames under the newest turn
/// number. The gate rides the driver's own per-prompt-process flag, so
/// the answer is 409 with a plain-words message, exactly one process ever
/// ran (the fixture's run ledger), and the gate re-opens once the turn
/// completes.
#[tokio::test]
async fn a_second_agy_prompt_while_working_answers_409_with_one_process() {
    let (daemon, ws) = agy_daemon(Some("slow")).await;
    let id = create_agy(&daemon, &ws).await;
    let first = call(
        &daemon,
        "POST",
        &format!("/sessions/{id}/prompts"),
        Some(json!({"request_id": "a-p1", "text": "first"})),
    )
    .await;
    assert_eq!(first.status, 200, "raw: {}", first.raw);

    // The slow scenario holds the turn open, so the second prompt lands
    // inside the Working window.
    let refused = call(
        &daemon,
        "POST",
        &format!("/sessions/{id}/prompts"),
        Some(json!({"request_id": "a-p2", "text": "second"})),
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
    let runs = std::fs::read_to_string(ws.path().join("fixture-agy-runs"))
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
        Some(json!({"request_id": "a-p3", "text": "third"})),
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
    let runs = std::fs::read_to_string(ws.path().join("fixture-agy-runs"))
        .expect("the fixture's run ledger");
    assert_eq!(
        runs.lines().count(),
        2,
        "the re-opened gate ran exactly one more process; ledger:\n{runs}"
    );
}

/// Antigravity's result frame carries the same input usage the other
/// Anthropic-wire harnesses do; the ring was empty because the driver read
/// three fields and dropped the fourth.
#[tokio::test]
async fn an_agy_turn_reports_context_used() {
    let (daemon, ws) = agy_daemon(Some("plain")).await;
    let id = create_agy(&daemon, &ws).await;
    call(
        &daemon,
        "POST",
        &format!("/sessions/{id}/prompts"),
        Some(json!({"request_id": "a-usage", "text": "hi"})),
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
        .expect("antigravity reports the context it saw");
    assert_eq!(context.1["used"], 12, "events:\n{}", dump(&events));
}
