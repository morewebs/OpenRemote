//! A template driver for an OpenRemote harness plugin.
//!
//! The daemon starts this program once per chat and speaks JSON lines over
//! stdin and stdout. Every answer is an event; the vocabulary is in
//! `docs/harness-plugins.md`. The marked places are the only ones a real
//! driver has to change.
//!
//! This skeleton already completes the loop: `--probe` answers (as
//! unavailable, until you wire it), a `spawn` names a session, and a `prompt`
//! echoes back and closes a turn. Replace the echo with your harness.

use std::io::{BufRead, Write};

fn main() {
    // The probe is one JSON line and an exit. It runs at startup and after
    // every plugin change, so keep it cheap and side-effect free.
    if std::env::args().any(|argument| argument == "--probe") {
        // TODO: find the harness CLI (PATH, a config dir) and ask it for its
        // version and sign-in state. `available: false` plus a reason is the
        // honest answer while this is unwired.
        println!(
            r#"{{"available":false,"version":null,"signed_in":null,"reason":"the example driver's probe is not wired to a harness yet"}}"#
        );
        return;
    }

    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        let Ok(command) = serde_json::from_str::<serde_json::Value>(&line) else {
            eprintln!("example-harness-driver: not JSON: {line}");
            continue;
        };
        let kind = command
            .get("type")
            .and_then(|kind| kind.as_str())
            .unwrap_or_default();
        match kind {
            "spawn" => {
                // TODO: start the harness here, in the `cwd` the command
                // names, and resume the `resume` session when it is set.
                // `session_ref` is the id a later resume will be given.
                emit(
                    &mut stdout,
                    serde_json::json!({
                        "type": "initialized",
                        "session_ref": "example-1",
                        "model": null,
                        "permission_mode": null
                    }),
                );
            }
            "prompt" => {
                let text = command
                    .get("text")
                    .and_then(|text| text.as_str())
                    .unwrap_or_default();
                // TODO: send `text` to the harness, then emit what it does:
                // `tool_started` before a tool, `tool_result` with its output,
                // and text as it answers. `turn_completed` closes the turn,
                // and only the harness's own end may say `completed`.
                emit(
                    &mut stdout,
                    serde_json::json!({
                        "type": "text",
                        "text": format!("the example driver heard: {text}")
                    }),
                );
                emit(
                    &mut stdout,
                    serde_json::json!({
                        "type": "turn_completed",
                        "subtype": "completed",
                        "coarse": "completed",
                        "is_error": false,
                        "error_message": null
                    }),
                );
            }
            "interrupt" => {
                // TODO: cancel the turn in the harness. If the harness has no
                // end-of-turn signal, wait for it to settle and then send the
                // one boundary a driver may synthesize.
                emit(
                    &mut stdout,
                    serde_json::json!({
                        "type": "turn_completed",
                        "subtype": "interrupted",
                        "coarse": "interrupted",
                        "is_error": false
                    }),
                );
            }
            "settings" => {
                // A live model or effort change is optional. Refusing on
                // stderr is better than pretending.
                eprintln!("example-harness-driver: settings are not applied live");
            }
            "answer" => {
                eprintln!("example-harness-driver: this harness asks no approvals");
            }
            "shutdown" => break,
            other => eprintln!("example-harness-driver: unknown command '{other}'"),
        }
    }
}

/// One event per line on stdout, flushed so the daemon sees it at once.
fn emit(stdout: &mut impl Write, event: serde_json::Value) {
    writeln!(stdout, "{event}").expect("write an event");
    stdout.flush().expect("flush an event");
}
