//! OpenRemote core domain: sessions, turns, events, decisions, receipts.
//!
//! The daemon-neutral vocabulary (the harness payload rides inside verbatim;
//! parity is about the options the user sees, not our data model):
//!
//! - **Harness** - an installed agent CLI the daemon can drive.
//! - **Session** - one conversation with one harness in one workspace.
//!   Daemon-generated UUID identity; the harness's own id is *data*
//!   (`harness_session_ref`), never identity. The console's UI word is *chat*.
//! - **Turn** - one prompt→result cycle; per-session monotonic.
//! - **Event** - append-only, per-session, monotonic `seq`; the console renders
//!   only from events.
//! - **Decision** - one concept, two kinds (approval | question). The daemon
//!   never invents resolution events: retiring emits nothing; late answers
//!   get an explicit error.
//! - **Receipt** - request dedup for every mutating call. Crash between
//!   `accepted` and a terminal state is surfaced as `unknown`; uncertain work
//!   is never resent.

pub mod events;
pub mod fsx;
pub mod ids;
pub mod model;
pub mod store;

pub use events::{Event, EventPayload};
pub use ids::{DecisionId, DeviceId, MachineId, ProjectId, RuleId, SessionId};
pub use model::{
    AutomationRule, ChatMessage, Decision, DecisionKind, DecisionOption, DecisionState, Harness,
    InstallSpec, Machine, MachineStatus, MachineView, MessageRole, Plugin, Project, Receipt,
    ReceiptStatus, Session, SessionStatus, Trigger, TriggerKind, TurnOutcome,
};
pub use store::Store;

/// This computer's name: COMPUTERNAME on Windows, HOSTNAME when it is
/// exported, else the kernel's own name. On Linux HOSTNAME is usually a shell
/// variable that never reaches child processes, so the environment alone
/// named every Linux computer "this-computer".
pub fn hostname() -> String {
    std::env::var("COMPUTERNAME")
        .ok()
        .or_else(|| std::env::var("HOSTNAME").ok())
        .or_else(|| std::fs::read_to_string("/proc/sys/kernel/hostname").ok())
        .or_else(|| std::fs::read_to_string("/etc/hostname").ok())
        .or_else(|| {
            std::process::Command::new("hostname")
                .output()
                .ok()
                .and_then(|out| String::from_utf8(out.stdout).ok())
        })
        .map(|name| name.trim().to_string())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| "this-computer".to_string())
}

/// A chat's name: the first line of its first prompt, at most 44 characters
/// (the console's own rule).
pub fn title_from(text: &str) -> String {
    let first = text.trim().lines().next().unwrap_or("");
    if first.chars().count() > 44 {
        let cut: String = first.chars().take(44).collect();
        format!("{}…", cut.trim_end())
    } else {
        first.to_string()
    }
}

/// Compares two secrets in time that doesn't depend on where they differ.
pub fn same_secret(presented: &str, expected: &str) -> bool {
    let (a, b) = (presented.as_bytes(), expected.as_bytes());
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// Millis since the Unix epoch - the crate's clock surface, so tests can pin time.
pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}
