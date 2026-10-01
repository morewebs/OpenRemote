use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::ids::{DecisionId, SessionId};

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
    /// alive — the process persists between turns; only `stopped`/`failed`
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub permission_mode: Option<String>,
    /// The harness's own conversation id (`session_id` on the wire) — data, never identity.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub harness_session_ref: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
    /// Next turn number to hand out (per-session monotonic).
    #[serde(default)]
    pub next_turn: u64,
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
    /// decision retires — the console treats unresolved `decision.requested`
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
    /// `request_id`) — what an answer is routed with.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub harness_ref: Option<String>,
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
    /// window surfaces as `unknown` — uncertain work is never resent).
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
