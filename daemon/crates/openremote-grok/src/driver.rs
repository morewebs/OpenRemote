//! The Grok session driver: one process per prompt, one conversation
//! across them. The first prompt's `system/init` names the conversation
//! (grok's own session id); later prompts respawn with `--resume <id>`.
//!
//! EOF without a `result` frame means the process died mid-turn: an
//! interrupt maps to `interrupted`, a stop to `StdoutClosed`, anything
//! else to a failed turn with the reason - never silence.

use std::path::PathBuf;
use std::process::Stdio;
use std::sync::{Arc, Mutex as StdMutex};
use std::time::Duration;

use openremote_core::TurnOutcome;
use openremote_harness::anthropic_wire as wire;
use openremote_harness::{DriverError, DriverEvent, Resolution, SpawnOptions};
use serde_json::Value;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::{Mutex as AsyncMutex, mpsc};

/// Why a prompt process ended without a result frame.
#[derive(Clone, Copy, PartialEq, Eq)]
enum EofReason {
    /// A result frame already ended the turn.
    Completed,
    /// interrupt() killed the process.
    Interrupted,
    /// shutdown() killed the process - the session ends.
    Stopped,
}

struct Shared {
    child: Arc<AsyncMutex<Option<Child>>>,
    reason: StdMutex<EofReason>,
    /// Grok's own conversation id, learned from the first prompt's init
    /// and updated whenever a respawned process reports one - later
    /// prompts resume it.
    session_ref: StdMutex<Option<String>>,
}

pub struct Driver {
    shared: Arc<Shared>,
    session_tx: mpsc::Sender<DriverEvent>,
    resolution: Resolution,
    opts: SpawnOptions,
}

impl Driver {
    /// A grok session starts as pure configuration - no process until
    /// the first prompt. `Initialized` rides the first prompt's init
    /// frame, so a fresh session honestly shows `starting` until then.
    pub fn spawn(
        resolution: &Resolution,
        opts: SpawnOptions,
    ) -> Result<(Self, mpsc::Receiver<DriverEvent>), DriverError> {
        if !resolution.is_available() {
            return Err(DriverError::Spawn("grok CLI not found".to_string()));
        }
        let (tx, rx) = mpsc::channel(256);
        Ok((
            Self {
                shared: Arc::new(Shared {
                    child: Arc::new(AsyncMutex::new(None)),
                    reason: StdMutex::new(EofReason::Completed),
                    session_ref: StdMutex::new(None),
                }),
                session_tx: tx,
                resolution: resolution.clone(),
                opts,
            },
            rx,
        ))
    }

    /// Run one prompt as its own grok process; the reader maps the
    /// Anthropic-wire frames onto driver events.
    pub async fn send_prompt(&mut self, text: &str) -> Result<(), DriverError> {
        let (program, prefix): (PathBuf, Vec<std::ffi::OsString>) = match &self.resolution {
            Resolution::Executable(path) => (path.clone(), Vec::new()),
            Resolution::NodeScript { node, script } => {
                (node.clone(), vec![script.as_os_str().to_os_string()])
            }
            Resolution::Unavailable => {
                return Err(DriverError::Spawn("grok CLI not found".to_string()));
            }
        };

        let cwd = self.opts.cwd.clone();
        let mut command = Command::new(program);
        command.args(&prefix);
        command
            .arg("--single")
            .arg(text)
            .arg("--output-format")
            .arg("streaming-messages-json")
            .arg("--cwd")
            .arg(&cwd)
            .current_dir(&cwd)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if let Some(model) = &self.opts.model {
            command.arg("--model").arg(model);
        }
        if let Some(mode) = &self.opts.permission_mode {
            command.arg("--permission-mode").arg(mode);
        }
        let resume = self.shared.session_ref.lock().unwrap().clone();
        if let Some(resume) = resume {
            // grok's own UUID session id - value form, no flag surface.
            command.arg("--resume").arg(resume);
        }

        *self.shared.reason.lock().unwrap() = EofReason::Completed;
        let mut child = command
            .spawn()
            .map_err(|e| DriverError::Spawn(format!("grok: {e}")))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| DriverError::Spawn("stdout not piped".into()))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| DriverError::Spawn("stderr not piped".into()))?;
        {
            let mut guard = self.shared.child.lock().await;
            *guard = Some(child);
        }

        tokio::spawn(read_prompt(
            BufReader::new(stdout),
            self.session_tx.clone(),
            Arc::clone(&self.shared),
        ));
        tokio::spawn(drain_stderr(
            BufReader::new(stderr),
            self.session_tx.clone(),
        ));
        Ok(())
    }

    pub async fn interrupt(&mut self) -> Result<(), DriverError> {
        *self.shared.reason.lock().unwrap() = EofReason::Interrupted;
        let mut guard = self.shared.child.lock().await;
        if let Some(child) = guard.as_mut() {
            child.start_kill()?;
        }
        Ok(())
    }

    /// Stop the session: kill any running prompt. The stopped reason
    /// rides the reader's EOF as `StdoutClosed`.
    pub async fn shutdown(&mut self) -> Result<Option<i32>, DriverError> {
        *self.shared.reason.lock().unwrap() = EofReason::Stopped;
        let mut guard = self.shared.child.lock().await;
        if let Some(mut child) = guard.take() {
            child.start_kill()?;
            match tokio::time::timeout(Duration::from_secs(5), child.wait()).await {
                Ok(Ok(status)) => Ok(status.code()),
                _ => Ok(None),
            }
        } else {
            Ok(None)
        }
    }

    /// Grok print mode has no remote approval channel (its own
    /// `--permission-mode` words govern) - this is never called because
    /// no `ApprovalRequested` ever fires.
    pub async fn answer(
        &mut self,
        _harness_ref: &str,
        _choice: &str,
        _request: &Value,
    ) -> Result<(), DriverError> {
        Err(DriverError::Protocol(
            "grok print mode has no remote approval channel".into(),
        ))
    }
}

async fn read_prompt(
    mut reader: BufReader<tokio::process::ChildStdout>,
    tx: mpsc::Sender<DriverEvent>,
    shared: Arc<Shared>,
) {
    let mut buffer = String::new();
    let mut saw_result = false;
    loop {
        buffer.clear();
        // ICRNL lesson: both delimiters are legal.
        match reader.read_line(&mut buffer).await {
            Ok(0) => break,
            Ok(_) => {}
            Err(_) => break,
        }
        let line = buffer.trim_end_matches(['\r', '\n']);
        if line.trim().is_empty() {
            continue;
        }
        let Some(frame) = wire::parse_frame(line) else {
            continue;
        };
        match frame {
            wire::Frame::Init {
                session_id,
                model,
                permission_mode,
            } => {
                // The first init names the conversation - that fact rides
                // to the supervisor. Every init refreshes the resume ref
                // (a resumed process may mint its own id); repeats do not
                // re-emit.
                let first = shared.session_ref.lock().unwrap().is_none();
                *shared.session_ref.lock().unwrap() = Some(session_id.clone());
                if first {
                    let _ = tx
                        .send(DriverEvent::Initialized {
                            session_ref: session_id,
                            model,
                            permission_mode,
                        })
                        .await;
                }
            }
            wire::Frame::Assistant { content, .. } => {
                for text in wire::block_texts(&content) {
                    if tx.send(DriverEvent::AssistantText { text }).await.is_err() {
                        return;
                    }
                }
                // Grok shares claude's wire shape - thinking blocks ride
                // the same content array when the model shares them.
                for text in wire::block_thinking(&content) {
                    if tx.send(DriverEvent::ReasoningText { text }).await.is_err() {
                        return;
                    }
                }
            }
            wire::Frame::UserToolResults { content } => {
                for (_id, text, is_error) in wire::tool_result_blocks(&content) {
                    let _ = tx
                        .send(DriverEvent::ToolResult {
                            tool_use_id: String::new(),
                            name: "commandExecution".into(),
                            text,
                            is_error,
                        })
                        .await;
                }
            }
            wire::Frame::TextDelta { text } => {
                let _ = tx.send(DriverEvent::TextDelta { text }).await;
            }
            wire::Frame::ThinkingDelta { text } => {
                let _ = tx.send(DriverEvent::ReasoningDelta { text }).await;
            }
            wire::Frame::Result {
                subtype,
                terminal_reason,
                is_error,
                usage: _,
                cost_usd,
            } => {
                saw_result = true;
                let coarse = if subtype == "success" {
                    TurnOutcome::Completed
                } else if terminal_reason
                    .as_deref()
                    .is_some_and(|r| r.starts_with("aborted"))
                {
                    TurnOutcome::Interrupted
                } else {
                    TurnOutcome::Failed
                };
                // Grok shares claude's wire - the turn's cost rides the
                // result frame the same way, when the harness reports it.
                if let Some(cost) = cost_usd {
                    if tx
                        .send(DriverEvent::TurnCost { cost_usd: cost })
                        .await
                        .is_err()
                    {
                        return;
                    }
                }
                if tx
                    .send(DriverEvent::TurnCompleted {
                        subtype,
                        coarse,
                        is_error,
                        error_message: None,
                    })
                    .await
                    .is_err()
                {
                    return;
                }
            }
            _ => {}
        }
    }
    // EOF semantics: the reason decides what the silence meant.
    if !saw_result {
        let reason = *shared.reason.lock().unwrap();
        match reason {
            EofReason::Interrupted => {
                let _ = tx
                    .send(DriverEvent::TurnCompleted {
                        subtype: "interrupted".into(),
                        coarse: TurnOutcome::Interrupted,
                        is_error: false,
                        error_message: None,
                    })
                    .await;
            }
            EofReason::Stopped => {
                let _ = tx.send(DriverEvent::StdoutClosed).await;
            }
            EofReason::Completed => {
                let _ = tx
                    .send(DriverEvent::TurnCompleted {
                        subtype: "failed".into(),
                        coarse: TurnOutcome::Failed,
                        is_error: true,
                        error_message: Some("grok exited before the turn finished".to_string()),
                    })
                    .await;
            }
        }
    }
}

async fn drain_stderr(
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
