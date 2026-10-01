//! The harness-neutral driver vocabulary. Every harness crate (claude,
//! codex, grok, pi, opencode, agy, …) builds its driver on these types so
//! the supervisor stays harness-agnostic: it spawns through a registry,
//! consumes `DriverEvent`s, and routes answers back with the harness's own
//! correlation id.

pub mod anthropic_wire;
pub mod events;

pub use events::{ApprovalRequest, DecisionSpec, DriverEvent};

use std::path::PathBuf;

/// A failure inside any driver, in vocabulary the supervisor can route.
#[derive(Debug, thiserror::Error)]
pub enum DriverError {
    #[error("spawn failed: {0}")]
    Spawn(String),
    #[error("harness process io: {0}")]
    Io(#[from] std::io::Error),
    #[error("harness protocol: {0}")]
    Protocol(String),
    #[error("harness rejected the call: {0}")]
    Harness(String),
    #[error("harness process is gone")]
    Gone,
}

/// How a harness's CLI was found. `NodeScript` covers the npm layout where
/// the CLI is a script driven by node.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Resolution {
    Executable(PathBuf),
    NodeScript { node: PathBuf, script: PathBuf },
    Unavailable,
}

impl Resolution {
    pub fn is_available(&self) -> bool {
        !matches!(self, Resolution::Unavailable)
    }
}

/// Spawn options for one session's harness, harness-agnostic. Each driver
/// maps these onto its own flags.
#[derive(Clone, Debug, Default)]
pub struct SpawnOptions {
    pub cwd: PathBuf,
    pub model: Option<String>,
    /// The harness's own permission/approval policy word, passed verbatim
    /// (terminology parity: claude `--permission-mode`, codex
    /// `approvalPolicy`, grok `--permission-mode`, …).
    pub permission_mode: Option<String>,
    /// Resume the harness's own conversation (claude `--resume=<id>`,
    /// codex `thread/resume`, pi `--session`, …).
    pub resume: Option<String>,
    pub include_deltas: bool,
}

/// A model a harness advertises, in the harness's own words. Backs the
/// console's model slot; empty means the slot stays reserved.
#[derive(Clone, Debug, serde::Serialize)]
pub struct ModelDescriptor {
    pub model: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    /// The harness's own reasoning-effort words for this model.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub reasoning_efforts: Vec<String>,
}
