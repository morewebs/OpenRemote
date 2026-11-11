use serde::{Deserialize, Serialize};

use crate::ids::DecisionId;
use crate::model::{
    ChatMessage, Decision, ImageRef, QueuedMessage, Session, SessionStatus, TurnOutcome,
};

/// One append-only session event. The console renders only from these.
#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct Event {
    /// Per-session monotonic sequence number.
    pub seq: u64,
    pub session_id: crate::ids::SessionId,
    /// When it happened (ms), so every device dates a chat by its history,
    /// not by when the event reached it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub at: Option<i64>,
    #[serde(flatten)]
    pub payload: EventPayload,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
#[serde(tag = "type")]
pub enum EventPayload {
    #[serde(rename = "session.created")]
    SessionCreated { session: Session },
    /// Session facts changed outside the status lifecycle - session-scoped
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
    /// The harness's own reasoning for the turn, when it shares it. Dim
    /// and collapsible in the console; never mixed into the reply.
    #[serde(rename = "reasoning.added")]
    ReasoningAdded { turn: u64, text: String },
    /// A streaming fragment of the reasoning - the live-thinking shape.
    #[serde(rename = "reasoning.delta")]
    ReasoningDelta { turn: u64, text: String },
    /// The turn's thinking-token count - the harness's own number
    /// (claude's `output_tokens_details.thinking_tokens`), with the turn it
    /// belongs to. A re-reported turn under a new seq is then the same turn,
    /// so a reader can replace its tally instead of adding to it. The first
    /// turn is 0, so 0 is a real turn: "not said" is its own fact, `None`, and
    /// a reader must tell the two apart or the first turn's tally is added
    /// again on every report.
    #[serde(rename = "thinking.tokens")]
    ThinkingTokens {
        tokens: u64,
        #[serde(default)]
        turn: Option<u64>,
    },
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
        /// Pictures the tool answered with (an agent's screenshot of a
        /// page), kept in the blob store. Results written before pictures
        /// were kept have none.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        images: Vec<ImageRef>,
    },
    /// The harness reported the conversation's context size - its own
    /// numbers, never ours. The window only rides where the harness
    /// reports one.
    #[serde(rename = "context.used")]
    ContextUsed {
        used: u64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        window: Option<u64>,
    },
    /// The harness reported the turn's cost in USD - its own number,
    /// verbatim, with the turn it belongs to for the same reason as
    /// `thinking.tokens`: a repeat is the same turn, not another turn's money.
    /// The first turn is 0, so `Some(0)` is a real turn and `None` is the
    /// older daemon that did not say; a reader must not treat 0 as "unknown".
    #[serde(rename = "usage.cost")]
    UsageCost {
        cost_usd: f64,
        #[serde(default)]
        turn: Option<u64>,
    },
    /// An informational line in the transcript (compaction notices) - the
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
    /// The chat's queue as it now stands - the whole list and whether it
    /// is held. The only writer of the session's queue fields; a device
    /// that predates the queue keeps the event and skips it.
    #[serde(rename = "queue.changed")]
    QueueChanged {
        queue: Vec<QueuedMessage>,
        paused: bool,
    },
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
            EventPayload::ReasoningAdded { .. } => "reasoning.added",
            EventPayload::ReasoningDelta { .. } => "reasoning.delta",
            EventPayload::ThinkingTokens { .. } => "thinking.tokens",
            EventPayload::NoteAdded { .. } => "note.added",
            EventPayload::DecisionRequested { .. } => "decision.requested",
            EventPayload::DecisionResponded { .. } => "decision.responded",
            EventPayload::DaemonError { .. } => "daemon.error",
            EventPayload::QueueChanged { .. } => "queue.changed",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn round_trip(payload: &EventPayload) -> EventPayload {
        let wire = serde_json::to_value(payload).expect("a payload serializes");
        serde_json::from_value(wire).expect("a payload round-trips")
    }

    fn turn_of(payload: &EventPayload) -> Option<u64> {
        match payload {
            EventPayload::ThinkingTokens { turn, .. } | EventPayload::UsageCost { turn, .. } => {
                *turn
            }
            other => panic!("expected a usage payload, got {other:?}"),
        }
    }

    #[test]
    fn usage_events_carry_the_turn_so_a_repeat_replaces_its_tally() {
        // The console used to add every usage event it saw. The turn is what
        // tells a repeated report (same turn, a later seq) from a new turn's
        // report, so a reader can replace instead of add.
        let first = EventPayload::ThinkingTokens {
            tokens: 10,
            turn: Some(3),
        };
        let repeat = EventPayload::ThinkingTokens {
            tokens: 10,
            turn: Some(3),
        };
        let next = EventPayload::ThinkingTokens {
            tokens: 10,
            turn: Some(4),
        };
        assert_eq!(
            turn_of(&round_trip(&first)),
            turn_of(&round_trip(&repeat)),
            "the same turn survives the wire even though the events differ in seq"
        );
        assert_ne!(
            turn_of(&round_trip(&first)),
            turn_of(&round_trip(&next)),
            "a different turn cannot be mistaken for the same one"
        );
    }

    #[test]
    fn a_usage_event_from_an_older_daemon_decodes_without_a_turn() {
        // Absent means "an older daemon did not say": `None`, never 0, because
        // the first turn is 0 and is a real turn a reader must be able to
        // replace. The two must stay distinguishable on the wire.
        let thinking: EventPayload = serde_json::from_value(serde_json::json!({
            "type": "thinking.tokens",
            "tokens": 7,
        }))
        .expect("an old thinking event still decodes");
        assert_eq!(turn_of(&thinking), None);
        let cost: EventPayload = serde_json::from_value(serde_json::json!({
            "type": "usage.cost",
            "cost_usd": 1.5,
        }))
        .expect("an old cost event still decodes");
        assert_eq!(turn_of(&cost), None);

        // A stamped first turn arrives as `Some(0)`, and the round trip keeps
        // it distinct from the old daemon's `None`.
        let first_turn = EventPayload::UsageCost {
            cost_usd: 1.5,
            turn: Some(0),
        };
        assert_eq!(turn_of(&round_trip(&first_turn)), Some(0));
        assert_ne!(turn_of(&round_trip(&first_turn)), turn_of(&thinking));
    }
}
