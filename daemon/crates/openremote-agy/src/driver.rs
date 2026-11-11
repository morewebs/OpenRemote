//! The Antigravity session driver: one process per prompt, one
//! conversation across them (the grok print-mode shape with Antigravity's
//! own frames). The first prompt's `init` names the conversation; later
//! prompts respawn with `--conversation <id>`. The turn boundary is the
//! `result` frame with Antigravity's own status word.

use std::process::Stdio;
use std::sync::{Arc, Mutex as StdMutex};
use std::time::Duration;

use openremote_core::TurnOutcome;
use openremote_harness::{DriverError, DriverEvent, Prompt, Resolution, SpawnOptions, kill_tree};
use serde_json::Value;
use tokio::io::{AsyncWriteExt, BufReader};
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
                    // A resumed chat continues its own conversation: the
                    // first prompt's process starts on the id the resume
                    // handed over, never on a fresh one.
                    conversation: StdMutex::new(opts.resume.clone()),
                }),
                session_tx: tx,
                resolution: resolution.clone(),
                opts,
            },
            rx,
        ))
    }

    /// Run one prompt as its own agy process.
    ///
    /// The prompt enters on stdin as one NDJSON message and never touches
    /// argv: chat text is arbitrary, and a word a chat typed must not stand
    /// anywhere the CLI's parser could read it as a flag.
    pub async fn send_prompt(&mut self, prompt: &Prompt) -> Result<(), DriverError> {
        // Antigravity's print mode reads words alone: a picture is refused
        // here, before anything is spawned.
        let text = prompt.text_only("Antigravity")?;
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
        let conversation = self.shared.conversation.lock().unwrap().clone();
        let args = build_args(&self.opts, conversation.as_deref())?;
        let mut command = Command::new(program);
        command
            .args(&args)
            .current_dir(&cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        // Windows: no console flash, no Ctrl-C from a shared console; the
        // why lives on openremote_harness::creation_flags.
        #[cfg(windows)]
        command.creation_flags(openremote_harness::creation_flags());

        *self.shared.reason.lock().unwrap() = EofReason::Completed;
        let mut child = command
            .spawn()
            .map_err(|e| DriverError::Spawn(format!("agy: {e}")))?;
        let mut stdin = child
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
        {
            let mut guard = self.shared.child.lock().await;
            *guard = Some(child);
        }
        // One message for one prompt, then the pipe closes. Print mode runs a
        // turn per message and ends the run at stdin EOF, so closing the pipe
        // is what tells the CLI this prompt is complete.
        let frame = prompt_frame(text);
        if let Err(e) = stdin.write_all(frame.as_bytes()).await {
            return Err(DriverError::Spawn(format!("agy stdin: {e}")));
        }
        drop(stdin);
        tokio::spawn(read_prompt(
            openremote_harness::LineReader::new(BufReader::new(stdout)),
            self.session_tx.clone(),
            Arc::clone(&self.shared),
        ));
        tokio::spawn(drain_stderr(
            openremote_harness::LineReader::new(BufReader::new(stderr)),
            self.session_tx.clone(),
        ));
        Ok(())
    }

    pub async fn interrupt(&mut self) -> Result<(), DriverError> {
        *self.shared.reason.lock().unwrap() = EofReason::Interrupted;
        let mut guard = self.shared.child.lock().await;
        if let Some(child) = guard.as_mut() {
            kill_tree(child).await;
        }
        Ok(())
    }

    pub async fn shutdown(&mut self) -> Result<Option<i32>, DriverError> {
        *self.shared.reason.lock().unwrap() = EofReason::Stopped;
        let mut guard = self.shared.child.lock().await;
        if let Some(mut child) = guard.take() {
            kill_tree(&mut child).await;
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

/// The argv after the executable: every flag, then `--print=` last.
///
/// Print mode reads its prompt from stdin when `--print` carries none of its
/// own: `--input-format stream-json` reads one NDJSON message per line and runs
/// a turn per message. That is what keeps chat text out of argv. The form is
/// the CLI's own: an inline empty value turns print mode on without asking for
/// a prompt, and a `--print <word>` followed by more flags is refused rather
/// than guessed at (agy 1.4.7).
fn build_args(
    opts: &SpawnOptions,
    conversation: Option<&str>,
) -> Result<Vec<std::ffi::OsString>, DriverError> {
    let mut args: Vec<std::ffi::OsString> = vec![
        "--output-format".into(),
        "stream-json".into(),
        "--input-format".into(),
        "stream-json".into(),
    ];
    if let Some(model) = &opts.model {
        push_word(&mut args, "--model", model)?;
    }
    if let Some(mode) = &opts.permission_mode {
        // Antigravity's own execution-mode words.
        push_word(&mut args, "--mode", mode)?;
    }
    if let Some(conversation) = conversation {
        push_word(&mut args, "--conversation", conversation)?;
    }
    args.push("--print=".into());
    Ok(args)
}

/// The one line that carries a prompt.
///
/// Antigravity's stream input names each message `event`, and a user turn is a
/// `user` event holding an Anthropic-shaped message body (agy 1.4.7 refuses a
/// line whose `event` or `message` field is missing before any turn starts).
/// Building it as JSON is the whole point: the prompt is a string value, so no
/// word in it can be read as an option.
fn prompt_frame(text: &str) -> String {
    let mut frame = serde_json::json!({
        "event": "user",
        "message": {
            "role": "user",
            "content": [{"type": "text", "text": text}],
        },
    })
    .to_string();
    frame.push('\n');
    frame
}

/// One flag and its value, refused when the value could be read as grammar
/// instead of as a name.
fn push_word(
    args: &mut Vec<std::ffi::OsString>,
    flag: &str,
    value: &str,
) -> Result<(), DriverError> {
    openremote_core::validate_argv_word(value)
        .map_err(|why| DriverError::Spawn(format!("{flag} {value:?}: {why}")))?;
    args.push(flag.into());
    args.push(value.into());
    Ok(())
}

async fn read_prompt(
    mut reader: openremote_harness::LineReader<BufReader<tokio::process::ChildStdout>>,
    tx: mpsc::Sender<DriverEvent>,
    shared: Arc<Shared>,
) {
    let mut saw_result = false;
    loop {
        // Bounded, lossy reads (harness audit F11): an over-long line is
        // dropped and invalid UTF-8 decodes lossily, never a fatal break.
        let line = match reader.next_line().await {
            Ok(None) | Err(_) => break,
            Ok(Some(line)) => line,
        };
        if line.trim().is_empty() {
            continue;
        }
        let Ok(frame) = serde_json::from_str::<Value>(&line) else {
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
                // The result frame's usage is the same input usage the
                // Anthropic wire carries; only status, response and error
                // were read before, so the context ring stayed empty.
                if let Some(used) =
                    openremote_harness::anthropic_wire::context_used(result.get("usage"))
                {
                    let _ = tx
                        .send(DriverEvent::ContextUsed { used, window: None })
                        .await;
                }
                let coarse = if status == "SUCCESS" {
                    TurnOutcome::Completed
                } else {
                    TurnOutcome::Failed
                };
                let error_message = result
                    .get("error")
                    .and_then(Value::as_str)
                    .filter(|text| !text.is_empty())
                    .map(str::to_string);
                let _ = tx
                    .send(DriverEvent::TurnCompleted {
                        subtype: status.clone(),
                        coarse,
                        is_error: coarse == TurnOutcome::Failed,
                        error_message,
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
    mut reader: openremote_harness::LineReader<BufReader<tokio::process::ChildStderr>>,
    tx: mpsc::Sender<DriverEvent>,
) {
    loop {
        let line = match reader.next_line().await {
            Ok(None) | Err(_) => break,
            Ok(Some(line)) => line,
        };
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
    let mut command = Command::new(program);
    command
        .arg("models")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    // Windows: no console flash, no Ctrl-C from a shared console; the
    // why lives on openremote_harness::creation_flags.
    #[cfg(windows)]
    command.creation_flags(openremote_harness::creation_flags());
    let mut child = command
        .spawn()
        .map_err(|e| DriverError::Spawn(format!("agy models: {e}")))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| DriverError::Spawn("stdout not piped".into()))?;
    // The catalog must land inside this window. A CLI that hangs or dies
    // mid-print is a failed probe, never an advertisement of nothing: an
    // empty catalog reads as "no models" end to end, and the console
    // would hold it as the harness's own answer.
    let read = tokio::time::timeout(Duration::from_secs(30), async {
        let mut out = String::new();
        let mut reader = BufReader::new(stdout);
        tokio::io::AsyncReadExt::read_to_string(&mut reader, &mut out).await?;
        Ok::<_, std::io::Error>(out)
    });
    let output = match read.await {
        Ok(Ok(out)) => out,
        Ok(Err(e)) => {
            kill_tree(&mut child).await;
            return Err(DriverError::Spawn(format!("agy models: {e}")));
        }
        Err(_) => {
            kill_tree(&mut child).await;
            return Err(DriverError::Spawn(
                "agy models: the CLI never finished printing its catalog".into(),
            ));
        }
    };
    // The CLI's own exit is the not-signed-in / broken-install signal: a
    // nonzero exit never vouched for this catalog - a failed probe, not
    // an advertisement of nothing. Output ends at EOF, so the natural
    // exit lands shortly after; a CLI that lingers past this short grace
    // is a hung probe - killed and reported, never answered from.
    let status = match tokio::time::timeout(Duration::from_secs(10), child.wait()).await {
        Ok(Ok(status)) => status,
        Ok(Err(e)) => return Err(DriverError::Spawn(format!("agy models: {e}"))),
        Err(_) => {
            kill_tree(&mut child).await;
            return Err(DriverError::Spawn(
                "agy models: the CLI never exited".into(),
            ));
        }
    };
    if !status.success() {
        return Err(DriverError::Spawn(format!(
            "agy models: exited with {}",
            status.code().unwrap_or(-1)
        )));
    }
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
            // Antigravity's list carries names only: no cost line, no notice.
            detail: None,
            note: None,
        });
    }
    Ok(models)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opts(model: Option<&str>, mode: Option<&str>) -> SpawnOptions {
        SpawnOptions {
            cwd: std::path::PathBuf::from("/w"),
            model: model.map(str::to_string),
            permission_mode: mode.map(str::to_string),
            ..Default::default()
        }
    }

    #[test]
    fn the_prompt_rides_stdin_and_never_appears_in_argv() {
        let exploit = "--dangerously-skip-permissions fix the build";
        let args = build_args(&opts(Some("gemini-3-pro"), Some("auto")), None).expect("args build");
        let words: Vec<String> = args
            .iter()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();
        // Print mode is on, and it carries no prompt of its own.
        assert!(words.iter().any(|word| word == "--print="), "{words:?}");
        // Nothing a chat typed is in there, neither as a word nor inside one.
        for word in &words {
            assert!(!word.contains("--dangerously-skip-permissions"), "{word}");
            assert!(!word.contains("fix the build"), "{word}");
        }

        // The prompt itself is one JSON line on stdin.
        let frame = prompt_frame(exploit);
        assert_eq!(
            frame.matches('\n').count(),
            1,
            "one newline-terminated line"
        );
        let value: Value = serde_json::from_str(frame.trim_end()).expect("the frame is JSON");
        assert_eq!(value["event"], "user");
        assert_eq!(value["message"]["role"], "user");
        assert_eq!(value["message"]["content"][0]["type"], "text");
        assert_eq!(value["message"]["content"][0]["text"], exploit);
    }

    #[test]
    fn a_dash_leading_word_is_refused_before_it_reaches_argv() {
        assert!(build_args(&opts(Some("--dangerously-skip-permissions"), None), None).is_err());
        assert!(build_args(&opts(None, Some("-x")), None).is_err());
        assert!(build_args(&opts(None, None), Some("--dangerously-skip-permissions")).is_err());
    }
}
