//! The session driver: one persistent harness process per session.
//!
//! Spawn contract ported from the SDK: argv order, `CLAUDE_CODE_ENTRYPOINT`
//! set, inherited `CLAUDECODE` stripped (so the child doesn't think it
//! lives inside a Claude Code parent), `PWD` pinned to the workspace,
//! approvals enabled through `--permission-prompt-tool stdio`. Stdout
//! lines accept both `\r\n` and `\n` — the ICRNL lesson.

use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use openremote_core::{DecisionKind, DecisionOption, TurnOutcome};
use openremote_harness::{DriverError, DriverEvent, Resolution, SessionSettings, SpawnOptions};
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::{Mutex, mpsc};

use crate::frames::{self, Frame};
use openremote_harness::events::{ApprovalRequest, DecisionSpec};

pub struct Driver {
    stdin: tokio::process::ChildStdin,
    child: Arc<Mutex<Child>>,
    next_request_id: u64,
}

impl Driver {
    /// Spawn the harness process and start its reader loops. Returns the
    /// driver (for steering) and the event stream (for observing).
    pub fn spawn(
        resolution: &Resolution,
        opts: SpawnOptions,
    ) -> Result<(Self, mpsc::Receiver<DriverEvent>), DriverError> {
        let (program, mut argv): (std::path::PathBuf, Vec<std::ffi::OsString>) = match resolution {
            Resolution::Executable(path) => (path.clone(), Vec::new()),
            Resolution::NodeScript { node, script } => {
                (node.clone(), vec![script.as_os_str().to_os_string()])
            }
            Resolution::Unavailable => {
                return Err(DriverError::Spawn("claude CLI not found".to_string()));
            }
        };
        argv.extend(build_argv(&opts));

        let mut command = Command::new(&program);
        command
            .args(&argv)
            .current_dir(&opts.cwd)
            .env("CLAUDE_CODE_ENTRYPOINT", "openremote-rs")
            .env_remove("CLAUDECODE")
            .env("PWD", &opts.cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        // env::vars is not used on purpose; the child inherits the rest.
        let mut child = command
            .spawn()
            .map_err(|e| DriverError::Spawn(format!("{program:?}: {e}")))?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| DriverError::Spawn("stdin not piped".into()))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| DriverError::Spawn("stdout not piped".into()))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| DriverError::Spawn("stderr not piped".into()))?;

        let (tx, rx) = mpsc::channel(256);
        let child = Arc::new(Mutex::new(child));
        tokio::spawn(read_stdout(BufReader::new(stdout), tx.clone()));
        tokio::spawn(read_stderr(BufReader::new(stderr), tx));
        Ok((
            Self {
                stdin,
                child,
                next_request_id: 0,
            },
            rx,
        ))
    }

    /// Queue one user message; the CLI runs it as a turn.
    pub async fn send_prompt(&mut self, text: &str) -> Result<(), DriverError> {
        self.write_line(frames::user_envelope(text)).await
    }

    /// Claude's stream-json control protocol has no model, effort, or fast
    /// field after spawn. Those ride argv and `--settings` at process start
    /// (`--resume` keeps them). But the CLI's own slash commands speak the
    /// same user-message wire (`/model <id>`, `/effort <level>`, `/fast`)
    /// and answer with a synthetic confirmation — the daemon sends those
    /// as one-off turns; the fresh init frame that follows carries the new
    /// model, which the pump folds into the session.
    pub async fn apply_settings(&mut self, settings: &SessionSettings) -> Result<(), DriverError> {
        // One slash command per user envelope — the wire takes a single
        // command per turn, so each field rides its own prompt.
        if let Some(model) = &settings.model {
            self.send_prompt(&format!("/model {model}")).await?;
        }
        if let Some(effort) = &settings.effort {
            self.send_prompt(&format!("/effort {effort}")).await?;
        }
        if let Some(fast) = &settings.fast {
            // `/fast` toggles; an explicit off has no word — skip and let
            // the console's fast control stay start-time only.
            if *fast {
                self.send_prompt("/fast").await?;
            }
        }
        Ok(())
    }

    /// Ask the CLI to cancel the running turn.
    pub async fn interrupt(&mut self) -> Result<(), DriverError> {
        let request_id = format!("or-{}", self.next_request_id);
        self.next_request_id += 1;
        self.write_line(frames::interrupt_envelope(&request_id))
            .await
    }

    /// Answer a `can_use_tool` request with the chosen option id. On allow,
    /// `updated_input` defaults to the original input (CLIs before 2.1.207
    /// reject an allow without it); on deny the message goes back to the
    /// model.
    pub async fn answer(
        &mut self,
        harness_ref: &str,
        choice: &str,
        request: &Value,
    ) -> Result<(), DriverError> {
        let allow = choice == "allow";
        let original_input = request.get("input").cloned().unwrap_or(Value::Null);
        self.write_line(frames::can_use_tool_response(
            harness_ref,
            allow,
            &original_input,
            "Denied from OpenRemote",
        ))
        .await
    }

    /// SIGTERM, wait up to 5s, SIGKILL. Returns the exit code if reaped.
    pub async fn shutdown(&mut self) -> Result<Option<i32>, DriverError> {
        let child = self.child.lock().await;
        kill_and_reap(child).await
    }

    async fn write_line(&mut self, line: String) -> Result<(), DriverError> {
        self.stdin.write_all(line.as_bytes()).await?;
        self.stdin.write_all(b"\n").await?;
        self.stdin.flush().await?;
        Ok(())
    }
}

/// Coarse turn outcome from the claude result frame.
fn coarse_outcome(subtype: &str, terminal_reason: Option<&str>) -> TurnOutcome {
    if subtype == "success" {
        TurnOutcome::Completed
    } else if terminal_reason.is_some_and(|r| r.starts_with("aborted")) {
        TurnOutcome::Interrupted
    } else {
        TurnOutcome::Failed
    }
}

/// The SDK's own context math: what the model read this turn — its input
/// tokens plus both cache fields. `None` when the frame carried no usage.
fn context_used(usage: Option<&Value>) -> Option<u64> {
    let usage = usage?;
    let field = |name: &str| usage.get(name).and_then(|v| v.as_u64()).unwrap_or(0);
    let used = field("input_tokens")
        + field("cache_read_input_tokens")
        + field("cache_creation_input_tokens");
    (used > 0).then_some(used)
}

/// Build the decision spec from a `can_use_tool` request: approvals and
/// questions are one concept; the options are the harness's own words.
fn decision_spec(request: &Value) -> DecisionSpec {
    let tool_name = request
        .get("tool_name")
        .and_then(|t| t.as_str())
        .unwrap_or("tool");
    if tool_name == "AskUserQuestion" {
        // Structured questions ride the same callback: each question's own
        // options become the choices (S6, agent-sdk/user-input).
        let mut options = Vec::new();
        if let Some(questions) = request
            .get("input")
            .and_then(|i| i.get("questions"))
            .and_then(|q| q.as_array())
        {
            if let Some(first) = questions.first() {
                if let Some(question) = first.get("question").and_then(|q| q.as_str()) {
                    if let Some(opts) = first.get("options").and_then(|o| o.as_array()) {
                        for opt in opts {
                            if let Some(label) = opt.get("label").and_then(|l| l.as_str()) {
                                options.push(DecisionOption {
                                    id: label.to_string(),
                                    label: label.to_string(),
                                });
                            }
                        }
                        return DecisionSpec {
                            kind: DecisionKind::Question,
                            options,
                            tool_name: Some(tool_name.to_string()),
                            summary: Some(question.to_string()),
                            interrupts_turn: Vec::new(),
                        };
                    }
                }
            }
        }
        DecisionSpec {
            kind: DecisionKind::Question,
            options: Vec::new(),
            tool_name: Some(tool_name.to_string()),
            summary: None,
            interrupts_turn: Vec::new(),
        }
    } else {
        let summary = request
            .get("input")
            .and_then(|i| i.get("command"))
            .and_then(|c| c.as_str())
            .map(String::from);
        DecisionSpec {
            kind: DecisionKind::Approval,
            options: vec![
                DecisionOption {
                    id: "allow".into(),
                    label: "Allow".into(),
                },
                DecisionOption {
                    id: "deny".into(),
                    label: "Deny".into(),
                },
            ],
            tool_name: Some(tool_name.to_string()),
            summary,
            interrupts_turn: Vec::new(),
        }
    }
}

async fn kill_and_reap(
    mut child: tokio::sync::MutexGuard<'_, Child>,
) -> Result<Option<i32>, DriverError> {
    child.start_kill()?;
    match tokio::time::timeout(std::time::Duration::from_secs(5), child.wait()).await {
        Ok(Ok(status)) => Ok(status.code()),
        Ok(Err(e)) => Err(e.into()),
        Err(_) => {
            child.start_kill()?;
            let status = child.wait().await?;
            Ok(status.code())
        }
    }
}

/// The model catalog for `/harnesses/claude/models`: a short-lived CLI
/// process, the SDK's `initialize` handshake in, its `models` array out —
/// the same list the CLI's own `/model` picker serves (pick words, display
/// names, effort levels). No prompt is ever sent; the process is killed
/// once the catalog lands.
pub async fn models(
    resolution: &Resolution,
) -> Result<Vec<openremote_harness::ModelDescriptor>, DriverError> {
    let (program, argv): (std::path::PathBuf, Vec<std::ffi::OsString>) = match resolution {
        Resolution::Executable(path) => (path.clone(), Vec::new()),
        Resolution::NodeScript { node, script } => {
            (node.clone(), vec![script.as_os_str().to_os_string()])
        }
        Resolution::Unavailable => {
            return Err(DriverError::Spawn("claude CLI not found".to_string()));
        }
    };
    let mut command = Command::new(&program);
    command
        .args(&argv)
        .args([
            "--output-format",
            "stream-json",
            "--verbose",
            "--input-format",
            "stream-json",
            "--permission-prompt-tool",
            "stdio",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let mut child = command
        .spawn()
        .map_err(|e| DriverError::Spawn(format!("claude models: {e}")))?;
    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| DriverError::Spawn("stdin not piped".into()))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| DriverError::Spawn("stdout not piped".into()))?;

    // Write the handshake, then read lines until our response id comes back.
    let request_id = "or-models";
    let envelope = frames::initialize_envelope(request_id);
    let catalog = async {
        use tokio::io::AsyncWriteExt;
        stdin.write_all(envelope.as_bytes()).await?;
        stdin.write_all(b"\n").await?;
        stdin.flush().await?;
        drop(stdin);

        let mut models = Vec::new();
        let mut lines = BufReader::new(stdout).lines();
        while let Some(line) = lines.next_line().await? {
            let Some(frame) = frames::parse_frame(&line) else {
                continue;
            };
            let Frame::ControlResponse { response } = frame else {
                continue; // init banner, hook frames, keep_alive — not ours
            };
            if response.get("request_id").and_then(|r| r.as_str()) != Some(request_id) {
                continue;
            }
            let Some(list) = response.get("models").and_then(Value::as_array) else {
                break; // a shape we don't speak; empty beats a guess
            };
            for entry in list {
                let Some(value) = entry.get("value").and_then(Value::as_str) else {
                    continue;
                };
                let efforts = entry
                    .get("supportedEffortLevels")
                    .and_then(Value::as_array)
                    .map(|levels| {
                        levels
                            .iter()
                            .filter_map(|l| l.as_str())
                            .map(String::from)
                            .collect()
                    })
                    .unwrap_or_default();
                models.push(openremote_harness::ModelDescriptor {
                    model: value.to_string(),
                    display_name: entry
                        .get("displayName")
                        .and_then(Value::as_str)
                        .map(String::from),
                    reasoning_efforts: efforts,
                    is_default: false,
                });
            }
            break;
        }
        Ok::<_, std::io::Error>(models)
    };
    let (models, read_err) = match tokio::time::timeout(Duration::from_secs(30), catalog).await {
        Ok(Ok(models)) => (models, None),
        Ok(Err(e)) => (Vec::new(), Some(e)),
        Err(_) => (Vec::new(), None), // a CLI that never answers is no catalog, not a failure
    };
    let _ = child.start_kill();
    if let Some(e) = read_err {
        return Err(DriverError::Spawn(format!("claude models: {e}")));
    }
    Ok(models)
}

/// The argv after the executable — the SDK's documented order, plus
/// `--permission-prompt-tool stdio` so approvals reach us.
fn build_argv(opts: &SpawnOptions) -> Vec<std::ffi::OsString> {
    let mut argv: Vec<std::ffi::OsString> = vec![
        "--output-format".into(),
        "stream-json".into(),
        "--verbose".into(),
        "--input-format".into(),
        "stream-json".into(),
        "--permission-prompt-tool".into(),
        "stdio".into(),
    ];
    if let Some(model) = &opts.model {
        argv.push("--model".into());
        argv.push(model.into());
    }
    if let Some(mode) = &opts.permission_mode {
        argv.push("--permission-mode".into());
        argv.push(mode.into());
    }
    if opts.fast {
        // Claude's own headless fast-mode form: `--settings '{"fastMode":
        // true}'` (v2.1.205+). Not a model switch — the CLI keeps Opus and
        // only swaps the speed configuration.
        argv.push("--settings".into());
        argv.push(r#"{"fastMode":true}"#.into());
    }
    if !opts.mcp_servers.is_empty() {
        // Claude's own wire: `--mcp-config` takes a JSON string, wrapper
        // key camelCase `mcpServers`, stdio entries (CLI reference +
        // MCP docs). Passed verbatim in argv — no shell, no quoting.
        let mut servers = serde_json::Map::new();
        for server in &opts.mcp_servers {
            servers.insert(
                server.id.clone(),
                json!({"type": "stdio", "command": server.command, "args": server.args}),
            );
        }
        let config = json!({ "mcpServers": servers });
        argv.push("--mcp-config".into());
        argv.push(
            serde_json::to_string(&config)
                .expect("mcp config serializes")
                .into(),
        );
    }
    if let Some(id) = &opts.resume {
        argv.push(format!("--resume={id}").into());
    }
    if opts.include_deltas {
        argv.push("--include-partial-messages".into());
    }
    argv
}

async fn read_stdout(
    mut reader: BufReader<tokio::process::ChildStdout>,
    tx: mpsc::Sender<DriverEvent>,
) {
    // tool_use_id → name, learned from assistant tool_use blocks so later
    // tool_result frames can be named.
    let mut names: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    let mut buffer = String::new();
    loop {
        buffer.clear();
        // ICRNL lesson: accept \n and \r\n; a bare \r from a raw-mode child
        // would not flush a line anyway, and the fixture accepts both.
        match reader.read_line(&mut buffer).await {
            Ok(0) => break,
            Ok(_) => {}
            Err(e) => {
                let _ = tx
                    .send(DriverEvent::Stderr {
                        line: format!("stdout read error: {e}"),
                    })
                    .await;
                break;
            }
        }
        let line = buffer.trim_end_matches(['\r', '\n']);
        if line.trim().is_empty() {
            continue;
        }
        let Some(frame) = frames::parse_frame(line) else {
            continue;
        };
        let event = match frame {
            Frame::Init {
                session_id,
                model,
                permission_mode,
            } => Some(DriverEvent::Initialized {
                session_ref: session_id,
                model,
                permission_mode,
            }),
            Frame::Assistant { content, .. } => {
                for text in frames::block_texts(&content) {
                    if tx.send(DriverEvent::AssistantText { text }).await.is_err() {
                        return;
                    }
                }
                // The model's own thinking blocks, when it shares them —
                // dim and collapsible in the console, never mixed into the
                // reply text.
                for text in frames::block_thinking(&content) {
                    if tx.send(DriverEvent::ReasoningText { text }).await.is_err() {
                        return;
                    }
                }
                for (tool_use_id, name, input) in frames::tool_use_blocks(&content) {
                    names.insert(tool_use_id.clone(), name.clone());
                    if tx
                        .send(DriverEvent::ToolStarted {
                            tool_use_id,
                            name,
                            input,
                        })
                        .await
                        .is_err()
                    {
                        return;
                    }
                }
                continue;
            }
            Frame::UserToolResults { content } => {
                for (tool_use_id, text, is_error) in frames::tool_result_blocks(&content) {
                    let name = names.get(&tool_use_id).cloned().unwrap_or_default();
                    if tx
                        .send(DriverEvent::ToolResult {
                            tool_use_id,
                            name,
                            text,
                            is_error,
                        })
                        .await
                        .is_err()
                    {
                        return;
                    }
                }
                continue;
            }
            Frame::TextDelta { text } => Some(DriverEvent::TextDelta { text }),
            Frame::ThinkingDelta { text } => Some(DriverEvent::ReasoningDelta { text }),
            Frame::CompactBoundary => Some(DriverEvent::Compacted),
            Frame::Result {
                subtype,
                terminal_reason,
                is_error,
                usage,
                cost_usd,
            } => {
                // The turn's own usage is the conversation's context: the
                // SDK's input fields together (its own semantics — the
                // context the model saw). No window rides this wire.
                if let Some(used) = context_used(usage.as_ref()) {
                    if tx
                        .send(DriverEvent::ContextUsed { used, window: None })
                        .await
                        .is_err()
                    {
                        return;
                    }
                }
                // The turn's cost in USD, verbatim from the result frame.
                if let Some(cost) = cost_usd {
                    if tx.send(DriverEvent::TurnCost { cost_usd: cost }).await.is_err() {
                        return;
                    }
                }
                // The turn's thinking tokens, verbatim — the SDK's own
                // `usage.output_tokens_details.thinking_tokens`.
                if let Some(thinking) = usage
                    .as_ref()
                    .and_then(|u| u.pointer("/output_tokens_details/thinking_tokens"))
                    .and_then(|t| t.as_u64())
                {
                    if tx
                        .send(DriverEvent::ThinkingTokens { tokens: thinking })
                        .await
                        .is_err()
                    {
                        return;
                    }
                }
                Some(DriverEvent::TurnCompleted {
                    subtype: subtype.clone(),
                    coarse: coarse_outcome(&subtype, terminal_reason.as_deref()),
                    is_error,
                    error_message: None,
                })
            }
            Frame::ControlRequest {
                request_id,
                request,
            } => {
                let subtype = request
                    .get("subtype")
                    .and_then(|s| s.as_str())
                    .unwrap_or("");
                if subtype == "can_use_tool" {
                    Some(DriverEvent::ApprovalRequested {
                        approval: ApprovalRequest {
                            harness_ref: request_id,
                            spec: decision_spec(&request),
                            request,
                        },
                    })
                } else {
                    // Other CLI→host requests (hook callbacks, dialogs) can't
                    // arise under our argv; nothing to answer.
                    None
                }
            }
            Frame::ControlResponse { .. } => None, // responses to our interrupts; the result frame is the truth
            Frame::Other { .. } => None,           // keep_alive, hidden frames, future types
        };
        if let Some(event) = event {
            if tx.send(event).await.is_err() {
                return;
            }
        }
    }
    let _ = tx.send(DriverEvent::StdoutClosed).await;
}

async fn read_stderr(
    mut reader: BufReader<tokio::process::ChildStderr>,
    tx: mpsc::Sender<DriverEvent>,
) {
    let mut buffer = String::new();
    loop {
        buffer.clear();
        match reader.read_line(&mut buffer).await {
            Ok(0) => break,
            Ok(_) => {}
            Err(_) => break,
        }
        let line = buffer.trim_end_matches(['\r', '\n']).to_string();
        if line.is_empty() {
            continue;
        }
        if tx.send(DriverEvent::Stderr { line }).await.is_err() {
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use openremote_harness::McpServer;
    use serde_json::json;

    #[test]
    fn argv_orders_flags_the_sdk_way_and_resume_is_equals_form() {
        let opts = SpawnOptions {
            cwd: std::path::PathBuf::from("/w"),
            model: Some("sonnet".into()),
            permission_mode: Some("default".into()),
            resume: Some("abc; --dangerously-skip-permissions".into()),
            include_deltas: true,
            fast: false,
            mcp_servers: Vec::new(),
        };
        let argv = build_argv(&opts);
        let joined = argv
            .iter()
            .map(|s| s.to_string_lossy())
            .collect::<Vec<_>>()
            .join(" ");
        assert!(
            joined.starts_with("--output-format stream-json --verbose --input-format stream-json")
        );
        assert!(joined.contains("--permission-prompt-tool stdio"));
        assert!(joined.contains("--model sonnet"));
        assert!(joined.contains("--permission-mode default"));
        // equals form: the id cannot smuggle a second flag
        assert!(argv.iter().any(|s| *s == std::ffi::OsStr::new("--resume=abc; --dangerously-skip-permissions")));
        assert!(joined.contains("--include-partial-messages"));
    }

    #[test]
    fn fast_mode_rides_claudes_own_settings_flag() {
        let mut opts = SpawnOptions {
            cwd: std::path::PathBuf::from("/w"),
            ..Default::default()
        };
        let argv = build_argv(&opts);
        assert!(
            !argv
                .iter()
                .any(|s| *s == std::ffi::OsStr::new("--settings"))
        );

        opts.fast = true;
        let argv = build_argv(&opts);
        let settings = argv
            .iter()
            .position(|s| *s == std::ffi::OsStr::new("--settings"))
            .expect("fast mode passes --settings");
        assert_eq!(argv[settings + 1].to_string_lossy(), r#"{"fastMode":true}"#);
    }

    #[test]
    fn plugins_ride_claudes_own_mcp_config_flag() {
        let opts = SpawnOptions {
            cwd: std::path::PathBuf::from("/w"),
            mcp_servers: vec![McpServer {
                id: "github".into(),
                command: "npx".into(),
                args: vec!["-y".into(), "@modelcontextprotocol/server-github".into()],
            }],
            ..Default::default()
        };
        let argv = build_argv(&opts);
        let flag = argv
            .iter()
            .position(|s| *s == std::ffi::OsStr::new("--mcp-config"))
            .expect("plugins pass --mcp-config");
        let config: Value =
            serde_json::from_str(&argv[flag + 1].to_string_lossy()).expect("config is JSON");
        // Claude's own shape: camelCase wrapper, stdio entry.
        assert_eq!(
            config["mcpServers"]["github"],
            json!({"type": "stdio", "command": "npx", "args": ["-y", "@modelcontextprotocol/server-github"]})
        );

        // No plugins, no flag.
        let plain = build_argv(&SpawnOptions::default());
        assert!(
            !plain
                .iter()
                .any(|s| *s == std::ffi::OsStr::new("--mcp-config"))
        );
    }

    #[test]
    fn approval_specs_use_claudes_own_words() {
        let spec = decision_spec(&json!({
            "subtype": "can_use_tool",
            "tool_name": "Bash",
            "input": {"command": "cargo test", "description": "run tests"}
        }));
        assert_eq!(spec.kind, DecisionKind::Approval);
        assert_eq!(
            spec.options
                .iter()
                .map(|o| o.id.as_str())
                .collect::<Vec<_>>(),
            ["allow", "deny"]
        );
        assert_eq!(spec.summary.as_deref(), Some("cargo test"));

        let spec = decision_spec(&json!({
            "subtype": "can_use_tool",
            "tool_name": "AskUserQuestion",
            "input": {"questions": [{"question": "Which DB?", "options": [
                {"label": "Postgres"}, {"label": "SQLite"}
            ]}]}
        }));
        assert_eq!(spec.kind, DecisionKind::Question);
        assert_eq!(
            spec.options
                .iter()
                .map(|o| o.id.as_str())
                .collect::<Vec<_>>(),
            ["Postgres", "SQLite"]
        );
        assert_eq!(spec.summary.as_deref(), Some("Which DB?"));
    }
}
