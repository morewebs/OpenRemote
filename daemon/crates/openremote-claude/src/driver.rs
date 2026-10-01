//! The session driver: one persistent harness process per session.
//!
//! Spawn contract ported from the SDK: argv order, `CLAUDE_CODE_ENTRYPOINT`
//! set, inherited `CLAUDECODE` stripped (so the child doesn't think it
//! lives inside a Claude Code parent), `PWD` pinned to the workspace,
//! approvals enabled through `--permission-prompt-tool stdio`. Stdout
//! lines accept both `\r\n` and `\n` — the ICRNL lesson.

use std::process::Stdio;
use std::sync::Arc;

use openremote_core::{DecisionKind, DecisionOption, TurnOutcome};
use openremote_harness::{DriverError, DriverEvent, Resolution, SpawnOptions};
use serde_json::Value;
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
            Frame::Result {
                subtype,
                terminal_reason,
                is_error,
            } => Some(DriverEvent::TurnCompleted {
                subtype: subtype.clone(),
                coarse: coarse_outcome(&subtype, terminal_reason.as_deref()),
                is_error,
                error_message: None,
            }),
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
    use serde_json::json;

    #[test]
    fn argv_orders_flags_the_sdk_way_and_resume_is_equals_form() {
        let opts = SpawnOptions {
            cwd: std::path::PathBuf::from("/w"),
            model: Some("sonnet".into()),
            permission_mode: Some("default".into()),
            resume: Some("abc; --dangerously-skip-permissions".into()),
            include_deltas: true,
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
