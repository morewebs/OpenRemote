//! OpenCode e2e: the real HTTP API + SSE driving the real
//! openremote-opencode driver against the fixture's serve mode — a real
//! HTTP server with basic auth and the SSE bus, so the driver's entire
//! HTTP surface is exercised (spawn, listening-line discovery, auth,
//! session create, prompt, the event bus, permission and question
//! replies over HTTP).

mod common;

use common::*;
use serde_json::json;

async fn opencode_daemon(scenario: Option<&str>) -> (TestDaemon, tempfile::TempDir) {
    let daemon = start_daemon(&[
        ("claude", fixture_agent()),
        ("codex", fixture_agent()),
        ("grok", fixture_agent()),
        ("pi", fixture_agent()),
        ("opencode", fixture_agent()),
    ])
    .await;
    let ws = workspace(scenario);
    (daemon, ws)
}

async fn create_opencode(daemon: &TestDaemon, ws: &tempfile::TempDir) -> String {
    let created = call(
        daemon,
        "POST",
        "/sessions",
        Some(json!({"request_id": "oc-create", "harness": "opencode", "workspace": ws.path()})),
    )
    .await;
    assert_eq!(created.status, 201, "raw: {}", created.raw);
    created.body["id"].as_str().expect("session id").to_string()
}

#[tokio::test]
async fn an_opencode_prompt_runs_a_real_turn_over_http() {
    let (daemon, ws) = opencode_daemon(Some("plain")).await;
    let id = create_opencode(&daemon, &ws).await;

    let events = until_kinds(
        &daemon,
        &format!("/sessions/{id}/events"),
        &["session.status_changed"],
    )
    .await;
    let _ = events;
    let session = call(&daemon, "GET", &format!("/sessions/{id}"), None).await;
    assert_eq!(
        session.body["harness_session_ref"].as_str(),
        Some("ses_fixture_1"),
        "opencode's own session id is the ref: {}",
        session.raw
    );

    call(
        &daemon,
        "POST",
        &format!("/sessions/{id}/prompts"),
        Some(json!({"request_id": "oc-p1", "text": "ship the notes"})),
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
    assert_eq!(completed.1["outcome"], "stop", "opencode's own word");
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
async fn opencode_permissions_flow_with_opencode_words() {
    let (daemon, ws) = opencode_daemon(Some("ask")).await;
    let id = create_opencode(&daemon, &ws).await;
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
        Some(json!({"request_id": "oc-p1", "text": "run the thing"})),
    )
    .await;
    let events = until_kinds(
        &daemon,
        &format!("/sessions/{id}/events"),
        &["decision.requested", "session.status_changed"],
    )
    .await;
    let decision = events
        .iter()
        .find(|(_, p)| kind(p) == "decision.requested")
        .and_then(|(_, p)| p.get("decision"))
        .expect("the decision");
    assert_eq!(decision["kind"], "approval");
    assert_eq!(
        decision["harness_request"]["action"], "bash",
        "opencode's own permission action rides verbatim"
    );
    let options: Vec<&str> = decision["options"]
        .as_array()
        .expect("options")
        .iter()
        .map(|o| o["id"].as_str().expect("option id"))
        .collect();
    // OpenCode's own reply words.
    assert_eq!(options, vec!["once", "always", "reject"]);
    let decision_id = decision["id"].as_str().expect("decision id").to_string();

    let answer = call(
        &daemon,
        "POST",
        &format!("/decisions/{decision_id}/answer"),
        Some(json!({"request_id": "oc-a1", "choice": "once"})),
    )
    .await;
    assert_eq!(answer.status, 200, "raw: {}", answer.raw);

    let events = until_kinds(
        &daemon,
        &format!("/sessions/{id}/events"),
        &[
            "decision.responded",
            "session.status_changed",
            "tool.started",
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
    assert_eq!(tool.1["name"], "bash");
    assert_eq!(tool.1["is_error"], false);
    let completed = events
        .iter()
        .find(|(_, p)| kind(p) == "turn.completed")
        .expect("completed");
    assert_eq!(completed.1["outcome"], "stop");
}

#[tokio::test]
async fn opencode_questions_flow_with_their_own_labels() {
    let (daemon, ws) = opencode_daemon(Some("question")).await;
    let id = create_opencode(&daemon, &ws).await;
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
        Some(json!({"request_id": "oc-p1", "text": "migrate the db"})),
    )
    .await;
    let events = until_kinds(
        &daemon,
        &format!("/sessions/{id}/events"),
        &["decision.requested", "session.status_changed"],
    )
    .await;
    let decision = events
        .iter()
        .find(|(_, p)| kind(p) == "decision.requested")
        .and_then(|(_, p)| p.get("decision"))
        .expect("the decision");
    assert_eq!(decision["kind"], "question");
    assert_eq!(
        decision["harness_request"]["questions"][0]["question"],
        "Which database?"
    );
    let options: Vec<&str> = decision["options"]
        .as_array()
        .expect("options")
        .iter()
        .map(|o| o["id"].as_str().expect("option id"))
        .collect();
    assert_eq!(options, vec!["Postgres", "SQLite"]);
    let decision_id = decision["id"].as_str().expect("decision id").to_string();

    let answer = call(
        &daemon,
        "POST",
        &format!("/decisions/{decision_id}/answer"),
        Some(json!({"request_id": "oc-a1", "choice": "Postgres"})),
    )
    .await;
    assert_eq!(answer.status, 200, "raw: {}", answer.raw);
    let events = until_kinds(
        &daemon,
        &format!("/sessions/{id}/events"),
        &[
            "decision.responded",
            "session.status_changed",
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
            .contains("Postgres"),
        "the chosen label flows into the turn; events:\n{}",
        dump(&events)
    );
}

#[tokio::test]
async fn opencode_interrupts_and_resumes() {
    let (daemon, ws) = opencode_daemon(Some("ask")).await;
    let id = create_opencode(&daemon, &ws).await;
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
        Some(json!({"request_id": "oc-p1", "text": "needs approval"})),
    )
    .await;
    until_kinds(
        &daemon,
        &format!("/sessions/{id}/events"),
        &["decision.requested", "session.status_changed"],
    )
    .await;

    // Interrupt while the permission is pending.
    call(
        &daemon,
        "POST",
        &format!("/sessions/{id}/interrupt"),
        Some(json!({"request_id": "oc-int"})),
    )
    .await;
    let events = until_kinds(
        &daemon,
        &format!("/sessions/{id}/events"),
        &["turn.completed", "session.status_changed"],
    )
    .await;
    let completed = events
        .iter()
        .find(|(_, p)| kind(p) == "turn.completed")
        .expect("completed");
    assert_eq!(completed.1["outcome"], "aborted", "opencode's own word");
    assert_eq!(completed.1["coarse"], "interrupted");

    // Stop kills the serve child; resume reattaches the same session id.
    let stopped = call(
        &daemon,
        "POST",
        &format!("/sessions/{id}/stop"),
        Some(json!({"request_id": "oc-stop"})),
    )
    .await;
    assert_eq!(stopped.body["status"], "stopped", "raw: {}", stopped.raw);
    let resumed = call(
        &daemon,
        "POST",
        &format!("/sessions/{id}/resume"),
        Some(json!({"request_id": "oc-resume"})),
    )
    .await;
    assert_eq!(resumed.status, 200, "raw: {}", resumed.raw);
    let session = call(&daemon, "GET", &format!("/sessions/{id}"), None).await;
    assert_eq!(
        session.body["harness_session_ref"].as_str(),
        Some("ses_fixture_1"),
        "resume rides the same opencode session"
    );
}
