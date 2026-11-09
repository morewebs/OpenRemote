//! The fixture's Grok Build print mode: one run per prompt, emitting the
//! Anthropic-Messages streaming wire (`system/init`, `assistant`,
//! `result`) exactly as the openremote-grok driver expects. The argv the
//! real driver builds is the truth: `--output-format
//! streaming-messages-json --cwd <dir> [--model <m>]
//! [--permission-mode <mode>] [--resume <id>] --prompt-file <path>`. The
//! prompt is the file's contents, so chat text never stands in an argv
//! slot the CLI's parser could read; the inline `--single <text>` form is
//! still read because the CLI has it.
//!
//! Scenarios: `plain` completes the turn; `die` exits after init with no
//! result (the process died mid-turn - the daemon maps it to a failed
//! turn with a note); `slow` holds the turn open for a moment (the e2e
//! fires a second prompt inside the Working window).

use std::io::{BufRead, Write};

use serde_json::{Value, json};

pub fn main_grok(argv: &[String]) {
    let scenario = std::env::var("FIXTURE_AGENT_SCENARIO")
        .ok()
        .or_else(|| {
            std::fs::read_to_string("fixture-scenario")
                .ok()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
        })
        .unwrap_or_else(|| "plain".to_string());

    // One line per run (harness audit F8): the e2e counts these to prove a
    // refused prompt never spawned a second process - the transcript can
    // only show what a process said; this shows whether it existed.
    if let Ok(mut runs) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open("fixture-grok-runs")
    {
        let _ = writeln!(runs, "run");
    }
    if scenario == "slow" {
        // Long enough for the e2e's second prompt to land inside the
        // turn, short enough not to slow the suite down.
        std::thread::sleep(std::time::Duration::from_millis(1500));
    }
    if scenario == "hold" {
        // The e2e owns the turn's length: it releases the process when it
        // has seen what it needed. A fixed sleep would let a busy machine
        // end the turn before the test got there.
        crate::wait_for_release();
    }

    // argv: [--output-format, streaming-messages-json, --cwd, <dir>,
    //        (--model, <m>)?, (--permission-mode, <mode>)?,
    //        (--resume, <id>)?, --prompt-file, <path>]
    // The prompt is the file's contents, including a dash-leading one.
    // The e2e watches that "-v ..." comes back as text, not a flag guess.
    let mut prompt = String::new();
    let mut resume: Option<String> = None;
    let mut model = String::from("grok-4.7-fixture");
    let mut permission_mode = String::from("default");
    let mut i = 0;
    while i < argv.len() {
        match argv[i].as_str() {
            "--single" => {
                if let Some(text) = argv.get(i + 1) {
                    prompt = text.clone();
                    i += 1;
                }
            }
            "--prompt-file" => {
                if let Some(path) = argv.get(i + 1) {
                    prompt = std::fs::read_to_string(path).unwrap_or_default();
                    i += 1;
                }
            }
            "--resume" => {
                if let Some(id) = argv.get(i + 1) {
                    resume = Some(id.clone());
                    i += 1;
                }
            }
            "--model" => {
                if let Some(m) = argv.get(i + 1) {
                    model = m.clone();
                    i += 1;
                }
            }
            "--permission-mode" => {
                if let Some(mode) = argv.get(i + 1) {
                    permission_mode = mode.clone();
                    i += 1;
                }
            }
            _ => {}
        }
        i += 1;
    }

    // A fresh conversation unless resumed: a fixed id would make a resume
    // the driver dropped look exactly like one it kept.
    let session_id = resume.unwrap_or_else(|| format!("grok-fixture-{}", std::process::id()));
    let out = |value: Value| {
        let mut stdout = std::io::stdout().lock();
        serde_json::to_writer(&mut stdout, &value).ok();
        stdout.write_all(b"\n").ok();
        stdout.flush().ok();
    };

    out(json!({
        "type": "system",
        "subtype": "init",
        "session_id": session_id,
        "model": model,
        "permissionMode": permission_mode,
        "cwd": "."
    }));
    if scenario == "die" {
        // The process dies mid-turn: no assistant, no result.
        eprintln!("fixture-grok: dying mid-turn");
        return;
    }
    out(json!({
        "type": "assistant",
        "session_id": session_id,
        "message": {
            "id": "msg_fixture",
            "role": "assistant",
            "model": model,
            "content": [{"type": "text", "text": format!("done: {prompt}")}],
            "stop_reason": "end_turn",
            "usage": {"input_tokens": 10, "output_tokens": 5}
        }
    }));
    out(json!({
        "type": "result",
        "subtype": "success",
        "session_id": session_id,
        "result": format!("done: {prompt}"),
        "is_error": false,
        "num_turns": 1,
        "usage": {"input_tokens": 10, "output_tokens": 5},
        "total_cost_usd": 0.02
    }));
    // Keep stdin semantics irrelevant (grok runs with stdin closed); the
    // reader sees EOF and the result frame already ended the turn.
    let _ = std::io::stdin().lock().lines().next();
}
