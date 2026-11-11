//! Supervisor lifecycle e2e: the hardening paths the audit found - stop
//! settling a session whose harness already died (the fallback drain),
//! a harness uninstalled mid-run disappearing from the console's
//! offers (lazy eviction), and the daemon's exit sweep taking its
//! harness children with it instead of orphaning them - also when its
//! app dies and every pipe end goes at once.
//!
//! The quiet-session stop baseline lives in e2e.rs
//! (`stop_retires_pending_decisions_and_the_late_answer_conflicts`);
//! this suite stages what that one cannot: dead drivers, vanishing
//! binaries, and process-level exit.

mod common;

use common::*;
use serde_json::json;

/// A grok print-mode session: process-per-prompt, so a `die` turn leaves
/// the driver with no live child and no StdoutClosed ever coming - the
/// exact shape stop's fallback drain exists for.
async fn grok_session(scenario: &str) -> (TestDaemon, tempfile::TempDir, String) {
    let daemon = start_daemon(&[("grok", fixture_agent())]).await;
    let ws = workspace(Some(scenario));
    let created = call(
        &daemon,
        "POST",
        "/sessions",
        Some(json!({"request_id": "l-create", "harness": "grok", "workspace": ws.path()})),
    )
    .await;
    assert_eq!(created.status, 201, "raw: {}", created.raw);
    let id = created.body["id"].as_str().expect("session id").to_string();
    (daemon, ws, id)
}

/// Stop after the harness died on its own (harness audit F12's fallback
/// and F10's removal): a grok `die` turn kills the prompt process with
/// no result and no StdoutClosed - the pump will never answer the stop.
/// Stop must still settle the session (not leave it answerable), retire
/// its pending decisions, and remove the map entry: a later answer
/// conflicts by name, not by dangling promise.
#[tokio::test]
async fn stop_after_the_harness_died_still_settles_and_retires() {
    let (daemon, ws, id) = grok_session("die").await;
    call(
        &daemon,
        "POST",
        &format!("/sessions/{id}/prompts"),
        Some(json!({"request_id": "l-p1", "text": "die on me"})),
    )
    .await;
    // The failed turn boundary lands (grok's own EOF mapping); the
    // session then sits idle-alive with a dead child - the zombie window.
    until_kinds(
        &daemon,
        &format!("/sessions/{id}/events"),
        &["turn.completed", "session.status_changed"],
    )
    .await;

    let stop = call(
        &daemon,
        "POST",
        &format!("/sessions/{id}/stop"),
        Some(json!({"request_id": "l-stop"})),
    )
    .await;
    assert_eq!(stop.status, 200, "raw: {}", stop.raw);
    assert_eq!(
        stop.body["status"], "stopped",
        "the dead-driver session settles stopped; body: {}",
        stop.raw
    );

    // Nothing stays answerable: a prompt against the removed entry
    // refuses by name - the honest error for a chat no process backs.
    let late = call(
        &daemon,
        "POST",
        &format!("/sessions/{id}/prompts"),
        Some(json!({"request_id": "l-late", "text": "anyone there"})),
    )
    .await;
    assert_eq!(
        late.status, 404,
        "a stopped chat's prompt answers not-found; raw: {}",
        late.raw
    );
    assert!(
        late.body["error"]
            .as_str()
            .unwrap_or("")
            .contains("not running"),
        "the refusal names the contract; raw: {}",
        late.raw
    );

    // The transcript holds exactly one boundary for the turn - the
    // harness's own failed one, not a duplicate from the settle.
    let events = sse_collect(&daemon, &format!("/sessions/{id}/events"), 6).await;
    let boundaries = events
        .iter()
        .filter(|(_, p)| kind(p) == "turn.completed")
        .count();
    assert_eq!(
        boundaries,
        1,
        "one boundary for one turn; events:\n{}",
        dump(&events)
    );
    let _ = ws;
}

/// A harness uninstalled mid-run (harness audit F18): the registry's
/// probe facts are stale the moment the binary vanishes. The create
/// succeeds (grok spawns no process until the first prompt), the prompt
/// fails with a raw OS error, the supervisor looks, the files are gone,
/// and the harness stops being offered - the console can no longer open
/// chats against a binary that is not there.
#[tokio::test]
async fn a_harness_uninstalled_mid_run_disappears_from_the_offers() {
    // A fixture binary in a temp dir the test owns: present for the
    // probe, deleted before the prompt - the uninstall, staged.
    let dir = tempfile::tempdir().expect("fixture dir");
    let bin = dir.path().join(if cfg!(windows) {
        "vanished-harness.exe"
    } else {
        "vanished-harness"
    });
    std::fs::copy(fixture_agent(), &bin).expect("copy fixture");
    let daemon = start_daemon(&[("grok", bin.clone())]).await;
    let ws = workspace(None);

    // The probe saw it: grok is offered and the create takes it.
    let caps = call(&daemon, "GET", "/capabilities", None).await;
    let offered = caps.body["harnesses"]
        .as_array()
        .expect("harnesses")
        .iter()
        .any(|h| h["id"] == "grok" && h["available"] == true);
    assert!(offered, "grok is offered while the binary exists");
    let created = call(
        &daemon,
        "POST",
        "/sessions",
        Some(json!({"request_id": "l-create-gone", "harness": "grok", "workspace": ws.path()})),
    )
    .await;
    assert_eq!(created.status, 201, "raw: {}", created.raw);
    let id = created.body["id"].as_str().expect("session id").to_string();

    // The uninstall: the binary the registry resolved is gone. The next
    // prompt's spawn fails with the OS's own words.
    std::fs::remove_file(&bin).expect("delete the fixture binary");
    let prompted = call(
        &daemon,
        "POST",
        &format!("/sessions/{id}/prompts"),
        Some(json!({"request_id": "l-p1", "text": "gone already"})),
    )
    .await;
    assert_eq!(
        prompted.status, 500,
        "the failed spawn surfaces its own error; raw: {}",
        prompted.raw
    );
    let session = until_status(&daemon, &id, &["failed"]).await;
    assert!(
        session["last_error"]
            .as_str()
            .unwrap_or_default()
            .contains("spawn failed"),
        "the session's first fact is the spawn failure; got: {session}"
    );

    // The dangling turn the prompt opened got its failed boundary - the
    // F17 half of the same prompt path.
    let events = sse_collect(&daemon, &format!("/sessions/{id}/events"), 6).await;
    let kinds: Vec<String> = events.iter().map(|(_, p)| kind(p)).collect();
    assert!(
        kinds.contains(&"turn.completed".to_string()),
        "the turn the failed prompt opened is closed; kinds: {kinds:?}"
    );

    // The eviction: the console's next look no longer offers grok.
    let caps = call(&daemon, "GET", "/capabilities", None).await;
    let grok = caps.body["harnesses"]
        .as_array()
        .expect("harnesses")
        .iter()
        .find(|h| h["id"] == "grok")
        .expect("grok keeps its slot")
        .clone();
    assert_eq!(
        grok["available"], false,
        "the vanished harness is no longer offered; raw: {}",
        caps.raw
    );

    // And a second create answers the harness error, not another raw
    // OS spawn error: the slot is closed until a re-probe.
    let second = call(
        &daemon,
        "POST",
        "/sessions",
        Some(json!({"request_id": "l-create-again", "harness": "grok", "workspace": ws.path()})),
    )
    .await;
    assert_eq!(
        second.status, 400,
        "the evicted harness refuses cleanly; raw: {}",
        second.raw
    );
    assert!(
        second.body["error"]
            .as_str()
            .unwrap_or("")
            .contains("not available"),
        "the refusal names the harness, not the OS; raw: {}",
        second.raw
    );
}

/// The daemon's exit sweep (harness audit F3): the stdin watchdog path
/// runs a bounded shutdown of every live session before exit(0). The
/// opencode fixture is a real serve child with null stdin that would
/// otherwise outlive the daemon - exactly the orphan. The daemon's
/// stdin closes (the shell's death), the sweep runs, the daemon exits,
/// and the child must be gone with it.
#[tokio::test]
async fn the_daemons_exit_sweep_takes_the_serve_child_with_it() {
    let bin = fixture_agent();
    let data_dir = tempfile::tempdir().expect("temp data dir");
    let token = uuid::Uuid::new_v4().to_string();
    std::fs::create_dir_all(data_dir.path()).expect("data dir");
    std::fs::write(data_dir.path().join("token"), &token).expect("token file");
    let mut command = std::process::Command::new(daemon_binary());
    command
        .env("OPENREMOTE_DATA_DIR", data_dir.path())
        .env("OPENREMOTE_OPENCODE_PATH", bin.display().to_string())
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null());
    let mut child = command.spawn().expect("spawn daemon process");
    let mut stdout = child.stdout.take().expect("stdout piped");
    let stdin = child.stdin.take().expect("stdin piped");

    // The READY/DATA_DIR handshake, in the test (bounded).
    use std::io::BufRead;
    let addr = {
        let mut line = String::new();
        let mut reader = std::io::BufReader::new(&mut stdout);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        loop {
            if std::time::Instant::now() >= deadline {
                panic!("daemon never said READY");
            }
            line.clear();
            let _ = reader.read_line(&mut line);
            if let Some(rest) = line.trim().strip_prefix("READY ") {
                break rest.parse::<std::net::SocketAddr>().expect("address");
            }
        }
    };

    // A tiny TestDaemon stand-in for the call helpers: the address and
    // token the raw client needs.
    let daemon = TestDaemon::from_parts(addr, token.clone(), data_dir);

    // A live opencode session: the serve child is up.
    let ws = workspace(None);
    let created = call(
        &daemon,
        "POST",
        "/sessions",
        Some(json!({
            "request_id": "l-op-create",
            "harness": "opencode",
            "workspace": ws.path(),
        })),
    )
    .await;
    assert_eq!(created.status, 201, "raw: {}", created.raw);
    let id = created.body["id"].as_str().expect("session id").to_string();
    until_kinds(
        &daemon,
        &format!("/sessions/{id}/events"),
        &["session.status_changed"],
    )
    .await;

    let before = fixture_agent_pids().await;
    assert!(!before.is_empty(), "the opencode serve child is running");

    // The shell's death: close the daemon's stdin. The watchdog fires,
    // the sweep runs, the daemon exits 0 - and the serve child goes with
    // it. No kill from this side; the daemon's own exit path is the
    // thing under test.
    drop(stdin);
    let exited = tokio::task::spawn_blocking(move || child.wait().expect("daemon reaped"))
        .await
        .expect("wait task");
    assert!(exited.success(), "the daemon exited cleanly");

    // The sweep is bounded but real; give it the window and then some.
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(30);
    loop {
        let after = fixture_agent_pids().await;
        if after.is_empty() || tokio::time::Instant::now() >= deadline {
            assert!(
                after.is_empty(),
                "the serve child outlived the daemon - the orphan F3 names; still running: {after:?}"
            );
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
    }
}

/// The app's death as it really happens: every pipe end at once, the
/// reader of the daemon's stderr included. With that reader gone every
/// log line fails, and a goodbye printed with `eprintln!` panics the
/// watchdog's task before its sweep - the daemon lived on after its window
/// closed, its harness children with it. A host process stands in for the
/// app (it holds all three pipes, as the app does); a claude chat (its
/// child reads stdin) and an opencode chat (a serve child with null stdin)
/// run; the host is killed. The daemon must exit and take both children
/// with it.
#[tokio::test]
async fn the_daemon_leaves_when_its_app_dies() {
    let fixture = fixture_agent();
    let data_dir = tempfile::tempdir().expect("temp data dir");
    let token = uuid::Uuid::new_v4().to_string();
    std::fs::write(data_dir.path().join("token"), &token).expect("token file");
    let mut host = std::process::Command::new(&fixture)
        .arg("sidecar-host")
        .arg(daemon_binary())
        .env("OPENREMOTE_DATA_DIR", data_dir.path())
        .env("OPENREMOTE_CLAUDE_PATH", &fixture)
        .env("OPENREMOTE_OPENCODE_PATH", &fixture)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("spawn the sidecar host");
    let stdout = host.stdout.take().expect("host stdout piped");

    // DAEMON_PID from the host, then the daemon's own READY - read on a
    // thread, waited on with a deadline.
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        use std::io::BufRead;
        let mut pid = None;
        let mut lines = std::io::BufReader::new(stdout)
            .lines()
            .map_while(Result::ok);
        for line in lines.by_ref() {
            if let Some(rest) = line.strip_prefix("DAEMON_PID ") {
                pid = rest.trim().parse::<u32>().ok();
            }
            if let Some(rest) = line.strip_prefix("READY ") {
                let _ = tx.send((pid, rest.trim().parse::<std::net::SocketAddr>().ok()));
                break;
            }
        }
        // Keep reading, so the host never blocks on a full pipe.
        for _ in lines {}
    });
    let (pid, addr) = rx
        .recv_timeout(std::time::Duration::from_secs(30))
        .expect("the daemon said READY through its host");
    let daemon_pid = pid.expect("the host named the daemon's pid");
    let daemon = TestDaemon::from_parts(addr.expect("a loopback address"), token, data_dir);

    let ws = workspace(None);
    for (request_id, harness) in [("l-app-claude", "claude"), ("l-app-opencode", "opencode")] {
        let created = call(
            &daemon,
            "POST",
            "/sessions",
            Some(json!({"request_id": request_id, "harness": harness, "workspace": ws.path()})),
        )
        .await;
        assert_eq!(created.status, 201, "raw: {}", created.raw);
    }
    let started = tokio::time::Instant::now() + std::time::Duration::from_secs(30);
    let children = loop {
        let children = harness_children(daemon_pid);
        if children.len() >= 2 {
            break children;
        }
        if tokio::time::Instant::now() >= started {
            stop_pid(daemon_pid);
            panic!("both chats' harness children never ran; saw {children:?}");
        }
        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
    };

    // The app dies, and every pipe end the daemon talks to goes with it.
    let _ = host.kill();
    let _ = host.wait();

    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(30);
    while pid_alive(daemon_pid) {
        if tokio::time::Instant::now() >= deadline {
            stop_pid(daemon_pid);
            panic!("the daemon outlived its app by 30 s (its harness children: {children:?})");
        }
        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
    }
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(30);
    loop {
        let left: Vec<u32> = children
            .iter()
            .copied()
            .filter(|&pid| pid_alive(pid))
            .collect();
        if left.is_empty() {
            break;
        }
        if tokio::time::Instant::now() >= deadline {
            left.iter().for_each(|&pid| stop_pid(pid));
            panic!("harness children outlived the daemon: {left:?}");
        }
        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
    }
}

/// The fixture-agent children of one process, as PIDs: this test's own,
/// never another suite's running beside it.
fn harness_children(parent: u32) -> Vec<u32> {
    let output = if cfg!(windows) {
        std::process::Command::new("powershell")
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                &format!(
                    "Get-CimInstance Win32_Process -Filter 'ParentProcessId={parent}' | \
                     Where-Object {{ $_.Name -like 'fixture-agent*' }} | \
                     ForEach-Object {{ $_.ProcessId }}"
                ),
            ])
            .output()
            .expect("powershell runs")
    } else {
        std::process::Command::new("pgrep")
            .args(["-P", &parent.to_string(), "-f", "fixture-agent"])
            .output()
            .expect("pgrep runs")
    };
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| line.trim().parse().ok())
        .collect()
}

/// Whether a process is still running (a zombie waiting to be reaped is
/// not).
fn pid_alive(pid: u32) -> bool {
    if cfg!(windows) {
        let output = std::process::Command::new("tasklist")
            .args(["/FI", &format!("PID eq {pid}"), "/FO", "CSV", "/NH"])
            .output()
            .expect("tasklist runs");
        String::from_utf8_lossy(&output.stdout).contains(&format!(",\"{pid}\","))
    } else {
        let output = std::process::Command::new("ps")
            .args(["-p", &pid.to_string(), "-o", "stat="])
            .output()
            .expect("ps runs");
        let stat = String::from_utf8_lossy(&output.stdout);
        let stat = stat.trim();
        !stat.is_empty() && !stat.starts_with('Z')
    }
}

/// A failed run cleans up after itself: the process and its tree go.
fn stop_pid(pid: u32) {
    let _ = if cfg!(windows) {
        std::process::Command::new("taskkill")
            .args(["/F", "/T", "/PID", &pid.to_string()])
            .output()
    } else {
        std::process::Command::new("kill")
            .args(["-9", &pid.to_string()])
            .output()
    };
}

/// The fixture-agent processes running right now, as PIDs. Windows
/// tasklist speaks CSV; elsewhere pgrep lists them (exit 1 when none
/// match, which output() still returns) - the CI runners carry both.
async fn fixture_agent_pids() -> Vec<String> {
    let name = if cfg!(windows) {
        "fixture-agent.exe"
    } else {
        "fixture-agent"
    };
    let output = if cfg!(windows) {
        std::process::Command::new("tasklist")
            .args(["/FI", &format!("IMAGENAME eq {name}"), "/FO", "CSV", "/NH"])
            .output()
            .expect("tasklist runs")
    } else {
        std::process::Command::new("pgrep")
            .arg("-f")
            .arg(name)
            .output()
            .expect("pgrep runs")
    };
    let text = String::from_utf8_lossy(&output.stdout).to_string();
    if cfg!(windows) {
        text.lines()
            .filter(|line| line.contains(&format!("\"{name}\"")))
            .filter_map(|line| {
                // CSV: "name","pid",...
                line.split(',')
                    .nth(1)
                    .map(|pid| pid.trim_matches('"').to_string())
            })
            .collect()
    } else {
        // One PID per line, nothing to parse out.
        text.lines()
            .map(|line| line.trim().to_string())
            .filter(|pid| !pid.is_empty())
            .collect()
    }
}
