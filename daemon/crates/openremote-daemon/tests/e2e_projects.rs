//! Projects e2e: registration CRUD with the same folder bar a chat's own
//! create holds (a folder registers once), removal that leaves chats
//! untouched, and the project-less chat - no workspace at all, spawned in
//! the home folder - running the same fixture-driven path every chat runs.

mod common;

use common::*;
use serde_json::json;

fn project_body(ws: &tempfile::TempDir, over: serde_json::Value) -> serde_json::Value {
    let mut body = json!({
        "request_id": uuid::Uuid::new_v4().to_string(),
        "folders": [ws.path()],
    });
    let obj = body.as_object_mut().unwrap();
    for (key, value) in over.as_object().unwrap() {
        obj.insert(key.clone(), value.clone());
    }
    body
}

#[tokio::test]
async fn a_folder_registers_as_a_project_once() {
    let daemon = start_daemon(&[]).await;
    let ws = workspace(Some("plain"));

    let saved = call(
        &daemon,
        "POST",
        "/projects",
        Some(project_body(&ws, json!({}))),
    )
    .await;
    assert_eq!(saved.status, 201, "raw: {}", saved.raw);
    assert!(saved.body["id"].as_str().is_some());
    let folders: Vec<&str> = saved.body["folders"]
        .as_array()
        .expect("folders")
        .iter()
        .map(|f| f.as_str().expect("path"))
        .collect();
    assert_eq!(folders.len(), 1);
    assert_eq!(std::path::Path::new(folders[0]), ws.path());

    // The same folder in another project is refused.
    let dup = call(
        &daemon,
        "POST",
        "/projects",
        Some(project_body(&ws, json!({}))),
    )
    .await;
    assert_eq!(dup.status, 409, "raw: {}", dup.raw);

    // The folder must be real - the same bar a chat's own create holds.
    let bad = call(
        &daemon,
        "POST",
        "/projects",
        Some(project_body(
            &ws,
            json!({"folders": ["C:/definitely/not/a/folder"]}),
        )),
    )
    .await;
    assert_eq!(bad.status, 409, "raw: {}", bad.raw);

    // No folders is not a project.
    let bare = call(
        &daemon,
        "POST",
        "/projects",
        Some(json!({"request_id": uuid::Uuid::new_v4().to_string(), "folders": []})),
    )
    .await;
    assert_eq!(bare.status, 409, "raw: {}", bare.raw);

    // The list holds what was saved.
    let list = call(&daemon, "GET", "/projects", None).await;
    assert_eq!(list.status, 200, "raw: {}", list.raw);
    assert_eq!(list.body.as_array().map(Vec::len), Some(1));
}

#[tokio::test]
async fn removal_unregisters_but_never_deletes() {
    let daemon = start_daemon(&[]).await;
    let ws = workspace(Some("plain"));

    let saved = call(
        &daemon,
        "POST",
        "/projects",
        Some(project_body(&ws, json!({}))),
    )
    .await;
    assert_eq!(saved.status, 201, "raw: {}", saved.raw);
    let project_id = saved.body["id"].as_str().expect("project id").to_string();

    // A chat in the project's folder.
    let chat = call(
        &daemon,
        "POST",
        "/sessions",
        Some(json!({
            "request_id": uuid::Uuid::new_v4().to_string(),
            "harness": "claude",
            "workspace": ws.path(),
        })),
    )
    .await;
    assert_eq!(chat.status, 201, "raw: {}", chat.raw);
    let chat_id = chat.body["id"].as_str().expect("session id").to_string();

    // Removing the project leaves the chat and the folder.
    let removed = call(
        &daemon,
        "DELETE",
        &format!("/projects/{project_id}?request_id={}", uuid::Uuid::new_v4()),
        None,
    )
    .await;
    assert_eq!(removed.status, 200, "raw: {}", removed.raw);
    assert!(ws.path().is_dir(), "the folder survives");

    let list = call(&daemon, "GET", "/projects", None).await;
    assert_eq!(list.body.as_array().map(Vec::len), Some(0));

    let session = call(&daemon, "GET", &format!("/sessions/{chat_id}"), None).await;
    assert_eq!(session.status, 200, "raw: {}", session.raw);
    assert_eq!(
        session.body["workspace"].as_str(),
        Some(ws.path().to_str().unwrap())
    );

    // The folder is registrable again.
    let again = call(
        &daemon,
        "POST",
        "/projects",
        Some(project_body(&ws, json!({}))),
    )
    .await;
    assert_eq!(again.status, 201, "raw: {}", again.raw);
}

#[tokio::test]
async fn a_chat_can_belong_to_no_project() {
    let daemon = start_daemon(&[]).await;

    // No workspace key at all - the project-less chat.
    let created = call(
        &daemon,
        "POST",
        "/sessions",
        Some(json!({
            "request_id": uuid::Uuid::new_v4().to_string(),
            "harness": "claude",
        })),
    )
    .await;
    assert_eq!(created.status, 201, "raw: {}", created.raw);
    let chat = created.body["id"].as_str().expect("session id").to_string();
    assert!(
        created.body.get("workspace").is_none(),
        "the project-less chat names no workspace: {}",
        created.raw
    );

    // It runs the same path every chat runs: a prompt, a turn, a reply.
    let prompt = call(
        &daemon,
        "POST",
        &format!("/sessions/{chat}/prompts"),
        Some(json!({
            "request_id": uuid::Uuid::new_v4().to_string(),
            "text": "List the files in this folder.",
        })),
    )
    .await;
    assert_eq!(prompt.status, 200, "raw: {}", prompt.raw);

    let events = until_kinds(
        &daemon,
        &format!("/sessions/{chat}/events"),
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
    assert!(
        events.iter().any(|(_, p)| kind(p) == "message.added"),
        "the project-less chat answered:\n{}",
        dump(&events)
    );

    // An explicitly-null workspace is the same thing.
    let explicit = call(
        &daemon,
        "POST",
        "/sessions",
        Some(json!({
            "request_id": uuid::Uuid::new_v4().to_string(),
            "harness": "claude",
            "workspace": null,
        })),
    )
    .await;
    assert_eq!(explicit.status, 201, "raw: {}", explicit.raw);

    // The old bar still holds where a workspace is named.
    let bad = call(
        &daemon,
        "POST",
        "/sessions",
        Some(json!({
            "request_id": uuid::Uuid::new_v4().to_string(),
            "harness": "claude",
            "workspace": "C:/definitely/not/a/folder",
        })),
    )
    .await;
    assert_eq!(bad.status, 422, "raw: {}", bad.raw);
}
