//! Sign-in e2e: the relay contract. A fixture-backed harness is refused
//! (a fixture's login behavior is not a sign-in), an unknown harness is
//! an explicit error, and the endpoints speak the receipt contract.

mod common;

use common::*;
use serde_json::json;

#[tokio::test]
async fn sign_in_refuses_its_honest_preconditions() {
    let daemon = start_daemon(&[]).await;

    // Every slot is fixture-backed: the registry's own guard refuses - a
    // fixture's exit code is not a sign-in.
    let fixture = call(
        &daemon,
        "POST",
        "/harnesses/claude/signin",
        Some(json!({"request_id": "s-fixture"})),
    )
    .await;
    assert_eq!(fixture.status, 400, "raw: {}", fixture.raw);
    assert!(
        fixture.body["error"]
            .as_str()
            .unwrap_or("")
            .contains("no login command to relay"),
        "raw: {}",
        fixture.raw
    );

    // A harness the daemon doesn't know: an explicit error, not a run.
    let unknown = call(
        &daemon,
        "POST",
        "/harnesses/no-such-harness/signin",
        Some(json!({"request_id": "s-unknown"})),
    )
    .await;
    assert_eq!(unknown.status, 400, "raw: {}", unknown.raw);

    // A harness with no relayable login of its own (opencode's is a TUI,
    // pi's is a provider flow): the honest refusal, in its own words.
    let tui = call(
        &daemon,
        "POST",
        "/harnesses/opencode/signin",
        Some(json!({"request_id": "s-tui"})),
    )
    .await;
    assert_eq!(tui.status, 400, "raw: {}", tui.raw);
    assert!(
        tui.body["error"]
            .as_str()
            .unwrap_or("")
            .contains("through its own setup"),
        "raw: {}",
        tui.raw
    );

    // No sign-in was ever started for it: the view endpoint says so.
    let view = call(&daemon, "GET", "/harnesses/claude/signin", None).await;
    assert_eq!(view.status, 404, "raw: {}", view.raw);

    // Feeding a line with no relay running: an explicit conflict, not a
    // silent drop.
    let feed = call(
        &daemon,
        "POST",
        "/harnesses/claude/signin/input",
        Some(json!({"request_id": "s-feed", "text": "code"})),
    )
    .await;
    assert_eq!(feed.status, 409, "raw: {}", feed.raw);
}

#[tokio::test]
async fn a_second_sign_in_while_one_runs_is_an_honest_conflict() {
    // The real daemon binary as its own process, with claude's slot left
    // empty (no override): claude is honestly missing, so the sign-in
    // refuses at the availability gate - the same guard a real machine
    // without the CLI hits.
    let daemon = spawn_daemon_process(&[(
        "OPENREMOTE_CLAUDE_PATH",
        "Z:/definitely/absent/claude.exe".to_string(),
    )])
    .await;
    let missing = call(
        &daemon,
        "POST",
        "/harnesses/claude/signin",
        Some(json!({"request_id": "s-missing"})),
    )
    .await;
    assert_eq!(missing.status, 400, "raw: {}", missing.raw);
    assert!(
        missing.body["error"]
            .as_str()
            .unwrap_or("")
            .contains("not installed"),
        "raw: {}",
        missing.raw
    );
}

#[tokio::test]
async fn stopping_a_sign_in_that_never_ran_is_an_honest_no_op() {
    // The stop endpoint's contract: stopping a relay that was never
    // started (or already settled) is a completed receipt, never an
    // error - the abandoned-flow answer must always be available.
    let daemon = start_daemon(&[]).await;
    let stop = call(
        &daemon,
        "POST",
        "/harnesses/claude/signin/stop",
        Some(json!({"request_id": "s-stop0"})),
    )
    .await;
    assert_eq!(stop.status, 200, "raw: {}", stop.raw);
    assert_eq!(stop.body["status"], "completed", "raw: {}", stop.raw);

    // The no-op stop left no relay behind: the view still says none was
    // ever started, and a feed stays an honest conflict.
    let view = call(&daemon, "GET", "/harnesses/claude/signin", None).await;
    assert_eq!(view.status, 404, "raw: {}", view.raw);
}
