//! toy: an example OpenRemote process plugin, in one program with two
//! jobs. OpenRemote starts it with one of two flags:
//!
//! - `--tools`: the plugin's program. It answers the agent tools
//!   `word_count` and `greet` over newline-delimited JSON-RPC 2.0 on stdin
//!   and stdout (docs/plugins/protocol.md). OpenRemote starts it the first
//!   time a chat's agent calls one of them, with a cleared environment and
//!   the plugin's settings in `initialize`.
//! - `--driver`: the harness driver for "Toy CLI", started once per chat.
//!   It speaks the JSON lines of docs/harness-plugins.md: a stand-in for a
//!   real command-line agent that echoes (or mirrors) what it is told and
//!   asks before it shouts. `--driver --probe` answers the probe.
//!
//! Why one compiled program: OpenRemote runs only programs inside the
//! plugin's own folder, never one found on PATH, so a plugin ships what it
//! runs. Build it with ../build.sh, which puts it in bin/.
//!
//! stdout carries protocol messages only; anything for a person goes to
//! stderr. When stdin closes, OpenRemote is gone and the program exits.

use std::io::{BufRead, Write};

use serde_json::{json, Value};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let has = |flag: &str| args.iter().any(|a| a == flag);
    if has("--driver") {
        if has("--probe") {
            probe();
        } else {
            driver();
        }
    } else if has("--tools") {
        tools();
    } else {
        eprintln!("toy: start it with --tools (the plugin's program) or --driver (the harness)");
        std::process::exit(2);
    }
}

/// One message per line on stdout, flushed so OpenRemote sees it at once.
fn send(stdout: &mut impl Write, message: Value) {
    writeln!(stdout, "{message}").expect("write a message");
    stdout.flush().expect("flush a message");
}

fn count(text: &str) -> String {
    let words = text.split_whitespace().count();
    let lines = if text.is_empty() {
        0
    } else {
        text.lines().count()
    };
    format!(
        "{words} words, {lines} lines, {} characters",
        text.chars().count()
    )
}

// ---- The plugin's program: agent tools over JSON-RPC ----

fn tools() {
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    // The plugin's settings as OpenRemote hands them over: `initialize`
    // first, then `settings/changed` whenever the user saves new ones. A
    // secret's value arrives here and nowhere else (never in argv or the
    // environment).
    let mut settings = Value::Null;
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        let Ok(message) = serde_json::from_str::<Value>(&line) else {
            eprintln!("toy: a line that is not JSON was ignored");
            continue;
        };
        let method = message["method"].as_str().unwrap_or_default();
        let Some(id) = message.get("id").cloned() else {
            // A notification: no answer.
            if method == "settings/changed" {
                settings = message["params"]["settings"].clone();
            }
            continue;
        };
        let params = &message["params"];
        let answer = match method {
            "initialize" => {
                settings = params["settings"].clone();
                // `log` is a notification OpenRemote writes to its own log.
                send(
                    &mut stdout,
                    json!({"jsonrpc": "2.0", "method": "log", "params": {
                        "level": "info",
                        "message": format!("started on {}", params["platform"].as_str().unwrap_or("?")),
                    }}),
                );
                Ok(json!({"protocol": 1}))
            }
            "tools/call" => call_tool(params, &settings),
            other => Err((-32601, format!("toy has no method {other}"))),
        };
        let reply = match answer {
            Ok(result) => json!({"jsonrpc": "2.0", "id": id, "result": result}),
            Err((code, message)) => {
                json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}})
            }
        };
        send(&mut stdout, reply);
    }
}

/// One tool call, answered in MCP's tool-result shape.
fn call_tool(params: &Value, settings: &Value) -> Result<Value, (i64, String)> {
    let args = &params["arguments"];
    let text = |body: String| json!({"content": [{"type": "text", "text": body}]});
    match params["name"].as_str().unwrap_or_default() {
        "word_count" => match args["text"].as_str() {
            Some(body) => Ok(text(count(body))),
            None => Ok(json!({
                "content": [{"type": "text", "text": "word_count needs the text to count"}],
                "isError": true,
            })),
        },
        "greet" => {
            let greeting = settings["greeting"]
                .as_str()
                .filter(|g| !g.trim().is_empty())
                .unwrap_or("Hello");
            let name = args["name"].as_str().unwrap_or("there");
            // The toy only says whether a token is set: a secret is never
            // repeated back.
            let token = match settings["token"].as_str() {
                Some(t) if !t.is_empty() => "a token is set",
                _ => "no token is set",
            };
            Ok(text(format!("{greeting}, {name}! ({token})")))
        }
        other => Err((-32602, format!("toy has no tool named {other}"))),
    }
}

// ---- The harness driver: JSON lines, one process per chat ----

fn probe() {
    // A real driver looks for its CLI here and says whether it is signed
    // in; the toy is its own CLI, so it is always there.
    println!(
        "{}",
        json!({"available": true, "version": env!("CARGO_PKG_VERSION"), "signed_in": null, "reason": null})
    );
}

/// What one chat's driver keeps.
struct Chat {
    model: String,
    permission: String,
    /// An approval asked and not yet answered: its ref and the prompt.
    asked: Option<(String, String)>,
}

fn new_ref() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or_default();
    format!("toy-{:x}-{:x}", std::process::id(), nanos)
}

fn driver() {
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    let mut chat = Chat {
        model: "toy-echo".into(),
        permission: "ask".into(),
        asked: None,
    };
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        let Ok(command) = serde_json::from_str::<Value>(&line) else {
            eprintln!("toy: not JSON: {line}");
            continue;
        };
        match command["type"].as_str().unwrap_or_default() {
            "spawn" => {
                if let Some(model) = command["model"].as_str() {
                    chat.model = model.to_string();
                }
                if let Some(word) = command["permission_mode"].as_str() {
                    chat.permission = word.to_string();
                }
                // A real CLI resumes its own conversation from the ref it
                // gave last time; the toy keeps the ref and no history.
                let session_ref = command["resume"]
                    .as_str()
                    .map(String::from)
                    .unwrap_or_else(new_ref);
                send(
                    &mut stdout,
                    json!({
                        "type": "initialized",
                        "session_ref": session_ref,
                        "model": chat.model,
                        "permission_mode": chat.permission,
                    }),
                );
            }
            "prompt" => {
                let text = command["text"].as_str().unwrap_or_default().to_string();
                let shout = text.to_lowercase().contains("shout");
                if shout && chat.permission != "allow" {
                    // Shouting needs the user's yes: the ask becomes a
                    // decision card in the chat, answered with `answer`.
                    let harness_ref = new_ref();
                    send(
                        &mut stdout,
                        json!({
                            "type": "approval_requested",
                            "harness_ref": harness_ref,
                            "kind": "approval",
                            "tool_name": "Shout",
                            "summary": "Shout the reply?",
                            "request": {"prompt": text},
                            "options": [
                                {"id": "allow", "label": "Shout it"},
                                {"id": "deny", "label": "Keep it quiet"},
                            ],
                        }),
                    );
                    chat.asked = Some((harness_ref, text));
                } else {
                    reply(&mut stdout, &chat, &text, shout);
                }
            }
            "answer" => {
                let choice = command["choice"].as_str().unwrap_or_default();
                match chat.asked.take() {
                    Some((asked, text)) if command["harness_ref"] == asked.as_str() => {
                        reply(&mut stdout, &chat, &text, choice == "allow");
                    }
                    other => {
                        chat.asked = other;
                        eprintln!("toy: that question is no longer open");
                    }
                }
            }
            "interrupt" => {
                // The toy answers at once, so only an open question can be
                // interrupted; that is the one boundary a driver may make.
                if chat.asked.take().is_some() {
                    send(
                        &mut stdout,
                        json!({"type": "turn_completed", "subtype": "interrupted", "coarse": "interrupted", "is_error": false}),
                    );
                }
            }
            "settings" => {
                // The manifest does not say `settings: "live"`, so
                // OpenRemote restarts the driver on a new model instead.
                eprintln!("toy: settings are applied when the chat starts again");
            }
            "shutdown" => break,
            other => eprintln!("toy: unknown command '{other}'"),
        }
    }
}

/// One whole turn: a thought, a tool the toy "used", its answer, the end.
fn reply(stdout: &mut impl Write, chat: &Chat, prompt: &str, loud: bool) {
    send(
        stdout,
        json!({"type": "reasoning", "text": format!("Reading the prompt: {}.", count(prompt))}),
    );
    let tool = new_ref();
    send(
        stdout,
        json!({"type": "tool_started", "tool_use_id": tool, "name": "count_words", "input": {"text": prompt}}),
    );
    send(
        stdout,
        json!({"type": "tool_result", "tool_use_id": tool, "name": "count_words", "text": count(prompt), "is_error": false}),
    );
    let mut answer = if chat.model == "toy-mirror" {
        prompt.chars().rev().collect()
    } else {
        prompt.to_string()
    };
    if loud {
        answer = answer.to_uppercase();
    }
    send(stdout, json!({"type": "text", "text": answer}));
    send(
        stdout,
        json!({"type": "turn_completed", "subtype": "completed", "coarse": "completed", "is_error": false, "error_message": null}),
    );
}
