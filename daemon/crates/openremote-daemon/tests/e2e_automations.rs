//! Automations e2e: rules CRUD with their own validations, the webhook
//! (open route, the rule's key is the credential), Run-now (the same path
//! the webhook and the clock take), and the schedule engine firing on the
//! machine's own wall clock through a real daemon process.

mod common;

use common::*;
use serde_json::json;

async fn this_machine_id(daemon: &TestDaemon) -> String {
    let machines = call(daemon, "GET", "/machines", None).await;
    machines
        .body
        .as_array()
        .expect("machines")
        .iter()
        .find(|m| m["machine"]["this_machine"] == json!(true))
        .expect("this machine")["machine"]["id"]
        .as_str()
        .expect("machine id")
        .to_string()
}

fn rule_body(machine: &str, ws: &tempfile::TempDir, over: serde_json::Value) -> serde_json::Value {
    let mut body = json!({
        "request_id": uuid::Uuid::new_v4().to_string(),
        "name": "Nightly dependency audit",
        "trigger": { "kind": "schedule", "time": "09:00" },
        "harness": "claude",
        "workspace": ws.path(),
        "machine": machine,
        "task": "Check outdated dependencies and list the ones that are safe to bump.",
    });
    let obj = body.as_object_mut().unwrap();
    for (key, value) in over.as_object().unwrap() {
        obj.insert(key.clone(), value.clone());
    }
    body
}

#[tokio::test]
async fn rules_validate_their_own_shape() {
    let daemon = start_daemon(&[]).await;
    let machine = this_machine_id(&daemon).await;
    let ws = workspace(Some("plain"));

    // A good schedule rule saves, and its time is normalized through the
    // daemon's own clock.
    let saved = call(
        &daemon,
        "POST",
        "/automations",
        Some(rule_body(&machine, &ws, json!({}))),
    )
    .await;
    assert_eq!(saved.status, 201, "raw: {}", saved.raw);
    assert_eq!(saved.body["trigger"]["time"], "09:00");
    assert_eq!(saved.body["enabled"], true);
    assert!(saved.body["id"].as_str().is_some());

    // A schedule without a real HH:MM is refused.
    let bad_time = call(
        &daemon,
        "POST",
        "/automations",
        Some(rule_body(
            &machine,
            &ws,
            json!({"trigger": {"kind": "schedule", "time": "9am"}}),
        )),
    )
    .await;
    assert_eq!(bad_time.status, 409, "raw: {}", bad_time.raw);

    // The workspace must be real - the same bar a chat's own create holds.
    let bad_ws = call(
        &daemon,
        "POST",
        "/automations",
        Some(rule_body(
            &machine,
            &ws,
            json!({"workspace": "C:/definitely/not/a/folder"}),
        )),
    )
    .await;
    assert_eq!(bad_ws.status, 409, "raw: {}", bad_ws.raw);

    // A name and a task are the rule.
    let bare = call(
        &daemon,
        "POST",
        "/automations",
        Some(rule_body(
            &machine,
            &ws,
            json!({"name": "  ", "task": "  "}),
        )),
    )
    .await;
    assert_eq!(bare.status, 409, "raw: {}", bare.raw);
}

#[tokio::test]
async fn run_now_opens_the_chat_and_records_it() {
    let daemon = start_daemon(&[]).await;
    let machine = this_machine_id(&daemon).await;
    let ws = workspace(Some("plain"));

    let saved = call(
        &daemon,
        "POST",
        "/automations",
        Some(rule_body(&machine, &ws, json!({}))),
    )
    .await;
    assert_eq!(saved.status, 201, "raw: {}", saved.raw);
    let rule_id = saved.body["id"].as_str().expect("rule id").to_string();

    // Run now: the same path the webhook and the clock take.
    let run = call(
        &daemon,
        "POST",
        &format!("/automations/{rule_id}/run"),
        Some(json!({"request_id": uuid::Uuid::new_v4().to_string()})),
    )
    .await;
    assert_eq!(run.status, 200, "raw: {}", run.raw);
    let chat = run.body["id"].as_str().expect("session id").to_string();

    // The task was delivered as the chat's first prompt.
    let events = until_kinds(
        &daemon,
        &format!("/sessions/{chat}/events"),
        &[
            "turn.started",
            "message.added",
            // the init's model echo rides as session.updated
            "session.updated",
            "message.added",
            "context.used",
            "usage.cost",
            "turn.completed",
            "session.status_changed",
        ],
    )
    .await;
    let user = events
        .iter()
        .find(|(_, p)| kind(p) == "message.added" && p["message"]["role"] == "user")
        .expect("the task as the first prompt");
    assert!(
        user.1["message"]["text"]
            .as_str()
            .unwrap_or_default()
            .contains("outdated dependencies"),
        "events:\n{}",
        dump(&events)
    );

    // The rule remembers its last chat - the list's jump.
    let rules = call(&daemon, "GET", "/automations", None).await;
    let rule = &rules.body.as_array().expect("rules")[0];
    assert_eq!(rule["last_chat"].as_str(), Some(chat.as_str()));

    // A disabled rule never runs.
    let off = call(
        &daemon,
        "POST",
        &format!("/automations/{rule_id}/enabled"),
        Some(json!({"request_id": uuid::Uuid::new_v4().to_string(), "enabled": false})),
    )
    .await;
    assert_eq!(off.status, 200, "raw: {}", off.raw);
    let refused = call(
        &daemon,
        "POST",
        &format!("/automations/{rule_id}/run"),
        Some(json!({"request_id": uuid::Uuid::new_v4().to_string()})),
    )
    .await;
    assert_eq!(refused.status, 409, "raw: {}", refused.raw);

    // And the rule goes away.
    let removed = call(
        &daemon,
        "DELETE",
        &format!("/automations/{rule_id}?request_id={}", uuid::Uuid::new_v4()),
        None,
    )
    .await;
    assert_eq!(removed.status, 200, "raw: {}", removed.raw);
    let rules = call(&daemon, "GET", "/automations", None).await;
    assert_eq!(rules.body.as_array().map(Vec::len), Some(0));
}

#[tokio::test]
async fn webhooks_fire_only_with_their_own_key() {
    let daemon = start_daemon(&[]).await;
    let machine = this_machine_id(&daemon).await;
    let ws = workspace(Some("plain"));

    // A webhook rule is born with its key.
    let saved = call(
        &daemon,
        "POST",
        "/automations",
        Some(rule_body(
            &machine,
            &ws,
            json!({"trigger": {"kind": "webhook"}, "name": "Deploy hook"}),
        )),
    )
    .await;
    assert_eq!(saved.status, 201, "raw: {}", saved.raw);
    let rule_id = saved.body["id"].as_str().expect("rule id").to_string();
    let key = saved.body["trigger"]["key"]
        .as_str()
        .expect("key")
        .to_string();

    // The open route refuses every wrong ask.
    let no_key = call(&daemon, "POST", &format!("/hooks/{rule_id}"), None).await;
    assert_eq!(no_key.status, 409, "raw: {}", no_key.raw);
    let wrong_key = call(
        &daemon,
        "POST",
        &format!("/hooks/{rule_id}?key=not-the-key"),
        None,
    )
    .await;
    assert_eq!(wrong_key.status, 409, "raw: {}", wrong_key.raw);

    // The right key fires the rule - a chat opens with the task.
    let fired = call(
        &daemon,
        "POST",
        &format!("/hooks/{rule_id}?key={key}"),
        None,
    )
    .await;
    assert_eq!(fired.status, 200, "raw: {}", fired.raw);
    assert_eq!(fired.body["fired"], true);
    let chat = fired.body["chat"].as_str().expect("chat id").to_string();
    let events = until_kinds(
        &daemon,
        &format!("/sessions/{chat}/events"),
        &[
            "turn.started",
            "message.added",
            // the init's model echo rides as session.updated
            "session.updated",
            "message.added",
            "context.used",
            "usage.cost",
            "turn.completed",
            "session.status_changed",
        ],
    )
    .await;
    assert!(
        events
            .iter()
            .any(|(_, p)| kind(p) == "message.added" && p["message"]["role"] == "user"),
        "the task lands; events:\n{}",
        dump(&events)
    );

    // A schedule rule has no webhook - an honest refusal.
    let schedule = call(
        &daemon,
        "POST",
        "/automations",
        Some(rule_body(&machine, &ws, json!({"name": "Nightly"}))),
    )
    .await;
    let schedule_id = schedule.body["id"].as_str().expect("id").to_string();
    let not_a_hook = call(
        &daemon,
        "POST",
        &format!("/hooks/{schedule_id}?key=x"),
        None,
    )
    .await;
    assert_eq!(not_a_hook.status, 409, "raw: {}", not_a_hook.raw);
}

#[tokio::test]
async fn rules_wait_for_a_machine_that_hasnt_checked_in() {
    let daemon = start_daemon(&[]).await;
    let ws = workspace(Some("plain"));

    // A waiting machine: added by name, never checked in.
    let added = call(
        &daemon,
        "POST",
        "/machines",
        Some(json!({
            "request_id": uuid::Uuid::new_v4().to_string(),
            "name": "edge-box",
            "platform": "linux",
        })),
    )
    .await;
    assert_eq!(added.status, 201, "raw: {}", added.raw);
    let waiting = added.body["id"].as_str().expect("machine id").to_string();

    // A rule may name it (the API is the backstop the form fronts) -
    // but it cannot run there today.
    let saved = call(
        &daemon,
        "POST",
        "/automations",
        Some(rule_body(
            &waiting,
            &ws,
            json!({"name": "On the waiting box"}),
        )),
    )
    .await;
    assert_eq!(saved.status, 201, "raw: {}", saved.raw);
    let rule_id = saved.body["id"].as_str().expect("rule id").to_string();

    // Run now refuses with the wait named - never a quiet local run.
    let run = call(
        &daemon,
        "POST",
        &format!("/automations/{rule_id}/run"),
        Some(json!({"request_id": uuid::Uuid::new_v4().to_string()})),
    )
    .await;
    assert_eq!(run.status, 409, "raw: {}", run.raw);
    assert!(
        run.raw.contains("hasn't checked in"),
        "the refusal names the wait: {}",
        run.raw
    );

    // No chat opened anywhere.
    let sessions = call(&daemon, "GET", "/sessions", None).await;
    assert_eq!(
        sessions.body.as_array().map(Vec::len),
        Some(0),
        "the refusal opened nothing; raw: {}",
        sessions.raw
    );
}

#[tokio::test]
async fn the_clock_fires_schedules_on_the_machines_own_wall_time() {
    // A real daemon process with a fast clock tick and a fixture-backed
    // claude - the rule fires within the minute it was armed for.
    let env: Vec<(&str, String)> = vec![
        (
            "OPENREMOTE_CLAUDE_PATH",
            fixture_agent().display().to_string(),
        ),
        ("OPENREMOTE_SCHEDULE_TICK_MS", "500".to_string()),
    ];
    let daemon = spawn_daemon_process(&env).await;
    let machine = this_machine_id(&daemon).await;
    let ws = workspace(Some("plain"));

    // The machine's own HH:MM - the same clock the engine reads.
    let now = chrono::Local::now();
    let time = now.format("%H:%M").to_string();

    let saved = call(
        &daemon,
        "POST",
        "/automations",
        Some(rule_body(
            &machine,
            &ws,
            json!({"trigger": {"kind": "schedule", "time": time}}),
        )),
    )
    .await;
    assert_eq!(saved.status, 201, "raw: {}", saved.raw);
    let rule_id = saved.body["id"].as_str().expect("rule id").to_string();

    // The clock fires within a few fast ticks.
    let mut chat = None;
    for _ in 0..60 {
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        let rules = call(&daemon, "GET", "/automations", None).await;
        let rule = rules
            .body
            .as_array()
            .expect("rules")
            .iter()
            .find(|r| r["id"] == rule_id.as_str())
            .expect("rule");
        if let Some(id) = rule["last_chat"].as_str() {
            chat = Some(id.to_string());
            break;
        }
    }
    let chat = chat.expect("the clock fired the rule");
    let events = until_kinds(
        &daemon,
        &format!("/sessions/{chat}/events"),
        &[
            "turn.started",
            "message.added",
            // the init's model echo rides as session.updated
            "session.updated",
            "message.added",
            "context.used",
            "usage.cost",
            "turn.completed",
            "session.status_changed",
        ],
    )
    .await;
    assert!(
        events
            .iter()
            .any(|(_, p)| kind(p) == "message.added" && p["message"]["role"] == "user"),
        "the task lands; events:\n{}",
        dump(&events)
    );

    // And the engine's arming holds: no second chat in the same minute.
    tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
    let sessions = call(&daemon, "GET", "/sessions", None).await;
    assert_eq!(
        sessions.body.as_array().map(Vec::len),
        Some(1),
        "one fire per minute; raw: {}",
        sessions.raw
    );
}
