//! The process behind a plugin-provided harness: one child per chat, JSON
//! lines over its stdin and stdout, the daemon's own driver vocabulary on both
//! sides. The plugin owns everything harness-specific (its CLI, its PTY, its
//! transcript); this module only speaks the seam.
//!
//! A driver's stdout carries one event per line and nothing else. Its stderr
//! is diagnostics, forwarded to the console as they arrive, so a plugin author
//! can debug a driver without a debugger attached to the daemon.

use std::collections::HashMap;
use std::path::Path;
use std::process::Stdio;
use std::sync::{Arc, Mutex as StdMutex};
use std::time::Duration;

use openremote_core::{DecisionKind, DecisionOption, TurnOutcome};
use openremote_harness::{
    AgentDescriptor, AnswerOutcome, ApprovalRequest, CommandCatalog, CommandDescriptor,
    DecisionSpec, DriverError, DriverEvent, SessionSettings, SpawnOptions,
};
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, Command};
use tokio::sync::mpsc;

use crate::plugins::{HarnessSpec, HarnessTraits};

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

/// How long a driver's `--commands` may take before the composer's menu
/// reads as failed. It is asked when a menu opens, so it stays short.
const COMMANDS_DEADLINE: Duration = Duration::from_secs(20);

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

/// Run `<driver> --commands <folder>` and read the composer's lists.
///
/// `Ok(None)` is a harness whose manifest does not say `"commands": true`: no
/// process starts, and its own commands still run when typed. A driver that
/// says it answers but fails is an error, so the composer keeps an earlier
/// list and asks again next time.
pub async fn commands(
    spec: &HarnessSpec,
    cwd: &Path,
) -> Result<Option<CommandCatalog>, DriverError> {
    if !spec.traits.commands {
        return Ok(None);
    }
    if let Some(problem) = spec.problem.clone() {
        return Err(DriverError::Harness(problem));
    }
    let mut command = Command::new(&spec.driver);
    command
        .args(&spec.args)
        .arg("--commands")
        .arg(cwd)
        .current_dir(&spec.dir)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    #[cfg(windows)]
    command.creation_flags(openremote_harness::creation_flags());
    let output = match tokio::time::timeout(COMMANDS_DEADLINE, command.output()).await {
        Ok(Ok(output)) => output,
        Ok(Err(err)) => {
            return Err(DriverError::Harness(format!(
                "the driver did not start: {} ({err})",
                spec.driver.display()
            )));
        }
        Err(_) => {
            return Err(DriverError::Harness(format!(
                "the driver did not list its commands within {} seconds",
                COMMANDS_DEADLINE.as_secs()
            )));
        }
    };
    if !output.status.success() {
        return Err(DriverError::Harness(format!(
            "the driver exited with {} while listing its commands",
            output.status
        )));
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let Some(line) = text.lines().map(str::trim).find(|line| !line.is_empty()) else {
        return Err(DriverError::Harness(
            "the driver printed no commands".into(),
        ));
    };
    parse_commands(line).map(Some).map_err(DriverError::Harness)
}

/// The `--commands` answer: `commands` and `agents`, each entry needing a
/// name. A `reason` sentence is the driver saying it could not list them.
pub fn parse_commands(line: &str) -> Result<CommandCatalog, String> {
    let value: Value = serde_json::from_str(line)
        .map_err(|err| format!("the driver's commands answer is not JSON: {err}"))?;
    if let Some(reason) = optional_text(&value, "reason") {
        return Err(reason);
    }
    let entries = |key: &str| {
        value
            .get(key)
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default()
    };
    Ok(CommandCatalog {
        commands: entries("commands")
            .iter()
            .filter_map(|entry| {
                Some(CommandDescriptor {
                    name: optional_text(entry, "name")?,
                    description: optional_text(entry, "description"),
                    argument_hint: optional_text(entry, "argument_hint"),
                    aliases: words(entry, "aliases"),
                })
            })
            .collect(),
        agents: entries("agents")
            .iter()
            .filter_map(|entry| {
                Some(AgentDescriptor {
                    name: optional_text(entry, "name")?,
                    description: optional_text(entry, "description"),
                })
            })
            .collect(),
    })
}

/// A non-empty string field, or `None`.
fn optional_text(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(String::from)
}

/// An array of strings, skipping anything that is not one.
fn words(value: &Value, key: &str) -> Vec<String> {
    value
        .get(key)
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default()
}

/// What one of the driver's asks says about its own choices: which ones end
/// the turn, and which grant the tool for the rest of the chat. Kept by the
/// pump until the answer is written, because the answer's outcome is the
/// daemon's to decide before the driver's follow-up events arrive.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AskTerms {
    pub interrupts_turn: Vec<String>,
    pub session_grant: Vec<String>,
}

impl AskTerms {
    /// What answering with `choice` means, in the supervisor's terms.
    pub fn outcome(&self, choice: &str, tool_name: Option<&str>) -> AnswerOutcome {
        AnswerOutcome {
            interrupts_turn: self.interrupts_turn.iter().any(|id| id == choice),
            session_grant: self
                .session_grant
                .iter()
                .any(|id| id == choice)
                .then(|| tool_name.unwrap_or("tool").to_string()),
        }
    }
}

/// The asks a chat's driver has open, by the driver's own reference.
type OpenAsks = Arc<StdMutex<HashMap<String, AskTerms>>>;

/// The spawn command, as the driver reads it. Public for the tests: the wire
/// is a contract.
///
/// The chat's MCP servers can carry the user's own commands and keys, and
/// OpenRemote's per-chat server drives the browser, the team and other chats,
/// so a driver gets only what its plugin declared and the user approved:
/// - no `mcp` in the manifest: both lists are empty;
/// - `mcp`: the user's MCP servers, and tools pre-approved for them;
/// - `mcp` and the `agent_tools` power: OpenRemote's own server too, and its
///   pre-approved browser tools;
/// - plus the `read_chats` power: its pre-approved tools that read other chats.
pub fn spawn_line(opts: &SpawnOptions, traits: &HarnessTraits) -> Value {
    let (mcp_servers, allowed_tools) = handed_mcp(opts, traits);
    json!({
        "type": "spawn",
        "cwd": opts.cwd.display().to_string(),
        "model": opts.model,
        "permission_mode": opts.permission_mode,
        "effort": opts.effort,
        "resume": opts.resume,
        "fast": opts.fast,
        // The chat's MCP servers this driver may hand to its CLI.
        "mcp_servers": mcp_servers
            .iter()
            .map(|server| json!({
                "id": server.id,
                "command": server.command,
                "args": server.args,
            }))
            .collect::<Vec<_>>(),
        // Tools that run without asking, in the MCP servers' own names.
        "allowed_tools": allowed_tools,
    })
}

/// The id of OpenRemote's own per-chat MCP server.
const OPENREMOTE_SERVER: &str = "openremote";

/// The pre-approved tools on OpenRemote's own server that read other chats.
const CHAT_READING_PREFIX: &str = "mcp__openremote__agents_";

/// The servers and pre-approved tools a driver with these traits is handed.
fn handed_mcp<'a>(
    opts: &'a SpawnOptions,
    traits: &HarnessTraits,
) -> (Vec<&'a openremote_harness::McpServer>, Vec<&'a str>) {
    if !traits.takes_mcp() {
        return (Vec::new(), Vec::new());
    }
    let own_prefix = format!("mcp__{OPENREMOTE_SERVER}__");
    let servers = opts
        .mcp_servers
        .iter()
        .filter(|server| server.id != OPENREMOTE_SERVER || traits.takes_openremote_tools())
        .collect();
    let tools = opts
        .allowed_tools
        .iter()
        .map(String::as_str)
        .filter(|tool| {
            if tool.starts_with(CHAT_READING_PREFIX) {
                traits.takes_chat_reading_tools()
            } else if tool.starts_with(&own_prefix) {
                traits.takes_openremote_tools()
            } else {
                true
            }
        })
        .collect();
    (servers, tools)
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
    /// The manifest's `mcp`: the driver hands the chat's MCP servers on.
    mcp: bool,
    /// The manifest's prompt rules, read by the supervisor's prompt gate.
    one_prompt_at_a_time: bool,
    steers_mid_turn: bool,
    /// The driver's open asks, filled by the pump as each one arrives.
    asks: OpenAsks,
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
        let asks = OpenAsks::default();
        tokio::spawn(pump_stdout(stdout, tx.clone(), Arc::clone(&asks)));
        tokio::spawn(pump_stderr(stderr, tx));
        let mut driver = Self {
            child,
            stdin,
            settings_live: spec.settings_live,
            // The team tools and the prompt note need OpenRemote's own
            // server, which only an approved `agent_tools` plugin is handed.
            mcp: spec.traits.takes_openremote_tools(),
            one_prompt_at_a_time: spec.traits.one_prompt_at_a_time,
            steers_mid_turn: spec.traits.steers_mid_turn,
            asks,
        };
        driver.write(&spawn_line(&opts, &spec.traits)).await?;
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
    /// in the command shape the contract documents. What the choice means for
    /// the turn and the chat is what the ask itself declared
    /// (`interrupts_turn`, `session_grant`); an ask that declared neither
    /// continues the turn and grants nothing.
    pub async fn answer(
        &mut self,
        harness_ref: &str,
        choice: &str,
        _request: &Value,
        tool_name: Option<&str>,
    ) -> Result<AnswerOutcome, DriverError> {
        self.write(&json!({
            "type": "answer",
            "harness_ref": harness_ref,
            "choice": choice,
        }))
        .await?;
        let terms = self
            .asks
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(harness_ref)
            .unwrap_or_default();
        Ok(terms.outcome(choice, tool_name))
    }

    /// Whether the chat's MCP servers reach the agent through this driver.
    pub fn rides_mcp(&self) -> bool {
        self.mcp
    }

    /// Whether a second message waits for the running turn.
    pub fn per_prompt_process(&self) -> bool {
        self.one_prompt_at_a_time
    }

    /// Whether a message sent mid-turn lands inside that turn.
    pub fn steers_mid_turn(&self) -> bool {
        self.steers_mid_turn
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

async fn pump_stdout(
    stdout: tokio::process::ChildStdout,
    tx: mpsc::Sender<DriverEvent>,
    asks: OpenAsks,
) {
    let mut lines = BufReader::new(stdout).lines();
    loop {
        match lines.next_line().await {
            Ok(Some(line)) => {
                let line = line.trim();
                if line.is_empty() {
                    continue;
                }
                let event = match parse_line(line) {
                    Ok((event, terms)) => {
                        // Kept before the ask is sent on, so an answer can
                        // never be written before its terms are known.
                        if let (DriverEvent::ApprovalRequested { approval }, Some(terms)) =
                            (&event, terms)
                        {
                            asks.lock()
                                .unwrap_or_else(std::sync::PoisonError::into_inner)
                                .insert(approval.harness_ref.clone(), terms);
                        }
                        event
                    }
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
    parse_line(line).map(|(event, _)| event)
}

/// One protocol line to its event, plus an ask's terms when the line is one.
pub fn parse_line(line: &str) -> Result<(DriverEvent, Option<AskTerms>), String> {
    let value: Value = serde_json::from_str(line).map_err(|err| err.to_string())?;
    let event = event_from(&value)?;
    let terms = matches!(event, DriverEvent::ApprovalRequested { .. }).then(|| AskTerms {
        interrupts_turn: words(&value, "interrupts_turn"),
        session_grant: words(&value, "session_grant"),
    });
    Ok((event, terms))
}

fn event_from(value: &Value) -> Result<DriverEvent, String> {
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
        // The command names the harness listed as the chat started: a name
        // the composer's cached list lacks makes the next menu ask again.
        "commands_listed" => DriverEvent::CommandsListed {
            names: words(value, "names"),
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
                                    let id = optional_text(option, "id")?;
                                    // A choice with no label reads as its id.
                                    let label = optional_text(option, "label")
                                        .unwrap_or_else(|| id.clone());
                                    Some(DecisionOption { id, label })
                                })
                                .collect()
                        })
                        .unwrap_or_default(),
                    tool_name: optional("tool_name"),
                    summary: optional("summary"),
                    interrupts_turn: words(value, "interrupts_turn"),
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

    #[test]
    fn an_ask_names_the_choices_that_end_the_turn_and_grant_the_tool() {
        let (event, terms) = parse_line(
            r#"{"type":"approval_requested","harness_ref":"a-2","tool_name":"shell","options":[{"id":"once","label":"Allow once"},{"id":"always"},{"id":"cancel","label":"Stop"}],"interrupts_turn":["cancel"],"session_grant":["always"]}"#,
        )
        .expect("approval");
        let DriverEvent::ApprovalRequested { approval } = event else {
            panic!("wrong event");
        };
        // A choice with no label reads as its id, rather than vanishing.
        assert_eq!(approval.spec.options.len(), 3);
        assert_eq!(approval.spec.options[1].label, "always");
        assert_eq!(approval.spec.interrupts_turn, vec!["cancel".to_string()]);
        let terms = terms.expect("an ask carries its terms");
        assert_eq!(
            terms.outcome("always", Some("shell")),
            AnswerOutcome {
                interrupts_turn: false,
                session_grant: Some("shell".into()),
            }
        );
        assert_eq!(
            terms.outcome("cancel", Some("shell")),
            AnswerOutcome {
                interrupts_turn: true,
                session_grant: None,
            }
        );
        assert_eq!(
            terms.outcome("once", Some("shell")),
            AnswerOutcome::default()
        );
        // An ask that declares nothing continues the turn and grants nothing,
        // the old answer.
        assert_eq!(
            AskTerms::default().outcome("anything", None),
            AnswerOutcome::default()
        );
        // Only an ask carries terms.
        let (_, terms) = parse_line(r#"{"type":"text","text":"x"}"#).expect("text");
        assert_eq!(terms, None);
    }

    #[test]
    fn a_listed_command_set_maps_to_the_drivers_names() {
        let DriverEvent::CommandsListed { names } =
            parse_event(r#"{"type":"commands_listed","names":["review","ship",3]}"#)
                .expect("commands")
        else {
            panic!("wrong event");
        };
        assert_eq!(names, vec!["review".to_string(), "ship".to_string()]);
    }

    #[test]
    fn the_spawn_line_hands_mcp_only_as_far_as_the_manifest_and_powers_allow() {
        use openremote_core::PluginPower;
        let opts = SpawnOptions {
            cwd: std::path::PathBuf::from("work"),
            model: Some("m".into()),
            endpoint: None,
            permission_mode: Some("ask".into()),
            effort: None,
            resume: None,
            include_deltas: false,
            fast: true,
            mcp_servers: vec![
                openremote_harness::McpServer {
                    id: "github".into(),
                    command: "npx".into(),
                    args: vec!["-y".into(), "server-github".into()],
                },
                openremote_harness::McpServer {
                    id: "openremote".into(),
                    command: "openremote".into(),
                    args: vec!["mcp-chat".into(), "--session".into(), "s-1".into()],
                },
            ],
            plugin_dirs: Vec::new(),
            allowed_tools: vec![
                "mcp__openremote__browser_snapshot".into(),
                "mcp__openremote__agents_read".into(),
                "mcp__github__list_issues".into(),
            ],
            state_dir: None,
        };
        let ids = |line: &Value| -> Vec<String> {
            line["mcp_servers"]
                .as_array()
                .expect("a list")
                .iter()
                .map(|server| server["id"].as_str().unwrap_or_default().to_string())
                .collect()
        };
        let tools = |line: &Value| -> Vec<String> {
            line["allowed_tools"]
                .as_array()
                .expect("a list")
                .iter()
                .map(|tool| tool.as_str().unwrap_or_default().to_string())
                .collect()
        };

        // No `mcp`: nothing, whatever the powers. Empty lists, never absent.
        let silent = HarnessTraits {
            powers: vec![PluginPower::AgentTools, PluginPower::ReadChats],
            ..HarnessTraits::default()
        };
        let line = spawn_line(&opts, &silent);
        assert_eq!(line["type"], "spawn");
        assert_eq!(line["model"], "m");
        assert_eq!(line["permission_mode"], "ask");
        assert_eq!(line["fast"], true);
        assert_eq!(line["mcp_servers"], json!([]));
        assert_eq!(line["allowed_tools"], json!([]));

        // `mcp` without powers: the user's servers, not OpenRemote's own.
        let mcp_only = HarnessTraits {
            mcp: true,
            ..HarnessTraits::default()
        };
        let line = spawn_line(&opts, &mcp_only);
        assert_eq!(ids(&line), vec!["github"]);
        assert_eq!(line["mcp_servers"][0]["command"], "npx");
        assert_eq!(tools(&line), vec!["mcp__github__list_issues"]);

        // `agent_tools`: OpenRemote's server and its browser tools, but not
        // the tools that read other chats.
        let agent = HarnessTraits {
            mcp: true,
            powers: vec![PluginPower::AgentTools],
            ..HarnessTraits::default()
        };
        let line = spawn_line(&opts, &agent);
        assert_eq!(ids(&line), vec!["github", "openremote"]);
        assert_eq!(line["mcp_servers"][1]["args"][2], "s-1");
        assert_eq!(
            tools(&line),
            vec![
                "mcp__openremote__browser_snapshot",
                "mcp__github__list_issues"
            ]
        );

        // `read_chats` alone does not bring OpenRemote's server.
        let reader = HarnessTraits {
            mcp: true,
            powers: vec![PluginPower::ReadChats],
            ..HarnessTraits::default()
        };
        assert_eq!(ids(&spawn_line(&opts, &reader)), vec!["github"]);

        // Both powers: everything the chat has.
        let both = HarnessTraits {
            mcp: true,
            powers: vec![PluginPower::AgentTools, PluginPower::ReadChats],
            ..HarnessTraits::default()
        };
        let line = spawn_line(&opts, &both);
        assert_eq!(ids(&line), vec!["github", "openremote"]);
        assert_eq!(tools(&line).len(), 3);
    }

    #[test]
    fn a_commands_answer_keeps_named_entries_and_a_reason_is_a_failure() {
        let catalog = parse_commands(
            r#"{"commands":[{"name":"review","description":"Review the diff","argument_hint":"<path>","aliases":["r"]},{"description":"no name"}],"agents":[{"name":"planner"},{"name":""}]}"#,
        )
        .expect("a catalog");
        assert_eq!(
            catalog.commands.len(),
            1,
            "an entry with no name is skipped"
        );
        assert_eq!(catalog.commands[0].name, "review");
        assert_eq!(catalog.commands[0].argument_hint.as_deref(), Some("<path>"));
        assert_eq!(catalog.commands[0].aliases, vec!["r".to_string()]);
        assert_eq!(catalog.agents.len(), 1);
        assert_eq!(catalog.agents[0].name, "planner");
        assert_eq!(catalog.agents[0].description, None);

        let empty = parse_commands("{}").expect("nothing to list is a list");
        assert!(empty.commands.is_empty() && empty.agents.is_empty());

        let error = parse_commands(r#"{"reason":"not signed in"}"#).expect_err("a reason");
        assert_eq!(error, "not signed in");
        assert!(parse_commands("not json").is_err());
    }
}
