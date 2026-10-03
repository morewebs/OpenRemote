//! What a harness process reports, in vocabulary the supervisor
//! understands. Nothing here knows about `seq` or the store — the
//! supervisor translates.

use openremote_core::{DecisionKind, DecisionOption};
use serde_json::Value;

/// What the harness asked the operator, plus how the console renders it.
/// `harness_ref` is the harness's own correlation id — the answer routes
/// back through the driver with it. `request` rides verbatim.
#[derive(Clone, Debug)]
pub struct ApprovalRequest {
    pub harness_ref: String,
    pub request: Value,
    pub spec: DecisionSpec,
}

/// The rendering spec for one harness ask: one concept (approval |
/// question), options in the harness's own words.
#[derive(Clone, Debug)]
pub struct DecisionSpec {
    pub kind: DecisionKind,
    pub options: Vec<DecisionOption>,
    pub tool_name: Option<String>,
    /// A one-line human summary (the command, the question).
    pub summary: Option<String>,
    /// Choice ids that interrupt the turn instead of continuing it (codex
    /// `cancel`; claude has none) — the supervisor skips the post-answer
    /// working transition for these.
    pub interrupts_turn: Vec<String>,
}

/// What a harness process told us. The pump turns these into sequenced
/// events; the turn boundary is always explicit — never guess turn state
/// from silence.
#[derive(Debug, Clone)]
pub enum DriverEvent {
    /// The harness is up and named its own conversation (claude
    /// `system/init`, codex `thread/start` result, pi `get_state`).
    Initialized {
        session_ref: String,
        model: Option<String>,
        permission_mode: Option<String>,
    },
    AssistantText {
        text: String,
    },
    TextDelta {
        text: String,
    },
    ToolStarted {
        tool_use_id: String,
        name: String,
        input: Value,
    },
    ToolResult {
        tool_use_id: String,
        name: String,
        text: String,
        is_error: bool,
    },
    ApprovalRequested {
        approval: ApprovalRequest,
    },
    /// The turn boundary. `subtype` is the harness's own outcome string
    /// verbatim (claude `result.subtype`, codex `turn.status`, …); `coarse`
    /// is the grid-facing enum. `error_message` is the harness's own
    /// failure explanation when the turn failed (codex `turn.error.message`).
    TurnCompleted {
        subtype: String,
        coarse: openremote_core::TurnOutcome,
        is_error: bool,
        error_message: Option<String>,
    },
    /// The harness reported the conversation's context size — its own
    /// numbers (claude's result usage, codex `thread/tokenUsage/updated`).
    /// `window` is only set where the harness reports it (codex's
    /// `modelContextWindow`; claude's stream-json has no window field).
    ContextUsed {
        used: u64,
        window: Option<u64>,
    },
    /// The harness reported the turn's own cost in USD — claude's
    /// `total_cost_usd` on the result frame, verbatim. None where the
    /// harness doesn't report money.
    TurnCost {
        cost_usd: f64,
    },
    /// The harness compacted the conversation (claude's own
    /// `compact_boundary` marker, codex `thread/compacted`).
    Compacted,
    /// Harness stderr, line by line — diagnostics ride with everything.
    Stderr {
        line: String,
    },
    /// The session's process is gone (print-mode harnesses omit this per
    /// prompt; it only fires when the session itself is done).
    StdoutClosed,
}
