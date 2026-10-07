//! Chats that run on one of the user's devices and are read and driven from
//! the others, copied to every device, and deleted everywhere.

mod common;

use common::cloud::{device, make_machine, until};
use common::{call, kind, until_count, until_kinds, workspace};
use fake_cloud::FakeCloud;
use serde_json::{Value, json};

fn rid() -> String {
    uuid::Uuid::new_v4().to_string()
}

async fn sessions(daemon: &common::TestDaemon) -> Vec<Value> {
    call(daemon, "GET", "/sessions", None)
        .await
        .body
        .as_array()
        .cloned()
        .unwrap_or_default()
}

#[tokio::test]
async fn a_chat_runs_on_a_machine_and_is_driven_from_a_desktop() {
    let cloud = FakeCloud::start().await;
    let (desk, desk_id) = device(&cloud, "desk").await;
    let (machine, machine_id) = device(&cloud, "box").await;
    make_machine(&desk, &machine, &machine_id).await;
    until("the desk sees the machine online", || {
        common::cloud::sees(&desk, &machine_id, "online", json!(true))
    })
    .await;
    let ws = workspace(Some("approve"));

    // Created from the desk, it runs on the machine and is copied back.
    let created = call(
        &desk,
        "POST",
        "/sessions",
        Some(json!({"request_id": rid(), "harness": "claude", "workspace": ws.path(), "device_id": machine_id})),
    )
    .await;
    assert_eq!(created.status, 201, "{}", created.raw);
    let id = created.body["id"].as_str().unwrap().to_string();
    assert_eq!(created.body["executor"], machine_id.as_str());
    let copy = sessions(&desk).await;
    assert!(
        copy.iter().any(|s| s["id"] == id.as_str()),
        "the desk holds a copy"
    );
    let on_machine = sessions(&machine).await;
    assert!(
        on_machine
            .iter()
            .any(|s| s["id"] == id.as_str() && s["executor"] == machine_id.as_str())
    );

    // Prompted from the desk; the desk's open chat streams the machine's turn.
    let prompt = call(
        &desk,
        "POST",
        &format!("/sessions/{id}/prompts"),
        Some(json!({"request_id": rid(), "text": "run the thing"})),
    )
    .await;
    assert_eq!(prompt.status, 200, "{}", prompt.raw);
    let events = until_kinds(
        &desk,
        &format!("/sessions/{id}/events"),
        &["decision.requested"],
    )
    .await;
    let decision = events
        .iter()
        .find(|(_, p)| kind(p) == "decision.requested")
        .map(|(_, p)| p["decision"].clone())
        .expect("the machine's question reached the desk");

    // Answered from the desk, on the machine.
    let choice = decision["options"][0]["id"]
        .as_str()
        .unwrap_or("allow")
        .to_string();
    let answer = call(
        &desk,
        "POST",
        &format!("/decisions/{}/answer", decision["id"].as_str().unwrap()),
        Some(json!({"request_id": rid(), "choice": choice})),
    )
    .await;
    assert_eq!(answer.status, 200, "{}", answer.raw);
    until_count(
        &desk,
        &format!("/sessions/{id}/events"),
        "turn.completed",
        1,
    )
    .await;

    // A copy can't be acted on here directly, and the desk's private chats
    // stay out of the machine's reach.
    let private_ws = workspace(None);
    let private = call(
        &desk,
        "POST",
        "/sessions",
        Some(json!({"request_id": rid(), "harness": "claude", "workspace": private_ws.path()})),
    )
    .await;
    assert_eq!(private.status, 201);
    assert!(
        private.body["executor"].is_null(),
        "Local chats stay private"
    );
    make_machine(&machine, &desk, &desk_id).await;
    let reach = call(
        &machine,
        "POST",
        &format!(
            "/devices/{desk_id}/sessions/{}/prompts",
            private.body["id"].as_str().unwrap()
        ),
        Some(json!({"request_id": rid(), "text": "hi"})),
    )
    .await;
    assert_eq!(
        reach.status, 404,
        "a private chat is invisible to peers: {}",
        reach.raw
    );
}

#[tokio::test]
async fn laptops_that_are_never_online_together_meet_through_a_machine() {
    let cloud = FakeCloud::start().await;
    let (machine, machine_id) = device(&cloud, "box").await;
    let ws = workspace(Some("plain"));
    let id;
    {
        let (first, _) = device(&cloud, "laptop-1").await;
        make_machine(&first, &machine, &machine_id).await;
        until("laptop-1 sees the machine", || {
            common::cloud::sees(&first, &machine_id, "online", json!(true))
        })
        .await;
        let created = call(
            &first,
            "POST",
            "/sessions",
            Some(json!({"request_id": rid(), "harness": "claude", "workspace": ws.path(), "device_id": machine_id})),
        )
        .await;
        assert_eq!(created.status, 201, "{}", created.raw);
        id = created.body["id"].as_str().unwrap().to_string();
        let prompt = call(
            &first,
            "POST",
            &format!("/sessions/{id}/prompts"),
            Some(json!({"request_id": rid(), "text": "hello"})),
        )
        .await;
        assert_eq!(prompt.status, 200, "{}", prompt.raw);
        until_count(
            &first,
            &format!("/sessions/{id}/events"),
            "turn.completed",
            1,
        )
        .await;
    }

    // Laptop 2 signs in later and gets the chat, whole, from the machine.
    let (second, _) = device(&cloud, "laptop-2").await;
    let copied = until("laptop-2 has the chat", || async {
        sessions(&second)
            .await
            .into_iter()
            .find(|s| s["id"] == id.as_str() && s["title"] == "hello")
    })
    .await;
    assert_eq!(copied["executor"], machine_id.as_str());
    until_count(
        &second,
        &format!("/sessions/{id}/events"),
        "turn.completed",
        1,
    )
    .await;

    // Deleted from laptop 2: gone on the machine too, and never comes back.
    let deleted = call(&second, "DELETE", &format!("/sessions/{id}"), None).await;
    assert_eq!(deleted.status, 204, "{}", deleted.raw);
    until("the machine deletes it", || async {
        (!sessions(&machine)
            .await
            .iter()
            .any(|s| s["id"] == id.as_str()))
        .then_some(())
    })
    .await;
    assert!(
        !sessions(&second)
            .await
            .iter()
            .any(|s| s["id"] == id.as_str())
    );
}
