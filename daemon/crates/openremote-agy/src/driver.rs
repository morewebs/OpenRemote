//! The Antigravity session driver: one process per prompt, one
//! conversation across them (the grok print-mode shape with Antigravity's
//! own frames). The first prompt's `init` names the conversation; later
//! prompts respawn with `--conversation <id>`. The turn boundary is the
//! `result` frame with Antigravity's own status word.

use std::process::Stdio;
use std::sync::{Arc, Mutex as StdMutex};
use std::time::Duration;

use openremote_core::TurnOutcome;
use openremote_harness::{DriverError, DriverEvent, Resolution, SpawnOptions};
use serde_json::Value;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::{Mutex as AsyncMutex, mpsc};

/// Why a prompt process ended without a result frame.
#[derive(Clone, Copy, PartialEq, Eq)]
enum EofReason {
    Completed,
    Interrupted,
    Stopped,
}

struct Shared {
    child: Arc<AsyncMutex<Option<Child>>>,
    reason: StdMutex<EofReason>,
    /// Antigravity's own conversation id, learned from the first init
    /// and updated whenever a respawned process reports one.
    conversation: StdMutex<Option<String>>,
}

pub struct Driver {
    shared: Arc<Shared>,
    session_tx: mpsc::Sender<DriverEvent>,
    resolution: Resolution,
    opts: SpawnOptions,
}

impl Driver {
    /// An agy session starts as pure configuration - no process until
    /// the first prompt. `Initialized` rides the first prompt's init
    /// frame.
    pub fn spawn(
        resolution: &Resolution,
        opts: SpawnOptions,
    ) -> Result<(Self, mpsc::Receiver<DriverEvent>), DriverError> {
        if !resolution.is_available() {
            return Err(DriverError::Spawn("agy CLI not found".to_string()));
        }
        let (tx, rx) = mpsc::channel(256);
        Ok((
            Self {
                shared: Arc::new(Shared {
                    child: Arc::new(AsyncMutex::new(None)),
                    reason: StdMutex::new(EofReason::Completed),
                    conversation: StdMutex::new(None),
                }),
                session_tx: tx,
                resolution: resolution.clone(),
                opts,
            },
            rx,
        ))
    }

    /// Run one prompt as its own agy process.
    pub async fn send_prompt(&mut self, text: &str) -> Result<(), DriverError> {
        let program = match &self.resolution {
            Resolution::Executable(path) => path.clone(),
            Resolution::NodeScript { .. } => {
                return Err(DriverError::Spawn(
                    "agy does not ship a node-script layout".to_string(),
                ));
            }
            Resolution::Unavailable => {
                return Err(DriverError::Spawn("agy CLI not found".to_string()));
            }
        };
        let cwd = self.opts.cwd.clone();
        let mut command = Command::new(program);
        command
            .arg("--print")
            .arg(text)
            .arg("--output-format")
            .arg("stream-json")
            .current_dir(&cwd)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if let Some(model) = &self.opts.model {
            command.arg("--model").arg(model);
        }
        if let Some(mode) = &self.opts.permission_mode {
            // Antigravity's own execution-mode words.
            command.arg("--mode").arg(mode);
        }
        if let Some(conversation) = self.shared.conversation.lock().unwrap().clone() {
            command.arg("--conversation").arg(conversation);
        }

        *self.shared.reason.lock().unwrap() = EofReason::Completed;
        let mut child = command
            .spawn()
            .map_err(|e| DriverError::Spawn(format!("agy: {e}")))?;
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

    /// Antigravity's permission prompts (its `ask_permission` tool) have
    /// no verified remote channel in print mode - nothing ever asks, so
    /// this is never called. Its own `--mode` words govern execution.
    pub async fn answer(
        &mut self,
        _harness_ref: &str,
        _choice: &str,
        _request: &Value,
    ) -> Result<(), DriverError> {
        Err(DriverError::Protocol(
            "antigravity print mode has no remote approval channel".into(),
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
        let Ok(frame) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        let event = frame
            .get("event")
            .and_then(Value::as_str)
            .unwrap_or_default();
        match event {
            "init" => {
                let Some(conversation) = frame.get("conversation_id").and_then(Value::as_str)
                else {
                    continue;
                };
                let first = shared.conversation.lock().unwrap().is_none();
                *shared.conversation.lock().unwrap() = Some(conversation.to_string());
                if first {
                    let _ = tx
                        .send(DriverEvent::Initialized {
                            session_ref: conversation.to_string(),
                            model: None,
                            permission_mode: None,
                        })
                        .await;
                }
            }
            "step_update" => {
                // Agent text streams as deltas; the result frame carries
                // the full response - deltas stream for live views.
                if let Some(delta) = frame
                    .pointer("/step_update/text_delta")
                    .and_then(Value::as_str)
                {
                    let _ = tx
                        .send(DriverEvent::TextDelta {
                            text: delta.to_string(),
                        })
                        .await;
                }
            }
            "result" => {
                saw_result = true;
                let result = frame.get("result").cloned().unwrap_or(Value::Null);
                let status = result
                    .get("status")
                    .and_then(Value::as_str)
                    .unwrap_or("FAILURE")
                    .to_string();
                if let Some(response) = result.get("response").and_then(Value::as_str) {
                    let text = response.trim_end_matches('\n').to_string();
                    if !text.is_empty() {
                        let _ = tx.send(DriverEvent::AssistantText { text }).await;
                    }
                }
                let coarse = if status == "SUCCESS" {
                    TurnOutcome::Completed
                } else {
                    TurnOutcome::Failed
                };
                let _ = tx
                    .send(DriverEvent::TurnCompleted {
                        subtype: status.clone(),
                        coarse,
                        is_error: coarse == TurnOutcome::Failed,
                        error_message: None,
                    })
                    .await;
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
                        subtype: "INTERRUPTED".into(),
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
                        subtype: "FAILURE".into(),
                        coarse: TurnOutcome::Failed,
                        is_error: true,
                        error_message: Some(
                            "antigravity exited before the turn finished".to_string(),
                        ),
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
            Ok(0) | Err(_) => break,
            Ok(_) => {}
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

/// The model catalog (`agy models`): one `id<TAB>name` line per model,
/// Antigravity's own words. Backs the console's model slot.
pub async fn models(
    resolution: &Resolution,
) -> Result<Vec<openremote_harness::ModelDescriptor>, DriverError> {
    let program = match resolution {
        Resolution::Executable(path) => path.clone(),
        _ => return Err(DriverError::Spawn("agy CLI not found".to_string())),
    };
    let mut child = Command::new(program)
        .arg("models")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| DriverError::Spawn(format!("agy models: {e}")))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| DriverError::Spawn("stdout not piped".into()))?;
    let output = tokio::time::timeout(Duration::from_secs(30), async {
        let mut out = String::new();
        let mut reader = BufReader::new(stdout);
        let _ = tokio::io::AsyncReadExt::read_to_string(&mut reader, &mut out).await;
        out
    })
    .await
    .unwrap_or_default();
    let _ = child.start_kill();
    let mut models = Vec::new();
    for line in output.lines() {
        let line = line.trim_end_matches(['\r', '\n']);
        if line.trim().is_empty() {
            continue;
        }
        let (id, name) = match line.split_once('\t') {
            Some((id, name)) => (id, name),
            None => (line, ""),
        };
        models.push(openremote_harness::ModelDescriptor {
            model: id.to_string(),
            display_name: (!name.is_empty()).then(|| name.to_string()),
            resolved_model: None,
            reasoning_efforts: Vec::new(),
            is_default: false,
        });
    }
    Ok(models)
}
