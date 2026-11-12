//! Moving a chat between projects: the route the sidebar's row calls. A move
//! lands the chat in the new folder and the old one no longer lists it; an
//! unknown project is refused with a reason; a chat with a turn in flight is
//! refused with the daemon's words.

mod common;

use std::path::Path;

use common::{TestDaemon, call, fixture_agent, start_daemon, until_status, workspace};
use serde_json::json;

/// One project, registered on this daemon, by folder.
async fn project(daemon: &TestDaemon, request: &str, folder: &Path) -> String {
    let made = call(
        daemon,
        "POST",
        "/projects",
        Some(json!({"request_id": request, "folders": [folder]})),
    )
    .await;
    assert_eq!(made.status, 201, "raw: {}", made.raw);
    made.body["id"].as_str().expect("a project id").to_string()
}

/// A chat running in `folder`.
async fn session_in(daemon: &TestDaemon, request: &str, folder: &Path) -> String {
    let made = call(
        daemon,
        "POST",
        "/sessions",
        Some(json!({"request_id": request, "harness": "claude", "workspace": folder})),
    )
    .await;
    assert_eq!(made.status, 201, "raw: {}", made.raw);
    made.body["id"].as_str().expect("a session id").to_string()
}

fn folder_of(session: &serde_json::Value) -> String {
    session["workspace"]
        .as_str()
        .unwrap_or_default()
        .to_string()
}

#[tokio::test]
async fn a_move_lands_the_chat_in_the_new_project() {
    let daemon = start_daemon(&[("claude", fixture_agent())]).await;
    let from = workspace(Some("plain"));
    let to = workspace(Some("plain"));
    let id = session_in(&daemon, "r-create", from.path()).await;
    until_status(&daemon, &id, &["idle"]).await;
    let project = project(&daemon, "r-project", to.path()).await;

    let moved = call(
        &daemon,
        "POST",
        &format!("/sessions/{id}/workspace"),
        Some(json!({"request_id": "r-move", "project_id": project})),
    )
    .await;
    assert_eq!(moved.status, 200, "raw: {}", moved.raw);
    assert_eq!(
        folder_of(&moved.body["result"]),
        to.path().to_string_lossy(),
        "the route answers with the chat in its new folder"
    );

    // The project a chat is in is its folder, so the old project no longer
    // lists it: the poll agrees with the route's answer.
    let listed = call(&daemon, "GET", "/sessions", None).await;
    let row = listed
        .body
        .as_array()
        .and_then(|all| all.iter().find(|s| s["id"] == id.as_str()))
        .cloned()
        .expect("the chat is still listed");
    assert_eq!(folder_of(&row), to.path().to_string_lossy());

    // The harness restarted in the new folder and settles there.
    until_status(&daemon, &id, &["idle"]).await;
}

#[tokio::test]
async fn a_move_to_an_unknown_project_is_refused_with_a_reason() {
    let daemon = start_daemon(&[("claude", fixture_agent())]).await;
    let here = workspace(Some("plain"));
    let id = session_in(&daemon, "r-create", here.path()).await;
    let refused = call(
        &daemon,
        "POST",
        &format!("/sessions/{id}/workspace"),
        Some(json!({
            "request_id": "r-move",
            "project_id": "00000000-0000-0000-0000-000000000000"
        })),
    )
    .await;
    assert_eq!(refused.status, 404, "raw: {}", refused.raw);
    assert!(
        refused.body["error"]
            .as_str()
            .is_some_and(|error| error.contains("isn't registered on this machine")),
        "raw: {}",
        refused.raw
    );
}

#[tokio::test]
async fn a_move_during_a_turn_is_refused_with_the_daemons_words() {
    let daemon = start_daemon(&[("claude", fixture_agent())]).await;
    let busy = workspace(Some("hold"));
    let other = workspace(Some("plain"));
    let id = session_in(&daemon, "r-create", busy.path()).await;
    until_status(&daemon, &id, &["idle"]).await;
    let project = project(&daemon, "r-project", other.path()).await;

    let prompt = call(
        &daemon,
        "POST",
        &format!("/sessions/{id}/prompts"),
        Some(json!({"request_id": "r-prompt", "text": "do the thing"})),
    )
    .await;
    assert_eq!(prompt.status, 200, "raw: {}", prompt.raw);
    until_status(&daemon, &id, &["working"]).await;

    let refused = call(
        &daemon,
        "POST",
        &format!("/sessions/{id}/workspace"),
        Some(json!({"request_id": "r-move", "project_id": project})),
    )
    .await;
    assert_eq!(refused.status, 409, "raw: {}", refused.raw);
    assert!(
        refused.body["error"]
            .as_str()
            .is_some_and(|error| error.contains("a turn is running in this chat")),
        "raw: {}",
        refused.raw
    );
}

/// The texts of a chat's messages of one role, in order.
fn messages(events: &[(u64, serde_json::Value)], role: &str) -> Vec<String> {
    events
        .iter()
        .filter(|(_, p)| common::kind(p) == "message.added")
        .filter(|(_, p)| p["message"]["role"] == role)
        .map(|(_, p)| {
            p["message"]["text"]
                .as_str()
                .unwrap_or_default()
                .to_string()
        })
        .collect()
}

#[tokio::test]
async fn a_conversation_that_cannot_follow_is_carried_into_the_next_message() {
    let daemon = start_daemon(&[("claude", fixture_agent())]).await;
    let from = workspace(Some("plain"));
    let to = workspace(Some("plain"));
    let id = session_in(&daemon, "r-create", from.path()).await;
    let path = format!("/sessions/{id}/events");
    let first = call(
        &daemon,
        "POST",
        &format!("/sessions/{id}/prompts"),
        Some(json!({"request_id": "r-p1", "text": "the first thing"})),
    )
    .await;
    assert_eq!(first.status, 200, "raw: {}", first.raw);
    common::until_count(&daemon, &path, "turn.completed", 1).await;
    until_status(&daemon, &id, &["idle"]).await;
    let project = project(&daemon, "r-project", to.path()).await;

    // A test's stand-in harness keeps no conversation to copy, so the move
    // starts a fresh one there and marks where the carry begins.
    let moved = call(
        &daemon,
        "POST",
        &format!("/sessions/{id}/workspace"),
        Some(json!({"request_id": "r-move", "project_id": project})),
    )
    .await;
    assert_eq!(moved.status, 200, "raw: {}", moved.raw);
    assert!(
        moved.body["result"]["carry_transcript"].is_u64(),
        "the move marks the carry: {}",
        moved.raw
    );
    until_status(&daemon, &id, &["idle", "starting"]).await;

    let second = call(
        &daemon,
        "POST",
        &format!("/sessions/{id}/prompts"),
        Some(json!({"request_id": "r-p2", "text": "the second thing"})),
    )
    .await;
    assert_eq!(second.status, 200, "raw: {}", second.raw);
    let events = common::until_count(&daemon, &path, "turn.completed", 2).await;

    // The harness heard the chat so far, marked as context, then the message.
    let replies = messages(&events, "assistant");
    let last = replies.last().cloned().unwrap_or_default();
    assert!(last.contains("<earlier-chat>"), "carried: {last}");
    assert!(last.contains("the first thing"), "carried: {last}");
    assert!(last.ends_with("the second thing"), "carried: {last}");
    // The transcript keeps the message as the user wrote it.
    let asked = messages(&events, "user");
    assert_eq!(asked.last().map(String::as_str), Some("the second thing"));
    // The carry is spent: the message after goes as it is.
    let after = call(&daemon, "GET", &format!("/sessions/{id}"), None).await;
    assert!(
        after.body["carry_transcript"].is_null(),
        "raw: {}",
        after.raw
    );
    let third = call(
        &daemon,
        "POST",
        &format!("/sessions/{id}/prompts"),
        Some(json!({"request_id": "r-p3", "text": "the third thing"})),
    )
    .await;
    assert_eq!(third.status, 200, "raw: {}", third.raw);
    let events = common::until_count(&daemon, &path, "turn.completed", 3).await;
    let replies = messages(&events, "assistant");
    assert_eq!(
        replies.last().map(String::as_str),
        Some("done: the third thing")
    );
}

#[tokio::test]
async fn a_stopped_chat_that_moved_resumes_into_a_fresh_conversation() {
    let daemon = start_daemon(&[("claude", fixture_agent())]).await;
    let from = workspace(Some("plain"));
    let to = workspace(Some("plain"));
    let id = session_in(&daemon, "r-create", from.path()).await;
    until_status(&daemon, &id, &["idle", "starting"]).await;
    let stopped = call(
        &daemon,
        "POST",
        &format!("/sessions/{id}/stop"),
        Some(json!({"request_id": "r-stop"})),
    )
    .await;
    assert_eq!(stopped.status, 200, "raw: {}", stopped.raw);
    let project = project(&daemon, "r-project", to.path()).await;
    let moved = call(
        &daemon,
        "POST",
        &format!("/sessions/{id}/workspace"),
        Some(json!({"request_id": "r-move", "project_id": project})),
    )
    .await;
    assert_eq!(moved.status, 200, "raw: {}", moved.raw);
    // It never named a conversation, yet it resumes: fresh, in the new folder.
    let resumed = call(
        &daemon,
        "POST",
        &format!("/sessions/{id}/resume"),
        Some(json!({"request_id": "r-resume"})),
    )
    .await;
    assert_eq!(resumed.status, 200, "raw: {}", resumed.raw);
    assert_eq!(folder_of(&resumed.body), to.path().to_string_lossy());
}
