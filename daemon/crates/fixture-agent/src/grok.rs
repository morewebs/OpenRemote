//! The fixture's Grok Build print mode: one run per prompt, emitting the
//! Anthropic-Messages streaming wire (`system/init`, `assistant`,
//! `result`) exactly as the openremote-grok driver expects. The argv the
//! real driver builds is the truth: `--single <text> --output-format
//! streaming-messages-json --cwd <dir> [--resume <id>]`.
//!
//! Scenarios: `plain` completes the turn; `die` exits after init with no
//! result (the process died mid-turn - the daemon maps it to a failed
//! turn with a note).

use std::io::{BufRead, Write};

use serde_json::{Value, json};

const SESSION_ID: &str = "grok-fixture-1";

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

    // argv: [.., --single, <text>, --output-format, streaming-messages-json,
    //        --cwd, <dir>, (--model, <m>)?, (--resume, <id>)?]
    let mut prompt = String::new();
    let mut resume: Option<String> = None;
    let mut model = String::from("grok-4.7-fixture");
    let mut i = 0;
    while i < argv.len() {
        match argv[i].as_str() {
            "--single" => {
                if let Some(text) = argv.get(i + 1) {
                    prompt = text.clone();
                }
            }
            "--resume" => {
                if let Some(id) = argv.get(i + 1) {
                    resume = Some(id.clone());
                }
            }
            "--model" => {
                if let Some(m) = argv.get(i + 1) {
                    model = m.clone();
                }
            }
            _ => {}
        }
        i += 1;
    }

    let session_id = resume.unwrap_or_else(|| SESSION_ID.to_string());
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
        "permissionMode": "default",
        "cwd": "."
    }));
    if scenario == "die" {
        // The process dies mid-turn: no assistant, no result.
        eprintln!("fixture-grok: dying mid-turn");
        return;
    }
    out(json!({
        "type": "assistant",
        "session_id": SESSION_ID,
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
        "session_id": SESSION_ID,
        "result": format!("done: {prompt}"),
        "is_error": false,
        "num_turns": 1,
        "usage": {"input_tokens": 10, "output_tokens": 5}
    }));
    // Keep stdin semantics irrelevant (grok runs with stdin closed); the
    // reader sees EOF and the result frame already ended the turn.
    let _ = std::io::stdin().lock().lines().next();
}
