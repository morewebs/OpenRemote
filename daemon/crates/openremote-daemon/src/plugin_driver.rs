//! The process behind a plugin-provided harness: one child per chat, JSON
//! lines over its stdin and stdout, the daemon's own driver vocabulary on both
//! sides. The plugin owns everything harness-specific (its CLI, its PTY, its
//! transcript); this module only speaks the seam.
//!
//! A driver's stdout carries one event per line and nothing else. Its stderr
//! is diagnostics, forwarded to the console as they arrive, so a plugin author
//! can debug a driver without a debugger attached to the daemon.

use std::process::Stdio;
use std::time::Duration;

use openremote_core::{DecisionKind, DecisionOption, TurnOutcome};
use openremote_harness::{
    ApprovalRequest, DecisionSpec, DriverError, DriverEvent, SessionSettings, SpawnOptions,
};
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, Command};
use tokio::sync::mpsc;

use crate::plugins::HarnessSpec;

/// How long a driver's `--probe` may take before the harness reads as
/// unavailable. A probe is a print-and-exit, never a chat.
const PROBE_DEADLINE: Duration = Duration::from_secs(5);

/// How long `shutdown` waits for the driver to end its harness child before
/// the daemon kills the driver. A TUI may need to restore the terminal.
const SHUTDOWN_DEADLINE: Duration = Duration::from_secs(3);

/// How long a driver's `--catalog` may take before the capture reads as
/// failed. It drives the harness's own picker screen, so it is minutes where
/// a probe is seconds.
const CATALOG_DEADLINE: Duration = Duration::from_secs(60);

/// What a plugin driver answered to `--probe`.
#[derive(Clone, Debug, Default)]
pub struct ProbeAnswer {
    pub available: bool,
    pub version: Option<String>,
    pub signed_in: Option<bool>,
    /// Why the harness cannot be offered, in one sentence for the console.
    pub reason: Option<String>,
}

/// Run `<driver> --probe` and read its one JSON line.
pub async fn probe(spec: &HarnessSpec) -> ProbeAnswer {
    let Some(problem) = spec.problem.clone() else {
        return probe_driver(spec).await;
    };
    ProbeAnswer {
        reason: Some(problem),
        ..ProbeAnswer::default()
    }
}

async fn probe_driver(spec: &HarnessSpec) -> ProbeAnswer {
    let mut command = Command::new(&spec.driver);
    command
        .args(&spec.args)
        .arg("--probe")
        .current_dir(&spec.dir)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    #[cfg(windows)]
    command.creation_flags(openremote_harness::creation_flags());
    let output = match tokio::time::timeout(PROBE_DEADLINE, command.output()).await {
        Ok(Ok(output)) => output,
        Ok(Err(err)) => {
            return ProbeAnswer {
                reason: Some(format!(
                    "the driver did not start: {} ({err})",
                    spec.driver.display()
                )),
                ..ProbeAnswer::default()
            };
        }
        Err(_) => {
            return ProbeAnswer {
                reason: Some(format!(
                    "the driver did not answer --probe within {} seconds",
                    PROBE_DEADLINE.as_secs()
                )),
                ..ProbeAnswer::default()
            };
        }
    };
    let text = String::from_utf8_lossy(&output.stdout);
    let line = text.lines().map(str::trim).find(|line| !line.is_empty());
    let Some(line) = line else {
        return ProbeAnswer {
            reason: Some("the driver's --probe printed nothing".into()),
            ..ProbeAnswer::default()
        };
    };
    match serde_json::from_str::<Value>(line) {
        Ok(value) => ProbeAnswer {
            available: value
                .get("available")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            version: value
                .get("version")
                .and_then(Value::as_str)
                .filter(|s| !s.trim().is_empty())
                .map(String::from),
            signed_in: value.get("signed_in").and_then(Value::as_bool),
            reason: value
                .get("reason")
                .and_then(Value::as_str)
                .filter(|s| !s.trim().is_empty())
                .map(String::from),
        },
        Err(err) => ProbeAnswer {
            reason: Some(format!("the driver's --probe answer is not JSON: {err}")),
            ..ProbeAnswer::default()
        },
    }
}

/// Run `<driver> --catalog` and decide whether it captured a catalog.
///
/// The driver writes the catalog file beside itself, where the next probe
/// reads it; this only needs the driver's own answer, because a failed
/// capture prints the same shape with a `reason` sentence and still exits 0.
/// That sentence is what the console shows, so a failed run is never a
/// silent stale list.
pub async fn capture_catalog(spec: &HarnessSpec) -> Result<(), String> {
    if let Some(problem) = spec.problem.clone() {
        return Err(problem);
    }
    let mut command = Command::new(&spec.driver);
    command
        .args(&spec.args)
        .arg("--catalog")
        .current_dir(&spec.dir)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        // A driver prints its picker's own screen to stderr; it can carry
        // balance text and the signed-in address, so it never reaches an
        // answer the console or a peer can read.
        .stderr(Stdio::null())
        .kill_on_drop(true);
    #[cfg(windows)]
    command.creation_flags(openremote_harness::creation_flags());
    let output = match tokio::time::timeout(CATALOG_DEADLINE, command.output()).await {
        Ok(Ok(output)) => output,
        Ok(Err(err)) => {
            return Err(format!(
                "the driver did not start: {} ({err})",
                spec.driver.display()
            ));
        }
        Err(_) => {
            return Err(format!(
                "the driver did not finish its catalog within {} seconds",
                CATALOG_DEADLINE.as_secs()
            ));
        }
    };
    if !output.status.success() {
        // The contract says a failure still exits 0 and carries its reason;
        // a non-zero exit is a broken driver, and its stderr is not safe to
        // quote, so the sentence names only the exit.
        return Err(format!(
            "the driver exited with {} while capturing its catalog",
            output.status
        ));
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let Some(line) = text.lines().map(str::trim).find(|line| !line.is_empty()) else {
        return Err("the driver printed no catalog".into());
    };
    let value: Value = serde_json::from_str(line)
        .map_err(|err| format!("the driver's catalog answer is not JSON: {err}"))?;
    match value.get("reason").and_then(Value::as_str) {
        Some(reason) if !reason.trim().is_empty() => Err(reason.to_string()),
        _ => Ok(()),
    }
}

/// A live driver. The daemon writes commands; the pump task turns its events
/// into `DriverEvent`s the supervisor already understands.
pub struct PluginDriver {
    child: Child,
    stdin: ChildStdin,
    /// Whether this harness's driver can apply a settings change to a running
    /// chat. Read from the manifest at spawn, so the refusal is decided by
    /// what the plugin declared, not by guessing from the driver's answer.
    settings_live: bool,
}

impl PluginDriver {
    /// Start the driver for one chat and hand it the spawn options.
    pub async fn spawn(
        spec: &HarnessSpec,
        opts: SpawnOptions,
    ) -> Result<(Self, mpsc::Receiver<DriverEvent>), DriverError> {
        if let Some(problem) = &spec.problem {
            return Err(DriverError::Spawn(problem.clone()));
        }
        let mut command = Command::new(&spec.driver);
        command
            .args(&spec.args)
            .current_dir(&spec.dir)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        #[cfg(windows)]
        command.creation_flags(openremote_harness::creation_flags());
        let mut child = command
            .spawn()
            .map_err(|err| DriverError::Spawn(format!("{}: {err}", spec.driver.display())))?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| DriverError::Spawn("the driver has no stdin".into()))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| DriverError::Spawn("the driver has no stdout".into()))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| DriverError::Spawn("the driver has no stderr".into()))?;
        let (tx, rx) = mpsc::channel(256);
        tokio::spawn(pump_stdout(stdout, tx.clone()));
        tokio::spawn(pump_stderr(stderr, tx));
        let mut driver = Self {
            child,
            stdin,
            settings_live: spec.settings_live,
        };
        driver
            .write(&json!({
                "type": "spawn",
                "cwd": opts.cwd.display().to_string(),
                "model": opts.model,
                "permission_mode": opts.permission_mode,
                "effort": opts.effort,
                "resume": opts.resume,
                "fast": opts.fast,
            }))
            .await?;
        Ok((driver, rx))
    }

    pub async fn send_prompt(&mut self, text: &str) -> Result<(), DriverError> {
        self.write(&json!({"type": "prompt", "text": text})).await
    }

    pub async fn interrupt(&mut self) -> Result<(), DriverError> {
        self.write(&json!({"type": "interrupt"})).await
    }

    /// A plugin harness starts every chat with its stored facts. A driver that
    /// declared `"settings": "live"` in its manifest takes a later change on
    /// the running process; any other keeps the stored word and is restarted
    /// onto it, which is what the supervisor recognises by the sentence.
    pub async fn apply_settings(&mut self, settings: &SessionSettings) -> Result<(), DriverError> {
        if !self.settings_live {
            return Err(DriverError::Harness(
                "this harness applies model and effort when the chat starts - stop it and resume to change them"
                    .into(),
            ));
        }
        // The driver decides what a word it cannot apply means, and answers
        // a refusal in one sentence; the daemon does not pre-filter fields.
        self.write(&json!({
            "type": "settings",
            "model": settings.model,
            "effort": settings.effort,
            "fast": settings.fast,
        }))
        .await
    }

    /// A plugin that asked for an approval gets the answer on the same pipe,
    /// in the command shape the contract documents. The daemon's own outcome
    /// stays the default: the driver's follow-up events say what happened.
    pub async fn answer(
        &mut self,
        harness_ref: &str,
        choice: &str,
        _request: &Value,
    ) -> Result<(), DriverError> {
        self.write(&json!({
            "type": "answer",
            "harness_ref": harness_ref,
            "choice": choice,
        }))
        .await
    }

    pub async fn shutdown(&mut self) -> Result<Option<i32>, DriverError> {
        let _ = self.write(&json!({"type": "shutdown"})).await;
        match tokio::time::timeout(SHUTDOWN_DEADLINE, self.child.wait()).await {
            Ok(Ok(status)) => Ok(status.code()),
            _ => {
                let _ = self.child.kill().await;
                Ok(None)
            }
        }
    }

    async fn write(&mut self, value: &Value) -> Result<(), DriverError> {
        let mut line = serde_json::to_vec(value)
            .map_err(|err| DriverError::Protocol(format!("could not encode a command: {err}")))?;
        line.push(b'\n');
        self.stdin.write_all(&line).await?;
        self.stdin.flush().await?;
        Ok(())
    }
}

async fn pump_stdout(stdout: tokio::process::ChildStdout, tx: mpsc::Sender<DriverEvent>) {
    let mut lines = BufReader::new(stdout).lines();
    loop {
        match lines.next_line().await {
            Ok(Some(line)) => {
                let line = line.trim();
                if line.is_empty() {
                    continue;
                }
                let event = match parse_event(line) {
                    Ok(event) => event,
                    Err(err) => DriverEvent::Stderr {
                        line: format!("plugin driver: {err}: {line}"),
                    },
                };
                if tx.send(event).await.is_err() {
                    return;
                }
            }
            Ok(None) => {
                let _ = tx.send(DriverEvent::StdoutClosed).await;
                return;
            }
            Err(err) => {
                let _ = tx
                    .send(DriverEvent::Stderr {
                        line: format!("plugin driver stdout: {err}"),
                    })
                    .await;
                let _ = tx.send(DriverEvent::StdoutClosed).await;
                return;
            }
        }
    }
}

async fn pump_stderr(stderr: tokio::process::ChildStderr, tx: mpsc::Sender<DriverEvent>) {
    let mut lines = BufReader::new(stderr).lines();
    while let Ok(Some(line)) = lines.next_line().await {
        if tx.send(DriverEvent::Stderr { line }).await.is_err() {
            return;
        }
    }
}

/// One protocol line to one driver event. Public for the tests: the wire is a
/// contract, so it is tested on its own.
pub fn parse_event(line: &str) -> Result<DriverEvent, String> {
    let value: Value = serde_json::from_str(line).map_err(|err| err.to_string())?;
    let kind = value
        .get("type")
        .and_then(Value::as_str)
        .ok_or("the line names no type")?;
    let text = |key: &str| {
        value
            .get(key)
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string()
    };
    let optional = |key: &str| {
        value
            .get(key)
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .map(String::from)
    };
    Ok(match kind {
        "initialized" => DriverEvent::Initialized {
            session_ref: text("session_ref"),
            model: optional("model"),
            permission_mode: optional("permission_mode"),
        },
        "model_reported" => DriverEvent::ModelReported {
            model: text("model"),
        },
        "settings_refused" => DriverEvent::SettingsRefused {
            message: text("message"),
        },
        "text" => DriverEvent::AssistantText { text: text("text") },
        "text_delta" => DriverEvent::TextDelta { text: text("text") },
        "reasoning" => DriverEvent::ReasoningText { text: text("text") },
        "reasoning_delta" => DriverEvent::ReasoningDelta { text: text("text") },
        "tool_started" => DriverEvent::ToolStarted {
            tool_use_id: text("tool_use_id"),
            name: text("name"),
            input: value.get("input").cloned().unwrap_or(Value::Null),
        },
        "tool_result" => DriverEvent::ToolResult {
            tool_use_id: text("tool_use_id"),
            name: text("name"),
            text: text("text"),
            is_error: value
                .get("is_error")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            // The plugin contract's tool result carries words alone.
            images: Vec::new(),
        },
        "approval_requested" => DriverEvent::ApprovalRequested {
            approval: ApprovalRequest {
                harness_ref: text("harness_ref"),
                request: value.get("request").cloned().unwrap_or(Value::Null),
                spec: DecisionSpec {
                    kind: match value.get("kind").and_then(Value::as_str) {
                        Some("question") => DecisionKind::Question,
                        _ => DecisionKind::Approval,
                    },
                    options: value
                        .get("options")
                        .and_then(Value::as_array)
                        .map(|options| {
                            options
                                .iter()
                                .filter_map(|option| {
                                    Some(DecisionOption {
                                        id: option.get("id")?.as_str()?.to_string(),
                                        label: option.get("label")?.as_str()?.to_string(),
                                    })
                                })
                                .collect()
                        })
                        .unwrap_or_default(),
                    tool_name: optional("tool_name"),
                    summary: optional("summary"),
                    interrupts_turn: Vec::new(),
                },
            },
        },
        "turn_completed" => DriverEvent::TurnCompleted {
            subtype: {
                let subtype = text("subtype");
                if subtype.is_empty() {
                    "completed".into()
                } else {
                    subtype
                }
            },
            coarse: match value.get("coarse").and_then(Value::as_str) {
                Some("interrupted") => TurnOutcome::Interrupted,
                // A driver may spell a failure "failed" or "error"; both are
                // a failed turn, never a completed one. An absent or unknown
                // word stays completed only when `is_error` is false, which
                // the check below cannot see; the fallback is the turn's own
                // subtype, read after this match.
                Some("failed") | Some("error") => TurnOutcome::Failed,
                _ => TurnOutcome::Completed,
            },
            is_error: value
                .get("is_error")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            error_message: optional("error_message"),
        },
        "context_used" => DriverEvent::ContextUsed {
            used: value.get("used").and_then(Value::as_u64).unwrap_or(0),
            window: value.get("window").and_then(Value::as_u64),
        },
        "turn_cost" => DriverEvent::TurnCost {
            cost_usd: value.get("cost_usd").and_then(Value::as_f64).unwrap_or(0.0),
        },
        "compacted" => DriverEvent::Compacted,
        "stderr" => DriverEvent::Stderr { line: text("line") },
        "closed" => DriverEvent::StdoutClosed,
        other => return Err(format!("unknown event type '{other}'")),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_turn_maps_one_for_one_onto_the_driver_vocabulary() {
        let DriverEvent::Initialized {
            session_ref,
            model,
            permission_mode,
        } = parse_event(
            r#"{"type":"initialized","session_ref":"c-1","model":"m","permission_mode":null}"#,
        )
        .expect("initialized")
        else {
            panic!("wrong event");
        };
        assert_eq!(session_ref, "c-1");
        assert_eq!(model.as_deref(), Some("m"));
        assert_eq!(permission_mode, None);

        let DriverEvent::AssistantText { text } =
            parse_event(r#"{"type":"text","text":"hello"}"#).expect("text")
        else {
            panic!("wrong event");
        };
        assert_eq!(text, "hello");

        let DriverEvent::ToolStarted {
            tool_use_id,
            name,
            input,
        } = parse_event(
            r#"{"type":"tool_started","tool_use_id":"t1","name":"write_file","input":{"path":"a"}}"#,
        )
        .expect("tool")
        else {
            panic!("wrong event");
        };
        assert_eq!(tool_use_id, "t1");
        assert_eq!(name, "write_file");
        assert_eq!(input["path"], "a");
    }

    #[test]
    fn a_reported_model_and_a_refused_setting_map_to_the_drivers_own_words() {
        assert!(matches!(
            parse_event(r#"{"type":"model_reported","model":"m"}"#),
            Ok(DriverEvent::ModelReported { model }) if model == "m"
        ));
        assert!(matches!(
            parse_event(r#"{"type":"settings_refused","message":"no"}"#),
            Ok(DriverEvent::SettingsRefused { message }) if message == "no"
        ));
    }

    #[test]
    fn an_interrupted_turn_keeps_its_own_coarse_word() {
        let DriverEvent::TurnCompleted {
            subtype,
            coarse,
            is_error,
            error_message,
        } = parse_event(
            r#"{"type":"turn_completed","subtype":"interrupted","coarse":"interrupted","is_error":false}"#,
        )
        .expect("turn")
        else {
            panic!("wrong event");
        };
        assert_eq!(subtype, "interrupted");
        assert_eq!(coarse, TurnOutcome::Interrupted);
        assert!(!is_error);
        assert_eq!(error_message, None);
        // A missing subtype is the ordinary successful end.
        let DriverEvent::TurnCompleted {
            subtype, coarse, ..
        } = parse_event(r#"{"type":"turn_completed","coarse":"completed"}"#).expect("turn")
        else {
            panic!("wrong event");
        };
        assert_eq!(subtype, "completed");
        assert_eq!(coarse, TurnOutcome::Completed);
        // A driver that spells the failure "error" is a failed turn too: an
        // error the console painted as completed would be worse than none.
        let DriverEvent::TurnCompleted { coarse, .. } = parse_event(
            r#"{"type":"turn_completed","subtype":"error","coarse":"error","is_error":true}"#,
        )
        .expect("turn") else {
            panic!("wrong event");
        };
        assert_eq!(coarse, TurnOutcome::Failed);
    }

    #[test]
    fn an_unknown_event_is_a_protocol_error_for_that_line_only() {
        let error = parse_event(r#"{"type":"hologram","text":"x"}"#).expect_err("unknown");
        assert!(error.contains("hologram"), "{error}");
        let error = parse_event("not json").expect_err("malformed");
        assert!(!error.is_empty());
    }

    #[test]
    fn an_approval_carries_its_options_verbatim() {
        let DriverEvent::ApprovalRequested { approval } = parse_event(
            r#"{"type":"approval_requested","harness_ref":"a-1","tool_name":"write_file","summary":"Write a.py","kind":"approval","options":[{"id":"allow","label":"Allow"},{"id":"deny","label":"Deny"}],"request":{"path":"a.py"}}"#,
        )
        .expect("approval")
        else {
            panic!("wrong event");
        };
        assert_eq!(approval.harness_ref, "a-1");
        assert_eq!(approval.spec.tool_name.as_deref(), Some("write_file"));
        assert_eq!(approval.spec.summary.as_deref(), Some("Write a.py"));
        assert_eq!(approval.spec.options.len(), 2);
        assert_eq!(approval.spec.options[1].id, "deny");
        assert_eq!(approval.request["path"], "a.py");
    }
}
