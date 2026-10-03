//! Context accounting e2e: the harness's own numbers ride to the console
//! (claude's result usage, codex's tokenUsage with the model's window),
//! and the compaction marker lands as the transcript's own note.

mod common;

use common::*;
use serde_json::json;

#[tokio::test]
async fn claude_reports_its_turns_context() {
    let daemon = start_daemon(&[("claude", fixture_agent())]).await;
    let ws = workspace(Some("plain"));
    let created = call(
        &daemon,
        "POST",
        "/sessions",
        Some(json!({"request_id": "c-create", "harness": "claude", "workspace": ws.path()})),
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
        Some(json!({"request_id": "c-p", "text": "hello fixture"})),
    )
    .await;
    let events = until_kinds(
        &daemon,
        &format!("/sessions/{id}/events"),
        &["turn.completed", "session.status_changed"],
    )
    .await;
    let used = events
        .iter()
        .find(|(_, p)| kind(p) == "context.used")
        .expect("claude's own usage rides the turn");
    // The fixture's result usage: 10 input tokens, no cache fields.
    assert_eq!(used.1["used"], json!(10));
    assert!(
        used.1.get("window").is_none(),
        "claude's stream-json carries no window; events:\n{}",
        dump(&events)
    );

    // The turn's cost, verbatim from the fixture's result frame.
    let cost = events
        .iter()
        .find(|(_, p)| kind(p) == "usage.cost")
        .expect("claude's own cost rides the turn");
    assert_eq!(cost.1["cost_usd"], json!(0.042));
}

#[tokio::test]
async fn claude_compaction_lands_as_the_transcripts_note() {
    let daemon = start_daemon(&[("claude", fixture_agent())]).await;
    let ws = workspace(Some("compact"));
    let created = call(
        &daemon,
        "POST",
        "/sessions",
        Some(json!({"request_id": "c-create", "harness": "claude", "workspace": ws.path()})),
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
        Some(json!({"request_id": "c-p", "text": "hello fixture"})),
    )
    .await;
    let events = until_kinds(
        &daemon,
        &format!("/sessions/{id}/events"),
        &["turn.completed", "session.status_changed"],
    )
    .await;
    let note = events
        .iter()
        .find(|(_, p)| kind(p) == "note.added")
        .expect("the compaction marker becomes a note");
    assert_eq!(
        note.1["text"],
        json!("This session was compacted before this message."),
        "events:\n{}",
        dump(&events)
    );
    // The compacted turn's context: 40 input + 12000 cache-read + 260
    // cache-write — the SDK's own context math.
    let used = events
        .iter()
        .find(|(_, p)| kind(p) == "context.used")
        .expect("usage after compaction");
    assert_eq!(used.1["used"], json!(12300));
}

#[tokio::test]
async fn codex_reports_context_against_the_models_window() {
    let daemon = start_daemon(&[("claude", fixture_agent()), ("codex", fixture_agent())]).await;
    let ws = workspace(Some("plain"));
    let created = call(
        &daemon,
        "POST",
        "/sessions",
        Some(json!({"request_id": "x-create", "harness": "codex", "workspace": ws.path()})),
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
        Some(json!({"request_id": "x-p", "text": "run the thing"})),
    )
    .await;
    let events = until_kinds(
        &daemon,
        &format!("/sessions/{id}/events"),
        &["turn.completed", "session.status_changed"],
    )
    .await;
    let used = events
        .iter()
        .find(|(_, p)| kind(p) == "context.used")
        .expect("codex's tokenUsage rides the turn");
    assert_eq!(used.1["used"], json!(1250));
    assert_eq!(used.1["window"], json!(200000));
}

#[tokio::test]
async fn codex_compaction_lands_as_the_transcripts_note() {
    let daemon = start_daemon(&[("claude", fixture_agent()), ("codex", fixture_agent())]).await;
    let ws = workspace(Some("compact"));
    let created = call(
        &daemon,
        "POST",
        "/sessions",
        Some(json!({"request_id": "x-create", "harness": "codex", "workspace": ws.path()})),
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
        Some(json!({"request_id": "x-p", "text": "run the thing"})),
    )
    .await;
    let events = until_kinds(
        &daemon,
        &format!("/sessions/{id}/events"),
        &["turn.completed", "session.status_changed"],
    )
    .await;
    let note = events
        .iter()
        .find(|(_, p)| kind(p) == "note.added")
        .expect("the compaction marker becomes a note");
    assert_eq!(
        note.1["text"],
        json!("This session was compacted before this message."),
        "events:\n{}",
        dump(&events)
    );
}
