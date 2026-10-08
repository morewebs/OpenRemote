//! The harness-neutral driver vocabulary. Every harness crate (claude,
//! codex, grok, pi, opencode, agy, …) builds its driver on these types so
//! the supervisor stays harness-agnostic: it spawns through a registry,
//! consumes `DriverEvent`s, and routes answers back with the harness's own
//! correlation id.

pub mod anthropic_wire;
pub mod events;
pub mod paths;

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
    /// Run the harness's own fast mode (claude `fastMode`, codex service
    /// tier `fast`) - the harness's speed tier, never a model switch.
    pub fast: bool,
    /// MCP servers an enabled plugin asked to ride along - each harness
    /// maps these onto its own wire (claude `--mcp-config`, codex
    /// `mcp_servers.<id>.*` config overrides). A harness with no wire for
    /// them simply doesn't run plugins.
    pub mcp_servers: Vec<McpServer>,
}

/// One MCP server a session should start, from an enabled plugin on the
/// machine the session runs on.
#[derive(Clone, Debug)]
pub struct McpServer {
    /// The server's id on the harness's own wire (claude's key inside
    /// `mcpServers`, codex's `<id>` in `mcp_servers.<id>`).
    pub id: String,
    pub command: String,
    pub args: Vec<String>,
}

/// What one decision answer means for the turn and the session - filled
/// from each harness's own semantics at the dispatch layer.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AnswerOutcome {
    /// The answer interrupts the running turn; its boundary event settles it.
    pub interrupts_turn: bool,
    /// The harness's own session-scope choice word was picked, granting
    /// this tool for the rest of the session (codex `acceptForSession`,
    /// opencode `always`) - named in the harness's own tool vocabulary.
    pub session_grant: Option<String>,
}

/// A change to a live chat's model, effort, or fast mode. `None` means
/// "leave it". A harness that cannot apply a field on a running process
/// returns an error for that field - the console does not render the
/// control in that case.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SessionSettings {
    pub model: Option<String>,
    /// The harness's own effort word (`low`, `high`, `max`, …).
    pub effort: Option<String>,
    /// The harness's own fast mode. `Some(false)` returns to its standard
    /// speed tier.
    pub fast: Option<bool>,
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
    /// The harness's own marker for the entry a fresh chat runs (codex's
    /// `isDefault`, claude's `default` alias) - the pre-send fact where
    /// its config says nothing.
    #[serde(default)]
    pub is_default: bool,
}
