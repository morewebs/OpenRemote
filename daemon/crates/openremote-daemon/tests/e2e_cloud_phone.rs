//! The Android app: a phone runs the daemon in-process with no harnesses,
//! signs in through the app's own return address, and drives chats that
//! run on the user's machines. It is never a machine itself.

mod common;

use common::cloud::{config, device, make_machine, sees, until};
use common::{call, kind, start_embedded, until_count, until_kinds, workspace};
use fake_cloud::FakeCloud;
use openremote_cloud::https::Http;
use serde_json::{Value, json};

fn rid() -> String {
    uuid::Uuid::new_v4().to_string()
}

/// A phone signed in the way the app does it: the browser lands on the
/// app's scheme, and the OS hands that address to the app.
async fn phone(
    cloud: &FakeCloud,
) -> (
    common::TestDaemon,
    std::sync::Arc<openremote_daemon::app::App>,
    String,
) {
    let mut config = config(cloud, "Pixel 8");
    config.platform = "android".into();
    config.redirect_uri = Some(openremote_cloud::config::APP_REDIRECT.into());
    let (daemon, app) = start_embedded(config).await;

    let started = call(
        &daemon,
        "POST",
        "/cloud/signin",
        Some(json!({"open": false})),
    )
    .await;
    assert_eq!(started.body["opened"], false);
    let url = started.body["authorize_url"].as_str().unwrap().to_string();
    assert!(
        url.contains("redirect_uri=space.moreweb.openremote%3A%2Fcloud%2Fcallback"),
        "{url}"
    );
    let page = Http::new().request("GET", &url, &[], None).await.unwrap();
    let location = page.headers["location"].to_str().unwrap().to_string();
    assert!(
        location.starts_with("space.moreweb.openremote:/cloud/callback?"),
        "{location}"
    );
    assert!(openremote_daemon::embedded::sign_in_returned(&app, &location).await);

    let view = call(&daemon, "GET", "/cloud", None).await.body;
    assert_eq!(view["device"]["platform"], "android");
    assert_eq!(view["device"]["kind"], "desktop");
    let id = view["device"]["id"].as_str().unwrap().to_string();
    (daemon, app, id)
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
async fn a_phone_drives_a_chat_on_a_machine() {
    let cloud = FakeCloud::start().await;
    let (phone, app, phone_id) = phone(&cloud).await;
    let (machine, machine_id) = device(&cloud, "box").await;
    make_machine(&phone, &machine, &machine_id).await;
    until("the phone sees the machine online", || {
        sees(&phone, &machine_id, "online", json!(true))
    })
    .await;
    assert_eq!(
        call(&phone, "GET", "/capabilities", None).await.body["harnesses"],
        json!([]),
        "a phone runs no harnesses"
    );

    // Started from the phone, run on the machine, copied to the phone.
    let ws = workspace(Some("approve"));
    let created = call(
        &phone,
        "POST",
        "/sessions",
        Some(json!({"request_id": rid(), "harness": "claude", "workspace": ws.path(), "device_id": machine_id})),
    )
    .await;
    assert_eq!(created.status, 201, "{}", created.raw);
    let id = created.body["id"].as_str().unwrap().to_string();
    assert!(
        sessions(&phone)
            .await
            .iter()
            .any(|s| s["id"] == id.as_str())
    );

    // Prompted, its question answered and its turn finished, all from the
    // phone, streamed live from the phone's own copy.
    let prompt = call(
        &phone,
        "POST",
        &format!("/sessions/{id}/prompts"),
        Some(json!({"request_id": rid(), "text": "run the thing"})),
    )
    .await;
    assert_eq!(prompt.status, 200, "{}", prompt.raw);
    let events = until_kinds(
        &phone,
        &format!("/sessions/{id}/events"),
        &["decision.requested"],
    )
    .await;
    let decision = events
        .iter()
        .find(|(_, p)| kind(p) == "decision.requested")
        .map(|(_, p)| p["decision"].clone())
        .expect("the machine's question reached the phone");
    let choice = decision["options"][0]["id"]
        .as_str()
        .unwrap_or("allow")
        .to_string();
    let answer = call(
        &phone,
        "POST",
        &format!("/decisions/{}/answer", decision["id"].as_str().unwrap()),
        Some(json!({"request_id": rid(), "choice": choice})),
    )
    .await;
    assert_eq!(answer.status, 200, "{}", answer.raw);
    until_count(
        &phone,
        &format!("/sessions/{id}/events"),
        "turn.completed",
        1,
    )
    .await;

    let stopped = call(
        &phone,
        "POST",
        &format!("/sessions/{id}/stop"),
        Some(json!({"request_id": rid()})),
    )
    .await;
    assert_eq!(stopped.status, 200, "{}", stopped.raw);

    // Coming back to the front reconnects without losing anything.
    app.cloud.reconnect_now();
    until("the phone is online again", || async {
        let view = call(&phone, "GET", "/cloud", None).await.body;
        (view["state"] == "online").then_some(())
    })
    .await;

    // Removing the phone from the account (its Settings) wipes its copies.
    let removed = call(
        &phone,
        "DELETE",
        &format!("/cloud/devices/{phone_id}"),
        None,
    )
    .await;
    assert_eq!(removed.status, 200, "{}", removed.raw);
    assert_eq!(removed.body["state"], "signed_out");
    assert!(
        sessions(&phone).await.is_empty(),
        "the phone's copies are gone"
    );
}

#[tokio::test]
async fn a_phone_is_never_a_machine() {
    let cloud = FakeCloud::start().await;
    let (phone, _app, phone_id) = phone(&cloud).await;
    let (desk, desk_id) = device(&cloud, "desk").await;
    until("the desk sees the phone", || {
        sees(&desk, &phone_id, "online", json!(true))
    })
    .await;

    // Not from the phone itself...
    let own = call(
        &phone,
        "POST",
        &format!("/cloud/devices/{phone_id}/kind"),
        Some(json!({"kind": "machine"})),
    )
    .await;
    assert_eq!(own.status, 409, "{}", own.raw);
    // ...and not from another device either.
    let other = call(
        &desk,
        "POST",
        &format!("/cloud/devices/{phone_id}/kind"),
        Some(json!({"kind": "machine"})),
    )
    .await;
    assert_eq!(other.status, 409, "{}", other.raw);

    // A chat can't run on the phone, synced or not.
    let ws = workspace(None);
    for body in [
        json!({"request_id": rid(), "harness": "claude", "workspace": ws.path()}),
        json!({"request_id": rid(), "harness": "claude", "workspace": ws.path(), "synced": true}),
        json!({"request_id": rid(), "harness": "claude", "workspace": ws.path(), "device_id": phone_id}),
    ] {
        let refused = call(&phone, "POST", "/sessions", Some(body)).await;
        assert_eq!(refused.status, 409, "{}", refused.raw);
    }
    // From the phone, the desk can still be made a machine.
    make_machine(&phone, &desk, &desk_id).await;
}

#[tokio::test]
async fn a_phone_manages_projects_on_its_machine() {
    let cloud = FakeCloud::start().await;
    let (phone, _app, _phone_id) = phone(&cloud).await;
    let (machine, machine_id) = device(&cloud, "box").await;
    make_machine(&phone, &machine, &machine_id).await;
    until("the phone sees the machine online", || {
        sees(&phone, &machine_id, "online", json!(true))
    })
    .await;

    // The machine's project list, through the phone's device-prefixed
    // route - the same call the console's project picker makes.
    let empty = call(
        &phone,
        "GET",
        &format!("/devices/{machine_id}/projects"),
        None,
    )
    .await;
    assert_eq!(empty.status, 200, "{}", empty.raw);
    assert_eq!(empty.body.as_array().map(Vec::len), Some(0));

    // A folder on the machine registers as a project, from the phone.
    let ws = workspace(None);
    let saved = call(
        &phone,
        "POST",
        &format!("/devices/{machine_id}/projects"),
        Some(json!({"request_id": rid(), "folders": [ws.path()]})),
    )
    .await;
    assert_eq!(saved.status, 201, "{}", saved.raw);
    let project_id = saved.body["id"].as_str().unwrap().to_string();

    let listed = call(
        &phone,
        "GET",
        &format!("/devices/{machine_id}/projects"),
        None,
    )
    .await;
    assert_eq!(listed.status, 200, "{}", listed.raw);
    let one = listed.body.as_array().and_then(|a| a.first()).cloned();
    assert_eq!(
        one.as_ref().and_then(|p| p["id"].as_str()),
        Some(project_id.as_str()),
        "the machine holds the project the phone registered"
    );

    // And the phone can take it back off.
    let removed = call(
        &phone,
        "DELETE",
        &format!(
            "/devices/{machine_id}/projects/{project_id}?request_id={}",
            rid()
        ),
        None,
    )
    .await;
    assert_eq!(removed.status, 200, "{}", removed.raw);
    let after = call(
        &phone,
        "GET",
        &format!("/devices/{machine_id}/projects"),
        None,
    )
    .await;
    assert_eq!(after.body.as_array().map(Vec::len), Some(0));
}
