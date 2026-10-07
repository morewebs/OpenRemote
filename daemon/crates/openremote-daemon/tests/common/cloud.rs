//! Helpers for the Cloud suites: devices signed in to the stand-in.

use std::time::Duration;

use fake_cloud::FakeCloud;
use openremote_cloud::CloudConfig;
use openremote_cloud::https::Http;
use serde_json::{Value, json};

use super::{TestDaemon, call, raw_http, start_cloud_daemon};

pub fn config(cloud: &FakeCloud, name: &str) -> CloudConfig {
    CloudConfig {
        api_base: cloud.api(),
        auth_issuer: cloud.issuer(),
        client_id: openremote_cloud::config::CLIENT_ID.into(),
        device_name: name.into(),
    }
}

/// Signs a daemon in the way the system browser would; its device id.
pub async fn sign_in(daemon: &TestDaemon) -> String {
    let started = call(daemon, "POST", "/cloud/signin", None).await;
    let url = started.body["authorize_url"].as_str().unwrap().to_string();
    let page = Http::new().request("GET", &url, &[], None).await.unwrap();
    let location = page.headers["location"].to_str().unwrap().to_string();
    let (_, rest) = location.split_once("/cloud/callback").unwrap();
    let landed = raw_http(daemon, "GET", &format!("/cloud/callback{rest}"), &[], None).await;
    assert_eq!(landed.status, 200, "{}", landed.raw);
    call(daemon, "GET", "/cloud", None).await.body["device"]["id"]
        .as_str()
        .unwrap()
        .to_string()
}

/// A signed-in daemon (every harness slot is the fixture agent).
pub async fn device(cloud: &FakeCloud, name: &str) -> (TestDaemon, String) {
    let daemon = start_cloud_daemon(&[], Some(config(cloud, name))).await;
    let id = sign_in(&daemon).await;
    (daemon, id)
}

/// Polls `check` until it returns Some, for up to fifteen seconds.
pub async fn until<T, F, Fut>(what: &str, mut check: F) -> T
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Option<T>>,
{
    for _ in 0..150 {
        if let Some(v) = check().await {
            return v;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!("timed out waiting for {what}");
}

/// Some(()) once `daemon`'s device list shows `id` with `field` == `want`.
pub async fn sees(daemon: &TestDaemon, id: &str, field: &str, want: Value) -> Option<()> {
    let devices = call(daemon, "GET", "/cloud/devices", None).await.body;
    devices["devices"]
        .as_array()?
        .iter()
        .any(|d| d["id"] == id && d[field] == want)
        .then_some(())
}

/// `by` makes `id` a machine, and waits until `id` knows it.
pub async fn make_machine(by: &TestDaemon, target: &TestDaemon, id: &str) {
    let made = call(
        by,
        "POST",
        &format!("/cloud/devices/{id}/kind"),
        Some(json!({"kind": "machine"})),
    )
    .await;
    assert_eq!(made.status, 200, "{}", made.raw);
    until("the machine knows it is one", || {
        sees(target, id, "kind", json!("machine"))
    })
    .await;
}
