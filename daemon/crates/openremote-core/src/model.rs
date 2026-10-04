use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::ids::{DecisionId, MachineId, RuleId, SessionId};

/// An installed agent CLI the daemon can drive (`claude`, `codex`, …).
#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct Harness {
    /// Harness id, e.g. `claude`.
    pub id: String,
    /// Human name as the harness itself spells it, e.g. `Claude Code`.
    pub name: String,
    /// Resolved executable path, when one was found.
    pub path: Option<String>,
    /// `claude --version` output, when probed.
    pub version: Option<String>,
    pub available: bool,
    /// The harness's own fast mode is usable at this version (claude
    /// `fastMode` headless ≥ 2.1.205, codex service tier `fast` ≥ 0.110.0).
    #[serde(default)]
    pub fast_supported: bool,
    /// Whether this CLI is signed in, in its own words. `None` means this
    /// harness has no status command we can ask - the row stays Ready or
    /// Missing, never a guessed sign-in.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signed_in: Option<bool>,
    /// The model a fresh chat on this harness will run, from the harness's
    /// own config - its words. The wire's first-turn truth replaces it.
    /// `None` when the harness says nothing pre-send (slot reserved).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_model: Option<String>,
    /// The harness's own effort word that rides with `default_model`
    /// (grok's `default_reasoning_effort`), when one was read.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_effort: Option<String>,
}

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Debug)]
#[serde(rename_all = "snake_case")]
pub enum SessionStatus {
    Starting,
    Working,
    Waiting,
    Idle,
    Stopped,
    Failed,
}

impl SessionStatus {
    /// True while a harness process is attached to the session. `idle` is
    /// alive - the process persists between turns; only `stopped`/`failed`
    /// end it (resume spawns a new one).
    pub fn is_alive(self) -> bool {
        matches!(
            self,
            SessionStatus::Starting
                | SessionStatus::Working
                | SessionStatus::Waiting
                | SessionStatus::Idle
        )
    }
}

/// One conversation with one harness in one workspace.
#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct Session {
    pub id: SessionId,
    pub harness: String,
    pub workspace: PathBuf,
    pub status: SessionStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// The harness's own effort word for this chat, when one was chosen.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub permission_mode: Option<String>,
    /// The harness's own conversation id (`session_id` on the wire) - data, never identity.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub harness_session_ref: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
    /// Next turn number to hand out (per-session monotonic).
    #[serde(default)]
    pub next_turn: u64,
    /// The chat runs the harness's own fast mode (claude `fastMode`, codex
    /// service tier `fast`) - its speed tier, not a model.
    #[serde(default)]
    pub fast: bool,
    /// Tools the harness granted for the rest of this session, in its own
    /// vocabulary (codex `acceptForSession`, opencode `always`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub approved_tools: Vec<String>,
}

/// One computer that can run agent chats. The daemon's own machine is
/// always present and online; others wait until their agent checks in
/// (the check-in protocol is the next machines pass).
#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Debug)]
#[serde(rename_all = "snake_case")]
pub enum MachineStatus {
    Online,
    Offline,
    Waiting,
}

/// One computer you own - the console's *machine*.
#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct Machine {
    pub id: MachineId,
    /// Hostname slug, as it shows in the grid.
    pub name: String,
    /// `windows` / `macos` / `linux` - the daemon's own OS for this
    /// machine; the OS picked at add time for a waiting one.
    pub platform: String,
    pub status: MachineStatus,
    /// The daemon's own machine - always online, never removable.
    #[serde(default)]
    pub this_machine: bool,
    /// Enrollment credential for the future check-in (waiting machines).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enrollment_token: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_seen: Option<i64>,
}

/// A harness whose install on this machine is a plain npm package - the
/// command is shown verbatim and run as-is. Harnesses that install through
/// their own roots (Grok, Antigravity) have no row: nothing honest to run.
#[derive(Clone, Serialize, Debug)]
pub struct InstallSpec {
    pub harness_id: String,
    pub name: String,
    /// The command that installs it, shown and run verbatim.
    pub command: String,
}

/// One MCP server a session can call, installed on a machine from the
/// marketplace or written by hand. The launch command is what that machine
/// would start; a key, if the plugin needs one, never leaves the machine -
/// `has_key` only records that the machine has one.
#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct Plugin {
    /// `catalogId@machine` from the marketplace; `custom-…@machine` when
    /// written by hand.
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub catalog_id: Option<String>,
    pub name: String,
    pub detail: String,
    /// The launch command, as that machine would start it.
    pub command: String,
    pub machine: MachineId,
    pub needs_key: bool,
    #[serde(default)]
    pub has_key: bool,
    #[serde(default = "default_true")]
    pub enabled: bool,
    pub created_at: i64,
    pub updated_at: i64,
}

fn default_true() -> bool {
    true
}

/// What makes a rule fire. The kinds with real event sources - the clock
/// and an arriving webhook. The connector-backed kinds (pipeline, errors,
/// review, release) appear when their connectors do.
#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Debug)]
#[serde(rename_all = "snake_case")]
pub enum TriggerKind {
    Schedule,
    Webhook,
}

/// One rule's trigger, in its own words.
#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct Trigger {
    pub kind: TriggerKind,
    /// `HH:MM` local, for schedules.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub time: Option<String>,
    /// The webhook's own key - every hook URL carries one; a request
    /// without the matching key does not fire the rule.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
}

/// One automation: when a trigger fires, a chat opens on the rule's
/// machine with the rule's task. Run-now is the same path by hand.
#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct AutomationRule {
    pub id: RuleId,
    pub name: String,
    pub trigger: Trigger,
    pub harness: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// The chat's workspace - absolute, validated at save time.
    pub workspace: PathBuf,
    pub machine: MachineId,
    pub task: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_chat: Option<SessionId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_run: Option<i64>,
    pub created_at: i64,
    pub updated_at: i64,
}

impl Plugin {
    /// The console's state word, derived like the design rules: a
    /// disabled plugin is off; one still waiting on its key acknowledgment
    /// is `needs-key`; otherwise it rides sessions - running.
    pub fn state(&self) -> &'static str {
        if !self.enabled {
            "off"
        } else if self.needs_key && !self.has_key {
            "needs-key"
        } else {
            "running"
        }
    }

    /// The server id on a harness's own MCP wire.
    pub fn server_id(&self) -> String {
        self.catalog_id
            .clone()
            .unwrap_or_else(|| self.name.to_lowercase().replace([' ', '_'], "-"))
    }
}

/// One machine as the console sees it: the machine plus what is really on
/// it - the harness inventory (the daemon's own probe), the chats running
/// there, the agent's presence over the last 24 hours, and the harnesses
/// that could be installed.
#[derive(Clone, Serialize, Debug)]
pub struct MachineView {
    pub machine: Machine,
    pub harnesses: Vec<Harness>,
    pub sessions: Vec<Session>,
    /// 48 half-hour slices over the last 24h, oldest first; true = the
    /// agent was present (the daemon runs, or the check-in arrived).
    pub presence: Vec<bool>,
    pub installable: Vec<InstallSpec>,
    /// The waiting machine's install command, per its OS - the enrollment
    /// credential rides beside it (the check-in consumes both).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub install_command: Option<String>,
}

/// Coarse turn outcome for grid-at-a-glance; the harness's own string rides
/// beside it verbatim in the `turn.completed` event.
#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Debug)]
#[serde(rename_all = "snake_case")]
pub enum TurnOutcome {
    Completed,
    Interrupted,
    Failed,
}

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Debug)]
#[serde(rename_all = "snake_case")]
pub enum MessageRole {
    User,
    Assistant,
}

/// A transcript message. Tool executions are their own events, not messages.
#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct ChatMessage {
    pub id: String,
    pub turn: u64,
    pub role: MessageRole,
    pub text: String,
}

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Debug)]
#[serde(rename_all = "snake_case")]
pub enum DecisionKind {
    Approval,
    Question,
}

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Debug)]
#[serde(rename_all = "snake_case")]
pub enum DecisionState {
    Pending,
    Answered,
    /// The turn or session died with it unanswered. Nothing is emitted when a
    /// decision retires - the console treats unresolved `decision.requested`
    /// on non-alive sessions as history.
    Retired,
}

/// One answer choice the console renders, with the harness's own words.
#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct DecisionOption {
    pub id: String,
    pub label: String,
}

/// One concept, two kinds: approvals and questions both block the harness.
#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct Decision {
    pub id: DecisionId,
    pub session_id: SessionId,
    pub turn: u64,
    pub kind: DecisionKind,
    pub state: DecisionState,
    /// The harness-native request, verbatim (e.g. claude `can_use_tool` params).
    pub harness_request: serde_json::Value,
    /// The harness's own correlation id for the request (claude control
    /// `request_id`) - what an answer is routed with.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub harness_ref: Option<String>,
    /// The tool the harness asked about, in its own vocabulary
    /// (`Bash`, `commandExecution`, opencode's action word, …).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_name: Option<String>,
    /// Answer choices rendered with the harness's own vocabulary.
    pub options: Vec<DecisionOption>,
    pub created_at: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub answer: Option<DecisionAnswer>,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct DecisionAnswer {
    pub choice: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_input: Option<serde_json::Value>,
    pub answered_at: i64,
}

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Debug)]
#[serde(rename_all = "snake_case")]
pub enum ReceiptStatus {
    /// Accepted and queued; the outcome isn't known yet (a crash in this
    /// window surfaces as `unknown` - uncertain work is never resent).
    Accepted,
    Completed,
    Failed,
}

/// Dedup record for one mutating request, keyed by the client's `request_id`.
#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct Receipt {
    pub request_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<SessionId>,
    pub status: ReceiptStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    pub updated_at: i64,
}
