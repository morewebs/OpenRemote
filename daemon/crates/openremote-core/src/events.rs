use serde::{Deserialize, Serialize};

use crate::ids::DecisionId;
use crate::model::{ChatMessage, Decision, Session, SessionStatus, TurnOutcome};

/// One append-only session event. The console renders only from these.
#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct Event {
    /// Per-session monotonic sequence number.
    pub seq: u64,
    pub session_id: crate::ids::SessionId,
    #[serde(flatten)]
    pub payload: EventPayload,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
#[serde(tag = "type")]
pub enum EventPayload {
    #[serde(rename = "session.created")]
    SessionCreated { session: Session },
    /// Session facts changed outside the status lifecycle — session-scoped
    /// tool grants landing. The whole session rides along; the console
    /// folds the fields it renders.
    #[serde(rename = "session.updated")]
    SessionUpdated { session: Session },
    #[serde(rename = "session.status_changed")]
    SessionStatusChanged {
        status: SessionStatus,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
    },
    #[serde(rename = "turn.started")]
    TurnStarted { turn: u64 },
    #[serde(rename = "turn.completed")]
    TurnCompleted {
        turn: u64,
        /// The harness's own outcome string, verbatim (`success`, `error_during_execution`, …).
        outcome: String,
        coarse: TurnOutcome,
    },
    #[serde(rename = "message.added")]
    MessageAdded { message: ChatMessage },
    #[serde(rename = "message.delta")]
    MessageDelta { turn: u64, text: String },
    #[serde(rename = "tool.started")]
    ToolStarted {
        turn: u64,
        name: String,
        input: serde_json::Value,
    },
    #[serde(rename = "tool.result")]
    ToolResult {
        turn: u64,
        name: String,
        output: serde_json::Value,
        is_error: bool,
    },
    /// The harness reported the conversation's context size — its own
    /// numbers, never ours. The window only rides where the harness
    /// reports one.
    #[serde(rename = "context.used")]
    ContextUsed {
        used: u64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        window: Option<u64>,
    },
    /// The harness reported the turn's cost in USD — its own number,
    /// verbatim. Only where the harness reports money.
    #[serde(rename = "usage.cost")]
    UsageCost { cost_usd: f64 },
    /// An informational line in the transcript (compaction notices) — the
    /// console renders it like its other notes.
    #[serde(rename = "note.added")]
    NoteAdded { text: String },
    #[serde(rename = "decision.requested")]
    DecisionRequested { decision: Decision },
    #[serde(rename = "decision.responded")]
    DecisionResponded {
        decision_id: DecisionId,
        choice: String,
    },
    #[serde(rename = "daemon.error")]
    DaemonError { message: String },
}

impl Event {
    /// The event's own name as it appears on the wire (`session.created`).
    pub fn kind(&self) -> &'static str {
        match &self.payload {
            EventPayload::SessionCreated { .. } => "session.created",
            EventPayload::SessionUpdated { .. } => "session.updated",
            EventPayload::SessionStatusChanged { .. } => "session.status_changed",
            EventPayload::TurnStarted { .. } => "turn.started",
            EventPayload::TurnCompleted { .. } => "turn.completed",
            EventPayload::MessageAdded { .. } => "message.added",
            EventPayload::MessageDelta { .. } => "message.delta",
            EventPayload::ToolStarted { .. } => "tool.started",
            EventPayload::ToolResult { .. } => "tool.result",
            EventPayload::ContextUsed { .. } => "context.used",
            EventPayload::UsageCost { .. } => "usage.cost",
            EventPayload::NoteAdded { .. } => "note.added",
            EventPayload::DecisionRequested { .. } => "decision.requested",
            EventPayload::DecisionResponded { .. } => "decision.responded",
            EventPayload::DaemonError { .. } => "daemon.error",
        }
    }
}
