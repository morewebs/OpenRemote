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
use openremote_harness::{DriverError, DriverEvent, Prompt, Resolution, SpawnOptions, kill_tree};
use serde_json::Value;
use tokio::io::BufReader;
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
        if opts.endpoint.is_some() {
            return Err(DriverError::Harness(
                "custom endpoints are not available for this harness yet".to_string(),
            ));
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
                    session_ref: StdMutex::new(opts.resume.clone()),
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
    ///
    /// The prompt reaches the CLI as a file (`--prompt-file`), never as an
    /// argv word: chat text is arbitrary, and `--single <prompt>` would put it
    /// where grok's parser looks for options.
    pub async fn send_prompt(&mut self, prompt: &Prompt) -> Result<(), DriverError> {
        // Grok's print mode reads words alone: a picture is refused here,
        // before the prompt file is written.
        let text = prompt.text_only("Grok Build")?;
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
        let resume = self.shared.session_ref.lock().unwrap().clone();
        // The file is claimed first because the argv has to name it; a word
        // the guard refuses removes it again, so a refusal still leaves
        // nothing behind.
        let prompt_file = write_prompt_fresh(text)?;
        let args = match build_args(&self.opts, resume.as_deref(), &prompt_file) {
            Ok(args) => args,
            Err(e) => {
                let _ = std::fs::remove_file(&prompt_file);
                return Err(e);
            }
        };
        let mut command = Command::new(program);
        command
            .args(&prefix)
            .args(&args)
            .current_dir(&cwd)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        // A key set in the user environment is a sign-in. This process may
        // have been started before that variable existed, so hand it to
        // the child when it is missing here.
        for (name, value) in crate::auth::missing_api_keys() {
            command.env(name, value);
        }
        // Windows: no console flash, no Ctrl-C from a shared console; the
        // why lives on openremote_harness::creation_flags.
        #[cfg(windows)]
        command.creation_flags(openremote_harness::creation_flags());

        *self.shared.reason.lock().unwrap() = EofReason::Completed;
        let mut child = match command.spawn() {
            Ok(child) => child,
            Err(e) => {
                let _ = std::fs::remove_file(&prompt_file);
                return Err(DriverError::Spawn(format!("grok: {e}")));
            }
        };
        let stdout = match child.stdout.take() {
            Some(stdout) => stdout,
            None => {
                let _ = std::fs::remove_file(&prompt_file);
                return Err(DriverError::Spawn("stdout not piped".into()));
            }
        };
        let stderr = match child.stderr.take() {
            Some(stderr) => stderr,
            None => {
                let _ = std::fs::remove_file(&prompt_file);
                return Err(DriverError::Spawn("stderr not piped".into()));
            }
        };
        {
            let mut guard = self.shared.child.lock().await;
            *guard = Some(child);
        }

        tokio::spawn(read_prompt(
            openremote_harness::LineReader::new(BufReader::new(stdout)),
            self.session_tx.clone(),
            Arc::clone(&self.shared),
            prompt_file,
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

    /// Stop the session: kill any running prompt. The stopped reason
    /// rides the reader's EOF as `StdoutClosed`.
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

/// The argv after the executable, in grok's print-mode shape.
///
/// The prompt is a file the CLI reads (`--prompt-file`), never a word in here:
/// chat text is arbitrary, and `--single <prompt>` would put it where grok's
/// parser looks for options.
fn build_args(
    opts: &SpawnOptions,
    resume: Option<&str>,
    prompt_file: &std::path::Path,
) -> Result<Vec<std::ffi::OsString>, DriverError> {
    let mut args: Vec<std::ffi::OsString> = vec![
        "--output-format".into(),
        "streaming-messages-json".into(),
        "--cwd".into(),
        opts.cwd.as_os_str().to_owned(),
    ];
    if let Some(model) = &opts.model {
        push_word(&mut args, "--model", model)?;
    }
    if let Some(mode) = &opts.permission_mode {
        push_word(&mut args, "--permission-mode", mode)?;
    }
    if let Some(effort) = &opts.effort {
        // The harness's own effort word, verbatim. The console offers the
        // tiers grok's catalog names; before this flag the pick did nothing.
        push_word(&mut args, "--reasoning-effort", effort)?;
    }
    if opts.include_deltas {
        // Grok emits text and thinking deltas only with this flag; without
        // it the reply arrives whole and the delta branches are dead code.
        args.push("--include-partial-messages".into());
    }
    if let Some(resume) = resume {
        // grok's own UUID session id - value form, no flag surface.
        push_word(&mut args, "--resume", resume)?;
    }
    args.push("--prompt-file".into());
    args.push(prompt_file.as_os_str().to_owned());
    Ok(args)
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

/// A fresh path for one prompt, in the OS temp folder.
///
/// Not the workspace: that folder is the user's own project, and a chat has no
/// business writing files into it. Not a fixed name either: the counter and
/// the process id keep two turns, and two daemons, apart.
fn prompt_path() -> std::path::PathBuf {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    std::env::temp_dir().join(format!("openremote-grok-{}-{n}.prompt", std::process::id()))
}

/// Write the prompt where the CLI reads it.
///
/// `create_new` refuses a path that already exists instead of following
/// whatever sits there, so nothing planted by another process can redirect the
/// write; a failed write leaves no half file behind. On unix the file is
/// owner-only: for the length of the turn the prompt sits in a temp directory
/// other users can list, and the argv it replaces was never that exposed.
fn write_prompt(path: &std::path::Path, text: &str) -> std::io::Result<()> {
    use std::io::Write;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path)?;
    if let Err(e) = file.write_all(text.as_bytes()) {
        drop(file);
        let _ = std::fs::remove_file(path);
        return Err(e);
    }
    Ok(())
}

/// Write the prompt to a fresh path, and say which one.
fn write_prompt_fresh(text: &str) -> Result<std::path::PathBuf, DriverError> {
    claim_prompt_file(text, prompt_path)
}

/// Claim a path nobody holds, and write the prompt into it.
///
/// A file left behind by a daemon that died with a turn open can own the name
/// this process is about to use, once the pid comes back and the counter
/// starts over, so a taken name earns one more try instead of a turn that
/// fails with "file exists". One retry is enough: the counter and the pid
/// together make a second collision a coincidence, not a pattern.
fn claim_prompt_file(
    text: &str,
    mut next_path: impl FnMut() -> std::path::PathBuf,
) -> Result<std::path::PathBuf, DriverError> {
    let mut attempt = 0;
    loop {
        let path = next_path();
        match write_prompt(&path, text) {
            Ok(()) => return Ok(path),
            Err(e) if attempt == 0 && e.kind() == std::io::ErrorKind::AlreadyExists => {
                attempt += 1;
            }
            Err(e) => return Err(DriverError::Spawn(format!("grok prompt file: {e}"))),
        }
    }
}

/// Removes the prompt file when the turn's reader ends - early return, EOF or
/// kill alike.
struct PromptFile(Option<std::path::PathBuf>);

impl Drop for PromptFile {
    fn drop(&mut self) {
        if let Some(path) = self.0.take() {
            let _ = std::fs::remove_file(path);
        }
    }
}

async fn read_prompt(
    mut reader: openremote_harness::LineReader<BufReader<tokio::process::ChildStdout>>,
    tx: mpsc::Sender<DriverEvent>,
    shared: Arc<Shared>,
    prompt_file: std::path::PathBuf,
) {
    let _prompt_file = PromptFile(Some(prompt_file));
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
        let Some(frame) = wire::parse_frame(&line) else {
            continue;
        };
        match frame {
            wire::Frame::Init {
                session_id,
                model,
                permission_mode,
                ..
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
                for block in wire::tool_result_blocks(&content) {
                    let _ = tx
                        .send(DriverEvent::ToolResult {
                            tool_use_id: String::new(),
                            name: "commandExecution".into(),
                            text: block.text,
                            is_error: block.is_error,
                            images: block.images,
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
                usage,
                cost_usd,
            } => {
                saw_result = true;
                // The turn's usage is the conversation's context: the SDK's
                // input fields together. The ring was empty because this
                // frame's usage was thrown away.
                if let Some(used) = wire::context_used(usage.as_ref()) {
                    if tx
                        .send(DriverEvent::ContextUsed { used, window: None })
                        .await
                        .is_err()
                    {
                        return;
                    }
                }
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
    fn the_prompt_is_a_file_and_never_an_argv_word() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("prompt.txt");
        let exploit = "--dangerously-skip-permissions fix the build";
        write_prompt(&path, exploit).expect("prompt file");
        assert_eq!(std::fs::read_to_string(&path).expect("read back"), exploit);

        let args = build_args(
            &opts(Some("grok-4.7"), Some("default")),
            Some("0b1c"),
            &path,
        )
        .expect("args build");
        let words: Vec<String> = args
            .iter()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();
        // grok's documented data path: the flag names the file, and only the
        // path rides the command line.
        let at = words
            .iter()
            .position(|word| word == "--prompt-file")
            .expect("the prompt goes in a file");
        assert_eq!(words[at + 1], path.to_string_lossy());
        assert!(!words.iter().any(|word| word == "--single"));
        for word in &words {
            assert!(!word.contains("--dangerously-skip-permissions"), "{word}");
            assert!(!word.contains("fix the build"), "{word}");
        }
    }

    #[test]
    fn a_dash_leading_word_is_refused_before_it_reaches_argv() {
        let path = std::path::PathBuf::from("/tmp/openremote.prompt");
        assert!(build_args(&opts(Some("--always-approve"), None), None, &path).is_err());
        assert!(build_args(&opts(None, Some("-x")), None, &path).is_err());
        assert!(
            build_args(
                &opts(None, None),
                Some("--dangerously-skip-permissions"),
                &path
            )
            .is_err()
        );
    }

    #[test]
    fn a_prompt_file_is_created_fresh_and_never_written_over_a_link() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("prompt.txt");
        write_prompt(&path, "one").expect("first write");
        // A second write at the same path refuses instead of following
        // whatever is there now.
        assert!(write_prompt(&path, "two").is_err());
        assert_eq!(std::fs::read_to_string(&path).expect("read back"), "one");
    }

    #[test]
    fn the_prompt_file_goes_when_the_turn_ends() {
        // The removal rides a drop guard, so this is the test that fails if
        // someone moves the guard off the turn's reader: the prompt would
        // outlive the turn in a temp directory.
        let path = write_prompt_fresh("hello").expect("prompt file");
        assert!(path.exists(), "the CLI has something to read");
        drop(PromptFile(Some(path.clone())));
        assert!(!path.exists(), "the turn's end takes the prompt with it");
    }

    #[test]
    fn a_stale_prompt_file_costs_one_retry_and_no_turn() {
        // A daemon that died with a turn open leaves its prompt behind; once
        // the pid comes back and the counter restarts, the next turn's first
        // name is taken. One more try, and the turn runs.
        let dir = tempfile::tempdir().expect("temp dir");
        let stale = dir.path().join("openremote-grok-stale.prompt");
        std::fs::write(&stale, "from a daemon that died").expect("stale file");
        let fresh = dir.path().join("openremote-grok-fresh.prompt");
        let mut names = vec![stale.clone(), fresh.clone()].into_iter();
        let claimed = claim_prompt_file("hello", || names.next().expect("two names"))
            .expect("the second name is free");
        assert_eq!(claimed, fresh);
        assert_eq!(std::fs::read_to_string(&fresh).expect("read back"), "hello");
        // The stale file is left alone: it is evidence, not ours to overwrite.
        assert_eq!(
            std::fs::read_to_string(&stale).expect("stale kept"),
            "from a daemon that died"
        );
    }

    #[test]
    fn grok_effort_and_deltas_ride_the_argv_when_set() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("prompt.txt");
        write_prompt(&path, "hi").expect("prompt file");

        let mut with_effort = opts(Some("grok-4.7"), None);
        with_effort.effort = Some("high".into());
        with_effort.include_deltas = true;
        let words: Vec<String> = build_args(&with_effort, None, &path)
            .expect("args build")
            .iter()
            .map(|word| word.to_string_lossy().into_owned())
            .collect();
        let at = words
            .iter()
            .position(|word| word == "--reasoning-effort")
            .expect("effort passes the harness's own flag");
        assert_eq!(words[at + 1], "high");
        assert!(
            words
                .iter()
                .any(|word| word == "--include-partial-messages")
        );

        // Unset: neither flag appears, so the harness's own default holds.
        let plain = opts(Some("grok-4.7"), None);
        let words: Vec<String> = build_args(&plain, None, &path)
            .expect("args build")
            .iter()
            .map(|word| word.to_string_lossy().into_owned())
            .collect();
        assert!(!words.iter().any(|word| word == "--reasoning-effort"));
        assert!(
            !words
                .iter()
                .any(|word| word == "--include-partial-messages")
        );
    }

    #[test]
    fn a_dash_leading_effort_is_refused_like_every_other_word() {
        let path = std::path::PathBuf::from("/tmp/openremote.prompt");
        let mut opts = opts(Some("grok-4.7"), None);
        opts.effort = Some("--dangerously-skip-permissions".into());
        assert!(build_args(&opts, None, &path).is_err());
    }
}
