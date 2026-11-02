//! The harness-neutral driver vocabulary. Every harness crate (claude,
//! codex, grok, pi, opencode, agy, …) builds its driver on these types so
//! the supervisor stays harness-agnostic: it spawns through a registry,
//! consumes `DriverEvent`s, and routes answers back with the harness's own
//! correlation id.

pub mod anthropic_wire;
pub mod events;
pub mod line_reader;
pub mod paths;
pub mod process;

pub use events::{ApprovalRequest, DecisionSpec, DriverEvent};
pub use line_reader::LineReader;
pub use process::kill_tree;

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

/// The Windows creation flags every harness child spawn carries. The
/// daemon runs console-less in both production modes (a Tauri sidecar; a
/// service), so Windows would otherwise allocate a fresh visible console
/// for every console-subsystem child: a window per session for the
/// persistent harnesses, a flash per prompt for the process-per-prompt
/// ones. CREATE_NO_WINDOW suppresses that - the same flag the installer's
/// `detach()` already sets in openremote-daemon/src/install.rs.
/// CREATE_NEW_PROCESS_GROUP puts each child in its own group so a Ctrl-C
/// typed into a shared console (the daemon run by hand, a test run)
/// cannot kill a harness the supervisor did not order to die.
#[cfg(windows)]
pub fn creation_flags() -> u32 {
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
    CREATE_NO_WINDOW | CREATE_NEW_PROCESS_GROUP
}

/// What a harness CLI calls its own version: `<program> <leading> --version`,
/// first non-empty line, trimmed, capped. `None` when the CLI did not answer
/// in time or could not be started at all.
///
/// This exists so an answer to one of our own handshakes that we do not
/// recognise can be reported as "claude 2.1.0 answered a shape this build
/// does not speak" instead of "this harness ships no models". Those are two
/// different problems with two different fixes, and only the version tells
/// them apart. `--version` prints and exits: no model runs and no account is
/// reached. A version is worth putting in a failure sentence, never worth
/// failing over, which is why the answer is an `Option` and not a `Result`.
pub async fn cli_version(
    program: &std::path::Path,
    leading: &[std::ffi::OsString],
) -> Option<String> {
    const DEADLINE: std::time::Duration = std::time::Duration::from_secs(5);
    const CAP: usize = 120;
    let mut command = tokio::process::Command::new(program);
    command
        .args(leading)
        .arg("--version")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        // A CLI that hangs on the flag must not outlive this probe.
        .kill_on_drop(true);
    // Windows: no console flash, no Ctrl-C from a shared console; the why
    // lives on creation_flags above.
    #[cfg(windows)]
    command.creation_flags(creation_flags());
    let output = tokio::time::timeout(DEADLINE, command.output())
        .await
        .ok()?
        .ok()?;
    let text = String::from_utf8_lossy(&output.stdout);
    let line = text.lines().map(str::trim).find(|line| !line.is_empty())?;
    let capped: String = line.chars().take(CAP).collect();
    Some(capped.trim_end().to_string())
}

/// A resolved custom endpoint for one spawn: metadata plus the secret.
/// The secret's own Debug stays redacted, so spawn options never log it.
#[derive(Clone, Debug)]
pub struct EndpointBinding {
    pub endpoint_id: String,
    pub harness: String,
    pub endpoint_name: String,
    pub base_url: String,
    pub auth_kind: openremote_core::EndpointAuthKind,
    pub secret: Option<openremote_core::EndpointSecret>,
}

/// Spawn options for one session's harness, harness-agnostic. Each driver
/// maps these onto its own flags.
#[derive(Clone, Debug, Default)]
pub struct SpawnOptions {
    pub cwd: PathBuf,
    pub model: Option<String>,
    /// A custom endpoint binding when the chat runs through one. None
    /// means the harness's own provider. Drivers that cannot honor a
    /// binding must refuse before spawning.
    pub endpoint: Option<EndpointBinding>,
    /// The harness's own permission/approval policy word, passed verbatim
    /// (terminology parity: claude `--permission-mode`, codex
    /// `approvalPolicy`, grok `--permission-mode`, …).
    pub permission_mode: Option<String>,
    /// The chat's effort in the harness's own word, set at every start -
    /// the first spawn and each resume (claude `--effort`, codex
    /// `model_reasoning_effort`, pi `set_thinking_level`). A harness with
    /// no effort of its own ignores it.
    pub effort: Option<String>,
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
    /// The folders of the plugins the user installed and left on - Claude
    /// Code's own plugin layout. claude loads each with `--plugin-dir`;
    /// the other harnesses get them when plugins reach every harness.
    pub plugin_dirs: Vec<PathBuf>,
    /// Tools that run without asking, in the harness's own names
    /// (OpenRemote's browser tools that only read: claude
    /// `--allowedTools`). A harness that asks for nothing ignores it.
    pub allowed_tools: Vec<String>,
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
    /// The harness's own permission/approval word (claude
    /// `--permission-mode`, codex `approvalPolicy`). Start-time on every
    /// harness - no driver changes it live, so a live change is an error
    /// and a stopped chat's change lands on resume.
    pub permission_mode: Option<String>,
}

/// A model a harness advertises, in the harness's own words. Backs the
/// console's model slot; empty means the slot stays reserved. A plugin's
/// manifest declares a static catalog in the same shape, so the daemon
/// deserializes it as well as serializing it.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct ModelDescriptor {
    /// The pick word the harness's own picker takes (`opus`) or the full
    /// id when the harness offers no alias (`claude-fable-5-1[1m]`).
    pub model: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    /// The model id the harness actually runs once the pick word is
    /// resolved (`claude-opus-5-5`) - the value a running session
    /// reports back, where the harness splits pick word from id. The
    /// console matches both spellings against a session's model so a
    /// resolved id still finds its row's display name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolved_model: Option<String>,
    /// The harness's own reasoning-effort words for this model.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub reasoning_efforts: Vec<String>,
    /// The harness's own marker for the entry a fresh chat runs (codex's
    /// `isDefault`, claude's `default` alias) - the pre-send fact where
    /// its config says nothing.
    #[serde(default)]
    pub is_default: bool,
    /// A short second line for the row, in the harness's own words: a cost
    /// tier with its flags (`3x · Recommended`), or the tagline the picker
    /// prints under a name. Optional, so a harness that fills neither field
    /// keeps a one line row.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    /// The notice the harness prints under a model (`May use data for AI
    /// training`), shown as the row's hover title rather than as its second
    /// line.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// A command a harness runs when a prompt starts with `/name`, in its own
/// words: a built-in, one of its skills, a plugin's (`plugin:skill`).
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct CommandDescriptor {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// What it takes after its name, as the harness hints it (`<issue>`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub argument_hint: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub aliases: Vec<String>,
}

/// One of a harness's subagents, which a prompt can mention.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct AgentDescriptor {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_descriptor_with_a_detail_and_a_note_round_trips_and_the_old_shape_still_parses() {
        // The new shape: a cost line and a data-use notice, neither required.
        let json = r#"{"model":"DeepSeek V4.1 Flash 10/17 • high","display_name":"DeepSeek V4.1 Flash 10/17 • high","detail":"3x · Recommended","note":"May use data for AI training","is_default":true}"#;
        let descriptor: ModelDescriptor = serde_json::from_str(json).expect("the new shape");
        assert_eq!(descriptor.detail.as_deref(), Some("3x · Recommended"));
        assert_eq!(
            descriptor.note.as_deref(),
            Some("May use data for AI training")
        );
        assert!(descriptor.is_default);
        let encoded = serde_json::to_value(&descriptor).expect("serialize");
        assert_eq!(encoded["detail"], "3x · Recommended");
        assert_eq!(encoded["note"], "May use data for AI training");

        // The old shape, written before the two fields existed, still reads.
        let old = r#"{"model":"opus","display_name":"Opus"}"#;
        let descriptor: ModelDescriptor = serde_json::from_str(old).expect("the old shape");
        assert_eq!(descriptor.detail, None);
        assert_eq!(descriptor.note, None);
        // And a descriptor with neither never serializes them.
        let encoded = serde_json::to_value(&descriptor).expect("serialize");
        assert!(encoded.get("detail").is_none());
        assert!(encoded.get("note").is_none());
    }

    #[tokio::test]
    async fn a_version_probe_reads_the_first_line_of_both_answers() {
        // `cargo` is the one program guaranteed to exist in the process
        // doing the asking, and `cargo --version` is a print-and-exit:
        // the probe must read it, not hang on it.
        let version = cli_version(std::path::Path::new(env!("CARGO")), &[]).await;
        let version = version.expect("cargo answers --version");
        assert!(
            version.starts_with("cargo "),
            "unexpected version line: {version}"
        );
        // A program that cannot be started is no version, never a panic.
        let missing = cli_version(std::path::Path::new("no-such-harness-cli-xyz"), &[]).await;
        assert_eq!(missing, None);
    }
}

/// What a harness offers a composer: its commands and its agents.
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize)]
pub struct CommandCatalog {
    pub commands: Vec<CommandDescriptor>,
    pub agents: Vec<AgentDescriptor>,
}
