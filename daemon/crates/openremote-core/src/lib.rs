//! OpenRemote core domain: sessions, turns, events, decisions, receipts.
//!
//! The daemon-neutral vocabulary (the harness payload rides inside verbatim;
//! parity is about the options the user sees, not our data model):
//!
//! - **Harness** — an installed agent CLI the daemon can drive.
//! - **Session** — one conversation with one harness in one workspace.
//!   Daemon-generated UUID identity; the harness's own id is *data*
//!   (`harness_session_ref`), never identity. The console's UI word is *chat*.
//! - **Turn** — one prompt→result cycle; per-session monotonic.
//! - **Event** — append-only, per-session, monotonic `seq`; the console renders
//!   only from events.
//! - **Decision** — one concept, two kinds (approval | question). The daemon
//!   never invents resolution events: retiring emits nothing; late answers
//!   get an explicit error.
//! - **Receipt** — request dedup for every mutating call. Crash between
//!   `accepted` and a terminal state is surfaced as `unknown`; uncertain work
//!   is never resent.

pub mod events;
pub mod ids;
pub mod model;
pub mod store;

pub use events::{Event, EventPayload};
pub use ids::{DecisionId, MachineId, SessionId};
pub use model::{
    ChatMessage, Decision, DecisionKind, DecisionOption, DecisionState, Harness, InstallSpec,
    Machine, MachineStatus, MachineView, MessageRole, Receipt, ReceiptStatus, Session,
    SessionStatus, TurnOutcome,
};
pub use store::Store;

/// Millis since the Unix epoch — the crate's clock surface, so tests can pin time.
pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}
