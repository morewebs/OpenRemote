//! Pictures in a chat: a message's attachments are kept in the blob store,
//! named in the user's message event by their sha256 (never stored as
//! bytes in the log), and reach the harnesses that read pictures in their
//! own wire's form - claude as base64 image blocks, codex as local image
//! files. A harness that reads none refuses such a message before anything
//! is written, in a sentence naming the app.

mod common;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use common::*;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

/// A PNG by its signature, with words after it a test can find again.
const PNG: &[u8] = b"\x89PNG\r\n\x1a\nfixture picture";

fn png_b64() -> String {
    STANDARD.encode(PNG)
}

fn png_sha() -> String {
    Sha256::digest(PNG)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn attachment() -> Value {
    json!([{"kind": "image", "mime": "image/png", "data": png_b64()}])
}

async fn create(daemon: &TestDaemon, harness: &str, ws: &tempfile::TempDir) -> String {
    let created = call(
        daemon,
        "POST",
        "/sessions",
        Some(json!({"request_id": format!("i-create-{harness}"), "harness": harness, "workspace": ws.path()})),
    )
    .await;
    assert_eq!(created.status, 201, "raw: {}", created.raw);
    created.body["id"].as_str().expect("session id").to_string()
}

/// The last line a fixture recorded in the workspace, as JSON.
fn last_recorded(ws: &tempfile::TempDir, file: &str) -> Value {
    let recorded = std::fs::read_to_string(ws.path().join(file))
        .unwrap_or_else(|e| panic!("{file} not recorded: {e}"));
    serde_json::from_str(recorded.lines().last().expect("a recorded line")).unwrap()
}

/// The user message's pictures, as its event names them.
fn user_images(events: &[(u64, Value)]) -> Vec<Value> {
    events
        .iter()
        .filter(|(_, e)| kind(e) == "message.added" && e["message"]["role"] == "user")
        .flat_map(|(_, e)| {
            e["message"]["images"]
                .as_array()
                .cloned()
                .unwrap_or_default()
        })
        .collect()
}

fn queue_len(session: &Value) -> usize {
    session["queue"].as_array().map(|q| q.len()).unwrap_or(0)
}

async fn picture_is_served(daemon: &TestDaemon) {
    let blob = raw_http(daemon, "GET", &format!("/blobs/{}", png_sha()), &[], None).await;
    assert_eq!(blob.status, 200, "{}", blob.raw);
    assert_eq!(blob.header("content-type"), Some("image/png"));
    assert!(blob.text().ends_with("fixture picture"), "{}", blob.raw);
}

#[tokio::test]
async fn a_picture_reaches_a_claude_chat_as_an_image_block() {
    let daemon = start_daemon(&[("claude", fixture_agent())]).await;
    let ws = workspace(Some("plain"));
    let id = create(&daemon, "claude", &ws).await;
    let path = format!("/sessions/{id}/events");

    let prompt = call(
        &daemon,
        "POST",
        &format!("/sessions/{id}/prompts"),
        Some(json!({"request_id": "i-c1", "text": "what is this?", "attachments": attachment()})),
    )
    .await;
    assert_eq!(prompt.status, 200, "raw: {}", prompt.raw);
    let events = until_kinds(&daemon, &path, &["turn.completed"]).await;

    // The event names the picture by its hash; the bytes stay out of it.
    assert_eq!(
        user_images(&events),
        vec![json!({"blob": png_sha(), "mime": "image/png"})],
        "{}",
        dump(&events)
    );
    assert!(!dump(&events).contains(&png_b64()), "{}", dump(&events));
    let log = std::fs::read_to_string(
        daemon
            .data_dir()
            .join("sessions")
            .join(&id)
            .join("events.jsonl"),
    )
    .expect("the chat's log");
    assert!(log.contains(&png_sha()), "the log names the blob");
    assert!(!log.contains(&png_b64()), "the log never holds the bytes");

    // Claude read the words first, then the picture as a base64 block.
    let content = last_recorded(&ws, "fixture-claude-input");
    assert_eq!(content[0], json!({"type": "text", "text": "what is this?"}));
    assert_eq!(
        content[1],
        json!({"type": "image", "source": {"type": "base64", "media_type": "image/png", "data": png_b64()}})
    );
    picture_is_served(&daemon).await;
}

#[tokio::test]
async fn a_picture_reaches_a_codex_chat_as_a_local_image() {
    let daemon = start_daemon(&[("codex", fixture_agent())]).await;
    let ws = workspace(Some("plain"));
    let id = create(&daemon, "codex", &ws).await;
    let path = format!("/sessions/{id}/events");
    until_kinds(&daemon, &path, &["session.status_changed"]).await;

    // A picture alone is a whole message.
    let prompt = call(
        &daemon,
        "POST",
        &format!("/sessions/{id}/prompts"),
        Some(json!({"request_id": "i-x1", "text": "", "attachments": attachment()})),
    )
    .await;
    assert_eq!(prompt.status, 200, "raw: {}", prompt.raw);
    let events = until_kinds(&daemon, &path, &["turn.completed"]).await;
    assert_eq!(
        user_images(&events),
        vec![json!({"blob": png_sha(), "mime": "image/png"})],
        "{}",
        dump(&events)
    );

    let input = last_recorded(&ws, "fixture-codex-input");
    let items = input.as_array().expect("input items");
    assert_eq!(items.len(), 1, "no empty text item: {input}");
    assert_eq!(items[0]["type"], "localImage", "{input}");
    let file = std::path::PathBuf::from(items[0]["path"].as_str().unwrap());
    assert_eq!(file.extension().and_then(|e| e.to_str()), Some("png"));
    assert_eq!(std::fs::read(&file).unwrap(), PNG);
    picture_is_served(&daemon).await;
}

#[tokio::test]
async fn queued_and_steered_messages_keep_their_pictures() {
    let daemon = start_daemon(&[("claude", fixture_agent())]).await;
    let ws = workspace(Some("hold"));
    let id = create(&daemon, "claude", &ws).await;
    let path = format!("/sessions/{id}/events");

    call(
        &daemon,
        "POST",
        &format!("/sessions/{id}/prompts"),
        Some(json!({"request_id": "i-q0", "text": "first"})),
    )
    .await;
    until_kinds(&daemon, &path, &["tool.started"]).await;
    let queued = call(
        &daemon,
        "POST",
        &format!("/sessions/{id}/queue"),
        Some(json!({"request_id": "i-q1", "text": "queued", "attachments": attachment()})),
    )
    .await;
    assert_eq!(queued.status, 200, "raw: {}", queued.raw);
    // The queue holds where the picture is, never its bytes.
    assert_eq!(
        queued.body["result"]["queue"][0]["images"],
        json!([{"blob": png_sha(), "mime": "image/png"}]),
        "{}",
        queued.raw
    );
    std::fs::write(ws.path().join("fixture-release"), b"go").unwrap();
    let events = until_count(&daemon, &path, "turn.completed", 2).await;
    until_status(&daemon, &id, &["idle"]).await;
    assert_eq!(user_images(&events).len(), 1, "{}", dump(&events));
    let content = last_recorded(&ws, "fixture-claude-input");
    assert_eq!(content[1]["source"]["data"], png_b64(), "{content}");

    // With nothing running, a steered message simply sends - pictures too.
    std::fs::write(ws.path().join("fixture-scenario"), "plain").unwrap();
    let steered = call(
        &daemon,
        "POST",
        &format!("/sessions/{id}/steer"),
        Some(json!({"request_id": "i-s1", "text": "steered", "attachments": attachment()})),
    )
    .await;
    assert_eq!(steered.status, 200, "raw: {}", steered.raw);
    let events = until_count(&daemon, &path, "turn.completed", 3).await;
    assert_eq!(user_images(&events).len(), 2, "{}", dump(&events));
    let content = last_recorded(&ws, "fixture-claude-input");
    assert_eq!(content[0]["text"], "steered", "{content}");
    assert_eq!(content[1]["source"]["data"], png_b64(), "{content}");
}

#[tokio::test]
async fn a_harness_that_reads_no_pictures_refuses_them_before_anything_is_written() {
    let daemon = start_daemon(&[("grok", fixture_agent())]).await;
    let ws = workspace(Some("plain"));
    let id = create(&daemon, "grok", &ws).await;
    let log = daemon
        .data_dir()
        .join("sessions")
        .join(&id)
        .join("events.jsonl");
    let before = std::fs::read_to_string(&log).expect("the chat's log");
    // A mention is a write too (the chat's grant to read another): a
    // refused message must not leave one behind.
    let other = uuid::Uuid::new_v4().to_string();

    for (route, request) in [("prompts", "i-g1"), ("queue", "i-g2"), ("steer", "i-g3")] {
        let refused = call(
            &daemon,
            "POST",
            &format!("/sessions/{id}/{route}"),
            Some(json!({
                "request_id": request,
                "text": "look",
                "mentioned_chats": [other],
                "attachments": attachment()
            })),
        )
        .await;
        assert_eq!(refused.status, 409, "{route}: {}", refused.raw);
        assert!(
            refused
                .raw
                .contains("Grok Build doesn't read pictures - send the message without them"),
            "{route}: {}",
            refused.raw
        );
    }

    // Nothing was kept or written: no picture, no new events.
    let blob = raw_http(&daemon, "GET", &format!("/blobs/{}", png_sha()), &[], None).await;
    assert_eq!(blob.status, 404, "{}", blob.raw);
    let after = std::fs::read_to_string(&log).expect("the chat's log");
    assert_eq!(before, after, "the refusals wrote nothing to the log");
    let session = call(&daemon, "GET", &format!("/sessions/{id}"), None).await;
    assert_eq!(queue_len(&session.body), 0, "{}", session.raw);
    assert!(
        session.body["mentioned_chats"].is_null(),
        "no chat was granted: {}",
        session.raw
    );
    assert!(!daemon.data_dir().join("blobs").exists());

    // No receipt was kept either: the same request without its picture
    // is a new message, not the refusal again.
    let words = call(
        &daemon,
        "POST",
        &format!("/sessions/{id}/prompts"),
        Some(json!({"request_id": "i-g1", "text": "look"})),
    )
    .await;
    assert_eq!(words.status, 200, "{}", words.raw);
}

#[tokio::test]
async fn an_attachment_past_the_limits_is_refused_in_words() {
    let daemon = start_daemon(&[("claude", fixture_agent())]).await;
    let ws = workspace(Some("plain"));
    let id = create(&daemon, "claude", &ws).await;
    let mut big = PNG.to_vec();
    big.resize(5 * 1024 * 1024 + 1, 0);
    let refused = call(
        &daemon,
        "POST",
        &format!("/sessions/{id}/prompts"),
        Some(json!({
            "request_id": "i-big",
            "text": "too big",
            "attachments": [{"kind": "image", "mime": "image/png", "data": STANDARD.encode(&big)}]
        })),
    )
    .await;
    assert_eq!(refused.status, 400, "{}", refused.raw);
    assert!(refused.raw.contains("over 5 MB"), "{}", refused.raw);
}
