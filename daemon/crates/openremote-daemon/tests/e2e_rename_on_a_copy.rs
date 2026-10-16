//! A rename must land on the machine that runs the chat, never on a copy.
//!
//! Every other write path refuses a chat that runs elsewhere with a 409, so the
//! copy stays a read-only mirror. A rename used to be the exception: the copy
//! wrote its own `SessionUpdated` at the sequence number it had reached, the
//! machine then wrote its turn at that same number, and the copy's pull starts
//! after it, so the turn was never fetched and nothing reported it. The copy
//! stopped being a prefix of the runner's log, and a third device pulling from
//! the copy inherited the damage with no way back short of a full re-pull.

mod common;

use std::sync::{Arc, Mutex as StdMutex};

use openremote_core::{
    ChatMessage, DeviceId, EventPayload, MessageRole, Session, SessionId, SessionStatus, Store,
};
use openremote_daemon::app::{App, AppOptions};
use openremote_daemon::registry::HarnessRegistry;
use openremote_daemon::supervisor::Supervisor;
use serde_json::json;

/// A chat record with no history yet. `executor` is the machine that runs it:
/// `None` means this device, `Some(other)` is the copy shape.
fn chat(executor: Option<DeviceId>) -> Session {
    Session {
        id: SessionId::new(),
        harness: "claude".into(),
        workspace: None,
        status: SessionStatus::Stopped,
        model: None,
        endpoint_id: None,
        effort: None,
        permission_mode: None,
        harness_session_ref: None,
        created_at: 0,
        updated_at: 0,
        last_error: None,
        next_turn: 1,
        fast: false,
        approved_tools: Vec::new(),
        executor,
        title: None,
        queue: Vec::new(),
        queue_paused: false,
        mentioned_chats: Vec::new(),
        parent: None,
    }
}

#[tokio::test]
async fn a_rename_on_a_copy_is_refused_and_leaves_its_log_alone() {
    let data_dir = tempfile::tempdir().expect("temp data dir");
    let elsewhere = DeviceId::new();
    let copy = chat(Some(elsewhere));
    let ours = chat(None);
    {
        let mut store = Store::open(data_dir.path().to_path_buf()).expect("store");
        for session in [&copy, &ours] {
            store
                .append_batch(
                    session.id,
                    vec![
                        EventPayload::SessionCreated {
                            session: session.clone(),
                        },
                        EventPayload::MessageAdded {
                            message: ChatMessage {
                                id: "m1".into(),
                                turn: 1,
                                role: MessageRole::User,
                                text: "fix the login redirect loop".into(),
                                origin: None,
                            },
                        },
                    ],
                )
                .expect("prime the log");
        }
    }
    let before = log_lines(data_dir.path(), &copy.id);

    // The daemon over that folder: what a device holds after pulling these
    // chats from the machine that runs them.
    let token = "e2e-owner-token-never-on-the-lan".to_string();
    let store = Arc::new(StdMutex::new(
        Store::open(data_dir.path().to_path_buf()).expect("store"),
    ));
    let supervisor = Supervisor::new(store.clone(), HarnessRegistry::none());
    let app = App::assemble(
        AppOptions {
            data_dir: data_dir.path().to_path_buf(),
            token: token.clone(),
            cloud: common::dead_cloud(),
        },
        store,
        supervisor,
    );
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("addr");
    let daemon = common::TestDaemon::from_parts(addr, token, data_dir);
    tokio::spawn(async move {
        let _ = axum::serve(listener, openremote_daemon::http::router(app)).await;
    });

    // The route refuses the rename, and nothing is written, so the copy is
    // still exactly what the runner sent: no extra event at the sequence
    // number its pull will skip past. The status is 503 rather than the 409
    // the siblings give because `forward_remote` answers first, for a device
    // that is not signed in; the handler's own guard is pinned by
    // `the_supervisor_refuses_a_rename_on_a_copy` below, which is the layer
    // every other write path uses.
    let refused = common::call(
        &daemon,
        "POST",
        &format!("/sessions/{}/title", copy.id),
        Some(json!({ "request_id": "r-copy", "title": "renamed on a copy" })),
    )
    .await;
    assert_eq!(refused.status, 503, "a rename on a copy: {}", refused.body);
    assert!(
        refused.body["error"]
            .as_str()
            .unwrap_or_default()
            .contains("this chat runs on another device"),
        "the refusal says why: {}",
        refused.body
    );
    assert_eq!(
        log_lines(daemon.data_dir(), &copy.id),
        before,
        "the copy's log grew by a rename it must not accept"
    );

    // The control: the chat this device runs renames, so the guard is about
    // where the chat runs and not about renames being broken.
    let renamed = common::call(
        &daemon,
        "POST",
        &format!("/sessions/{}/title", ours.id),
        Some(json!({ "request_id": "r-here", "title": "renamed here" })),
    )
    .await;
    assert_eq!(renamed.status, 200, "a rename here: {}", renamed.body);
    assert_eq!(renamed.body["result"]["title"], "renamed here");
}

/// How many events a chat's log holds, read off the disk the way a peer's copy
/// of it would be.
fn log_lines(data_dir: &std::path::Path, id: &SessionId) -> usize {
    let log = data_dir
        .join("sessions")
        .join(id.to_string())
        .join("events.jsonl");
    std::fs::read_to_string(&log)
        .expect("that chat's log")
        .lines()
        .count()
}

/// The layer the twelve siblings guard: `rename` and `note_mentions` both write
/// an event, and both must refuse a chat that runs elsewhere, because a write
/// on a copy is what puts it out of step with the machine that runs it. The
/// route above is only safe while its forwarding middleware stays mounted; this
/// is the guard that does not depend on that.
#[tokio::test]
async fn the_supervisor_refuses_a_write_on_a_copy() {
    let data_dir = tempfile::tempdir().expect("temp data dir");
    let copy = chat(Some(DeviceId::new()));
    let ours = chat(None);
    let store = Arc::new(StdMutex::new(
        Store::open(data_dir.path().to_path_buf()).expect("store"),
    ));
    {
        let mut held = store.lock().expect("store");
        for session in [&copy, &ours] {
            held.append_batch(
                session.id,
                vec![EventPayload::SessionCreated {
                    session: session.clone(),
                }],
            )
            .expect("prime the log");
        }
    }
    let before = log_lines(data_dir.path(), &copy.id);
    let sup = Supervisor::new(store.clone(), HarnessRegistry::none());
    let elsewhere = |err: &openremote_daemon::supervisor::SupervisorError| {
        matches!(err, openremote_daemon::supervisor::SupervisorError::Conflict(message)
            if message == "this chat runs on another device")
    };

    let renamed = sup.rename(&copy.id, "renamed on a copy");
    assert!(
        renamed.as_ref().err().is_some_and(elsewhere),
        "a rename on a copy was not refused with its siblings' 409: {renamed:?}"
    );
    let noted = sup.note_mentions(&copy.id, &[ours.id]);
    assert!(
        noted.as_ref().err().is_some_and(elsewhere),
        "a mention note on a copy was not refused: {noted:?}"
    );
    assert_eq!(
        log_lines(data_dir.path(), &copy.id),
        before,
        "the copy's log grew by a write it must not accept"
    );

    // The control: both paths work where the chat runs.
    assert!(sup.rename(&ours.id, "renamed here").is_ok());
    assert!(sup.note_mentions(&ours.id, &[copy.id]).is_ok());
}
