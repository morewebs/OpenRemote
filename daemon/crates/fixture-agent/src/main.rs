//! A scriptable fake harness CLI for daemon e2e tests.
//!
//! One binary, many wire protocols - chosen by the argv the real driver
//! builds, so e2e never smuggles fake flags:
//!
//! - Claude Code (`--output-format stream-json --input-format
//!   stream-json …`): user envelopes on stdin, NDJSON frames on stdout,
//!   `control_request`/`control_response` for approvals and interrupts,
//!   `result` as the turn boundary.
//! - Codex (`app-server --listen stdio://`): JSON-lines RPC - calls,
//!   notifications, and server→client approval requests answered with
//!   `{id, result: {decision}}`.
//!
//! The scenario is chosen with `FIXTURE_AGENT_SCENARIO` (or a
//! `fixture-scenario` file in the cwd).
//!
//! One reader owns stdin for the whole run - a turn that waits for an
//! approval parks in `pending_approval` and the main loop keeps reading;
//! nesting a second `stdin().lock()` deadlocks (Stdin's mutex is not
//! reentrant), which is exactly how the first e2e run hung.
//!
//! Lines are accepted with either `\r\n` or `\n` delimiters - the ICRNL
//! lesson, paid for once in a PTY and never again.

mod agy;
mod codex;
mod grok;
mod opencode;
mod pi;
mod sidecar;

use std::io::{BufRead, Write};

/// Append one line to a file in the working directory (the session's
/// workspace): how a fixture tells the e2e suite what the real driver
/// handed it - an argv, a start-time setting - without a wire of its own.
pub fn record(file: &str, line: &str) {
    use std::io::Write as _;
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(file)
    {
        let _ = writeln!(f, "{line}");
    }
}

/// Every scenario name the fixtures here implement, one per behaviour, all
/// protocols included. The e2e harness asks for this list (`--scenarios`) and
/// refuses to drive a scenario the binary on disk does not know, because
/// `cargo test` never rebuilds this binary: an old one turns a scenario it has
/// not heard of into a plain turn, the event the test waits for never arrives,
/// and half a minute later the report blames the daemon. A scenario added to a
/// match below or in `codex.rs`, `grok.rs`, `pi.rs`, `opencode.rs` or `agy.rs`
/// belongs here too; forgetting refuses a valid run with this list in the
/// message, which is the loud direction to be wrong in.
pub const SCENARIOS: &[&str] = &[
    "plain",
    "approve",
    "deny",
    "question",
    "steer",
    "slow",
    "hold",
    "fail",
    "stream",
    "think",
    "compact",
    "giant-line",
    "ask",
    "drop",
    "boot-crash",
    "dialog",
    "die",
    "tree",
    "mcp-tool",
];

/// Hold a scenario's turn open until the e2e creates `fixture-release` in
/// the workspace it handed the harness. A test that needs the turn still
/// running (to queue a message, to interrupt it) then decides the length
/// itself, instead of racing a fixed sleep that a loaded machine can
/// outrun. The wait is capped so a forgotten release still ends the turn.
pub fn wait_for_release() {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    while std::time::Instant::now() < deadline {
        if std::path::Path::new("fixture-release").is_file() {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
}

/// A helper process of the sort a real CLI starts and tells nobody about: a
/// plugin's MCP server, a language server. `tree` mode starts one so a test
/// can ask what stopping the chat did to it, and writes its pid to
/// `fixture-grandchild.pid` in the workspace the test is watching. The
/// helper's own output goes nowhere; its point is to outlive its parent if
/// nothing kills the tree.
///
/// The helper is started directly rather than through a shell, so the pid
/// here is the leaf of the tree: a `cmd /c ping` wrapper would put one more
/// level between the CLI and the recorded pid, and a test on that pid would
/// prove the walk reaches a middle process rather than the end of it.
pub fn spawn_grandchild() {
    use std::process::{Command, Stdio};
    #[cfg(windows)]
    let mut command = {
        // `ping` is in System32, which is on every PATH, and it runs for
        // about a minute: long enough that the test cannot mistake its own
        // exit for the kill.
        let mut command = Command::new("ping");
        command.args(["-n", "60", "127.0.0.1"]);
        command
    };
    #[cfg(not(windows))]
    let mut command = {
        let mut command = Command::new("sleep");
        command.arg("60");
        command
    };
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    match command.spawn() {
        Ok(child) => record("fixture-grandchild.pid", &child.id().to_string()),
        Err(err) => eprintln!("fixture: could not start a helper for `tree`: {err}"),
    }
}

fn main() {
    // Protocol inference from the real driver's argv. `--version` is the
    // registry's catalog probe - answered before any protocol dispatch.
    let argv: Vec<String> = std::env::args().skip(1).collect();
    if argv.iter().any(|a| a == "--version") {
        let version =
            std::env::var("FIXTURE_AGENT_VERSION").unwrap_or_else(|_| "9.9.9".to_string());
        println!("fixture-agent {version}");
        return;
    }
    // Which scenarios this binary implements, for the harness to check before
    // it drives one. Answered before protocol dispatch, like `--version`, and
    // never asked by the daemon: no real run changes.
    if argv.iter().any(|a| a == "--scenarios") {
        for name in SCENARIOS {
            println!("{name}");
        }
        return;
    }
    // Not a harness: the desktop app's side of the daemon sidecar.
    if argv.first().is_some_and(|a| a == "sidecar-host") {
        sidecar::main_sidecar_host(&argv);
        return;
    }
    if argv.iter().any(|a| a == "app-server") {
        codex::main_codex();
        return;
    }
    // grok's single-turn forms: inline `--single <text>` and the prompt
    // file the driver uses (`--prompt-file <path>`).
    if argv.iter().any(|a| a == "--single" || a == "--prompt-file") {
        grok::main_grok(&argv);
        return;
    }
    if argv.iter().any(|a| a == "--mode") {
        pi::main_pi();
        return;
    }
    if argv.iter().any(|a| a == "serve") {
        opencode::main_opencode();
        return;
    }
    // agy's print mode, whichever form names it: `--print <text>`, the
    // inline `--print=<text>`, and the stdin form the driver uses
    // (`--print=` alone, with `--input-format stream-json`).
    if argv.first().is_some_and(|a| a == "models")
        || argv.iter().any(|a| {
            a == "--print"
                || a == "-p"
                || a.starts_with("--print=")
                || a.starts_with("--prompt=")
                || a.starts_with("-p=")
        })
    {
        agy::main_agy(&argv);
        return;
    }
    let mut agent = FixtureAgent::from_env();
    agent.run();
}

/// A turn parked waiting for the host's approval answer.
struct PendingApproval {
    tool_id: String,
    tool_name: String,
    prompt: String,
}

struct FixtureAgent {
    session_id: String,
    scenario: String,
    /// The `--permission-mode` word the real driver passed - echoed in
    /// init so e2e can see a chosen mode survive the round trip.
    permission_mode: String,
    next_request_id: u64,
    next_tool_id: u64,
    prompt_count: u64,
    pending_approval: Option<PendingApproval>,
    /// The `steer` scenario's open turn: its tool's id and the message
    /// that started it, waiting for the message that lands mid-turn.
    steer_open: Option<(String, String)>,
}

/// The commands the fixture's claude lists, as the real CLI does in its
/// initialize answer and its init frame: two built-ins, each plugin's
/// skills from its `--plugin-dir` as `plugin:skill`, and - so a test can
/// make the list change between two starts - the name in a
/// `fixture-extra-command` file in the working folder.
fn fixture_commands() -> Vec<(String, String)> {
    let mut commands = vec![
        (
            "compact".to_string(),
            "Compact the conversation".to_string(),
        ),
        ("clear".to_string(), "Clear the conversation".to_string()),
    ];
    let argv: Vec<String> = std::env::args().collect();
    for pair in argv.windows(2) {
        if pair[0] != "--plugin-dir" {
            continue;
        }
        let dir = std::path::Path::new(&pair[1]);
        let name = std::fs::read_to_string(dir.join(".claude-plugin").join("plugin.json"))
            .ok()
            .and_then(|raw| serde_json::from_str::<serde_json::Value>(&raw).ok())
            .and_then(|v| v.get("name").and_then(|n| n.as_str()).map(String::from));
        let (Some(name), Ok(skills)) = (name, std::fs::read_dir(dir.join("skills"))) else {
            continue;
        };
        let mut skills: Vec<String> = skills
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        skills.sort();
        for skill in skills {
            commands.push((format!("{name}:{skill}"), format!("({name}) {skill}")));
        }
    }
    if let Ok(extra) = std::fs::read_to_string("fixture-extra-command") {
        let extra = extra.trim();
        if !extra.is_empty() {
            commands.push((extra.to_string(), "an extra command".to_string()));
        }
    }
    commands
}

impl FixtureAgent {
    fn from_env() -> Self {
        let mut session_id = std::env::var("FIXTURE_AGENT_SESSION_ID")
            .unwrap_or_else(|_| format!("fixture-session-{}", std::process::id()));
        let mut permission_mode = String::from("default");
        let argv: Vec<String> = std::env::args().skip(1).collect();
        // Only a chat's own start (stream-json): the daemon's probes run
        // this binary too (`auth status`, the models handshake), from
        // wherever the daemon was started.
        if argv.iter().any(|a| a == "--input-format")
            && argv.iter().any(|a| a == "--permission-prompt-tool")
        {
            record("fixture-claude-argv", &argv.join(" "));
        }
        let mut i = 0;
        while i < argv.len() {
            // The equals form is one argv word (`--resume=<id>`); the
            // mode flag is two (`--permission-mode <word>`).
            if let Some(id) = argv[i].strip_prefix("--resume=") {
                session_id = id.to_string();
            } else if argv[i] == "--permission-mode" {
                if let Some(mode) = argv.get(i + 1) {
                    permission_mode = mode.clone();
                    i += 1;
                }
            }
            i += 1;
        }
        // Scenario resolution: env first, then a `fixture-scenario` file in
        // the cwd - the per-session channel the e2e suite uses without
        // smuggling fake flags into the argv the daemon builds.
        let scenario = std::env::var("FIXTURE_AGENT_SCENARIO")
            .ok()
            .or_else(|| {
                std::fs::read_to_string("fixture-scenario")
                    .ok()
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
            })
            .unwrap_or_else(|| "plain".to_string());
        Self {
            session_id,
            scenario,
            permission_mode,
            next_request_id: 1,
            next_tool_id: 1,
            prompt_count: 0,
            pending_approval: None,
            steer_open: None,
        }
    }

    fn run(&mut self) {
        self.emit_init();
        let stdin = std::io::stdin();
        // ICRNL lesson: split on \n, trim a trailing \r - both delimiters are legal.
        for line in stdin.lock().lines() {
            let line = match line {
                Ok(l) => l.trim_end_matches('\r').to_string(),
                Err(_) => break,
            };
            if line.trim().is_empty() {
                continue;
            }
            let frame: serde_json::Value = match serde_json::from_str(&line) {
                Ok(v) => v,
                Err(e) => {
                    eprintln!("fixture-agent: unparsable stdin line ({e}): {line}");
                    continue;
                }
            };
            self.handle_frame(frame);
        }
    }

    fn handle_frame(&mut self, frame: serde_json::Value) {
        // A parked turn answers only its approval (or an interrupt).
        if self.pending_approval.is_some() {
            match frame.get("type").and_then(|t| t.as_str()) {
                Some("control_response") => self.resolve_approval(&frame),
                Some("control_request") => {
                    if frame["request"]["subtype"] == "interrupt" {
                        self.handle_control_request(&frame);
                        let pending = self.pending_approval.take().expect("parked turn");
                        self.result(
                            "error_during_execution",
                            "aborted while waiting",
                            "aborted_tools",
                        );
                        let _ = pending;
                    } else {
                        self.handle_control_request(&frame);
                    }
                }
                _ => eprintln!("fixture-agent: while awaiting approval, ignored: {frame}"),
            }
            return;
        }
        match frame.get("type").and_then(|t| t.as_str()) {
            Some("user") => self.handle_user_message(&frame),
            Some("control_request") => self.handle_control_request(&frame),
            Some("control_response") => eprintln!("fixture-agent: stray control_response ignored"),
            _ => eprintln!("fixture-agent: ignored frame: {frame}"),
        }
    }

    // ---- wire output helpers ----

    fn out(&self, value: serde_json::Value) {
        let mut stdout = std::io::stdout().lock();
        serde_json::to_writer(&mut stdout, &value).ok();
        stdout.write_all(b"\n").ok();
        stdout.flush().ok();
    }

    fn emit_init(&self) {
        self.out(serde_json::json!({
            "type": "system",
            "subtype": "init",
            "session_id": self.session_id,
            "model": "fixture-sonnet",
            "permissionMode": self.permission_mode,
            "tools": ["Read", "Bash", "Edit"],
            "slash_commands": fixture_commands()
                .into_iter()
                .map(|(name, _)| name)
                .collect::<Vec<_>>(),
            "cwd": "."
        }));
    }

    fn assistant_text(&self, text: &str) {
        self.out(serde_json::json!({
            "type": "assistant",
            "session_id": self.session_id,
            "uuid": format!("asst-{}", self.prompt_count),
            "parent_tool_use_id": null,
            "message": {
                "id": format!("msg_{}", self.prompt_count),
                "role": "assistant",
                "model": "fixture-sonnet",
                "content": [{"type": "text", "text": text}],
                "stop_reason": null,
                "usage": {"input_tokens": 10, "output_tokens": 5}
            }
        }));
    }

    fn assistant_tool_use(&self, tool_id: &str, name: &str, input: serde_json::Value) {
        self.out(serde_json::json!({
            "type": "assistant",
            "session_id": self.session_id,
            "uuid": format!("asst-tool-{}", self.prompt_count),
            "parent_tool_use_id": null,
            "message": {
                "id": format!("msg_tool_{}", self.prompt_count),
                "role": "assistant",
                "model": "fixture-sonnet",
                "content": [{"type": "tool_use", "id": tool_id, "name": name, "input": input}],
                "stop_reason": null,
                "usage": {"input_tokens": 10, "output_tokens": 5}
            }
        }));
    }

    fn tool_result(&self, tool_id: &str, text: &str, is_error: bool) {
        self.out(serde_json::json!({
            "type": "user",
            "session_id": self.session_id,
            "parent_tool_use_id": null,
            "tool_use_result": text,
            "message": {
                "role": "user",
                "content": [{"type": "tool_result", "tool_use_id": tool_id, "is_error": is_error,
                             "content": [{"type": "text", "text": text}]}]
            }
        }));
    }

    fn result(&self, subtype: &str, text: &str, terminal_reason: &str) {
        self.out(serde_json::json!({
            "type": "result",
            "subtype": subtype,
            "session_id": self.session_id,
            "result": text,
            "is_error": subtype != "success",
            "num_turns": self.prompt_count,
            "terminal_reason": terminal_reason,
            "duration_ms": 7,
            "total_cost_usd": 0.042,
            "usage": {"input_tokens": 10, "output_tokens": 5}
        }));
    }

    fn can_use_tool_request(
        &mut self,
        tool_id: &str,
        name: &str,
        input: serde_json::Value,
    ) -> String {
        let request_id = format!("cr-{}", self.next_request_id);
        self.next_request_id += 1;
        self.out(serde_json::json!({
            "type": "control_request",
            "request_id": request_id,
            "request": {
                "subtype": "can_use_tool",
                "tool_name": name,
                "tool_use_id": tool_id,
                "input": input
            }
        }));
        request_id
    }

    fn control_response_ok(&self, request_id: &str, response: serde_json::Value) {
        self.out(serde_json::json!({
            "type": "control_response",
            "response": {"subtype": "success", "request_id": request_id, "response": response}
        }));
    }

    // ---- frame handling ----

    fn prompt_text(frame: &serde_json::Value) -> String {
        let content = &frame["message"]["content"];
        if let Some(text) = content.as_str() {
            return text.to_string();
        }
        if let Some(blocks) = content.as_array() {
            let texts: Vec<String> = blocks
                .iter()
                .filter_map(|b| b.get("text").and_then(|t| t.as_str()).map(String::from))
                .collect();
            return texts.join("\n");
        }
        String::new()
    }

    fn handle_user_message(&mut self, frame: &serde_json::Value) {
        self.prompt_count += 1;
        // What the driver handed the turn, block by block: the words and
        // any pictures riding with them.
        record(
            "fixture-claude-input",
            &frame["message"]["content"].to_string(),
        );
        let prompt = Self::prompt_text(frame);
        match self.scenario.as_str() {
            "approve" | "deny" => self.scenario_approve(&prompt),
            "question" => self.scenario_question(&prompt),
            "steer" => self.scenario_steer(&prompt),
            // A turn that keeps working for a moment, then finishes on its
            // own: long enough for the e2e to queue or interrupt inside
            // it, short enough not to slow the suite down.
            "slow" => {
                let tool_id = format!("toolu_{}", self.next_tool_id);
                self.next_tool_id += 1;
                self.assistant_tool_use(
                    &tool_id,
                    "Bash",
                    serde_json::json!({"command": "echo fixture", "description": "a fixture command"}),
                );
                std::thread::sleep(std::time::Duration::from_millis(700));
                self.tool_result(&tool_id, "fixture", false);
                self.assistant_text(&format!("done: {prompt}"));
                self.result("success", &format!("done: {prompt}"), "completed");
            }
            // A turn the suite ends when it is ready: it waits, polling,
            // for `fixture-release` in the workspace instead of sleeping
            // for a window the test has to beat. An assertion that races
            // a turn's length (a queue filling, an interrupt landing
            // mid-turn) is then decided by the test's own order of
            // operations, not by how busy the machine is. The wait is
            // bounded so a forgotten release ends the turn instead of
            // hanging the suite.
            "hold" => {
                let tool_id = format!("toolu_{}", self.next_tool_id);
                self.next_tool_id += 1;
                self.assistant_tool_use(
                    &tool_id,
                    "Bash",
                    serde_json::json!({"command": "echo fixture", "description": "a fixture command"}),
                );
                wait_for_release();
                self.tool_result(&tool_id, "fixture", false);
                self.assistant_text(&format!("done: {prompt}"));
                self.result("success", &format!("done: {prompt}"), "completed");
            }
            "fail" => {
                self.assistant_text("the fixture is about to fail");
                self.result("error_during_execution", "fixture failure", "api_error");
            }
            "stream" => {
                for chunk in ["Hel", "lo ", "wor", "ld"] {
                    self.out(serde_json::json!({
                        "type": "stream_event",
                        "parent_tool_use_id": null,
                        "event": {"type": "content_block_delta",
                                  "delta": {"type": "text_delta", "text": chunk}}
                    }));
                }
                self.assistant_text("Hello world");
                self.result("success", "Hello world", "completed");
            }
            // The model shares its reasoning: thinking deltas stream, a
            // thinking content block lands, and the result reports the
            // turn's thinking tokens (claude's own fields).
            "think" => {
                for chunk in ["Let me compute ", "17*23 ", "step by step."] {
                    self.out(serde_json::json!({
                        "type": "stream_event",
                        "parent_tool_use_id": null,
                        "event": {"type": "content_block_delta",
                                  "delta": {"type": "thinking_delta", "thinking": chunk}}
                    }));
                }
                self.out(serde_json::json!({
                    "type": "assistant",
                    "session_id": self.session_id,
                    "uuid": format!("asst-{}", self.prompt_count),
                    "parent_tool_use_id": null,
                    "message": {
                        "id": format!("msg_{}", self.prompt_count),
                        "role": "assistant",
                        "model": "fixture-sonnet",
                        "content": [
                            {"type": "thinking", "thinking": "Let me compute 17*23 step by step."},
                            {"type": "text", "text": format!("done: {prompt}")}
                        ],
                        "stop_reason": null,
                        "usage": {"input_tokens": 10, "output_tokens": 5}
                    }
                }));
                self.out(serde_json::json!({
                    "type": "result",
                    "subtype": "success",
                    "session_id": self.session_id,
                    "result": format!("done: {prompt}"),
                    "is_error": false,
                    "num_turns": self.prompt_count,
                    "terminal_reason": "completed",
                    "duration_ms": 9,
                    "total_cost_usd": 0.042,
                    "usage": {"input_tokens": 10, "output_tokens": 5,
                              "output_tokens_details": {"thinking_tokens": 42}}
                }));
            }
            // The conversation outgrew its window: the harness compacted
            // before answering (claude's own marker).
            "compact" => {
                self.out(serde_json::json!({
                    "type": "system",
                    "subtype": "compact_boundary"
                }));
                self.assistant_text("the fixture compacted and continued");
                self.out(serde_json::json!({
                    "type": "result",
                    "subtype": "success",
                    "session_id": self.session_id,
                    "result": "done",
                    "is_error": false,
                    "num_turns": self.prompt_count,
                    "terminal_reason": "completed",
                    "duration_ms": 9,
                    "total_cost_usd": 0.0075,
                    "usage": {"input_tokens": 40, "cache_read_input_tokens": 12000,
                              "cache_creation_input_tokens": 260, "output_tokens": 60}
                }));
            }
            // A runaway prelude: an 8 MiB blob of
            // junk on one line, then a line with invalid UTF-8 mid-line,
            // then the well-formed frames and the turn's result. The
            // bounded reader drops the blob without buffering it and
            // decodes the corrupt line lossily, so the turn after them
            // completes - where the old read_line reader either bloated
            // memory on the blob or broke the session on the bad bytes.
            "giant-line" => {
                let mut blob = std::io::stdout().lock();
                blob.write_all(&vec![b'x'; 8 * 1024 * 1024]).ok();
                blob.write_all(b"\n").ok();
                blob.write_all(b"\xff\xfe not utf-8 mid-line\n").ok();
                blob.flush().ok();
                self.assistant_text(&format!("done: {prompt}"));
                self.result("success", &format!("done: {prompt}"), "completed");
            }
            // A name no arm here knows: normally an old binary beside a new
            // test, since `cargo test` does not rebuild this one. Finish the
            // turn like `plain` so nothing waits forever, and put the cause on
            // stderr, where the daemon quotes it into the session's reason -
            // the harness checks this case up front (`--scenarios`), and this
            // line is what a run that got past that check leaves behind.
            other => {
                eprintln!(
                    "fixture-agent: unknown scenario '{other}'; this binary predates it, rebuild with cargo build -p fixture-agent"
                );
                self.assistant_text(&format!("done: {prompt}"));
                self.result("success", &format!("done: {prompt}"), "completed");
            }
        }
    }

    /// Emits a tool_use and asks the host to approve it, then parks the
    /// turn; the main loop resumes reading and `resolve_approval` finishes
    /// it when the answer arrives.
    fn scenario_approve(&mut self, prompt: &str) {
        let tool_id = format!("toolu_{}", self.next_tool_id);
        self.next_tool_id += 1;
        let input =
            serde_json::json!({"command": "echo fixture", "description": "a fixture command"});
        self.assistant_tool_use(&tool_id, "Bash", input.clone());
        self.can_use_tool_request(&tool_id, "Bash", input);
        self.pending_approval = Some(PendingApproval {
            tool_id,
            tool_name: "Bash".to_string(),
            prompt: prompt.to_string(),
        });
    }

    /// What the real CLI does with a message that arrives while a turn is
    /// still working (claude 2.1.300, seen live): it folds the message
    /// into the turn that is running. The first message starts a tool and
    /// leaves the turn open; the next one lands inside it, the tool
    /// finishes, and one reply and one result close the whole turn -
    /// both messages answered at once.
    fn scenario_steer(&mut self, prompt: &str) {
        match self.steer_open.take() {
            None => {
                let tool_id = format!("toolu_{}", self.next_tool_id);
                self.next_tool_id += 1;
                let input = serde_json::json!({"command": "echo fixture", "description": "a fixture command"});
                self.assistant_tool_use(&tool_id, "Bash", input);
                self.steer_open = Some((tool_id, prompt.to_string()));
            }
            Some((tool_id, first)) => {
                self.tool_result(&tool_id, "fixture", false);
                let reply = format!("done: {first} + {prompt}");
                self.assistant_text(&reply);
                self.result("success", &reply, "completed");
            }
        }
    }

    /// The structured-question stand-in: the real CLI asks AskUserQuestion
    /// over the same can_use_tool callback. Emits the tool_use, asks the
    /// host, parks the turn; `resolve_approval` echoes what actually
    /// arrived so the e2e can prove the user's choice reached the harness.
    fn scenario_question(&mut self, prompt: &str) {
        let tool_id = format!("toolu_{}", self.next_tool_id);
        self.next_tool_id += 1;
        let input = serde_json::json!({"questions": [{
            "question": "Which database?",
            "options": [{"label": "Postgres"}, {"label": "SQLite"}]
        }]});
        self.assistant_tool_use(&tool_id, "AskUserQuestion", input.clone());
        self.can_use_tool_request(&tool_id, "AskUserQuestion", input);
        self.pending_approval = Some(PendingApproval {
            tool_id,
            tool_name: "AskUserQuestion".to_string(),
            prompt: prompt.to_string(),
        });
    }

    fn resolve_approval(&mut self, frame: &serde_json::Value) {
        let Some(pending) = self.pending_approval.take() else {
            return;
        };
        let response = &frame["response"]["response"];
        let behavior = response["behavior"].as_str().unwrap_or("deny");
        let allowed = behavior == "allow";
        // A structured question is answered on the SDK's own wire:
        // updatedInput.answers keyed by question text. Echo exactly what
        // arrived so the e2e can tell a delivered answer from a denial
        // (the bug this pins: every choice collapsed into a deny).
        if pending.tool_name == "AskUserQuestion" {
            let answers = &response["updatedInput"]["answers"];
            let text = if allowed && !answers.is_null() {
                format!("question answered: {answers}")
            } else {
                format!("question denied: behavior={behavior}")
            };
            self.tool_result(&pending.tool_id, &text, !allowed);
            self.assistant_text(&text);
            self.result("success", &text, "completed");
            return;
        }
        self.tool_result(
            &pending.tool_id,
            if allowed {
                "fixture output"
            } else {
                "permission denied"
            },
            !allowed,
        );
        let closing = if allowed {
            format!("ran the command for: {}", pending.prompt)
        } else {
            format!("permission denied for: {}", pending.prompt)
        };
        self.assistant_text(&closing);
        self.result("success", &closing, "completed");
    }

    fn handle_control_request(&mut self, frame: &serde_json::Value) {
        let request_id = frame["request_id"].as_str().unwrap_or("").to_string();
        match frame["request"]["subtype"].as_str() {
            Some("interrupt") => {
                self.control_response_ok(
                    &request_id,
                    serde_json::json!({"still_queued": [], "cancelled": []}),
                );
            }
            // The initialize handshake's answer: the same `models` array
            // the real CLI serves (pick words, resolved ids, display
            // names, and the description whose opening versions the row),
            // so the console's catalog path runs against real shapes.
            Some("initialize") => {
                let commands: Vec<serde_json::Value> = fixture_commands()
                    .into_iter()
                    .map(|(name, description)| {
                        serde_json::json!({"name": name, "description": description, "argumentHint": ""})
                    })
                    .collect();
                self.control_response_ok(
                    &request_id,
                    serde_json::json!({
                    "commands": commands,
                    "agents": [
                        {"name": "Explore", "description": "Reads widely, reports back"},
                        {"name": "Plan", "description": "Plans before editing"}
                    ],
                    "models": [
                        {"value": "default",
                         "resolvedModel": "fixture-sonnet",
                         "displayName": "Default (recommended)",
                         "description": "Sonnet 9.9 · the catalog the e2e suite serves",
                         "supportedEffortLevels": ["low", "high"]},
                        {"value": "sonnet",
                         "resolvedModel": "fixture-sonnet",
                         "displayName": "Sonnet",
                         "description": "Sonnet 9.9 · the catalog the e2e suite serves",
                         "supportedEffortLevels": ["low", "high"]},
                        {"value": "fixture-fable-9-9[1m]",
                         "resolvedModel": "fixture-fable-9-9[1m]",
                         "displayName": "Fable",
                         "description": "Fable 9.9 · the long-running tier",
                         "supportedEffortLevels": ["low", "high"]}
                    ]}),
                );
            }
            Some(other) => {
                eprintln!("fixture-agent: control request {other} acknowledged, no-op");
                self.control_response_ok(&request_id, serde_json::json!({}));
            }
            None => eprintln!("fixture-agent: control request without subtype"),
        }
    }
}
