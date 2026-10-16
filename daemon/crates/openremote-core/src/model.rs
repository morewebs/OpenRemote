use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::ids::{DecisionId, MachineId, ProjectId, RuleId, SessionId};

/// A permission word the harness's CLI takes, with a human label.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, Debug)]
pub struct PermissionWord {
    /// The CLI flag the harness takes (e.g. `acceptEdits`, `dontAsk`).
    #[serde(rename = "w")]
    pub word: String,
    /// A human label shown in pickers instead of the raw flag.
    #[serde(rename = "l")]
    pub label: String,
}

impl PermissionWord {
    #[must_use]
    pub fn new(word: &str, label: &str) -> Self {
        Self {
            word: word.into(),
            label: label.into(),
        }
    }
}

/// What a harness's driver actually reads, in the console's control terms.
/// Absence of a control used to be the only signal; this says which fields
/// the driver honors, so a capability can be stated rather than inferred.
#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Debug, Default)]
pub struct HarnessHonors {
    pub effort: bool,
    pub fast: bool,
    pub mcp: bool,
    pub plugins: bool,
    pub tools: bool,
    pub attachments: bool,
}

/// An installed agent CLI the daemon can drive (`claude`, `codex`, …).
#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct Harness {
    /// Harness id, e.g. `claude`.
    pub id: String,
    /// Human name as the harness itself spells it, e.g. `Claude Code`.
    pub name: String,
    /// The plugin that provides this harness, as `name@marketplace` - how a
    /// plugin's harness says where it came from. `None` for the built-in six.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plugin: Option<String>,
    /// Why this harness cannot be used, when it cannot: its driver or CLI is
    /// missing, or its plugin speaks a version this OpenRemote does not.
    /// `None` on a usable harness.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub problem: Option<String>,
    /// Resolved executable path, when one was found.
    pub path: Option<String>,
    /// `claude --version` output, when probed.
    pub version: Option<String>,
    pub available: bool,
    /// The user has its built-in plugin on (Plugins). Off hides the
    /// harness from every picker; its chats keep working.
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// The harness's own fast mode is usable at this version (claude
    /// `fastMode` headless ≥ 2.1.205, codex service tier `fast` ≥ 0.110.0).
    #[serde(default)]
    pub fast_supported: bool,
    /// What this driver actually honors. A missing object means an older
    /// daemon did not say, which is unknown rather than false.
    #[serde(default)]
    pub honors: HarnessHonors,
    /// Whether this CLI is signed in, in its own words. `None` means this
    /// harness has no status command we can ask - the row stays Ready or
    /// Missing, never a guessed sign-in.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signed_in: Option<bool>,
    /// Running on an API key, with no browser login. The chat can start;
    /// sign-in stays optional.
    #[serde(default)]
    pub using_api_key: bool,
    /// The model a fresh chat on this harness will run, from the harness's
    /// own config - its words. The wire's first-turn truth replaces it.
    /// `None` when the harness says nothing pre-send (slot reserved).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_model: Option<String>,
    /// The harness's own effort word that rides with `default_model`
    /// (grok's `default_reasoning_effort`), when one was read.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_effort: Option<String>,
    /// Whether this harness can run a chat through a user-configured
    /// custom endpoint. False hides custom-model controls for it.
    #[serde(default)]
    pub custom_endpoint_supported: bool,
    /// The permission words the harness's own CLI takes (claude
    /// `--permission-mode`, codex `approvalPolicy`, grok
    /// `--permission-mode`) - its vocabulary, so the console's picker
    /// offers only what the harness understands. Empty where the
    /// harness has no enumerable words (agy, pi, opencode): no picker,
    /// no dead UI. Each word carries a human label the console shows
    /// instead of the raw CLI flag.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub permission_modes: Vec<PermissionWord>,
    /// The word the harness runs when none is picked - its own default
    /// (claude `default`, codex `on-request`), `None` where the harness
    /// has no words at all.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_permission: Option<String>,
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

/// A write-only endpoint credential. The value persists in the owner's
/// private store and is resolved into a harness child process only; it is
/// never part of an API view, event, receipt, or error. Debug is redacted
/// so a stray log cannot carry the secret.
#[derive(Clone, Serialize, Deserialize)]
pub struct EndpointSecret(String);

impl EndpointSecret {
    pub fn new(value: String) -> Self {
        Self(value)
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// The secret, for endpoint probing and harness-child injection only.
    pub fn expose_secret(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Debug for EndpointSecret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("EndpointSecret([redacted])")
    }
}

/// How an endpoint expects its credential: bearer token, API key, or an
/// explicitly keyless local endpoint.
#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Debug)]
#[serde(rename_all = "snake_case")]
pub enum EndpointAuthKind {
    Bearer,
    ApiKey,
    None,
}

/// One user-configured custom model endpoint on this machine. The record
/// is machine-local: a chat created here resolves it here, and the
/// secret never leaves this machine through OpenRemote views or events.
#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct ModelEndpoint {
    pub id: String,
    pub harness: String,
    pub name: String,
    pub base_url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub models_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_model: Option<String>,
    pub auth_kind: EndpointAuthKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key: Option<EndpointSecret>,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub is_default: bool,
    pub created_at: i64,
    pub updated_at: i64,
}

/// A model endpoint as the console may see it: metadata only. The secret
/// is replaced by `has_key`; no API response carries key material.
#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct ModelEndpointView {
    pub id: String,
    pub harness: String,
    pub name: String,
    pub base_url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub models_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_model: Option<String>,
    pub auth_kind: EndpointAuthKind,
    pub has_key: bool,
    pub enabled: bool,
    pub is_default: bool,
    pub created_at: i64,
    pub updated_at: i64,
}

/// One model reported by a custom endpoint, with its endpoint attached so
/// the console can group custom rows without guessing their identity.
#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct EndpointModel {
    pub endpoint_id: String,
    pub endpoint_name: String,
    pub model: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
}

/// The harnesses that may use a custom endpoint record. Codex and Grok
/// records can be stored, but execution stays gated until a verified
/// per-chat override path exists for that harness.
pub fn validate_endpoint_harness(harness: &str) -> Result<(), String> {
    match harness {
        "claude" | "codex" | "grok" => Ok(()),
        _ => Err(format!(
            "custom endpoints support claude, codex, and grok - not '{harness}'"
        )),
    }
}

pub fn validate_endpoint_name(name: &str) -> Result<(), String> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err("name a custom endpoint".to_string());
    }
    if trimmed.chars().count() > 120 {
        return Err("keep the endpoint name to 120 characters".to_string());
    }
    Ok(())
}

pub fn validate_model_id(model: &str) -> Result<(), String> {
    let trimmed = model.trim();
    if trimmed.is_empty() {
        return Err("name a custom model id".to_string());
    }
    if trimmed.chars().count() > 256 {
        return Err("keep the custom model id to 256 characters".to_string());
    }
    if trimmed.chars().any(char::is_control) {
        return Err("the custom model id must not contain control characters".to_string());
    }
    Ok(())
}

/// Normalize a user-entered endpoint base URL: absolute HTTP(S), no
/// credentials, query, or fragment, with one canonical trailing-slash
/// rule. Local loopback and LAN hosts are allowed: custom endpoints are
/// often a model server on this machine or network.
pub fn normalize_endpoint_base_url(raw: &str) -> Result<String, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err("name a custom base URL".to_string());
    }
    if trimmed.chars().count() > 2083 {
        return Err("keep the base URL to 2083 characters".to_string());
    }
    if trimmed.chars().any(char::is_control) {
        return Err("the base URL must not contain control characters".to_string());
    }
    let (scheme, rest) = trimmed
        .split_once("://")
        .ok_or_else(|| "use an absolute http:// or https:// base URL".to_string())?;
    let scheme = scheme.to_ascii_lowercase();
    if scheme != "http" && scheme != "https" {
        return Err("use an absolute http:// or https:// base URL".to_string());
    }
    let (authority, path) = rest.split_once('/').map_or((rest, ""), |(a, p)| (a, p));
    if authority.is_empty() || authority.chars().any(|c| c.is_whitespace()) {
        return Err("the base URL needs a host".to_string());
    }
    if authority.contains('@') {
        return Err("put no credentials in the base URL - save the key separately".to_string());
    }
    if path.contains('?') || path.contains('#') {
        return Err("put no query or fragment in the base URL".to_string());
    }
    let path = path.trim_matches('/');
    let host = authority.to_ascii_lowercase();
    if path.is_empty() {
        Ok(format!("{scheme}://{host}"))
    } else {
        Ok(format!("{scheme}://{host}/{path}"))
    }
}

/// Normalize an optional model-list path override: an absolute HTTP(S)
/// URL, or a relative path without whitespace, control characters, or
/// parent-directory escapes.
pub fn normalize_endpoint_models_path(raw: &str) -> Result<String, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err("name a model-list path override or leave it empty".to_string());
    }
    if trimmed.chars().count() > 512 {
        return Err("keep the model-list override to 512 characters".to_string());
    }
    if trimmed.chars().any(char::is_control) || trimmed.chars().any(char::is_whitespace) {
        return Err("the model-list override must not contain whitespace".to_string());
    }
    if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
        return normalize_endpoint_base_url(trimmed);
    }
    if trimmed.split('/').any(|segment| segment == "..") {
        return Err("the model-list override must not escape its base URL".to_string());
    }
    Ok(trimmed.trim_matches('/').to_string())
}

/// The model-list URL for an endpoint: an explicit override when the
/// gateway uses one, else `{base}/v1/models`, or `{base}/models` when the
/// base already ends in `/v1`.
pub fn endpoint_models_url(base_url: &str, models_path: Option<&str>) -> Result<String, String> {
    let base = normalize_endpoint_base_url(base_url)?;
    if let Some(path) = models_path.map(str::trim).filter(|p| !p.is_empty()) {
        if path.starts_with("http://") || path.starts_with("https://") {
            return normalize_endpoint_base_url(path);
        }
        let path = path.trim_matches('/');
        return Ok(format!("{base}/{path}"));
    }
    if base.ends_with("/v1") {
        Ok(format!("{base}/models"))
    } else {
        Ok(format!("{base}/v1/models"))
    }
}

impl ModelEndpoint {
    pub fn view(&self) -> ModelEndpointView {
        ModelEndpointView {
            id: self.id.clone(),
            harness: self.harness.clone(),
            name: self.name.clone(),
            base_url: self.base_url.clone(),
            models_path: self.models_path.clone(),
            default_model: self.default_model.clone(),
            auth_kind: self.auth_kind,
            has_key: self.key.as_ref().is_some_and(|key| !key.is_empty()),
            enabled: self.enabled,
            is_default: self.is_default,
            created_at: self.created_at,
            updated_at: self.updated_at,
        }
    }
}

/// One conversation with one harness in one workspace.
#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct Session {
    pub id: SessionId,
    pub harness: String,
    /// The folder the chat runs in - absent when it belongs to no project
    /// (it then spawns in the home folder). Absolute, validated at create
    /// time; the harness's own resolution replaces nothing here.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace: Option<PathBuf>,
    pub status: SessionStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// A custom model endpoint this chat runs through. None means the
    /// harness's own provider; Some names a machine-local endpoint record.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub endpoint_id: Option<String>,
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
    /// The device that runs a chat synced to the user's other devices
    /// (Cloud). None: a private chat that never leaves this computer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub executor: Option<crate::ids::DeviceId>,
    /// The first line of the first prompt, so a chat has a name on every
    /// device without replaying its log.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Messages waiting for the running turn to end, oldest first. Only
    /// `queue.changed` writes these two fields - a `session.updated`
    /// carries them but never changes them.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub queue: Vec<QueuedMessage>,
    /// An interrupt, a stop, or a failure holds the queue until the user
    /// resumes it (or steers one message out of it).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub queue_paused: bool,
    /// The other chats the user pointed this one at with @ - the only ones
    /// its agent may read through OpenRemote's read-a-chat tool.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub mentioned_chats: Vec<SessionId>,
    /// Who started this chat, when another chat's agent did: it is a worker
    /// in that chat's team. Set when the chat is created and never changed,
    /// so a copy on another device shows the same tree. A device that
    /// predates teams ignores it and shows the chat on its own.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<ParentLink>,
}

/// Where a team member stands: the chat that started it, how many steps
/// below the user's own chat it is, and the brief it was given.
#[derive(Clone, Serialize, Deserialize, Debug, PartialEq, Eq)]
pub struct ParentLink {
    /// The chat whose agent started this one.
    pub chat: SessionId,
    /// The user's own chat is depth 0, its workers 1, theirs 2.
    pub depth: u32,
    /// What it is to its parent (`worker`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    /// The first 200 characters of the brief, so a copy of the chat on
    /// another device says what it was for without replaying the log.
    pub brief: String,
    pub spawned_at: i64,
}

/// A message that did not come from the user: another agent in the same
/// team sent it. The chat shows who; the harness is told in words it can
/// not mistake for the user's.
#[derive(Clone, Serialize, Deserialize, Debug, PartialEq, Eq)]
pub struct Origin {
    /// `agent` is the only kind there is.
    pub kind: String,
    /// The chat that sent it, and its name when it did.
    pub chat: SessionId,
    pub title: String,
    /// A worker's report to its lead. A newer report from the same worker
    /// replaces an older one that has not been delivered yet.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub report: bool,
}

/// One message in a chat's queue: sent on its own when the turn before it
/// ends, or steered into the running turn by hand.
#[derive(Clone, Serialize, Deserialize, Debug, PartialEq, Eq)]
pub struct QueuedMessage {
    pub id: String,
    pub text: String,
    pub at: i64,
    /// Set when another agent sent it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin: Option<Origin>,
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

/// A harness this machine can install through its owner's own installer -
/// the command is shown verbatim and is what runs.
#[derive(Clone, Serialize, Debug)]
pub struct InstallSpec {
    pub harness_id: String,
    pub name: String,
    /// The command that installs it, shown verbatim.
    pub command: String,
    /// `official` (the owner's install script) or `release` (the owner's
    /// release artifact, unpacked by OpenRemote where no script exists).
    pub source: String,
    /// Plain words beside the command when it does more than it says.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    /// The harness needs a runtime this machine doesn't have yet (pi on
    /// Node.js); the install waits for the human's go-ahead to set it up.
    pub needs_runtime: bool,
}

/// One MCP server the user set up on a machine (Settings > MCP servers) -
/// a session on that machine can call it. The launch command is what that machine
/// would start; a key, if the plugin needs one, never leaves the machine -
/// `has_key` only records that the machine has one.
#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct McpServerEntry {
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

/// One plugin installed on this machine (the Plugins page). The six
/// harnesses are built-in plugins and are never stored here - only whether
/// the user turned one off.
#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct InstalledPlugin {
    /// The plugin's name: its marketplace entry's name, or for a direct
    /// install its manifest's. Unique within its marketplace only - the
    /// same name from two marketplaces installs side by side.
    pub name: String,
    /// The marketplace it came from; `local` for a folder, git repo or
    /// archive installed directly (what 6a's installs were, unnamed).
    #[serde(default = "local_marketplace")]
    pub marketplace: String,
    /// What exactly was fetched - the commit, the archive's sha256, the
    /// npm version, a folder's content hash - so an update can tell a
    /// changed plugin from the same one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolved: Option<String>,
    /// The command a `command` source printed the plugin folder with,
    /// exactly as the user approved it: only this command ever runs again.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<ApprovedCommand>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub runtime: PluginRuntime,
    /// The powers it declared, which the user approved at install.
    #[serde(default)]
    pub powers: Vec<PluginPower>,
    pub source: PluginSource,
    pub scope: PluginScope,
    /// The harness this plugin provides, when it provides one. The record
    /// keeps the id and name for the plugin list; the daemon re-reads the
    /// folder's manifest when it probes the driver, so the folder stays the
    /// one source of truth for the driver path.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub harness: Option<PluginHarnessRef>,
    #[serde(default = "default_true")]
    pub enabled: bool,
    pub installed_at: i64,
    pub updated_at: i64,
}

/// The harness a plugin declares, as its install record remembers it.
#[derive(Clone, Serialize, Deserialize, Debug, PartialEq, Eq)]
pub struct PluginHarnessRef {
    pub id: String,
    pub name: String,
}

fn local_marketplace() -> String {
    LOCAL_MARKETPLACE.to_string()
}

/// The marketplace name a direct install (folder, git repo, archive) is
/// filed under. No real marketplace may take it.
pub const LOCAL_MARKETPLACE: &str = "local";

impl InstalledPlugin {
    /// `name@marketplace` - the plugin's id on the machine, in routes and
    /// in the store.
    pub fn id(&self) -> String {
        format!("{}@{}", self.name, self.marketplace)
    }
}

/// A `command` source's command as the user approved it at install.
#[derive(Clone, Serialize, Deserialize, Debug, PartialEq, Eq)]
pub struct ApprovedCommand {
    pub command: String,
    pub timeout_secs: u64,
}

/// A plugin marketplace the user added, or one OpenRemote added for them
/// (Plugins > Marketplaces). Its catalog is Claude Code's own
/// `.claude-plugin/marketplace.json`, cached on disk; this record is what
/// the machine remembers about it.
#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct MarketplaceRecord {
    /// The catalog's own `name` - what plugin ids end in.
    pub name: String,
    pub source: MarketplaceSource,
    /// Off hides it from Discover; what was installed from it stays.
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// OpenRemote's own marketplace can be turned off, not removed.
    #[serde(default = "default_true")]
    pub removable: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default)]
    pub plugin_count: usize,
    pub added_at: i64,
    /// The last time its catalog was fetched; never, until the first
    /// Discover opens it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refreshed_at: Option<i64>,
    /// Why the last fetch failed, in its own words. The cached copy (if
    /// any) keeps serving Discover.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// For installed plugins whose entry follows a branch with no version
    /// to compare: the commit that branch pointed at when the catalog was
    /// last fetched, by plugin name. Unequal to what was installed means
    /// an update.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub heads: BTreeMap<String, String>,
}

/// Where a marketplace's catalog is fetched from - the forms Claude Code's
/// own `marketplace add` takes.
#[derive(Clone, Serialize, Deserialize, Debug, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MarketplaceSource {
    /// `owner/repo` on GitHub, cloned over https.
    Github {
        repo: String,
        #[serde(default, rename = "ref", skip_serializing_if = "Option::is_none")]
        reference: Option<String>,
    },
    /// Any git repository by URL.
    Git {
        url: String,
        #[serde(default, rename = "ref", skip_serializing_if = "Option::is_none")]
        reference: Option<String>,
    },
    /// A folder on this machine, read in place.
    Directory { path: String },
    /// A link to a `marketplace.json` alone: its entries must point
    /// elsewhere, there are no files beside it.
    Url { url: String },
}

/// How a plugin runs. `content` is a Claude Code plugin with no
/// OpenRemote part of its own: skills, agents, commands, hooks, MCP.
#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Debug)]
#[serde(rename_all = "snake_case")]
pub enum PluginRuntime {
    Process,
    Js,
    Wasm,
    Content,
}

/// What a plugin may do, declared in its manifest and approved by the
/// user at install.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, Debug)]
#[serde(rename_all = "snake_case")]
pub enum PluginPower {
    /// Start programs on this machine.
    RunPrograms,
    /// Read chats and the project's files.
    ReadChats,
    /// Give agents tools; start, steer and stop chats.
    AgentTools,
    /// Reach the internet.
    Network,
    /// Use the microphone.
    Microphone,
}

/// Where an installed plugin came from - what an update fetches again.
#[derive(Clone, Serialize, Deserialize, Debug, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PluginSource {
    Folder {
        path: String,
    },
    Git {
        url: String,
    },
    Archive {
        location: String,
    },
    /// An entry in a marketplace's catalog; `from` says, in a line, where
    /// the entry itself fetched it (a repo, a subfolder, a package).
    Marketplace {
        marketplace: String,
        plugin: String,
        from: String,
    },
}

/// Who a plugin is installed for. This machine only, for now; Global,
/// Project-local and Project arrive with the scopes.
#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Debug)]
#[serde(rename_all = "snake_case")]
pub enum PluginScope {
    Machine,
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
    /// A repeating cadence in minutes, for schedules. Exactly one of
    /// `time` and `every_minutes` is set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub every_minutes: Option<u32>,
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
    /// The harness's own effort word the rule's chat opens with, carried
    /// the way a chat's effort is (claude `--effort`, codex
    /// `model_reasoning_effort`). Unset means the harness's own default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort: Option<String>,
    /// The harness's own permission word the rule's chat opens with
    /// (claude `--permission-mode`, codex `approvalPolicy`). Unset means
    /// the harness's own default holds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub permission_mode: Option<String>,
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

/// A registered project: the folders a chat's workspace can belong to.
/// Exactly one folder this release - the list is the multi-folder future,
/// arriving additively. A project never stores its name: it *is* its
/// folder's basename, so the disk stays the only place the name lives.
/// Removing a project unregisters it - the folder on disk, and every chat
/// that ran in it, are untouched.
#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct Project {
    pub id: ProjectId,
    /// The folders that make up the project - one this release, absolute
    /// existing directories, no folder registered twice.
    pub folders: Vec<PathBuf>,
    pub created_at: i64,
    pub updated_at: i64,
}

impl McpServerEntry {
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
    /// Set on a user-role message another agent sent (a lead's brief, a
    /// worker's report): the chat shows who it came from.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin: Option<Origin>,
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
