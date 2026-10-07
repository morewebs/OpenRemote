//! Joining OpenRemote Cloud: a desktop signs in through the browser and the
//! loopback callback; a machine enrolls with an install code.

mod common;

use common::{call, raw_http, start_cloud_daemon};
use fake_cloud::FakeCloud;
use openremote_cloud::CloudConfig;
use openremote_cloud::https::Http;

fn config(cloud: &FakeCloud, name: &str) -> CloudConfig {
    CloudConfig {
        api_base: cloud.api(),
        auth_issuer: cloud.issuer(),
        client_id: openremote_cloud::config::CLIENT_ID.into(),
        device_name: name.into(),
    }
}

/// What the system browser does with the sign-in page: moreweb redirects to
/// the daemon's callback, and the browser follows.
async fn browser_sign_in(daemon: &common::TestDaemon) -> (u16, String, String) {
    let started = call(daemon, "POST", "/cloud/signin", None).await;
    assert_eq!(started.status, 200, "{}", started.raw);
    let url = started.body["authorize_url"].as_str().unwrap().to_string();
    let page = Http::new().request("GET", &url, &[], None).await.unwrap();
    assert!(
        page.status == 302 || page.status == 303,
        "authorize redirects"
    );
    let location = page.headers["location"].to_str().unwrap().to_string();
    let path = location
        .split_once("/cloud/callback")
        .map(|(_, rest)| format!("/cloud/callback{rest}"))
        .expect("back to this daemon");
    let landed = raw_http(daemon, "GET", &path, &[], None).await;
    (landed.status, landed.raw, path)
}

#[tokio::test]
async fn a_desktop_signs_in_through_the_browser_and_stays_the_same_device() {
    let cloud = FakeCloud::start().await;
    let daemon = start_cloud_daemon(&[], Some(config(&cloud, "desk-1"))).await;

    let view = call(&daemon, "GET", "/cloud", None).await;
    assert_eq!(
        view.body["state"], "signed_out",
        "first launch never signs in"
    );

    let (status, html, path) = browser_sign_in(&daemon).await;
    assert_eq!(status, 200, "{html}");
    assert!(html.contains("You're signed in"), "{html}");
    let view = call(&daemon, "GET", "/cloud", None).await.body;
    assert_eq!(view["state"], "connecting");
    assert_eq!(view["account"]["email"], "user@example.test");
    assert_eq!(view["device"]["kind"], "desktop");
    let device_id = view["device"]["id"].as_str().unwrap().to_string();
    let devices = cloud.devices();
    assert_eq!(devices.len(), 1);
    assert_eq!(
        (devices[0].id.as_str(), devices[0].name.as_str()),
        (device_id.as_str(), "desk-1")
    );

    // The callback is single-use.
    let replay = raw_http(&daemon, "GET", &path, &[], None).await;
    assert_eq!(replay.status, 400, "{}", replay.raw);

    // Signing out keeps the identity: signing in again is the same device.
    let out = call(&daemon, "POST", "/cloud/signout", None).await;
    assert_eq!(out.body["state"], "signed_out");
    assert_eq!(out.body["device"], serde_json::Value::Null);
    let (status, _, _) = browser_sign_in(&daemon).await;
    assert_eq!(status, 200);
    let again = call(&daemon, "GET", "/cloud", None).await.body;
    assert_eq!(again["device"]["id"], device_id.as_str());
    assert_eq!(cloud.devices().len(), 1, "no second device");

    // Another account on the same computer joins as a new device.
    call(&daemon, "POST", "/cloud/signout", None).await;
    cloud.set_user("other@example.test");
    let (status, _, _) = browser_sign_in(&daemon).await;
    assert_eq!(status, 200);
    let other = call(&daemon, "GET", "/cloud", None).await.body;
    assert_eq!(other["account"]["email"], "other@example.test");
    assert_ne!(other["device"]["id"], device_id.as_str());
}

#[tokio::test]
async fn a_wrong_or_cancelled_callback_changes_nothing() {
    let cloud = FakeCloud::start().await;
    let daemon = start_cloud_daemon(&[], Some(config(&cloud, "desk-2"))).await;
    call(&daemon, "POST", "/cloud/signin", None).await;

    let forged = raw_http(
        &daemon,
        "GET",
        "/cloud/callback?code=x&state=forged",
        &[],
        None,
    )
    .await;
    assert_eq!(forged.status, 400);
    assert_eq!(
        call(&daemon, "GET", "/cloud", None).await.body["state"],
        "signing_in",
        "a forged state doesn't cancel the real sign-in"
    );

    let cancelled = raw_http(
        &daemon,
        "GET",
        "/cloud/callback?error=access_denied&state=x",
        &[],
        None,
    )
    .await;
    assert_eq!(cancelled.status, 200);
    assert!(cancelled.raw.contains("Sign-in cancelled"));
    assert_eq!(
        call(&daemon, "GET", "/cloud", None).await.body["state"],
        "signed_out"
    );

    // The routes besides the callback still need the daemon token.
    let anonymous = common::call_with_token(&daemon, "wrong", "POST", "/cloud/signin", None).await;
    assert_eq!(anonymous.status, 401);
}

/// Runs the daemon binary with `args` against `cloud`, in `data_dir`.
fn run_binary(data_dir: &std::path::Path, cloud: &FakeCloud, args: &[&str]) -> (i32, String) {
    let out = std::process::Command::new(common::daemon_binary())
        .args(args)
        .env("OPENREMOTE_DATA_DIR", data_dir)
        .env("OPENREMOTE_CLOUD_API", cloud.api())
        .env("OPENREMOTE_AUTH_ISSUER", cloud.issuer())
        .env("OPENREMOTE_DEVICE_NAME", "build-box")
        .stdin(std::process::Stdio::null())
        .output()
        .expect("run the daemon binary");
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned() + &String::from_utf8_lossy(&out.stderr),
    )
}

#[tokio::test]
async fn a_machine_enrolls_with_an_install_code() {
    let cloud = FakeCloud::start().await;
    let dir = tempfile::tempdir().unwrap();
    let data = dir.path().to_path_buf();
    let code = cloud.enrollment_code("owner@example.test");

    let (status, _) = tokio::task::spawn_blocking({
        let (data, cloud) = (data.clone(), cloud.clone());
        move || run_binary(&data, &cloud, &["agent"])
    })
    .await
    .unwrap();
    assert_eq!(status, 78, "an agent that hasn't enrolled stops for good");
    let (status, _) = tokio::task::spawn_blocking({
        let (data, cloud) = (data.clone(), cloud.clone());
        move || run_binary(&data, &cloud, &["status"])
    })
    .await
    .unwrap();
    assert_eq!(status, 1);

    let (status, out) = tokio::task::spawn_blocking({
        let (data, cloud, code) = (data.clone(), cloud.clone(), code.clone());
        move || run_binary(&data, &cloud, &["enroll", &code])
    })
    .await
    .unwrap();
    assert_eq!(status, 0, "{out}");
    assert!(out.contains("owner@example.test"), "{out}");
    let machine = cloud
        .devices()
        .into_iter()
        .find(|d| d.name == "build-box")
        .expect("registered");
    assert_eq!(
        (machine.kind.as_str(), machine.created_via),
        ("machine", "enrollment")
    );

    let (status, out) = tokio::task::spawn_blocking({
        let (data, cloud) = (data.clone(), cloud.clone());
        move || run_binary(&data, &cloud, &["status"])
    })
    .await
    .unwrap();
    assert_eq!(status, 0, "{out}");
    let (status, _) = tokio::task::spawn_blocking({
        let (data, cloud) = (data.clone(), cloud.clone());
        move || run_binary(&data, &cloud, &["enroll", &code])
    })
    .await
    .unwrap();
    assert_ne!(status, 0, "a second enroll is refused");

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(data.join("cloud/credentials.json"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600, "the device credential is owner-only");
    }
}

#[tokio::test]
async fn a_desktop_mints_an_install_command_for_a_machine() {
    let cloud = FakeCloud::start().await;
    let daemon = start_cloud_daemon(&[], Some(config(&cloud, "desk-3"))).await;
    let refused = call(&daemon, "POST", "/cloud/enrollments", None).await;
    assert_eq!(refused.status, 401, "signed out: {}", refused.raw);
    let (status, _, _) = browser_sign_in(&daemon).await;
    assert_eq!(status, 200);
    let minted = call(&daemon, "POST", "/cloud/enrollments", None).await;
    assert_eq!(minted.status, 201, "{}", minted.raw);
    let command = minted.body["install_command"].as_str().unwrap();
    let code = minted.body["code"].as_str().unwrap();
    assert!(command.starts_with("curl -fsSL https://moreweb.space/openremote/install.sh |"));
    assert!(command.ends_with(&format!("sh -s -- {code}")), "{command}");
    assert!(
        command.contains(&format!("OPENREMOTE_CLOUD_API={}", cloud.api())),
        "a non-default backend rides along: {command}"
    );
}
