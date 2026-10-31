//! A plugin harness driver fixture. It is the smallest thing that speaks the
//! seam: one `--probe` line, then one scripted turn per prompt.
//!
//! The e2e suite points a `HarnessSpec` at this binary so the whole path runs
//! for real - the daemon starts the process, writes `spawn` and `prompt`, and
//! the supervisor translates the events. A plugin author can read it beside
//! `docs/harness-plugins.md` for the shape of a driver that needs no harness
//! of its own.

use std::io::{BufRead, Write};

fn main() {
    if std::env::args().any(|argument| argument == "--probe") {
        // A probe is one line and an exit: no chat, no account reached.
        println!(r#"{{"available":true,"version":"0.0.1","signed_in":true,"reason":null}}"#);
        return;
    }

    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&line) else {
            eprintln!("fixture-driver: not JSON: {line}");
            continue;
        };
        let kind = value
            .get("type")
            .and_then(|kind| kind.as_str())
            .unwrap_or_default();
        match kind {
            "spawn" => emit(
                &mut stdout,
                serde_json::json!({
                    "type": "initialized",
                    "session_ref": "fixture-1",
                    "model": null,
                    "permission_mode": null
                }),
            ),
            "prompt" => {
                let text = value
                    .get("text")
                    .and_then(|text| text.as_str())
                    .unwrap_or_default();
                // Tool events first, then the reply: the console's stream
                // ends on the message the turn completed with, the way the
                // real drivers order theirs.
                emit(
                    &mut stdout,
                    serde_json::json!({
                        "type": "tool_started",
                        "tool_use_id": "call-1",
                        "name": "read_files",
                        "input": {"paths": ["a.py"]}
                    }),
                );
                emit(
                    &mut stdout,
                    serde_json::json!({
                        "type": "tool_result",
                        "tool_use_id": "call-1",
                        "name": "read_files",
                        "text": "print(1)",
                        "is_error": false
                    }),
                );
                emit(
                    &mut stdout,
                    serde_json::json!({
                        "type": "text",
                        "text": format!("fixture heard: {text}")
                    }),
                );
                emit(
                    &mut stdout,
                    serde_json::json!({
                        "type": "turn_completed",
                        "subtype": "finished",
                        "coarse": "completed",
                        "is_error": false,
                        "error_message": null
                    }),
                );
            }
            "interrupt" => emit(
                &mut stdout,
                serde_json::json!({
                    "type": "turn_completed",
                    "subtype": "interrupted",
                    "coarse": "interrupted",
                    "is_error": false
                }),
            ),
            "settings" => eprintln!("fixture-driver: settings are not applied live"),
            "answer" => eprintln!("fixture-driver: asks no approvals"),
            "shutdown" => break,
            other => eprintln!("fixture-driver: unknown command '{other}'"),
        }
    }
}

fn emit(stdout: &mut impl Write, value: serde_json::Value) {
    writeln!(stdout, "{value}").expect("write an event");
    stdout.flush().expect("flush an event");
}
