//! Moving a chat that runs on another of the user's devices: the desk's move
//! is forwarded to the machine that runs the chat, and the machine serves
//! `/sessions/{id}/workspace` to the account's own devices only, through the
//! peer gate - a synced chat that runs there, into one of that machine's own
//! projects, named by id and never by a path. Everything else is refused.

mod common;

use common::cloud::{device, make_machine, until};
use common::{call, call_with_token, until_status, workspace};
use fake_cloud::FakeCloud;
use serde_json::{Value, json};

fn rid() -> String {
    uuid::Uuid::new_v4().to_string()
}

fn folder_of(session: &Value) -> Option<String> {
    session["workspace"].as_str().map(str::to_string)
}

#[tokio::test]
async fn a_machine_moves_a_synced_chat_for_the_accounts_devices_and_nothing_more() {
    let cloud = FakeCloud::start().await;
    let (desk, desk_id) = device(&cloud, "desk").await;
    let (machine, machine_id) = device(&cloud, "box").await;
    make_machine(&desk, &machine, &machine_id).await;
    until("the desk sees the machine online", || {
        common::cloud::sees(&desk, &machine_id, "online", json!(true))
    })
    .await;

    // A chat that runs on the machine, made from the desk, and one of the
    // machine's own projects to move it into.
    let from = workspace(Some("plain"));
    let to = workspace(Some("plain"));
    let created = call(
        &desk,
        "POST",
        "/sessions",
        Some(json!({"request_id": rid(), "harness": "claude", "workspace": from.path(), "device_id": machine_id})),
    )
    .await;
    assert_eq!(created.status, 201, "{}", created.raw);
    let id = created.body["id"].as_str().unwrap().to_string();
    until_status(&machine, &id, &["idle", "starting"]).await;
    let project = call(
        &machine,
        "POST",
        "/projects",
        Some(json!({"request_id": rid(), "folders": [to.path()]})),
    )
    .await;
    assert_eq!(project.status, 201, "{}", project.raw);
    let project = project.body["id"].as_str().unwrap().to_string();

    // The desk's own route, forwarded to the machine that runs the chat.
    let moved = call(
        &desk,
        "POST",
        &format!("/sessions/{id}/workspace"),
        Some(json!({"request_id": rid(), "project_id": project})),
    )
    .await;
    assert_eq!(moved.status, 200, "{}", moved.raw);
    let there = call(&machine, "GET", &format!("/sessions/{id}"), None).await;
    assert_eq!(
        folder_of(&there.body),
        Some(to.path().to_string_lossy().into_owned()),
        "the machine moved its own chat: {}",
        there.raw
    );

    // The machine's peer route, asked directly: out of every project.
    let home = call(
        &desk,
        "POST",
        &format!("/devices/{machine_id}/sessions/{id}/workspace"),
        Some(json!({"request_id": rid(), "project_id": null})),
    )
    .await;
    assert_eq!(home.status, 200, "{}", home.raw);
    let there = call(&machine, "GET", &format!("/sessions/{id}"), None).await;
    assert_eq!(folder_of(&there.body), None, "{}", there.raw);

    // A path is never a destination: a project is named by its id, so a
    // folder, a traversal or anything else that is not one is refused
    // before the chat is touched.
    let outside = std::env::temp_dir();
    for bad in [
        outside.to_string_lossy().into_owned(),
        "../../../Windows".to_string(),
        format!("{project}/../.."),
        "..".to_string(),
    ] {
        let refused = call(
            &desk,
            "POST",
            &format!("/devices/{machine_id}/sessions/{id}/workspace"),
            Some(json!({"request_id": rid(), "project_id": bad})),
        )
        .await;
        assert_eq!(refused.status, 400, "{bad}: {}", refused.raw);
    }
    // A stray folder field is not a way in either: the body names a project
    // or none, and this one names none of the machine's projects.
    let stray = call(
        &desk,
        "POST",
        &format!("/devices/{machine_id}/sessions/{id}/workspace"),
        Some(json!({
            "request_id": rid(),
            "project_id": uuid::Uuid::new_v4().to_string(),
            "workspace": outside,
        })),
    )
    .await;
    assert_eq!(stray.status, 404, "{}", stray.raw);
    let there = call(&machine, "GET", &format!("/sessions/{id}"), None).await;
    assert_eq!(
        folder_of(&there.body),
        None,
        "no refused move touched the chat"
    );

    // A project of another device is not one of the machine's.
    let desk_folder = workspace(None);
    let desk_project = call(
        &desk,
        "POST",
        "/projects",
        Some(json!({"request_id": rid(), "folders": [desk_folder.path()]})),
    )
    .await;
    assert_eq!(desk_project.status, 201, "{}", desk_project.raw);
    let foreign = call(
        &desk,
        "POST",
        &format!("/devices/{machine_id}/sessions/{id}/workspace"),
        Some(json!({"request_id": rid(), "project_id": desk_project.body["id"]})),
    )
    .await;
    assert_eq!(foreign.status, 404, "{}", foreign.raw);

    // A private chat on the machine is invisible to every peer.
    let private_ws = workspace(Some("plain"));
    let private = call(
        &machine,
        "POST",
        "/sessions",
        Some(json!({"request_id": rid(), "harness": "claude", "workspace": private_ws.path()})),
    )
    .await;
    assert_eq!(private.status, 201, "{}", private.raw);
    assert!(private.body["executor"].is_null(), "{}", private.raw);
    let hidden = call(
        &desk,
        "POST",
        &format!(
            "/devices/{machine_id}/sessions/{}/workspace",
            private.body["id"].as_str().unwrap()
        ),
        Some(json!({"request_id": rid(), "project_id": project})),
    )
    .await;
    assert_eq!(hidden.status, 404, "{}", hidden.raw);

    // A device the account never paired is no peer at all.
    let stranger = call(
        &desk,
        "POST",
        &format!("/devices/{}/sessions/{id}/workspace", uuid::Uuid::new_v4()),
        Some(json!({"request_id": rid(), "project_id": project})),
    )
    .await;
    assert_eq!(stranger.status, 404, "{}", stranger.raw);

    // The desk's own API wants its own token: a wrong one moves nothing,
    // directly or through the forward.
    for path in [
        format!("/sessions/{id}/workspace"),
        format!("/devices/{machine_id}/sessions/{id}/workspace"),
    ] {
        let wrong = call_with_token(
            &desk,
            "not-the-token",
            "POST",
            &path,
            Some(json!({"request_id": rid(), "project_id": project})),
        )
        .await;
        assert_eq!(wrong.status, 401, "{path}: {}", wrong.raw);
    }
    let there = call(&machine, "GET", &format!("/sessions/{id}"), None).await;
    assert_eq!(folder_of(&there.body), None, "{}", there.raw);

    // A desktop that isn't a machine is driven by nobody: the machine can't
    // move the desk's synced chat on the desk.
    let desk_ws = workspace(Some("plain"));
    let desk_chat = call(
        &desk,
        "POST",
        "/sessions",
        Some(json!({"request_id": rid(), "harness": "claude", "workspace": desk_ws.path(), "synced": true})),
    )
    .await;
    assert_eq!(desk_chat.status, 201, "{}", desk_chat.raw);
    let not_machine = call(
        &machine,
        "POST",
        &format!(
            "/devices/{desk_id}/sessions/{}/workspace",
            desk_chat.body["id"].as_str().unwrap()
        ),
        Some(json!({"request_id": rid(), "project_id": null})),
    )
    .await;
    assert_eq!(not_machine.status, 409, "{}", not_machine.raw);
    assert_eq!(not_machine.body["code"], "not_a_machine");
}
