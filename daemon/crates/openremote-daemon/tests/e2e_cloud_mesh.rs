//! Two of an account's devices reach each other end to end through the
//! relay: a desktop drives a machine's API, a desktop that isn't a machine
//! refuses, private chats stay private, removal ends it, and a relay that
//! drops every socket mid-operation is recovered from.

mod common;

use std::time::Duration;

use common::{TestDaemon, call, raw_http, start_cloud_daemon};
use fake_cloud::FakeCloud;
use openremote_cloud::CloudConfig;
use openremote_cloud::https::Http;
use serde_json::{Value, json};

fn config(cloud: &FakeCloud, name: &str) -> CloudConfig {
    CloudConfig {
        api_base: cloud.api(),
        auth_issuer: cloud.issuer(),
        client_id: openremote_cloud::config::CLIENT_ID.into(),
        device_name: name.into(),
        platform: std::env::consts::OS.into(),
        redirect_uri: None,
    }
}

async fn sign_in(daemon: &TestDaemon) -> String {
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

/// Polls `check` until it returns Some, for up to ten seconds.
async fn until<T, F, Fut>(what: &str, mut check: F) -> T
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Option<T>>,
{
    for _ in 0..100 {
        if let Some(v) = check().await {
            return v;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!("timed out waiting for {what}");
}

async fn sees(daemon: &TestDaemon, id: &str, field: &str, want: Value) -> Option<()> {
    let devices = call(daemon, "GET", "/cloud/devices", None).await.body;
    devices["devices"]
        .as_array()?
        .iter()
        .any(|d| d["id"] == id && d[field] == want)
        .then_some(())
}

/// Runs the daemon binary with `args` against `cloud`, in `data_dir`; the
/// enroll flows run as their own process, the way a machine's do.
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
async fn a_desktop_drives_a_machine_through_the_relay_and_nothing_more() {
    let cloud = FakeCloud::start().await;
    let a = start_cloud_daemon(&[], Some(config(&cloud, "desk-a"))).await;
    let b = start_cloud_daemon(&[], Some(config(&cloud, "desk-b"))).await;
    let id_a = sign_in(&a).await;
    let id_b = sign_in(&b).await;

    until("a sees b online", || sees(&a, &id_b, "online", json!(true))).await;
    until("b sees a online", || sees(&b, &id_a, "online", json!(true))).await;
    assert_eq!(
        call(&a, "GET", "/cloud", None).await.body["state"],
        "online"
    );

    // A desktop that isn't a machine refuses to be driven.
    let refused = call(&a, "GET", &format!("/devices/{id_b}/capabilities"), None).await;
    assert_eq!(refused.status, 409, "{}", refused.raw);
    assert_eq!(refused.body["code"], "not_a_machine");
    let accounts_path = format!("/devices/{id_b}/harnesses/codex/accounts");
    let refused = call(&a, "GET", &accounts_path, None).await;
    assert_eq!(refused.body["code"], "not_a_machine");

    // Made a machine from the other desktop, it answers with its own API.
    let made = call(
        &a,
        "POST",
        &format!("/cloud/devices/{id_b}/kind"),
        Some(json!({"kind": "machine"})),
    )
    .await;
    assert_eq!(made.status, 200, "{}", made.raw);
    until("b knows it is a machine", || {
        sees(&b, &id_b, "kind", json!("machine"))
    })
    .await;
    let caps = call(&a, "GET", &format!("/devices/{id_b}/capabilities"), None).await;
    assert_eq!(caps.status, 200, "{}", caps.raw);
    assert!(caps.body["harnesses"].is_array(), "{}", caps.raw);
    let dirs = call(&a, "GET", &format!("/devices/{id_b}/fs/dirs"), None).await;
    assert_eq!(dirs.status, 200, "{}", dirs.raw);
    assert!(dirs.body["dirs"].is_array());
    let accounts = call(&a, "GET", &accounts_path, None).await;
    assert_eq!(accounts.status, 409, "fixture App: {}", accounts.raw);
    assert!(
        accounts.raw.contains("managed accounts"),
        "{}",
        accounts.raw
    );
    let update_account = call(
        &a,
        "PATCH",
        &format!("{accounts_path}/{}", uuid::Uuid::new_v4()),
        Some(json!({"request_id": "patch-foreign-app-account", "label": "Work"})),
    )
    .await;
    assert_eq!(update_account.status, 409, "{}", update_account.raw);
    assert!(
        update_account.raw.contains("managed accounts"),
        "{}",
        update_account.raw
    );
    let foreign_account = call(
        &a,
        "POST",
        "/sessions/with-app-account",
        Some(json!({
            "request_id": "foreign-app-account",
            "device_id": id_b,
            "harness": "claude",
            "app_account_id": uuid::Uuid::new_v4().to_string()
        })),
    )
    .await;
    assert_eq!(foreign_account.status, 409, "{}", foreign_account.raw);
    assert!(
        foreign_account.raw.contains("managed accounts"),
        "{}",
        foreign_account.raw
    );

    // Only what the allowlist names reaches a peer; Cloud's own routes
    // aren't even routed for one.
    for path in [
        "plugins",
        "marketplaces",
        "marketplaces/catalog",
        "mcp-servers",
        "automations",
        "sessions",
        "receipts/x",
    ] {
        let denied = call(&a, "GET", &format!("/devices/{id_b}/{path}"), None).await;
        assert_eq!(denied.status, 403, "{path}: {}", denied.raw);
    }
    let cloud_route = call(&a, "GET", &format!("/devices/{id_b}/cloud"), None).await;
    assert_eq!(cloud_route.status, 404, "{}", cloud_route.raw);

    // A private chat on the machine never shows up for the other device.
    let ws = common::workspace(None);
    let private = call(
        &b,
        "POST",
        "/sessions",
        Some(json!({"request_id": uuid::Uuid::new_v4().to_string(), "harness": "claude", "workspace": ws.path()})),
    )
    .await;
    assert_eq!(private.status, 201, "{}", private.raw);
    let machines = call(&a, "GET", &format!("/devices/{id_b}/machines"), None).await;
    assert_eq!(machines.status, 200, "{}", machines.raw);
    assert_eq!(
        machines.body[0]["sessions"],
        json!([]),
        "private chats stay private"
    );
    let own = call(&b, "GET", "/machines", None).await;
    assert_eq!(own.body[0]["sessions"].as_array().map(Vec::len), Some(1));

    let stranger = call(
        &a,
        "GET",
        &format!("/devices/{}/capabilities", uuid::Uuid::new_v4()),
        None,
    )
    .await;
    assert_eq!(stranger.status, 404, "{}", stranger.raw);

    // Removing the machine from the other desktop ends it there: its synced
    // chats would go, and its private chat stays.
    let removed = call(&a, "DELETE", &format!("/cloud/devices/{id_b}"), None).await;
    assert_eq!(removed.status, 204, "{}", removed.raw);
    until("b learns it was removed", || async {
        (call(&b, "GET", "/cloud", None).await.body["state"] == "revoked").then_some(())
    })
    .await;
    assert_eq!(
        call(&b, "GET", "/sessions", None)
            .await
            .body
            .as_array()
            .map(Vec::len),
        Some(1)
    );
    let gone = call(&a, "GET", &format!("/devices/{id_b}/capabilities"), None).await;
    assert_ne!(gone.status, 200);
}

/// A removed desktop signs back in and its mesh comes back. The removal
/// ends the relay loop for good (the relay closes with the removed code);
/// the slot that loop filled must be released, or the sign-in's own
/// `start` finds it occupied and this device stays silent until a process
/// restart - the re-sign-in joins as a new device, and the mesh request
/// that worked before the removal works again after it.
#[tokio::test]
async fn a_removed_device_signs_back_in_and_the_mesh_returns() {
    let cloud = FakeCloud::start().await;
    let a = start_cloud_daemon(&[], Some(config(&cloud, "desk-a"))).await;
    let b = start_cloud_daemon(&[], Some(config(&cloud, "desk-b"))).await;
    let id_a = sign_in(&a).await;
    let id_b = sign_in(&b).await;

    until("a sees b online", || sees(&a, &id_b, "online", json!(true))).await;
    let made = call(
        &a,
        "POST",
        &format!("/cloud/devices/{id_b}/kind"),
        Some(json!({"kind": "machine"})),
    )
    .await;
    assert_eq!(made.status, 200, "{}", made.raw);
    until("b knows it is a machine", || {
        sees(&b, &id_b, "kind", json!("machine"))
    })
    .await;
    let caps = call(&a, "GET", &format!("/devices/{id_b}/capabilities"), None).await;
    assert_eq!(
        caps.status, 200,
        "the mesh works before the removal: {}",
        caps.raw
    );

    // The other desktop removes it: its relay loop stops for good.
    let removed = call(&a, "DELETE", &format!("/cloud/devices/{id_b}"), None).await;
    assert_eq!(removed.status, 204, "{}", removed.raw);
    until("b learns it was removed", || async {
        (call(&b, "GET", "/cloud", None).await.body["state"] == "revoked").then_some(())
    })
    .await;

    // Signing back in (as the new device a removed one becomes) brings
    // the mesh back in the same process: the dead run must not hold it.
    let id_b2 = sign_in(&b).await;
    assert_ne!(id_b2, id_b, "a removed device joins as a new one");
    until("b is online again", || async {
        (call(&b, "GET", "/cloud", None).await.body["state"] == "online").then_some(())
    })
    .await;
    until("b sees a online again", || {
        sees(&b, &id_a, "online", json!(true))
    })
    .await;
    until("a sees the new device online", || {
        sees(&a, &id_b2, "online", json!(true))
    })
    .await;

    let made = call(
        &a,
        "POST",
        &format!("/cloud/devices/{id_b2}/kind"),
        Some(json!({"kind": "machine"})),
    )
    .await;
    assert_eq!(made.status, 200, "{}", made.raw);
    until("the new device knows it is a machine", || {
        sees(&b, &id_b2, "kind", json!("machine"))
    })
    .await;
    let caps = call(&a, "GET", &format!("/devices/{id_b2}/capabilities"), None).await;
    assert_eq!(caps.status, 200, "the mesh works again: {}", caps.raw);
}

/// The relay drops every socket mid-operation, as a restart of it would.
/// A request is in flight when the drop lands; both daemons must
/// reconnect on their own (a bounded wait; nobody signs in again or
/// restarts anything), the presence must cross back, and a request issued
/// into the dead window after a further drop of one side's socket must
/// ride the recovery and answer. What is pinned here is the reconnect
/// itself; the finer drain behavior (a queued first handshake surviving
/// the backoff) is covered by the cloud crate's own relay-loop test,
/// where no daemon housekeeping can rescue it.
#[tokio::test]
async fn the_mesh_recovers_from_a_relay_restart_mid_operation() {
    use std::sync::Arc;
    let cloud = FakeCloud::start().await;
    let a = Arc::new(start_cloud_daemon(&[], Some(config(&cloud, "desk-a"))).await);
    let b = start_cloud_daemon(&[], Some(config(&cloud, "desk-b"))).await;
    let id_b = sign_in(&b).await;
    let id_a = sign_in(&a).await;

    until("a sees b online", || sees(&a, &id_b, "online", json!(true))).await;
    let made = call(
        &a,
        "POST",
        &format!("/cloud/devices/{id_b}/kind"),
        Some(json!({"kind": "machine"})),
    )
    .await;
    assert_eq!(made.status, 200, "{}", made.raw);
    until("b knows it is a machine", || {
        sees(&b, &id_b, "kind", json!("machine"))
    })
    .await;
    let caps = call(&a, "GET", &format!("/devices/{id_b}/capabilities"), None).await;
    assert_eq!(
        caps.status, 200,
        "the mesh works before the drop: {}",
        caps.raw
    );

    // Mid-operation: a request is in flight as the relay drops every
    // socket it holds. Its round trip dies with the socket unless it won
    // the race, so its answer is the drop's own outcome; what must never
    // happen is a hang.
    let in_flight = {
        let a = Arc::clone(&a);
        let path = format!("/devices/{id_b}/capabilities");
        tokio::spawn(async move { call(&a, "GET", &path, None).await })
    };
    cloud.hub.restart();
    until("a notices the drop", || async {
        (call(&a, "GET", "/cloud", None).await.body["state"] != "online").then_some(())
    })
    .await;
    let dropped = in_flight.await.expect("the interrupted request returns");
    assert!(
        matches!(dropped.status, 200 | 502 | 503),
        "the drop lands mid-operation: the round trip either finished or reports the drop, never hangs: {}",
        dropped.raw
    );

    // Both sides come back on their own, and the peer settles online
    // first: from here on only the requester's socket drops, so the peer
    // is guaranteed to be in the relay's room when the requester returns.
    until("a is online again", || async {
        (call(&a, "GET", "/cloud", None).await.body["state"] == "online").then_some(())
    })
    .await;
    until("b is online again", || async {
        (call(&b, "GET", "/cloud", None).await.body["state"] == "online").then_some(())
    })
    .await;
    until("a sees b online again", || {
        sees(&a, &id_b, "online", json!(true))
    })
    .await;

    // One more drop of the requester's socket alone, after its session
    // came back, and a request issued straight into the dead window that
    // follows: the relay session is down, the loop is sleeping off its
    // backoff, and the request's first handshake queues until the next
    // session. It must ride the recovery and answer, not wedge.
    let account = cloud.account_id("user@example.test");
    cloud
        .hub
        .close_device(&account, &id_a, fake_cloud::relay::CLOSE_RESTART);
    until("the requester notices the drop", || async {
        (call(&a, "GET", "/cloud", None).await.body["state"] != "online").then_some(())
    })
    .await;
    let queued = {
        let a = Arc::clone(&a);
        let path = format!("/devices/{id_b}/capabilities");
        tokio::spawn(async move { call(&a, "GET", &path, None).await })
    };

    // The mesh reconnects on its own and the presence crosses back. A
    // mesh that wedged offline here would time these waits out.
    until("a is online again", || async {
        (call(&a, "GET", "/cloud", None).await.body["state"] == "online").then_some(())
    })
    .await;
    until("a sees b online again", || {
        sees(&a, &id_b, "online", json!(true))
    })
    .await;

    let rode = queued.await.expect("the queued request returns");
    assert_eq!(
        rode.status, 200,
        "the request issued into the dead window rides the recovery: {}",
        rode.raw
    );

    let caps = call(&a, "GET", &format!("/devices/{id_b}/capabilities"), None).await;
    assert_eq!(
        caps.status, 200,
        "the mesh works after the drop: {}",
        caps.raw
    );
    let dirs = call(&a, "GET", &format!("/devices/{id_b}/fs/dirs"), None).await;
    assert_eq!(
        dirs.status, 200,
        "a second request rides the same recovered mesh: {}",
        dirs.raw
    );
}

/// The reconnect's drain, pinned down where nothing can rescue it. A
/// request that starts while the relay socket is down queues its first
/// handshake until the next session; the reconnect must deliver that
/// handshake, not discard it. The daemon's own housekeeping re-establishes
/// links on its own (a returning peer's presence triggers a sync), which
/// would rescue a discarded handshake and hide the cost, so this drives
/// two meshes directly with a host that does nothing but answer: a
/// discarded handshake then costs the request its whole link-attempt
/// timeout, and a delivered one costs only the backoff window.
#[tokio::test]
async fn a_handshake_queued_through_a_dead_window_rides_the_reconnect() {
    use futures_util::future::BoxFuture;
    use openremote_cloud::rpc::{Request, Response};
    use openremote_cloud::{Cloud, CloudHost, PeerContext, Phase};
    use std::time::Instant;

    /// A host with no housekeeping: dispatch answers, nothing else acts.
    struct Plain;
    impl CloudHost for Plain {
        fn dispatch(&self, _: PeerContext, _: Request) -> BoxFuture<'static, Response> {
            Box::pin(std::future::ready(Response {
                status: 200,
                content_type: None,
                body: b"ok".to_vec(),
            }))
        }
        fn note(&self, _: PeerContext, _: Value) {}
        fn linked(&self, _: openremote_core::DeviceId) {}
        fn online(&self, _: openremote_core::DeviceId) {}
        fn revoked(&self) {}
    }

    let cloud = FakeCloud::start().await;
    let host: std::sync::Arc<dyn CloudHost> = std::sync::Arc::new(Plain);
    let dir_a = tempfile::tempdir().unwrap();
    let dir_b = tempfile::tempdir().unwrap();
    let a = Cloud::open(config(&cloud, "desk-a"), dir_a.path());
    let b = Cloud::open(config(&cloud, "desk-b"), dir_b.path());
    a.enroll(&cloud.enrollment_code("user@example.test"))
        .await
        .expect("a enrolls");
    b.enroll(&cloud.enrollment_code("user@example.test"))
        .await
        .expect("b enrolls");
    a.start(std::sync::Arc::clone(&host));
    b.start(std::sync::Arc::clone(&host));
    let (id_a, id_b) = (
        a.device_id().expect("a's id"),
        b.device_id().expect("b's id"),
    );

    // Both relay sessions up and the link between them proven.
    let deadline = Instant::now() + Duration::from_secs(10);
    while !(a.phase() == Phase::Online && b.phase() == Phase::Online) {
        assert!(Instant::now() < deadline, "the meshes never came online");
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    let ask = || Request {
        method: "GET".into(),
        path: "/".into(),
        content_type: None,
        body: Vec::new(),
    };
    let before = a
        .mesh()
        .expect("a's mesh")
        .request(id_b, ask(), Duration::from_secs(15))
        .await
        .expect("the mesh works before the drop");
    assert_eq!(before.status, 200);

    // Drop only the requester's socket: its relay loop arms a backoff
    // window (1s plus up to 1s of jitter) while the peer stays connected,
    // so whatever the requester queues during the window arrives to find
    // the peer's socket still in the relay's room.
    let account = cloud.account_id("user@example.test");
    cloud.hub.close_device(
        &account,
        &id_a.to_string(),
        fake_cloud::relay::CLOSE_RESTART,
    );
    let deadline = Instant::now() + Duration::from_secs(10);
    while a.phase() == Phase::Online {
        assert!(
            Instant::now() < deadline,
            "the requester never noticed the drop"
        );
        tokio::time::sleep(Duration::from_millis(25)).await;
    }

    // The request into the dead window: its first handshake (or its
    // nudge) queues while the session is down. Delivered at the
    // reconnect, the request completes as the window closes, within
    // about 2.1s of its start. Discarded instead, its first link attempt
    // burns the full 4s timeout before the retry delivers, so its answer
    // arrives at 4s or later. The bound sits between the two.
    let at = Instant::now();
    let rode = a
        .mesh()
        .expect("a's mesh")
        .request(id_b, ask(), Duration::from_secs(15))
        .await
        .expect("the queued request completes after the reconnect");
    let waited = at.elapsed();
    assert_eq!(rode.status, 200);
    assert!(
        waited < Duration::from_millis(3200),
        "the queued handshake rode the reconnect ({waited:?}) instead of stalling through a discarded one"
    );
}

/// A frame past the ceiling the relay's hello advertises closes the socket
/// (1009, the too-big close code) instead of being carried, and a frame
/// at the ceiling is carried: the bound is honest both ways. The devices'
/// own mesh traffic is far inside it and never notices.
#[tokio::test]
async fn an_oversized_frame_closes_the_socket_and_the_mesh_survives_it() {
    let cloud = FakeCloud::start().await;
    let a = start_cloud_daemon(&[], Some(config(&cloud, "desk-a"))).await;
    let b = start_cloud_daemon(&[], Some(config(&cloud, "desk-b"))).await;
    let id_b = sign_in(&b).await;
    sign_in(&a).await;
    until("a sees b online", || sees(&a, &id_b, "online", json!(true))).await;
    let made = call(
        &a,
        "POST",
        &format!("/cloud/devices/{id_b}/kind"),
        Some(json!({"kind": "machine"})),
    )
    .await;
    assert_eq!(made.status, 200, "{}", made.raw);
    until("b knows it is a machine", || {
        sees(&b, &id_b, "kind", json!("machine"))
    })
    .await;
    let caps = call(&a, "GET", &format!("/devices/{id_b}/capabilities"), None).await;
    assert_eq!(caps.status, 200, "the mesh works beforehand: {}", caps.raw);

    // A machine's row with no daemon behind it (enrolled, its agent not
    // started): a credential that speaks the relay directly and displaces
    // nobody's session. That is exactly what a freshly installed machine
    // looks like to the relay, before its agent first runs.
    let dir = tempfile::tempdir().unwrap();
    let code = cloud.enrollment_code("user@example.test");
    let (status, out) = {
        let (data, cloud, code) = (dir.path().to_path_buf(), cloud.clone(), code);
        tokio::task::spawn_blocking(move || run_binary(&data, &cloud, &["enroll", &code]))
            .await
            .unwrap()
    };
    assert_eq!(status, 0, "the machine enrolled: {out}");
    let machine = cloud
        .devices()
        .into_iter()
        .find(|d| d.name == "build-box")
        .expect("the machine's row");
    let url = format!("{}/v1/openremote/relay", cloud.base).replace("http://", "ws://");

    // The wire frame is [16-byte device id][payload]; the ceiling covers
    // the whole frame. One payload byte past it: closed too-big.
    let ceiling = fake_cloud::relay::MAX_FRAME - 16;
    let past = vec![0u8; ceiling + 1];
    assert!(
        matches!(
            relay_send(&url, &machine.credential, &id_b, &past).await,
            RelayAnswer::ClosedTooBig
        ),
        "the relay closes on a frame past its ceiling"
    );
    // At the ceiling exactly: carried.
    let at = vec![0u8; ceiling];
    assert!(
        matches!(
            relay_send(&url, &machine.credential, &id_b, &at).await,
            RelayAnswer::Carried
        ),
        "a frame at the ceiling is carried"
    );

    // The daemons' own frames are far inside the ceiling: the mesh
    // between them never noticed any of it.
    let caps = call(&a, "GET", &format!("/devices/{id_b}/capabilities"), None).await;
    assert_eq!(caps.status, 200, "the mesh is unaffected: {}", caps.raw);
}

/// What the relay did with a frame a device sent it.
enum RelayAnswer {
    /// Closed the socket: the frame was past the ceiling the hello
    /// advertises.
    ClosedTooBig,
    /// Carried the frame: the socket answered a ping after it.
    Carried,
}

/// Speaks the relay socket directly, as `credential`'s device: registers,
/// sends one binary frame to `peer`, and reports what the relay did with
/// it. A closed socket stays quiet; a live one answers the ping that
/// follows.
async fn relay_send(url: &str, credential: &str, peer: &str, payload: &[u8]) -> RelayAnswer {
    use futures_util::{SinkExt, StreamExt};
    use tokio_tungstenite::tungstenite::Message;
    use tokio_tungstenite::tungstenite::client::IntoClientRequest;

    let mut request = url.into_client_request().expect("the relay url");
    let auth = format!("Bearer {credential}");
    request
        .headers_mut()
        .insert("authorization", auth.parse().expect("the header"));
    let authority = url
        .trim_start_matches("ws://")
        .split('/')
        .next()
        .unwrap_or_default()
        .to_string();
    let stream = tokio::net::TcpStream::connect(authority)
        .await
        .expect("connect the relay");
    let (mut sink, mut stream) = tokio_tungstenite::client_async(request, stream)
        .await
        .expect("the relay handshake")
        .0
        .split();
    let hello = tokio::time::timeout(Duration::from_secs(5), stream.next())
        .await
        .expect("the relay answers")
        .expect("the socket is alive")
        .expect("the frame parses");
    match hello {
        Message::Text(t) if t.as_str().contains("\"hello\"") => {}
        other => panic!("the relay's first word is hello, not {other:?}"),
    }
    let mut frame = uuid::Uuid::parse_str(peer)
        .expect("the peer's id")
        .as_bytes()
        .to_vec();
    frame.extend_from_slice(payload);
    sink.send(Message::Binary(frame.into()))
        .await
        .expect("send");
    // The ping tells the two endings apart without waiting on a close
    // that only one of them gets.
    let _ = sink.send(Message::Text(r#"{"type":"ping"}"#.into())).await;
    loop {
        let answer = tokio::time::timeout(Duration::from_secs(5), stream.next())
            .await
            .expect("the relay says something");
        match answer {
            Some(Ok(Message::Close(frame))) => {
                let code = frame.map(|f| u16::from(f.code));
                assert_eq!(
                    code,
                    Some(1009),
                    "the relay closes an oversized frame with too-big, nothing else"
                );
                return RelayAnswer::ClosedTooBig;
            }
            Some(Ok(Message::Text(t))) if t.as_str().contains("\"pong\"") => {
                return RelayAnswer::Carried;
            }
            Some(Ok(_)) => {}
            _ => panic!("the relay went quiet with the socket open"),
        }
    }
}
