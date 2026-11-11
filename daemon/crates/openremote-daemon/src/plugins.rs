//! The plugin system (Plugins): what a plugin folder says about itself,
//! fetching one - from a folder, a git repo, an archive, or the source a
//! marketplace entry names - into a staging folder for the user to
//! approve, and the install itself. The six harnesses are built-in
//! plugins; what the user installs lives under
//! `<data dir>/plugins/<marketplace>/<name>/`, a direct install under the
//! marketplace name `local`.
//!
//! A plugin folder is Claude Code's own layout (`.claude-plugin/
//! plugin.json`, skills/, agents/, commands/, hooks, `.mcp.json`) plus
//! OpenRemote's part in `openremote.json` beside it, so one folder is a
//! plugin to both. OpenRemote writes Claude's file only where Claude Code
//! itself would compute one - from a marketplace entry (Claude's rule:
//! with no plugin.json, the entry is the manifest) - or a bare one for a
//! folder that carries Claude parts without it, so `--plugin-dir` reads
//! the folder as one plugin and never as a folder of plugins.

use std::collections::HashMap;
use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};
use std::sync::Mutex as StdMutex;
use std::time::Duration;

use openremote_core::model::PermissionWord;
use openremote_core::{
    ApprovedCommand, InstalledPlugin, LOCAL_MARKETPLACE, PluginPower, PluginRuntime, PluginSource,
};
use openremote_harness::ModelDescriptor;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::Digest;
use std::path::Component;

/// The `openremote.json` api this daemon speaks.
pub const API_VERSION: u64 = 1;

/// An archive is refused past this size, packed or unpacked.
const MAX_ARCHIVE_BYTES: u64 = 256 * 1024 * 1024;

/// A folder copied in - a direct folder, a command's output - is refused
/// past this many files and folders (and past the archive size).
const MAX_ENTRIES: usize = 20_000;

/// Names a plugin cannot take: the routes' own words.
const RESERVED_NAMES: &[&str] = &["stage"];

/// The ids of the harnesses this program carries, which a plugin may never
/// take: `plugin:name` namespacing and the plugin list would otherwise
/// show two different things under one id.
///
/// The install path passes the ids the harness registry probed; a command
/// plugin's update is re-read from another thread of the supervisor and
/// reaches no registry, so the ids are stated here. They are stable by
/// contract, and this list is the reason a copy that renames itself
/// `claude` is refused on update as well as at install.
const HARNESS_IDS: &[&str] = &["claude", "codex", "grok", "pi", "opencode", "agy"];

#[derive(Debug, thiserror::Error)]
pub enum PluginError {
    /// The plugin itself is wrong - its manifest, its name, its powers.
    #[error("{0}")]
    Invalid(String),
    /// Its source could not be read - a missing folder, a failed clone.
    #[error("{0}")]
    Source(String),
    #[error("{0}")]
    NotFound(String),
    #[error("{0}")]
    Io(#[from] std::io::Error),
}

/// What a plugin folder says about itself, as the approval screen shows it.
#[derive(Clone, Serialize, Debug)]
pub struct Manifest {
    pub name: String,
    pub version: Option<String>,
    pub description: Option<String>,
    pub runtime: PluginRuntime,
    /// Declared in openremote.json, plus what its parts imply: a process
    /// plugin runs a program; Claude hooks and MCP servers run programs.
    pub powers: Vec<PluginPower>,
    /// The Claude Code parts it carries: skills, agents, commands, hooks, mcp.
    pub claude_parts: Vec<String>,
    /// It runs programs as the user, outside any sandbox - said before the
    /// user approves (a JS or WASM plugin is sandboxed; a program is not).
    pub not_sandboxed: bool,
    /// The harness this plugin provides, when it provides one.
    pub harness: Option<HarnessManifest>,
}

/// The `harness` block of `openremote.json`: the seam that lets a plugin add
/// a harness without a line of core code.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct HarnessManifest {
    /// The seam's own version. A plugin with another number is refused, not
    /// guessed at.
    pub api: u64,
    /// Stable harness id, the word the console and the routes use.
    pub id: String,
    /// The display name, verbatim.
    pub name: String,
    /// Driver path per OS (`windows`, `macos`, `linux`), plus `default`.
    /// The Node spellings `win32` and `darwin` are accepted too.
    #[serde(default)]
    pub driver: HashMap<String, String>,
    #[serde(default)]
    pub args: Vec<String>,
    /// A static model catalog, in the harness's own words. `None` means this
    /// harness advertises nothing before a chat runs.
    #[serde(default)]
    pub models: Option<Vec<ModelDescriptor>>,
    #[serde(default)]
    pub default_model: Option<String>,
    /// The catalog file the driver writes beside itself (`models.json`). Read
    /// when the machine re-probes, which is cheap, and it wins over `models`
    /// when it holds a usable list. A missing or malformed file is not an
    /// error: the manifest's list is the shipped answer.
    #[serde(default)]
    pub models_file: Option<String>,
    /// `"live"` when this harness's driver can change the model on a running
    /// chat. The only word this daemon knows; anything else is refused at
    /// install rather than quietly treated as start-time.
    #[serde(default)]
    pub settings: Option<String>,
    /// The harness's own permission words, each a `{ "w", "l" }` row (a bare
    /// string is its own label). Empty means no permission picker.
    #[serde(default)]
    pub permission_modes: Vec<ManifestPermission>,
    /// The word a fresh chat starts with; one of `permission_modes`.
    #[serde(default)]
    pub default_permission: Option<String>,
    /// The harness has a fast mode of its own, and the driver reads `fast`.
    #[serde(default)]
    pub fast: bool,
    /// The driver hands the spawn's `mcp_servers` (OpenRemote's own per-chat
    /// server among them) and `allowed_tools` to its CLI. Without it the
    /// lists still arrive, but nothing tells the agent the tools exist.
    #[serde(default)]
    pub mcp: bool,
    /// The driver answers `--commands <folder>` with the composer's `/` and
    /// `@` lists. Without it no process starts for a menu.
    #[serde(default)]
    pub commands: bool,
    /// The driver takes one prompt at a time: a message sent while a turn
    /// runs waits in the queue. Unset is `true`, the safe answer for a
    /// driver that runs its CLI once per prompt.
    #[serde(default)]
    pub one_prompt_at_a_time: Option<bool>,
    /// A message sent while a turn runs lands inside that turn, which the
    /// harness closes with one result.
    #[serde(default)]
    pub steers_mid_turn: bool,
}

/// One permission word in a harness manifest: the `{ "w", "l" }` row the
/// pickers use, or a bare word that is its own label.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(untagged)]
pub enum ManifestPermission {
    Word(String),
    Row(PermissionWord),
}

impl ManifestPermission {
    pub fn row(&self) -> PermissionWord {
        match self {
            ManifestPermission::Word(word) => PermissionWord::new(word, word),
            ManifestPermission::Row(row) => row.clone(),
        }
    }
}

/// What a plugin harness declares about how it runs, beyond its driver and
/// models: the parts of the contract the registry and the supervisor read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HarnessTraits {
    pub permission_modes: Vec<PermissionWord>,
    pub default_permission: Option<String>,
    pub fast: bool,
    pub mcp: bool,
    pub commands: bool,
    pub one_prompt_at_a_time: bool,
    pub steers_mid_turn: bool,
}

impl Default for HarnessTraits {
    /// What a manifest that says nothing gets: no pickers, no MCP, no menus,
    /// and one prompt at a time.
    fn default() -> Self {
        Self {
            permission_modes: Vec::new(),
            default_permission: None,
            fast: false,
            mcp: false,
            commands: false,
            one_prompt_at_a_time: true,
            steers_mid_turn: false,
        }
    }
}

impl HarnessManifest {
    /// The traits this manifest declares, as the registry reads them.
    pub fn traits(&self) -> HarnessTraits {
        HarnessTraits {
            permission_modes: self
                .permission_modes
                .iter()
                .map(ManifestPermission::row)
                .collect(),
            default_permission: self.default_permission.clone(),
            fast: self.fast,
            mcp: self.mcp,
            commands: self.commands,
            // A driver that steers takes the second message by definition.
            one_prompt_at_a_time: self.one_prompt_at_a_time.unwrap_or(!self.steers_mid_turn),
            steers_mid_turn: self.steers_mid_turn,
        }
    }

    /// The driver path this OS runs, when the plugin names one.
    pub fn driver_path(&self) -> Option<&str> {
        let os = std::env::consts::OS;
        let exact = self.driver.get(os);
        let alias = match os {
            "windows" => self.driver.get("win32"),
            "macos" => self.driver.get("darwin"),
            _ => None,
        };
        exact
            .or(alias)
            .or_else(|| self.driver.get("default"))
            .map(String::as_str)
    }

    /// The driver path, resolved inside the plugin folder. An absolute path
    /// or one that climbs out with `..` is refused: a plugin may run programs,
    /// but it runs them from its own folder.
    pub fn resolved_driver(&self, dir: &Path) -> Result<PathBuf, String> {
        let value = self.driver_path().ok_or_else(|| {
            format!(
                "names no driver for {} (add one, or a default)",
                std::env::consts::OS
            )
        })?;
        let relative = Path::new(value);
        if relative.is_absolute()
            || relative.components().any(|c| {
                matches!(
                    c,
                    Component::ParentDir | Component::RootDir | Component::Prefix(_)
                )
            })
        {
            return Err(format!(
                "driver '{value}' leaves the plugin folder; a driver is a relative path inside it"
            ));
        }
        let full = dir.join(relative);
        let canonical = full.canonicalize().map_err(|_| {
            format!(
                "driver '{value}' is not in the plugin folder (expected {})",
                full.display()
            )
        })?;
        let root = dir.canonicalize().unwrap_or_else(|_| dir.to_path_buf());
        if !canonical.starts_with(&root) {
            return Err(format!("driver '{value}' leaves the plugin folder"));
        }
        Ok(canonical)
    }
}

/// An installed plugin's harness, ready for the registry: everything the
/// daemon needs to probe and spawn its driver, plus the reason it cannot when
/// something is wrong.
#[derive(Clone, Debug)]
pub struct HarnessSpec {
    pub id: String,
    pub name: String,
    /// `name@marketplace`, the plugin that provides it.
    pub plugin: String,
    pub dir: PathBuf,
    /// The resolved driver path; empty when `problem` explains why there is
    /// none.
    pub driver: PathBuf,
    pub args: Vec<String>,
    pub models: Option<Vec<ModelDescriptor>>,
    pub default_model: Option<String>,
    /// When the catalog above was captured, when it came from the driver's
    /// own file. `None` means the list is the manifest's, unmeasured.
    pub models_captured_at: Option<u64>,
    /// Whether the driver can change the model on a running chat.
    pub settings_live: bool,
    /// Its permission words, fast mode, MCP, command menus and prompt rules.
    pub traits: HarnessTraits,
    pub enabled: bool,
    /// Why this harness cannot be offered, in one sentence the console shows.
    pub problem: Option<String>,
}

/// The harnesses the installed plugins on this machine declare. A plugin whose
/// folder no longer reads gets a spec with the reason, so the console can say
/// why the harness it used to offer is gone instead of hiding it.
pub fn installed_harnesses(data_dir: &Path, installed: &[&InstalledPlugin]) -> Vec<HarnessSpec> {
    let mut specs = Vec::new();
    for plugin in installed {
        let Some(recorded) = &plugin.harness else {
            continue;
        };
        let dir = plugin_dir(data_dir, &plugin.marketplace, &plugin.name);
        let (manifest, problem) = match read_manifest(&dir, &[]) {
            Ok(manifest) => match manifest.harness {
                Some(harness) => (Some(harness), None),
                None => (
                    None,
                    Some("the plugin no longer declares a harness".to_string()),
                ),
            },
            Err(err) => (
                None,
                Some(format!("the plugin folder no longer reads: {err}")),
            ),
        };
        let Some(harness) = manifest else {
            specs.push(HarnessSpec {
                id: recorded.id.clone(),
                name: recorded.name.clone(),
                plugin: plugin.id(),
                dir,
                driver: PathBuf::new(),
                args: Vec::new(),
                models: None,
                default_model: None,
                models_captured_at: None,
                settings_live: false,
                traits: HarnessTraits::default(),
                enabled: plugin.enabled,
                problem,
            });
            continue;
        };
        let traits = harness.traits();
        let (driver, problem) = match harness.resolved_driver(&dir) {
            Ok(path) => (path, None),
            Err(reason) => (PathBuf::new(), Some(reason)),
        };
        // A catalog the driver captured wins over the manifest's list: it is
        // the machine's own answer and it carries the moment it was read. An
        // absent, malformed or escaping file falls back to the manifest.
        let (models, models_captured_at) = match harness.models_file.as_deref() {
            Some(name) => match read_catalog(&dir, name) {
                Some((captured_at, models)) => (Some(models), Some(captured_at)),
                None => (harness.models, None),
            },
            None => (harness.models, None),
        };
        specs.push(HarnessSpec {
            id: harness.id,
            name: harness.name,
            plugin: plugin.id(),
            dir,
            driver,
            args: harness.args,
            models,
            default_model: harness.default_model,
            models_captured_at,
            settings_live: harness.settings.as_deref() == Some(SETTINGS_LIVE),
            traits,
            enabled: plugin.enabled,
            problem,
        });
    }
    specs
}

/// The one word a harness may declare for live settings.
const SETTINGS_LIVE: &str = "live";

/// Read a catalog file the driver wrote inside the plugin folder.
///
/// The file is a capture, not source: it is rewritten by the driver and can be
/// missing, half written, written by an older build or pointed somewhere else.
/// Every one of those answers `None` and leaves the manifest's list in place,
/// because a harness with no models is worse than one with a shipped list.
fn read_catalog(dir: &Path, name: &str) -> Option<(u64, Vec<ModelDescriptor>)> {
    let relative = Path::new(name);
    if relative.is_absolute()
        || relative.components().any(|c| {
            matches!(
                c,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return None;
    }
    let full = dir.join(relative);
    let canonical = full.canonicalize().ok()?;
    let root = dir.canonicalize().unwrap_or_else(|_| dir.to_path_buf());
    if !canonical.starts_with(&root) {
        return None;
    }
    let bytes = std::fs::read(&canonical).ok()?;
    let value: Value = serde_json::from_slice(&bytes).ok()?;
    let captured_at = value.get("captured_at")?.as_u64()?;
    let mut models: Vec<ModelDescriptor> =
        serde_json::from_value(value.get("models")?.clone()).ok()?;
    models.retain(|model| !model.model.trim().is_empty());
    if models.is_empty() {
        return None;
    }
    Some((captured_at, models))
}

#[derive(Deserialize, Default)]
struct ClaudeManifest {
    name: Option<String>,
    version: Option<String>,
    description: Option<String>,
    hooks: Option<Value>,
    #[serde(rename = "mcpServers")]
    mcp_servers: Option<Value>,
    #[serde(rename = "lspServers")]
    lsp_servers: Option<Value>,
}

#[derive(Deserialize)]
struct OpenRemoteManifest {
    api: u64,
    name: Option<String>,
    version: Option<String>,
    description: Option<String>,
    runtime: Option<String>,
    #[serde(default)]
    powers: Vec<String>,
    #[serde(default)]
    harness: Option<HarnessManifest>,
}

/// The `harness` block's api this daemon speaks.
pub const HARNESS_API_VERSION: u64 = 1;

/// Check a harness block before its plugin may be installed. The same checks
/// run when the folder is re-read at probe time, so a folder edited on disk
/// cannot smuggle in a new id or a driver outside itself.
fn validate_harness(
    harness: &HarnessManifest,
    builtin_ids: &[String],
    plugin_name: &str,
) -> Result<(), PluginError> {
    if harness.api != HARNESS_API_VERSION {
        return Err(PluginError::Invalid(format!(
            "the plugin's harness says api {}; this OpenRemote speaks harness api {HARNESS_API_VERSION}",
            harness.api
        )));
    }
    if !valid_name(&harness.id) {
        return Err(PluginError::Invalid(format!(
            "'{}' is not a harness id: lowercase letters, digits and dashes, up to 64",
            harness.id
        )));
    }
    if builtin_ids.iter().any(|b| b == &harness.id) || RESERVED_NAMES.contains(&harness.id.as_str())
    {
        return Err(PluginError::Invalid(format!(
            "harness id '{}' is taken by OpenRemote itself",
            harness.id
        )));
    }
    if harness.name.trim().is_empty() {
        return Err(PluginError::Invalid(
            "the plugin's harness names no display name".into(),
        ));
    }
    if harness.driver_path().is_none() {
        return Err(PluginError::Invalid(format!(
            "the plugin's harness names no driver for {} (add one, or a default)",
            std::env::consts::OS
        )));
    }
    if let Some(word) = harness.settings.as_deref() {
        if word != SETTINGS_LIVE {
            return Err(PluginError::Invalid(format!(
                "the plugin's harness says settings '{word}'; the only word OpenRemote \
                 knows is '{SETTINGS_LIVE}'"
            )));
        }
    }
    let words: Vec<PermissionWord> = harness
        .permission_modes
        .iter()
        .map(ManifestPermission::row)
        .collect();
    for (index, row) in words.iter().enumerate() {
        if row.word.trim().is_empty() || row.label.trim().is_empty() {
            return Err(PluginError::Invalid(
                "the plugin's harness lists a permission word with no word or no label".into(),
            ));
        }
        if words[..index]
            .iter()
            .any(|earlier| earlier.word == row.word)
        {
            return Err(PluginError::Invalid(format!(
                "the plugin's harness lists the permission word '{}' twice",
                row.word
            )));
        }
    }
    if let Some(default) = harness.default_permission.as_deref() {
        if !words.iter().any(|row| row.word == default) {
            return Err(PluginError::Invalid(format!(
                "the plugin's harness starts with permission '{default}', which its \
                 permission_modes do not list"
            )));
        }
    }
    if harness.steers_mid_turn && harness.one_prompt_at_a_time == Some(true) {
        return Err(PluginError::Invalid(
            "the plugin's harness says it both steers mid-turn and takes one prompt at a \
             time; a harness that steers takes the second message inside the turn"
                .into(),
        ));
    }
    // The id and the plugin's own name may match (a plugin called freebuff
    // provides harness freebuff); only the built-in ids are reserved.
    let _ = plugin_name;
    Ok(())
}

fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<Option<T>, PluginError> {
    match std::fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes).map(Some).map_err(|e| {
            PluginError::Invalid(format!(
                "{} is not valid: {e}",
                path.file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("manifest")
            ))
        }),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.into()),
    }
}

/// Claude's own rule for a plugin name: kebab-case, short. A name passes
/// this before it may become a folder under `<data dir>/plugins/
/// <marketplace>/`, whoever supplied it - the plugin's own manifest, or the
/// catalog entry a marketplace install came from.
pub fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

pub fn power(word: &str) -> Option<PluginPower> {
    Some(match word {
        "run_programs" => PluginPower::RunPrograms,
        "read_chats" => PluginPower::ReadChats,
        "agent_tools" => PluginPower::AgentTools,
        "network" => PluginPower::Network,
        "microphone" => PluginPower::Microphone,
        _ => return None,
    })
}

/// openremote.json's runtime word.
pub fn runtime(word: &str) -> Option<PluginRuntime> {
    Some(match word {
        "process" => PluginRuntime::Process,
        "js" => PluginRuntime::Js,
        "wasm" => PluginRuntime::Wasm,
        "content" => PluginRuntime::Content,
        _ => return None,
    })
}

/// Read a plugin folder's manifests. `builtin_ids` are the harness ids no
/// installed plugin may take as its name.
pub fn read_manifest(dir: &Path, builtin_ids: &[String]) -> Result<Manifest, PluginError> {
    let claude: Option<ClaudeManifest> =
        read_json(&dir.join(".claude-plugin").join("plugin.json"))?;
    let ours: Option<OpenRemoteManifest> = read_json(&dir.join("openremote.json"))?;
    if claude.is_none() && ours.is_none() && claude_dirs(dir).is_empty() {
        return Err(PluginError::Invalid(
            "this is not a plugin: it has neither openremote.json nor .claude-plugin/plugin.json"
                .into(),
        ));
    }
    if let Some(o) = &ours {
        if o.api != API_VERSION {
            return Err(PluginError::Invalid(format!(
                "openremote.json says api {}; this OpenRemote speaks api {API_VERSION}",
                o.api
            )));
        }
    }
    let claude = claude.unwrap_or_default();
    let name = ours
        .as_ref()
        .and_then(|o| o.name.clone())
        .or(claude.name.clone())
        .ok_or_else(|| PluginError::Invalid("the plugin names itself nowhere".into()))?;
    if !valid_name(&name) {
        return Err(PluginError::Invalid(format!(
            "'{name}' is not a plugin name: lowercase letters, digits and dashes, up to 64"
        )));
    }
    if builtin_ids.iter().any(|b| b == &name) || RESERVED_NAMES.contains(&name.as_str()) {
        return Err(PluginError::Invalid(format!(
            "'{name}' is taken by OpenRemote itself"
        )));
    }
    let harness = ours.as_ref().and_then(|o| o.harness.clone());
    if let Some(harness) = &harness {
        validate_harness(harness, builtin_ids, &name)?;
    }

    let has_hooks = claude.hooks.is_some() || dir.join("hooks").join("hooks.json").is_file();
    let has_mcp = claude.mcp_servers.is_some() || dir.join(".mcp.json").is_file();
    let has_lsp = claude.lsp_servers.is_some() || dir.join(".lsp.json").is_file();
    let mut claude_parts = claude_dirs(dir);
    if has_hooks {
        claude_parts.push("hooks".into());
    }
    if has_mcp {
        claude_parts.push("mcp".into());
    }

    let runtime = match &ours {
        None => PluginRuntime::Content,
        Some(o) => match o.runtime.as_deref() {
            Some(word) => runtime(word).ok_or_else(|| {
                PluginError::Invalid(format!(
                    "openremote.json names runtime '{word}'; this OpenRemote runs process, js, wasm or content"
                ))
            })?,
            None => {
                return Err(PluginError::Invalid(
                    "openremote.json names no runtime".into(),
                ));
            }
        },
    };

    let mut powers = Vec::new();
    if let Some(o) = &ours {
        for word in &o.powers {
            powers.push(power(word).ok_or_else(|| {
                PluginError::Invalid(format!(
                    "openremote.json asks for '{word}'; the powers are run_programs, read_chats, agent_tools, network and microphone"
                ))
            })?);
        }
    }
    if runtime == PluginRuntime::Process || has_hooks || has_mcp || has_lsp {
        powers.push(PluginPower::RunPrograms);
    }
    powers.sort();
    powers.dedup();
    let not_sandboxed = powers.contains(&PluginPower::RunPrograms)
        && matches!(runtime, PluginRuntime::Process | PluginRuntime::Content);

    Ok(Manifest {
        version: ours
            .as_ref()
            .and_then(|o| o.version.clone())
            .or(claude.version),
        description: ours
            .as_ref()
            .and_then(|o| o.description.clone())
            .or(claude.description),
        name,
        runtime,
        powers,
        claude_parts,
        not_sandboxed,
        harness,
    })
}

/// The Claude part folders a plugin carries.
fn claude_dirs(dir: &Path) -> Vec<String> {
    ["skills", "agents", "commands"]
        .into_iter()
        .filter(|part| dir.join(part).is_dir())
        .map(String::from)
        .collect()
}

/// Where an installed plugin's folder lives.
pub fn plugin_dir(data_dir: &Path, marketplace: &str, name: &str) -> PathBuf {
    data_dir.join("plugins").join(marketplace).join(name)
}

/// A marketplace's name as one path segment: the characters a marketplace
/// name may carry, and never `.` or `..`.
fn safe_segment(name: &str) -> bool {
    one_segment(name)
        && !name.contains("..")
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
}

/// A name that is one path segment, whatever else it carries: no
/// separator, no `..`. Removal is held to this rather than to the
/// plugin-name gate, so a folder installed before that gate existed - or
/// one a catalog once talked this daemon into creating - can still be
/// removed, while nothing can reach outside the plugins folder.
///
/// Two shapes are refused on top of that: a name that ends in a dot, and
/// one that surrounding whitespace makes differ from its trimmed form.
/// Windows resolves `...`, `a.` and `a ` to `.`, `a` and `a`, so a segment
/// of those shapes names a different folder than the record does, and a
/// removal would then land on something the record never meant.
fn one_segment(name: &str) -> bool {
    !name.is_empty()
        && name.chars().count() <= 128
        && name.trim() == name
        && !name.ends_with('.')
        && !name.contains(['/', '\\', ':', '\0'])
}

/// An install's folder, with both segments gated before they become a
/// path. `plugin_dir` joins whatever it is given, and the answer decides
/// which folder an install replaces or an update swaps into, so a name
/// that came from a catalog - or from an old `state.json` - is checked
/// again here rather than trusted.
fn plugin_dir_checked(
    data_dir: &Path,
    marketplace: &str,
    name: &str,
) -> Result<PathBuf, PluginError> {
    if !valid_name(name) {
        return Err(PluginError::Invalid(format!(
            "'{name}' is not a plugin name: lowercase letters, digits and dashes, up to 64"
        )));
    }
    if !safe_segment(marketplace) {
        return Err(PluginError::Invalid(format!(
            "'{marketplace}' is not a marketplace name"
        )));
    }
    Ok(plugin_dir(data_dir, marketplace, name))
}

/// What a plugin offers a composer, read from its own folder the way
/// Claude Code names it: skills and command files as `plugin:name`,
/// agents as `plugin:name`, and its MCP servers by their keys.
#[derive(Default, Debug, PartialEq, Eq)]
pub struct PluginParts {
    pub commands: Vec<openremote_harness::CommandDescriptor>,
    /// Which of `commands` are skills (skills/<name>/SKILL.md).
    pub skills: Vec<String>,
    pub agents: Vec<openremote_harness::AgentDescriptor>,
    pub mcp_servers: Vec<String>,
}

/// Read a plugin folder's commands, agents and MCP server names. `prefix`
/// is the name its commands carry (the manifest's name).
pub fn plugin_parts(dir: &Path, prefix: &str) -> PluginParts {
    let mut parts = PluginParts::default();
    // skills/<name>/SKILL.md: one command each, unless the skill is for
    // the model alone (`user-invocable: false`).
    if let Ok(entries) = std::fs::read_dir(dir.join("skills")) {
        let mut found: Vec<PathBuf> = entries.flatten().map(|e| e.path()).collect();
        found.sort();
        for skill in found {
            let Ok(text) = std::fs::read_to_string(skill.join("SKILL.md")) else {
                continue;
            };
            let meta = frontmatter(&text);
            if meta.get("user-invocable").map(String::as_str) == Some("false") {
                continue;
            }
            let name = meta.get("name").cloned().unwrap_or_else(|| {
                skill
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default()
            });
            parts.skills.push(format!("{prefix}:{name}"));
            parts.commands.push(openremote_harness::CommandDescriptor {
                name: format!("{prefix}:{name}"),
                description: meta.get("description").cloned(),
                argument_hint: meta.get("argument-hint").cloned(),
                aliases: Vec::new(),
            });
        }
    }
    // commands/<name>.md, a folder deeper joined with ':' as Claude does.
    let mut files = Vec::new();
    markdown_files(&dir.join("commands"), &mut files);
    for file in files {
        let Ok(relative) = file.strip_prefix(dir.join("commands")) else {
            continue;
        };
        let name = relative
            .with_extension("")
            .to_string_lossy()
            .replace(['\\', '/'], ":");
        let meta = std::fs::read_to_string(&file)
            .map(|t| frontmatter(&t))
            .unwrap_or_default();
        parts.commands.push(openremote_harness::CommandDescriptor {
            name: format!("{prefix}:{name}"),
            description: meta.get("description").cloned(),
            argument_hint: meta.get("argument-hint").cloned(),
            aliases: Vec::new(),
        });
    }
    let mut agents = Vec::new();
    markdown_files(&dir.join("agents"), &mut agents);
    for file in agents {
        let meta = std::fs::read_to_string(&file)
            .map(|t| frontmatter(&t))
            .unwrap_or_default();
        let name = meta.get("name").cloned().unwrap_or_else(|| {
            file.file_stem()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default()
        });
        parts.agents.push(openremote_harness::AgentDescriptor {
            name: format!("{prefix}:{name}"),
            description: meta.get("description").cloned(),
        });
    }
    // .mcp.json (Claude's own file: `mcpServers` or the map itself) and
    // plugin.json's `mcpServers`.
    let mut servers = std::collections::BTreeSet::new();
    for (file, pointer) in [
        (dir.join(".mcp.json"), ""),
        (
            dir.join(".claude-plugin").join("plugin.json"),
            "/mcpServers",
        ),
    ] {
        let Ok(Some(value)) = read_json::<Value>(&file) else {
            continue;
        };
        let map = if pointer.is_empty() {
            value.get("mcpServers").unwrap_or(&value)
        } else {
            match value.pointer(pointer) {
                Some(v) => v,
                None => continue,
            }
        };
        if let Some(map) = map.as_object() {
            servers.extend(map.keys().cloned());
        }
    }
    parts.mcp_servers = servers.into_iter().collect();
    parts
}

/// Every `.md` under `dir`, in order.
fn markdown_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut paths: Vec<PathBuf> = entries.flatten().map(|e| e.path()).collect();
    paths.sort();
    for path in paths {
        if path.is_dir() {
            markdown_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "md") {
            out.push(path);
        }
    }
}

/// A markdown file's YAML frontmatter, read for the plain `key: value`
/// lines (and `>`/`|` blocks) the skill and command formats use.
pub fn frontmatter(text: &str) -> std::collections::HashMap<String, String> {
    let mut out = std::collections::HashMap::new();
    let mut lines = text.lines();
    if lines.next().map(str::trim) != Some("---") {
        return out;
    }
    let body: Vec<&str> = lines.take_while(|l| l.trim() != "---").collect();
    let mut i = 0;
    while i < body.len() {
        let line = body[i];
        i += 1;
        if line.starts_with(' ') || line.starts_with('\t') {
            continue;
        }
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let value = value.trim();
        let value = if value == ">" || value == "|" || value == ">-" || value == "|-" {
            let mut block = Vec::new();
            while i < body.len() && (body[i].starts_with(' ') || body[i].trim().is_empty()) {
                block.push(body[i].trim());
                i += 1;
            }
            block.join(if value.starts_with('|') { "\n" } else { " " })
        } else {
            value.trim_matches('"').trim_matches('\'').to_string()
        };
        out.insert(key.trim().to_string(), value.trim().to_string());
    }
    out
}

/// Where the user asked to install from.
#[derive(Deserialize, Clone, Debug)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SourceRequest {
    Folder {
        path: String,
    },
    Git {
        url: String,
    },
    /// A .zip or .tar.gz: a path on this machine or an http(s) URL.
    Archive {
        location: String,
    },
    /// An entry of an added marketplace, by names. A `command` entry runs
    /// only once `approve_command` repeats the exact command the user was
    /// shown.
    Marketplace {
        marketplace: String,
        plugin: String,
        #[serde(default)]
        approve_command: Option<String>,
    },
}

impl SourceRequest {
    /// A direct install's plan. None for a marketplace entry, which is
    /// resolved against its catalog first.
    pub fn direct(&self) -> Option<StageRequest> {
        let (fetch, recorded) = match self {
            SourceRequest::Folder { path } => (
                Fetch::Folder(PathBuf::from(path)),
                PluginSource::Folder { path: path.clone() },
            ),
            SourceRequest::Git { url } => (
                Fetch::Git {
                    url: url.clone(),
                    reference: None,
                    sha: None,
                    subdir: None,
                },
                PluginSource::Git { url: url.clone() },
            ),
            SourceRequest::Archive { location } => (
                Fetch::Archive {
                    location: location.clone(),
                    sha256: None,
                },
                PluginSource::Archive {
                    location: location.clone(),
                },
            ),
            SourceRequest::Marketplace { .. } => return None,
        };
        Some(StageRequest {
            fetch,
            marketplace: LOCAL_MARKETPLACE.to_string(),
            entry_name: None,
            entry: None,
            recorded,
        })
    }
}

/// What to fetch: a direct source, or what a marketplace entry names.
#[derive(Clone, Debug)]
pub enum Fetch {
    /// Copied in.
    Folder(PathBuf),
    /// Cloned at depth one, at a pinned commit or ref when there is one;
    /// `subdir` checks out that one folder (a plugin inside a monorepo).
    Git {
        url: String,
        reference: Option<String>,
        sha: Option<String>,
        subdir: Option<String>,
    },
    Archive {
        location: String,
        sha256: Option<String>,
    },
    /// Straight from the registry over https - never through an npm
    /// client, never running a package's scripts.
    Npm {
        package: String,
        version: Option<String>,
        registry: Option<String>,
    },
    /// Run the command the user approved; it prints the plugin folder.
    Command(ApprovedCommand),
}

/// An install to stage: what to fetch, and what the plugin will be called.
pub struct StageRequest {
    pub fetch: Fetch,
    pub marketplace: String,
    /// The marketplace entry's name - the id's first half. None for a
    /// direct install: the manifest names it.
    pub entry_name: Option<String>,
    /// The marketplace entry, for a fetched folder with no plugin.json
    /// (the entry is the manifest) or one the entry adds components to.
    pub entry: Option<Value>,
    pub recorded: PluginSource,
}

/// An install fetched and read, waiting for the user's approval.
pub struct Staged {
    dir: PathBuf,
    root: PathBuf,
    pub manifest: Manifest,
    pub name: String,
    pub marketplace: String,
    pub source: PluginSource,
    pub resolved: Option<String>,
    pub command: Option<ApprovedCommand>,
}

impl Staged {
    pub fn id(&self) -> String {
        format!("{}@{}", self.name, self.marketplace)
    }
}

/// The plugin folders on this machine, and the installs waiting for an
/// answer. A staged install that is never answered is swept with its
/// folder the next time the daemon starts.
pub struct PluginHost {
    data_dir: PathBuf,
    root: PathBuf,
    staged: StdMutex<HashMap<String, Staged>>,
}

impl PluginHost {
    pub fn new(data_dir: &Path) -> Self {
        let root = data_dir.join("plugins");
        let _ = std::fs::remove_dir_all(root.join(".staging"));
        let _ = std::fs::remove_dir_all(root.join(".trash"));
        file_flat_installs_under_local(&root);
        Self {
            data_dir: data_dir.to_path_buf(),
            root,
            staged: StdMutex::new(HashMap::new()),
        }
    }

    /// Fetch and read a plugin into a staging folder. Nothing is installed
    /// until `install` runs on the user's approval.
    pub async fn stage(
        &self,
        request: StageRequest,
        builtin_ids: Vec<String>,
    ) -> Result<(String, Manifest, String), PluginError> {
        // A marketplace install takes the catalog entry's name, not the
        // plugin's own, and that name becomes a folder. It passes the same
        // gate a manifest name passes, here, before anything is fetched or
        // written.
        if let Some(entry_name) = &request.entry_name {
            plugin_dir_checked(&self.data_dir, &request.marketplace, entry_name)?;
        }
        let id = uuid::Uuid::new_v4().to_string();
        let dir = self.root.join(".staging").join(&id);
        let fetched = dir.join("plugin");
        std::fs::create_dir_all(&dir)?;
        let outcome = async {
            let resolved = fetch(&request.fetch, &fetched).await?;
            let root = match &request.fetch {
                Fetch::Git {
                    subdir: Some(sub), ..
                } => {
                    let root = fetched.join(sub);
                    if !root.is_dir() {
                        return Err(PluginError::Source(format!(
                            "the repository has no folder {sub}"
                        )));
                    }
                    root
                }
                _ => plugin_root(&fetched),
            };
            if matches!(request.fetch, Fetch::Npm { .. }) {
                install_node_packages(&root).await?;
            }
            if let Some(entry) = &request.entry {
                apply_entry(&root, entry)?;
            }
            let manifest = read_manifest(&root, &builtin_ids)?;
            write_bare_claude_manifest(&root, &manifest)?;
            Ok((root, manifest, resolved))
        }
        .await;
        match outcome {
            Ok((root, manifest, resolved)) => {
                let name = request
                    .entry_name
                    .clone()
                    .unwrap_or_else(|| manifest.name.clone());
                let command = match &request.fetch {
                    Fetch::Command(approved) => Some(approved.clone()),
                    _ => None,
                };
                self.staged.lock().expect("staged lock").insert(
                    id.clone(),
                    Staged {
                        dir,
                        root,
                        manifest: manifest.clone(),
                        name: name.clone(),
                        marketplace: request.marketplace,
                        source: request.recorded,
                        resolved,
                        command,
                    },
                );
                Ok((id, manifest, name))
            }
            Err(e) => {
                let _ = std::fs::remove_dir_all(&dir);
                Err(e)
            }
        }
    }

    pub fn take_staged(&self, id: &str) -> Option<Staged> {
        self.staged.lock().expect("staged lock").remove(id)
    }

    pub fn cancel(&self, id: &str) -> bool {
        match self.take_staged(id) {
            Some(staged) => {
                let _ = std::fs::remove_dir_all(&staged.dir);
                true
            }
            None => false,
        }
    }

    /// Move an approved install into place. An installed plugin of the
    /// same id is replaced whole - an update never mixes two versions.
    pub fn install(&self, staged: Staged) -> Result<(), PluginError> {
        let result = plugin_dir_checked(&self.data_dir, &staged.marketplace, &staged.name)
            .and_then(|target| swap_in(&self.root, &staged.root, &target));
        let _ = std::fs::remove_dir_all(&staged.dir);
        result
    }

    /// Remove an installed plugin's folder, whatever its record says.
    ///
    /// The record names the folder, and a record written before the name
    /// gate existed may carry anything, so removal is held to its own rule
    /// rather than to the plugin-name gate: that keeps such a folder
    /// removable while nothing reaches outside the plugins folder.
    pub fn uninstall(&self, marketplace: &str, name: &str) -> Result<(), PluginError> {
        if one_segment(marketplace) {
            if one_segment(name) {
                return remove_plugin_folder(&self.data_dir, marketplace, name);
            }
            // Windows drops a trailing dot and a trailing space when it
            // makes a folder, so there a record carrying either was
            // installed under the trimmed name - `tidy.` is `tidy` on disk -
            // and refusing it would strand a plugin the user can no longer
            // remove anywhere. Everywhere else nothing was dropped at
            // install time: `tidy.` is a folder of its own there, and `tidy`
            // is a different plugin's folder in the same marketplace, so
            // trimming would delete a folder the record never named.
            let folder_name = installed_as(name, cfg!(windows));
            if folder_name != name && !folder_name.is_empty() && one_segment(folder_name) {
                return remove_plugin_folder(&self.data_dir, marketplace, folder_name);
            }
        }
        // The name is not one folder inside the plugins folder: it carries a
        // separator, a `..`, a drive letter, leading whitespace, nothing but
        // dots, or it is too long. Whether anything answers to it decides
        // which of the two remaining answers is right.
        let target = plugin_dir(&self.data_dir, marketplace, name);
        unremovable(marketplace, name, &target, target.try_exists())
    }
}

/// What to do about a record whose name is not one folder inside the plugins
/// folder, given what the file system said about the folder the name would
/// build.
///
/// Something there: refuse, because deleting it is not this daemon's call, and
/// a path built from such a record is exactly what the name gate exists to
/// stop. The message prints the path it will not touch, because deleting it
/// falls to the person, and calls it this plugin's folder rather than "that
/// folder": a name like `...` normalises to the marketplace folder itself on
/// Windows, where "delete that folder" would read as "delete every plugin in
/// here".
///
/// Nothing there: let the record go, which is how the plugin list gets rid of
/// one of these.
///
/// A record can be safe and still stuck: one whose name resolves to a folder
/// that is not this plugin's - `...`, say, which is the marketplace folder on
/// Windows - is refused every time it is asked, so it stays in the list and
/// nothing in the UI can drop it. The way out is the one the refusal names, by
/// hand, and for that shape it means deleting a folder this plugin does not
/// own. Leaving the record there is the price of never deleting a folder it did
/// not name, and it is the honest limit of this rule rather than a claim that
/// every record has a way out.
///
/// A check that failed: refuse too. A folder this daemon cannot stat is not an
/// absent folder, and dropping the record would leave it as an orphan nothing
/// looks at again.
fn unremovable(
    marketplace: &str,
    name: &str,
    target: &Path,
    checked: std::io::Result<bool>,
) -> Result<(), PluginError> {
    match checked {
        Ok(true) => Err(PluginError::Invalid(format!(
            "'{marketplace}/{name}' is not one folder inside the plugins folder - no separator, no '..', no leading space, no trailing dot, 128 characters at most. Its own folder would be {}, which OpenRemote will not delete: if that is this plugin's folder, delete it yourself, then remove the plugin again to drop its record",
            target.display()
        ))),
        Ok(false) => Ok(()),
        Err(e) => Err(PluginError::Source(format!(
            "could not check whether this plugin's folder {} is still there ({e}), so OpenRemote will not remove the plugin: try again once it can be read",
            target.display()
        ))),
    }
}

/// The name a record's folder was installed under. Windows drops trailing
/// dots and trailing spaces when it creates a folder, so there `tidy.` was
/// installed as `tidy`; every other system keeps them, so `tidy.` is the
/// folder's own name and `tidy` belongs to a different plugin. Taking the
/// system as an argument keeps the difference testable from either side.
fn installed_as(name: &str, windows: bool) -> &str {
    if windows {
        name.trim_end_matches(['.', ' '])
    } else {
        name
    }
}

/// One plugin's folder, removed. A folder that is not there was already
/// removed.
fn remove_plugin_folder(data_dir: &Path, marketplace: &str, name: &str) -> Result<(), PluginError> {
    match std::fs::remove_dir_all(plugin_dir(data_dir, marketplace, name)) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.into()),
    }
}

/// Put `from` at `target`, replacing what is there whole: the old folder
/// moves aside first, so a failure never leaves half of each.
pub(crate) fn swap_in(plugins_root: &Path, from: &Path, target: &Path) -> Result<(), PluginError> {
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent)?;
    }
    if target.exists() {
        let trash = plugins_root
            .join(".trash")
            .join(uuid::Uuid::new_v4().to_string());
        std::fs::create_dir_all(trash.parent().expect("trash parent"))?;
        std::fs::rename(target, &trash)?;
        if let Err(e) = std::fs::rename(from, target) {
            // The old copy is in .trash and nothing ever looks in there
            // again: a failed rename (a running chat holds the folder on
            // Windows) would leave the plugin gone while the store still
            // lists it as installed and on. Put it back, and when even
            // that fails, say where the copy is: the next start empties
            // .trash, so a silent failure here would lose it for good
            // while reporting only the first error.
            if let Err(back) = std::fs::rename(&trash, target) {
                return Err(PluginError::Source(format!(
                    "could not replace it ({e}) and could not put the old copy back ({back}); it is in {}, copy it out before the next start",
                    trash.display()
                )));
            }
            return Err(e.into());
        }
        let _ = std::fs::remove_dir_all(&trash);
    } else {
        std::fs::rename(from, target)?;
    }
    Ok(())
}

/// The first plugin builds put a plugin straight under `plugins/<name>`;
/// they are direct installs, so they move under `plugins/local/`. A
/// marketplace's folder never has a manifest at its top, a plugin's does.
fn file_flat_installs_under_local(root: &Path) {
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        if name.to_string_lossy().starts_with('.') || !path.is_dir() {
            continue;
        }
        if has_manifest(&path) {
            let target = root.join(LOCAL_MARKETPLACE).join(&name);
            if !target.exists() {
                let _ = std::fs::create_dir_all(root.join(LOCAL_MARKETPLACE));
                let _ = std::fs::rename(&path, &target);
            }
        }
    }
}

fn has_manifest(dir: &Path) -> bool {
    dir.join("openremote.json").is_file()
        || dir.join(".claude-plugin").join("plugin.json").is_file()
}

/// A plugin is the fetched folder itself, or - the common archive and
/// repo shape - the one folder inside it that holds the manifests.
fn plugin_root(fetched: &Path) -> PathBuf {
    if has_manifest(fetched) {
        return fetched.to_path_buf();
    }
    let dirs: Vec<PathBuf> = std::fs::read_dir(fetched)
        .map(|it| {
            it.filter_map(|e| e.ok())
                .map(|e| e.path())
                .filter(|p| p.is_dir())
                .collect()
        })
        .unwrap_or_default();
    match dirs.as_slice() {
        [only] if has_manifest(only) => only.clone(),
        _ => fetched.to_path_buf(),
    }
}

/// Fetch into `to`; what exactly arrived (a commit, a digest, a version,
/// a content hash) is what an update compares against.
async fn fetch(fetch: &Fetch, to: &Path) -> Result<Option<String>, PluginError> {
    match fetch {
        Fetch::Folder(from) => {
            if !from.is_dir() {
                return Err(PluginError::Source(format!(
                    "{} is not a folder",
                    from.display()
                )));
            }
            copy_in(from.clone(), to.to_path_buf()).await?;
            Ok(Some(content_hash(to)?))
        }
        Fetch::Git {
            url,
            reference,
            sha,
            subdir,
        } => git_fetch(
            url,
            reference.as_deref(),
            sha.as_deref(),
            subdir.as_deref(),
            to,
        )
        .await
        .map(Some),
        Fetch::Archive { location, sha256 } => {
            let bytes = read_archive(location).await?;
            let digest = hex(&sha2::Sha256::digest(&bytes));
            if let Some(want) = sha256 {
                if !digest.eq_ignore_ascii_case(want) {
                    return Err(PluginError::Source(format!(
                        "the archive's sha256 is {digest}, but its entry pins {want} - refusing a changed download"
                    )));
                }
            }
            let to = to.to_path_buf();
            tokio::task::spawn_blocking(move || unpack(&bytes, &to))
                .await
                .map_err(|e| PluginError::Source(e.to_string()))??;
            Ok(Some(digest))
        }
        Fetch::Npm {
            package,
            version,
            registry,
        } => npm_fetch(package, version.as_deref(), registry.as_deref(), to)
            .await
            .map(Some),
        Fetch::Command(approved) => {
            let printed = run_command(approved).await?;
            copy_in(printed, to.to_path_buf()).await?;
            Ok(Some(content_hash(to)?))
        }
    }
}

async fn copy_in(from: PathBuf, to: PathBuf) -> Result<(), PluginError> {
    tokio::task::spawn_blocking(move || {
        let mut budget = CopyBudget::default();
        copy_tree(&from, &to, &mut budget)
    })
    .await
    .map_err(|e| PluginError::Source(e.to_string()))?
}

#[derive(Default)]
struct CopyBudget {
    entries: usize,
    bytes: u64,
}

/// Copy a folder, leaving out its git history and anything that is not a
/// plain file or folder (a link could reach outside the plugin), within
/// the size limits.
fn copy_tree(from: &Path, to: &Path, budget: &mut CopyBudget) -> Result<(), PluginError> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let kind = entry.file_type()?;
        let name = entry.file_name();
        if name == ".git" {
            continue;
        }
        budget.entries += 1;
        if budget.entries > MAX_ENTRIES {
            return Err(PluginError::Source(format!(
                "the folder has more than {MAX_ENTRIES} files and folders"
            )));
        }
        if kind.is_dir() {
            copy_tree(&entry.path(), &to.join(&name), budget)?;
        } else if kind.is_file() {
            budget.bytes += entry.metadata()?.len();
            if budget.bytes > MAX_ARCHIVE_BYTES {
                return Err(PluginError::Source("the folder is too large".into()));
            }
            std::fs::copy(entry.path(), to.join(&name))?;
        }
    }
    Ok(())
}

/// A folder's content as one digest: every plain file's path and bytes,
/// in order, git history and links left out (as copying leaves them out).
pub fn content_hash(dir: &Path) -> Result<String, PluginError> {
    fn walk(dir: &Path, files: &mut Vec<PathBuf>) -> std::io::Result<()> {
        for entry in std::fs::read_dir(dir)? {
            let entry = entry?;
            let kind = entry.file_type()?;
            if entry.file_name() == ".git" {
                continue;
            }
            if kind.is_dir() {
                walk(&entry.path(), files)?;
            } else if kind.is_file() {
                files.push(entry.path());
            }
        }
        Ok(())
    }
    let mut files = Vec::new();
    walk(dir, &mut files)?;
    files.sort();
    let mut hasher = sha2::Sha256::new();
    for file in files {
        let relative = file.strip_prefix(dir).unwrap_or(&file);
        hasher.update(relative.to_string_lossy().replace('\\', "/").as_bytes());
        hasher.update([0]);
        hasher.update(std::fs::read(&file)?);
        hasher.update([0]);
    }
    Ok(hex(&hasher.finalize()))
}

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// The argument list git is started with: the neutral settings first, then
/// the call's own words. Kept apart from the spawn so that "every call is
/// neutral" is a thing a test can see without starting git.
fn git_argv(args: &[&str]) -> Vec<String> {
    GIT_NEUTRAL
        .iter()
        .chain(args.iter())
        .map(|a| (*a).to_string())
        .collect()
}

async fn git(dir: &Path, args: &[&str]) -> Result<String, PluginError> {
    // Every git call this daemon makes is one step of fetching a plugin,
    // and none of them should be shaped by the machine's git
    // configuration: the neutral arguments go first, before the
    // subcommand.
    let all = git_argv(args);
    let output = tokio::process::Command::new("git")
        .args(&all)
        .current_dir(dir)
        .stdin(std::process::Stdio::null())
        // git asks for nothing: a private repo it has no stored
        // credential for fails instead of waiting on a prompt.
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .await
        .map_err(|e| {
            PluginError::Source(if e.kind() == std::io::ErrorKind::NotFound {
                "installing from git needs git on this machine".into()
            } else {
                format!("git could not start: {e}")
            })
        })?;
    if !output.status.success() {
        let words = String::from_utf8_lossy(&output.stderr);
        return Err(PluginError::Source(
            words
                .trim()
                .lines()
                .last()
                .unwrap_or("git gave no reason")
                .to_string(),
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// A pinned commit: hex, 7 to 64 characters (git's shortest abbreviation
/// up to a full sha256 id). Hex excludes a leading dash by itself, which
/// is the point: git parses options anywhere on its command line, so a
/// `sha` of `--upload-pack=<program>` would make git run that program.
pub fn is_commit_id(sha: &str) -> bool {
    (7..=64).contains(&sha.len()) && sha.chars().all(|c| c.is_ascii_hexdigit())
}

/// A value from outside that git would read as an option if it led with a
/// dash - a URL, a branch or tag, a folder inside a repository.
fn not_an_option(word: &str) -> bool {
    !word.starts_with('-')
}

/// `git` arguments that make it deaf to the machine's own configuration
/// for the two settings that would otherwise change what a plugin fetch
/// does: a repository's `.gitmodules` must stay inert (whether checkout
/// recurses into submodules is `submodule.recurse`, which a user may have
/// turned on), and a `ext::` URL must never reach an external program
/// (git's own escape hatch, off by default and therefore worth stating).
const GIT_NEUTRAL: &[&str] = &[
    "-c",
    "submodule.recurse=false",
    "-c",
    "protocol.ext.allow=never",
];

/// Fetch one commit of a repository at depth one - the pinned `sha`, else
/// the `reference` (a branch or tag), else the default branch - with only
/// `subdir` checked out when one is named, and its history dropped.
/// Answers the commit that arrived.
pub async fn git_fetch(
    url: &str,
    reference: Option<&str>,
    sha: Option<&str>,
    subdir: Option<&str>,
    to: &Path,
) -> Result<String, PluginError> {
    // Every value here can come from a catalog, and git parses options
    // anywhere: each one is checked, and the ones that become positional
    // arguments are passed after `--` below, so nothing can be read as an
    // option. This runs before git starts, i.e. before the staged plugin
    // review - a program git was tricked into starting would run first.
    if !not_an_option(url) {
        return Err(PluginError::Invalid(format!("'{url}' is not a git URL")));
    }
    if let Some(reference) = reference {
        if !not_an_option(reference) {
            return Err(PluginError::Invalid(format!(
                "'{reference}' is not a branch or tag"
            )));
        }
    }
    if let Some(sha) = sha {
        if !is_commit_id(sha) {
            return Err(PluginError::Invalid(format!(
                "'{sha}' is not a commit id: hex, 7 to 64 characters"
            )));
        }
    }
    if let Some(sub) = subdir {
        if !relative_inside(sub) || !not_an_option(sub) {
            return Err(PluginError::Invalid(format!(
                "'{sub}' is not a folder inside the repository"
            )));
        }
    }
    std::fs::create_dir_all(to)?;
    let failed = |e: PluginError| PluginError::Source(format!("git could not fetch {url}: {e}"));
    git(to, &["init", "--quiet"]).await.map_err(failed)?;
    git(to, &["remote", "add", "--", "origin", url])
        .await
        .map_err(failed)?;
    if let Some(sub) = subdir {
        git(to, &["sparse-checkout", "init", "--cone"])
            .await
            .map_err(failed)?;
        git(to, &["sparse-checkout", "set", "--", sub])
            .await
            .map_err(failed)?;
    }
    let want = sha.or(reference).unwrap_or("HEAD");
    git(
        to,
        &["fetch", "--quiet", "--depth", "1", "--", "origin", want],
    )
    .await
    .map_err(failed)?;
    git(to, &["checkout", "--quiet", "FETCH_HEAD"])
        .await
        .map_err(failed)?;
    let commit = git(to, &["rev-parse", "HEAD"]).await.map_err(failed)?;
    if let Some(sha) = sha {
        if !commit.eq_ignore_ascii_case(sha) {
            return Err(PluginError::Source(format!(
                "{url} answered commit {commit}, not the pinned {sha}"
            )));
        }
    }
    let _ = std::fs::remove_dir_all(to.join(".git"));
    Ok(commit)
}

/// The commit a branch or tag (or the default branch) points at now,
/// without fetching anything.
pub async fn git_head(url: &str, reference: Option<&str>) -> Result<String, PluginError> {
    // `ls-remote` takes the same option set as `fetch`, `--upload-pack`
    // included, so both values pass the same gate and the positional ones
    // go after `--`.
    if !not_an_option(url) {
        return Err(PluginError::Invalid(format!("'{url}' is not a git URL")));
    }
    if let Some(reference) = reference {
        if !not_an_option(reference) {
            return Err(PluginError::Invalid(format!(
                "'{reference}' is not a branch or tag"
            )));
        }
    }
    let here = std::env::temp_dir();
    let out = git(
        &here,
        &["ls-remote", "--", url, reference.unwrap_or("HEAD")],
    )
    .await?;
    out.split_whitespace()
        .next()
        .map(String::from)
        .ok_or_else(|| PluginError::Source(format!("{url} has no {}", reference.unwrap_or("HEAD"))))
}

/// A path that stays inside its folder: relative, no `..`, forward
/// slashes.
pub fn relative_inside(path: &str) -> bool {
    !path.is_empty()
        && !path.starts_with('/')
        && !path.contains('\\')
        && !path.contains(':')
        && path.split('/').all(|part| part != "..")
}

/// GET over http(s), following a few redirects (release, repo and
/// registry links redirect once or twice), refused past `limit` bytes.
pub async fn http_get(location: &str, accept: &str, limit: u64) -> Result<Vec<u8>, PluginError> {
    let http = openremote_cloud::https::Http::new();
    let mut url = location.to_string();
    for _ in 0..5 {
        let response = http
            .request(
                "GET",
                &url,
                &[("user-agent", "OpenRemote"), ("accept", accept)],
                None,
            )
            .await
            .map_err(|e| PluginError::Source(format!("could not download {url}: {e}")))?;
        match response.status {
            200 => {
                if response.body.len() as u64 > limit {
                    return Err(PluginError::Source(format!("{url} is too large")));
                }
                return Ok(response.body);
            }
            301 | 302 | 303 | 307 | 308 => {
                url = response
                    .headers
                    .get("location")
                    .and_then(|v| v.to_str().ok())
                    .map(String::from)
                    .ok_or_else(|| PluginError::Source(format!("{url} redirects nowhere")))?;
            }
            status => {
                return Err(PluginError::Source(format!("{url} answered {status}")));
            }
        }
    }
    Err(PluginError::Source(format!(
        "{location} redirects too many times"
    )))
}

/// An archive's bytes: from a path on this machine, or downloaded.
async fn read_archive(location: &str) -> Result<Vec<u8>, PluginError> {
    if !(location.starts_with("https://") || location.starts_with("http://")) {
        let meta = std::fs::metadata(location)
            .map_err(|_| PluginError::Source(format!("{location} is not a file")))?;
        if meta.len() > MAX_ARCHIVE_BYTES {
            return Err(PluginError::Source("the archive is too large".into()));
        }
        return Ok(std::fs::read(location)?);
    }
    http_get(location, "*/*", MAX_ARCHIVE_BYTES).await
}

/// An npm package straight from its registry: the package's metadata, the
/// version the entry asks for (exact, a dist-tag, or a range), its
/// tarball - checked against the registry's integrity digest - unpacked.
/// Answers the version installed.
async fn npm_fetch(
    package: &str,
    version: Option<&str>,
    registry: Option<&str>,
    to: &Path,
) -> Result<String, PluginError> {
    let (tarball, integrity, resolved) = if package.starts_with("https://")
        || package.starts_with("http://")
    {
        (package.to_string(), None, package.to_string())
    } else {
        let (name, inline) = split_package(package);
        let registry = registry
            .unwrap_or("https://registry.npmjs.org")
            .trim_end_matches('/');
        let url = format!("{registry}/{}", name.replace('/', "%2f"));
        let body = http_get(&url, "application/json", 64 * 1024 * 1024).await?;
        let meta: Value = serde_json::from_slice(&body)
            .map_err(|e| PluginError::Source(format!("{url} is not package metadata: {e}")))?;
        let spec = inline.or(version).unwrap_or("latest");
        let chosen = npm_pick(&meta, spec).ok_or_else(|| {
            PluginError::Source(format!("{name} has no version matching '{spec}'"))
        })?;
        let dist = &meta["versions"][&chosen]["dist"];
        let tarball = dist["tarball"]
            .as_str()
            .ok_or_else(|| PluginError::Source(format!("{name} {chosen} has no tarball")))?
            .to_string();
        // A registry that offers no digest at all gets no install: an
        // unchecked tarball is unpacked into the staging folder and
        // read as a plugin, and an entry may name its own registry.
        let integrity = dist["integrity"]
                .as_str()
                .map(|i| Integrity::Sri(i.to_string()))
                .or_else(|| {
                    dist["shasum"]
                        .as_str()
                        .map(|s| Integrity::Sha1(s.to_string()))
                })
                .ok_or_else(|| {
                    PluginError::Source(format!(
                        "{name} {chosen} names no integrity digest, so the download cannot be checked - refusing it"
                    ))
                })?;
        (tarball, Some(integrity), chosen)
    };
    let bytes = http_get(&tarball, "*/*", MAX_ARCHIVE_BYTES).await?;
    if let Some(integrity) = integrity {
        integrity.check(&bytes, &tarball)?;
    }
    let to = to.to_path_buf();
    tokio::task::spawn_blocking(move || unpack(&bytes, &to))
        .await
        .map_err(|e| PluginError::Source(e.to_string()))??;
    Ok(resolved)
}

/// The digest a package download is checked against, as npm states it.
///
/// A package install's registry is named by the marketplace entry, so the
/// digest and the file it describes can come from the same document: what
/// this check proves is that the bytes arrived as that registry described
/// them, which against the real npm registry is a transport substitution
/// and nothing more. Where the entry names its own registry, the digest
/// buys nothing an attacker who chose that registry did not already
/// control, and it is not meant to - taking plugins from a third-party
/// catalog is trusting that catalog. What it does close is the silent
/// skip: an algorithm this program cannot check, or no digest at all, is
/// a refusal, not a pass.
enum Integrity {
    /// `sha512-<base64>` (the registry's `dist.integrity`).
    Sri(String),
    /// The older hex sha1 `dist.shasum`.
    Sha1(String),
}

impl Integrity {
    fn check(&self, bytes: &[u8], url: &str) -> Result<(), PluginError> {
        use base64::Engine;
        let ok = match self {
            Integrity::Sri(sri) => match sri.split_once('-') {
                Some(("sha512", want)) => {
                    base64::engine::general_purpose::STANDARD.encode(sha2::Sha512::digest(bytes))
                        == want
                }
                Some(("sha256", want)) => {
                    base64::engine::general_purpose::STANDARD.encode(sha2::Sha256::digest(bytes))
                        == want
                }
                Some(("sha1", want)) => {
                    base64::engine::general_purpose::STANDARD.encode(sha1::Sha1::digest(bytes))
                        == want
                }
                // A digest in an algorithm this daemon does not know is
                // refused instead of accepted: answering "fine" here would
                // silently drop the check entirely, and the registry that
                // chose the digest is the one serving the tarball.
                Some((algorithm, _)) => {
                    return Err(PluginError::Source(format!(
                        "{url} is checked with {algorithm}, an integrity algorithm this OpenRemote cannot verify - refusing it"
                    )));
                }
                None => {
                    return Err(PluginError::Source(format!(
                        "{url} names an integrity digest with no algorithm - refusing it"
                    )));
                }
            },
            Integrity::Sha1(want) => hex(&sha1::Sha1::digest(bytes)).eq_ignore_ascii_case(want),
        };
        if ok {
            Ok(())
        } else {
            Err(PluginError::Source(format!(
                "{url} does not match the registry's integrity digest - refusing it"
            )))
        }
    }
}

/// `@scope/name@1.2.3` -> (`@scope/name`, Some(`1.2.3`)).
fn split_package(package: &str) -> (&str, Option<&str>) {
    match package.rfind('@') {
        Some(at) if at > 0 => (&package[..at], Some(&package[at + 1..])),
        _ => (package, None),
    }
}

/// The version an npm spec picks: a dist-tag, an exact version, or the
/// highest release a range admits.
fn npm_pick(meta: &Value, spec: &str) -> Option<String> {
    if let Some(tagged) = meta["dist-tags"][spec].as_str() {
        return Some(tagged.to_string());
    }
    let versions = meta["versions"].as_object()?;
    if versions.contains_key(spec) {
        return Some(spec.to_string());
    }
    versions
        .keys()
        .filter_map(|v| semver::Version::parse(v).ok())
        .filter(|v| v.pre.is_empty() && npm_range_admits(spec, v))
        .max()
        .map(|v| v.to_string())
}

/// npm's range syntax read through semver's: `||` alternatives, hyphen
/// ranges, space-separated comparators, `x` wildcards.
pub fn npm_range_admits(range: &str, version: &semver::Version) -> bool {
    range.split("||").any(|part| {
        let part = part.trim();
        if part.is_empty() || part == "*" || part.eq_ignore_ascii_case("x") {
            return true;
        }
        let req = if let Some((low, high)) = part.split_once(" - ") {
            format!(">={}, <={}", low.trim(), high.trim())
        } else {
            let mut tight = part.to_string();
            for op in [">=", "<=", ">", "<", "=", "^", "~"] {
                tight = tight.replace(&format!("{op} "), op);
            }
            tight
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(", ")
                .replace(".x", ".*")
                .replace(".X", ".*")
        };
        semver::VersionReq::parse(&req).is_ok_and(|r| r.matches(version))
    })
}

/// A package with Node dependencies gets them - with npm, its scripts
/// never run. Without npm the install stops and says so.
async fn install_node_packages(root: &Path) -> Result<(), PluginError> {
    let package: Option<Value> = read_json(&root.join("package.json"))?;
    let needs = package
        .as_ref()
        .and_then(|p| p.get("dependencies"))
        .and_then(Value::as_object)
        .is_some_and(|deps| !deps.is_empty());
    if !needs {
        return Ok(());
    }
    let locked =
        root.join("package-lock.json").is_file() || root.join("npm-shrinkwrap.json").is_file();
    let mut args = vec![if locked { "ci" } else { "install" }];
    args.extend(["--ignore-scripts", "--omit=dev", "--no-audit", "--no-fund"]);
    if !locked {
        args.push("--no-package-lock");
    }
    let npm = if cfg!(windows) { "npm.cmd" } else { "npm" };
    let output = tokio::process::Command::new(npm)
        .args(&args)
        .current_dir(root)
        .stdin(std::process::Stdio::null())
        .output()
        .await
        .map_err(|e| {
            PluginError::Source(if e.kind() == std::io::ErrorKind::NotFound {
                "this plugin needs its Node packages installed, and that needs npm on this machine"
                    .into()
            } else {
                format!("npm could not start: {e}")
            })
        })?;
    if !output.status.success() {
        let words = String::from_utf8_lossy(&output.stderr);
        return Err(PluginError::Source(format!(
            "npm could not install the plugin's packages: {}",
            words.trim().lines().last().unwrap_or("no reason given")
        )));
    }
    Ok(())
}

fn home_dir() -> PathBuf {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
}

/// Run a `command` source's command - only the exact one the user
/// approved - from the home folder: it must print one line, the absolute
/// path of a folder that holds a plugin, and exit 0 in time.
pub async fn run_command(approved: &ApprovedCommand) -> Result<PathBuf, PluginError> {
    #[cfg(windows)]
    let mut command = {
        let mut c = tokio::process::Command::new("cmd");
        // As typed: cmd.exe reads the rest of its line itself, and the
        // usual argument quoting would change what the user approved.
        c.arg("/C").raw_arg(&approved.command);
        c
    };
    #[cfg(not(windows))]
    let mut command = {
        let mut c = tokio::process::Command::new("sh");
        c.arg("-c").arg(&approved.command);
        c
    };
    command
        .current_dir(home_dir())
        .stdin(std::process::Stdio::null())
        .kill_on_drop(true);
    let limit = approved.timeout_secs.clamp(1, 600);
    let output = tokio::time::timeout(Duration::from_secs(limit), command.output())
        .await
        .map_err(|_| PluginError::Source(format!("the command ran past {limit}s")))?
        .map_err(|e| PluginError::Source(format!("the command could not start: {e}")))?;
    if !output.status.success() {
        let words = String::from_utf8_lossy(&output.stderr);
        return Err(PluginError::Source(format!(
            "the command failed: {}",
            words.trim().lines().last().unwrap_or("no reason given")
        )));
    }
    let printed = String::from_utf8_lossy(&output.stdout);
    let lines: Vec<&str> = printed
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    let [line] = lines.as_slice() else {
        return Err(PluginError::Source(
            "the command must print one line: the plugin folder's path".into(),
        ));
    };
    let path = PathBuf::from(line);
    if !path.is_absolute() || !path.is_dir() {
        return Err(PluginError::Source(format!(
            "the command printed {line}, which is not a folder"
        )));
    }
    if !has_manifest(&path) && claude_dirs(&path).is_empty() && !path.join("hooks").is_dir() {
        return Err(PluginError::Invalid(format!("{line} holds no plugin")));
    }
    Ok(path)
}

/// At a chat's start: run an installed plugin's approved command again
/// and take its folder if it changed. Answers whether it changed and the
/// new content hash; an error leaves the last copy in place.
pub async fn rerun_command(
    data_dir: &Path,
    plugin: &InstalledPlugin,
) -> Result<Option<String>, PluginError> {
    let Some(approved) = &plugin.command else {
        return Ok(None);
    };
    let printed = run_command(approved).await?;
    let hash = {
        let printed = printed.clone();
        tokio::task::spawn_blocking(move || content_hash(&printed))
            .await
            .map_err(|e| PluginError::Source(e.to_string()))??
    };
    if plugin.resolved.as_deref() == Some(hash.as_str()) {
        return Ok(None);
    }
    let root = data_dir.join("plugins");
    let target = plugin_dir_checked(data_dir, &plugin.marketplace, &plugin.name)?;
    let fresh = root.join(".staging").join(uuid::Uuid::new_v4().to_string());
    copy_in(printed, fresh.clone()).await?;
    // The Claude manifest written at install (from the marketplace entry)
    // is not in the command's output: carry it over.
    let manifest = Path::new(".claude-plugin").join("plugin.json");
    if !fresh.join(&manifest).is_file() && target.join(&manifest).is_file() {
        std::fs::create_dir_all(fresh.join(".claude-plugin"))?;
        std::fs::copy(target.join(&manifest), fresh.join(&manifest))?;
    }
    // The folder the user approved was a statement about what the plugin
    // may do and who it is. A command's output can change afterwards, so
    // the new copy is read here, by the same gate an install passes (its
    // own name included: taking a harness id is refused whatever the
    // folder is called), and a power the record does not carry is not
    // swapped in live: it waits for an install from Plugins, where the
    // review shows it. A copy that would not pass is refused for the same
    // reason - the last copy stays, and the note says what happened.
    let harness_ids: Vec<String> = HARNESS_IDS.iter().map(|id| id.to_string()).collect();
    let new_powers = match read_manifest(&fresh, &harness_ids) {
        Ok(manifest) => manifest.powers,
        Err(e) => {
            let _ = std::fs::remove_dir_all(&fresh);
            return Err(PluginError::Invalid(format!(
                "its new copy is not something this OpenRemote would install ({e}) - install it from Plugins to review it"
            )));
        }
    };
    let added: Vec<&str> = new_powers
        .iter()
        .filter(|power| !plugin.powers.contains(power))
        .map(|power| power_word(*power))
        .collect();
    if !added.is_empty() {
        let _ = std::fs::remove_dir_all(&fresh);
        return Err(PluginError::Invalid(format!(
            "its new copy asks for {}, which you have not approved - install it from Plugins to review the change",
            added.join(", ")
        )));
    }
    let swapped = swap_in(&root, &fresh, &target);
    let _ = std::fs::remove_dir_all(&fresh);
    swapped?;
    Ok(Some(hash))
}

/// A power in the words the approval screen uses.
fn power_word(power: PluginPower) -> &'static str {
    match power {
        PluginPower::RunPrograms => "starting programs",
        PluginPower::ReadChats => "reading chats",
        PluginPower::AgentTools => "agent tools",
        PluginPower::Network => "the network",
        PluginPower::Microphone => "the microphone",
    }
}

/// The entry fields whose value is a path (or a list of paths) into the
/// plugin folder: Claude reads each one as that kind of part.
const ENTRY_PATH_COMPONENTS: &[&str] = &["commands", "agents", "skills", "outputStyles", "themes"];

/// Every component path a catalog entry names, checked against the plugin
/// folder before it is written into the plugin's Claude manifest.
///
/// A component path decides what Claude reads once the plugin is
/// installed, and the entry is network-fetched JSON, so a catalog could
/// otherwise point a plugin at any folder on this machine - `"skills":
/// ["../../.."]` - and have its contents arrive in chats as the plugin's
/// own parts. The rule is the same one every other folder inside a plugin
/// passes: relative, forward slashes, no `..`.
fn entry_paths_inside(entry: &Value) -> Result<(), PluginError> {
    let Some(fields) = entry.as_object() else {
        return Ok(());
    };
    let who = fields
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or("the marketplace entry");
    for component in ENTRY_PATH_COMPONENTS {
        let Some(value) = fields.get(*component) else {
            continue;
        };
        let paths = match value {
            Value::String(_) | Value::Array(_) => as_list(Some(value)),
            // Neither a path nor a list of them is not a shape Claude
            // reads, and skipping it would let an entry carry a value
            // nothing looked at.
            _ => {
                return Err(PluginError::Invalid(format!(
                    "{who}'s {component} is neither a path nor a list of paths - refusing it"
                )));
            }
        };
        for path in paths {
            let Some(path) = path.as_str() else {
                continue;
            };
            if !relative_inside(path) {
                // Say which rule the path breaks: a backslash path is
                // inside the plugin folder and still not the shape a
                // plugin folder uses, so calling it "outside" would be
                // false.
                let why = if path.split(['/', '\\']).any(|part| part == "..") {
                    "which is outside the plugin folder"
                } else if path.starts_with('/') || path.contains(':') {
                    "which is an absolute path, outside the plugin folder"
                } else if path.contains('\\') {
                    "which is a backslash path, and a plugin folder names its parts with forward slashes"
                } else {
                    "which is not a path inside the plugin folder"
                };
                return Err(PluginError::Invalid(format!(
                    "{who}'s {component} names '{path}', {why} - refusing it"
                )));
            }
        }
    }
    Ok(())
}

/// The marketplace entry's part in the plugin's Claude manifest, as Claude
/// Code itself applies it: with no plugin.json the entry is the manifest;
/// with one, a strict entry appends its components (hooks replace per
/// event) and a non-strict entry that lists components is a conflict.
pub fn apply_entry(root: &Path, entry: &Value) -> Result<(), PluginError> {
    // Checked before anything is written, and on both branches below: an
    // entry becomes the manifest itself when the folder has none, so the
    // same fields can arrive that way round.
    entry_paths_inside(entry)?;
    const COMPONENTS: &[&str] = &[
        "commands",
        "agents",
        "skills",
        "hooks",
        "outputStyles",
        "themes",
    ];
    // Catalog fields that are not the plugin's own.
    const ENTRY_ONLY: &[&str] = &[
        "source",
        "category",
        "tags",
        "strict",
        "relevance",
        "dependencies",
        "defaultEnabled",
        "metadata",
        "headers",
        "headersHelper",
    ];
    let Some(fields) = entry.as_object() else {
        return Ok(());
    };
    let path = root.join(".claude-plugin").join("plugin.json");
    let strict = fields
        .get("strict")
        .and_then(Value::as_bool)
        .unwrap_or(true);
    let listed: Vec<&str> = COMPONENTS
        .iter()
        .copied()
        .filter(|c| fields.contains_key(*c))
        .collect();
    let manifest = match read_json::<Value>(&path)? {
        None => {
            // An OpenRemote plugin with no Claude side needs no Claude
            // manifest; anything else gets the entry as its manifest.
            if root.join("openremote.json").is_file()
                && listed.is_empty()
                && claude_dirs(root).is_empty()
            {
                return Ok(());
            }
            let mut own = serde_json::Map::new();
            for (key, value) in fields {
                if !ENTRY_ONLY.contains(&key.as_str()) {
                    own.insert(key.clone(), value.clone());
                }
            }
            Value::Object(own)
        }
        Some(mut manifest) => {
            if listed.is_empty() {
                return Ok(());
            }
            if !strict {
                return Err(PluginError::Invalid(format!(
                    "{} has conflicting manifests: its plugin.json and its marketplace entry both list components",
                    fields
                        .get("name")
                        .and_then(Value::as_str)
                        .unwrap_or("the plugin")
                )));
            }
            let Some(own) = manifest.as_object_mut() else {
                return Err(PluginError::Invalid("plugin.json is not an object".into()));
            };
            for component in listed {
                let added = &fields[component];
                if component == "hooks" {
                    let mut hooks = own
                        .get("hooks")
                        .and_then(Value::as_object)
                        .cloned()
                        .unwrap_or_default();
                    if let Some(events) = added.as_object() {
                        for (event, matchers) in events {
                            hooks.insert(event.clone(), matchers.clone());
                        }
                    }
                    own.insert("hooks".into(), Value::Object(hooks));
                } else {
                    let mut list = as_list(own.get(component));
                    for item in as_list(Some(added)) {
                        if !list.contains(&item) {
                            list.push(item);
                        }
                    }
                    own.insert(component.into(), Value::Array(list));
                }
            }
            manifest
        }
    };
    std::fs::create_dir_all(root.join(".claude-plugin"))?;
    std::fs::write(
        &path,
        serde_json::to_vec_pretty(&manifest).expect("a manifest serializes"),
    )?;
    Ok(())
}

fn as_list(value: Option<&Value>) -> Vec<Value> {
    match value {
        Some(Value::Array(items)) => items.clone(),
        Some(Value::String(_)) => vec![value.cloned().expect("a string")],
        _ => Vec::new(),
    }
}

/// A folder that carries Claude parts with no plugin.json gets a bare one,
/// so claude's `--plugin-dir` takes the folder as one plugin.
fn write_bare_claude_manifest(root: &Path, manifest: &Manifest) -> Result<(), PluginError> {
    let path = root.join(".claude-plugin").join("plugin.json");
    if manifest.claude_parts.is_empty() || path.is_file() {
        return Ok(());
    }
    let mut own = serde_json::Map::new();
    own.insert("name".into(), Value::String(manifest.name.clone()));
    if let Some(version) = &manifest.version {
        own.insert("version".into(), Value::String(version.clone()));
    }
    if let Some(description) = &manifest.description {
        own.insert("description".into(), Value::String(description.clone()));
    }
    std::fs::create_dir_all(root.join(".claude-plugin"))?;
    std::fs::write(
        &path,
        serde_json::to_vec_pretty(&Value::Object(own)).expect("a manifest serializes"),
    )?;
    Ok(())
}

/// Unpack a .zip or .tar.gz into `to`, refusing any entry that would land
/// outside it and any archive that unpacks past the size limit.
fn unpack(bytes: &[u8], to: &Path) -> Result<(), PluginError> {
    std::fs::create_dir_all(to)?;
    let mut total: u64 = 0;
    if bytes.starts_with(b"PK\x03\x04") {
        let mut archive = zip::ZipArchive::new(Cursor::new(bytes))
            .map_err(|e| PluginError::Source(format!("the zip is unreadable: {e}")))?;
        for i in 0..archive.len() {
            let mut entry = archive
                .by_index(i)
                .map_err(|e| PluginError::Source(format!("the zip is unreadable: {e}")))?;
            let Some(relative) = entry.enclosed_name() else {
                return Err(PluginError::Invalid(
                    "the zip reaches outside its folder".into(),
                ));
            };
            let out = to.join(relative);
            if entry.is_dir() {
                std::fs::create_dir_all(&out)?;
                continue;
            }
            total += entry.size();
            if total > MAX_ARCHIVE_BYTES {
                return Err(PluginError::Source("the archive unpacks too large".into()));
            }
            if let Some(parent) = out.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let mut file = std::fs::File::create(&out)?;
            std::io::copy(&mut (&mut entry).take(MAX_ARCHIVE_BYTES), &mut file)?;
        }
        return Ok(());
    }
    if bytes.starts_with(&[0x1f, 0x8b]) {
        let mut archive = tar::Archive::new(flate2::read::GzDecoder::new(Cursor::new(bytes)));
        for entry in archive
            .entries()
            .map_err(|e| PluginError::Source(format!("the archive is unreadable: {e}")))?
        {
            let mut entry = entry
                .map_err(|e| PluginError::Source(format!("the archive is unreadable: {e}")))?;
            let kind = entry.header().entry_type();
            if !(kind.is_file() || kind.is_dir()) {
                continue;
            }
            total += entry.header().size().unwrap_or(0);
            if total > MAX_ARCHIVE_BYTES {
                return Err(PluginError::Source("the archive unpacks too large".into()));
            }
            // unpack_in refuses a path that climbs out of `to`.
            if !entry
                .unpack_in(to)
                .map_err(|e| PluginError::Source(format!("the archive is unreadable: {e}")))?
            {
                return Err(PluginError::Invalid(
                    "the archive reaches outside its folder".into(),
                ));
            }
        }
        return Ok(());
    }
    Err(PluginError::Invalid(
        "an archive is a .zip or a .tar.gz".into(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn folder(files: &[(&str, &str)]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        for (path, body) in files {
            let p = dir.path().join(path);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, body).unwrap();
        }
        dir
    }

    fn builtins() -> Vec<String> {
        vec!["claude".into(), "codex".into()]
    }

    fn plugin_json(dir: &Path) -> Value {
        serde_json::from_slice(&std::fs::read(dir.join(".claude-plugin/plugin.json")).unwrap())
            .unwrap()
    }

    #[test]
    fn a_process_plugin_runs_programs_and_says_it_is_not_sandboxed() {
        let dir = folder(&[(
            "openremote.json",
            r#"{"api": 1, "name": "toy", "version": "1.0.0", "runtime": "process", "powers": ["network"]}"#,
        )]);
        let m = read_manifest(dir.path(), &builtins()).unwrap();
        assert_eq!(m.runtime, PluginRuntime::Process);
        assert_eq!(
            m.powers,
            vec![PluginPower::RunPrograms, PluginPower::Network]
        );
        assert!(m.not_sandboxed);
    }

    #[test]
    fn a_claude_plugin_alone_is_content_and_its_mcp_runs_programs() {
        let dir = folder(&[
            (
                ".claude-plugin/plugin.json",
                r#"{"name": "docs", "version": "2.1.0"}"#,
            ),
            ("skills/search/SKILL.md", "# search"),
            (".mcp.json", "{}"),
        ]);
        let m = read_manifest(dir.path(), &builtins()).unwrap();
        assert_eq!(m.runtime, PluginRuntime::Content);
        assert_eq!(m.claude_parts, vec!["skills", "mcp"]);
        assert_eq!(m.powers, vec![PluginPower::RunPrograms]);
        assert!(m.not_sandboxed);
        assert_eq!(m.version.as_deref(), Some("2.1.0"));
    }

    #[test]
    fn a_js_plugin_is_sandboxed_even_with_the_microphone() {
        let dir = folder(&[(
            "openremote.json",
            r#"{"api": 1, "name": "dictation", "runtime": "js", "powers": ["microphone"]}"#,
        )]);
        let m = read_manifest(dir.path(), &builtins()).unwrap();
        assert_eq!(m.powers, vec![PluginPower::Microphone]);
        assert!(!m.not_sandboxed);
    }

    #[test]
    fn wrong_manifests_say_what_is_wrong() {
        let cases = [
            (r#"{"api": 2, "name": "x", "runtime": "js"}"#, "api 2"),
            (
                r#"{"api": 1, "name": "Bad Name", "runtime": "js"}"#,
                "not a plugin name",
            ),
            (
                r#"{"api": 1, "name": "claude", "runtime": "js"}"#,
                "taken by OpenRemote",
            ),
            (
                r#"{"api": 1, "name": "x", "runtime": "lua"}"#,
                "runtime 'lua'",
            ),
            (r#"{"api": 1, "name": "x"}"#, "no runtime"),
            (
                r#"{"api": 1, "name": "x", "runtime": "js", "powers": ["root"]}"#,
                "'root'",
            ),
        ];
        for (json, says) in cases {
            let dir = folder(&[("openremote.json", json)]);
            let err = read_manifest(dir.path(), &builtins())
                .unwrap_err()
                .to_string();
            assert!(err.contains(says), "{json}: {err}");
        }
        let empty = folder(&[("README.md", "hi")]);
        assert!(
            read_manifest(empty.path(), &builtins())
                .unwrap_err()
                .to_string()
                .contains("not a plugin")
        );
    }

    #[test]
    fn an_archive_that_climbs_out_is_refused() {
        let mut bytes = Vec::new();
        {
            let mut zip = zip::ZipWriter::new(Cursor::new(&mut bytes));
            zip.start_file("../escape.txt", zip::write::SimpleFileOptions::default())
                .unwrap();
            std::io::Write::write_all(&mut zip, b"nope").unwrap();
            zip.finish().unwrap();
        }
        let to = tempfile::tempdir().unwrap();
        let err = unpack(&bytes, to.path()).unwrap_err().to_string();
        assert!(err.contains("outside"), "{err}");
    }

    #[test]
    fn an_entry_cannot_aim_claude_at_a_folder_outside_the_plugin() {
        let dir = folder(&[("local-ai/SKILL.md", "# local ai")]);
        // Every component that is a path, as a list and as a bare string.
        for component in ["commands", "agents", "skills", "outputStyles", "themes"] {
            for value in [json!(["../../../secret-skills"]), json!("/etc/skills")] {
                let mut entry = serde_json::Map::new();
                entry.insert("name".into(), json!("climber"));
                entry.insert(component.to_string(), value);
                let err = apply_entry(dir.path(), &Value::Object(entry))
                    .unwrap_err()
                    .to_string();
                assert!(
                    err.contains("outside the plugin folder"),
                    "{component}: {err}"
                );
                // The refusal says which plugin asked.
                assert!(err.contains("climber"), "{component}: {err}");
            }
        }
        // Nothing was written for any of them.
        assert!(
            !dir.path()
                .join(".claude-plugin")
                .join("plugin.json")
                .exists()
        );
        // A path inside the plugin still rides, on the branch where the
        // entry is the manifest itself.
        apply_entry(
            dir.path(),
            &json!({"name": "climber", "skills": ["./local-ai"]}),
        )
        .unwrap();
        assert_eq!(plugin_json(dir.path())["skills"], json!(["./local-ai"]));
    }

    #[test]
    fn an_entry_appended_to_an_existing_manifest_is_checked_too() {
        // The other way an entry's components reach a Claude manifest: the
        // plugin ships `.claude-plugin/plugin.json` and a strict entry
        // appends its own components to it. The check is the first
        // statement of `apply_entry`, so this branch inherits it: this
        // test is what fails if a later change moves the check inside one
        // branch, since the write branch would then be the only one
        // covered.
        let dir = folder(&[("local-ai/SKILL.md", "# local ai")]);
        let manifest = dir.path().join(".claude-plugin");
        std::fs::create_dir_all(&manifest).unwrap();
        std::fs::write(
            manifest.join("plugin.json"),
            json!({"name": "shipped", "strict": true, "skills": ["./local-ai"]}).to_string(),
        )
        .unwrap();
        let before = std::fs::read_to_string(manifest.join("plugin.json")).unwrap();
        let err = apply_entry(
            dir.path(),
            &json!({"name": "appended", "strict": true, "skills": ["../../.."]}),
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("outside the plugin folder"), "{err}");
        assert!(err.contains("appended"), "{err}");
        // The shipped manifest is what it was: nothing was appended.
        assert_eq!(
            std::fs::read_to_string(manifest.join("plugin.json")).unwrap(),
            before
        );
        // A path inside the plugin still appends, so the branch works when
        // the entry is honest.
        apply_entry(
            dir.path(),
            &json!({"name": "appended", "strict": true, "skills": ["more"]}),
        )
        .unwrap();
        assert_eq!(
            plugin_json(dir.path())["skills"],
            json!(["./local-ai", "more"])
        );
    }

    #[test]
    fn a_component_that_is_neither_a_path_nor_a_list_is_refused() {
        let dir = folder(&[("local-ai/SKILL.md", "# local ai")]);
        let err = apply_entry(dir.path(), &json!({"name": "odd", "skills": 7}))
            .unwrap_err()
            .to_string();
        assert!(err.contains("neither a path nor a list"), "{err}");
        // Nothing was written.
        assert!(
            !dir.path()
                .join(".claude-plugin")
                .join("plugin.json")
                .exists()
        );
    }

    #[test]
    fn a_backslash_path_is_refused_as_the_shape_it_is() {
        let dir = folder(&[("local-ai/SKILL.md", "# local ai")]);
        // A Windows-style path is inside the plugin folder on Windows, and
        // still not the shape a plugin manifest uses: the refusal says
        // that rather than claiming the path is outside.
        let err = apply_entry(
            dir.path(),
            &json!({"name": "backslashed", "skills": ["local-ai\\nested"]}),
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("backslash path"), "{err}");
        assert!(!err.contains("outside the plugin folder"), "{err}");
    }

    #[test]
    fn with_no_plugin_json_the_marketplace_entry_is_the_manifest() {
        let dir = folder(&[("local-ai/SKILL.md", "# local ai")]);
        let entry = json!({
            "name": "amd-skills",
            "description": "AMD's skills",
            "source": {"source": "git-subdir", "url": "x", "path": "skills"},
            "strict": false,
            "category": "development",
            "skills": ["./local-ai"],
        });
        apply_entry(dir.path(), &entry).unwrap();
        let written = plugin_json(dir.path());
        assert_eq!(written["name"], "amd-skills");
        assert_eq!(written["skills"], json!(["./local-ai"]));
        // The catalog's own fields stay in the catalog.
        assert!(written.get("source").is_none());
        assert!(written.get("strict").is_none());
        assert!(written.get("category").is_none());
    }

    #[test]
    fn a_strict_entry_adds_its_components_and_a_loose_one_conflicts() {
        let dir = folder(&[(
            ".claude-plugin/plugin.json",
            r#"{"name": "fmt", "commands": "./commands", "hooks": {"PreToolUse": [1], "Stop": [2]}}"#,
        )]);
        let entry = json!({
            "name": "fmt",
            "commands": ["./extra"],
            "hooks": {"Stop": [3]},
        });
        apply_entry(dir.path(), &entry).unwrap();
        let written = plugin_json(dir.path());
        assert_eq!(written["commands"], json!(["./commands", "./extra"]));
        // hooks replace per event
        assert_eq!(written["hooks"], json!({"PreToolUse": [1], "Stop": [3]}));

        let loose = json!({"name": "fmt", "strict": false, "skills": ["./x"]});
        let err = apply_entry(dir.path(), &loose).unwrap_err().to_string();
        assert!(err.contains("conflicting manifests"), "{err}");
    }

    #[test]
    fn npm_ranges_read_as_npm_writes_them() {
        let v = |s: &str| semver::Version::parse(s).unwrap();
        assert!(npm_range_admits("^2.0.0", &v("2.3.1")));
        assert!(!npm_range_admits("^2.0.0", &v("3.0.0")));
        assert!(npm_range_admits(">= 1.2 < 2", &v("1.9.0")));
        assert!(npm_range_admits("1.x || 3.x", &v("3.4.0")));
        assert!(!npm_range_admits("1.x || 3.x", &v("2.0.0")));
        assert!(npm_range_admits("1.0.0 - 1.4.0", &v("1.4.0")));
        assert!(npm_range_admits("*", &v("9.9.9")));

        let meta = json!({
            "dist-tags": {"latest": "2.1.0", "next": "3.0.0-beta.1"},
            "versions": {"1.0.0": {}, "2.0.0": {}, "2.1.0": {}, "3.0.0-beta.1": {}},
        });
        assert_eq!(npm_pick(&meta, "latest").as_deref(), Some("2.1.0"));
        assert_eq!(npm_pick(&meta, "next").as_deref(), Some("3.0.0-beta.1"));
        assert_eq!(npm_pick(&meta, "1.0.0").as_deref(), Some("1.0.0"));
        assert_eq!(npm_pick(&meta, "^2").as_deref(), Some("2.1.0"));
        assert_eq!(npm_pick(&meta, "^4"), None);
        assert_eq!(split_package("@scope/x@1.2.3"), ("@scope/x", Some("1.2.3")));
        assert_eq!(split_package("@scope/x"), ("@scope/x", None));
    }

    #[test]
    fn an_integrity_digest_this_daemon_cannot_check_is_refused() {
        use base64::Engine;
        use sha2::Digest;
        let bytes = b"a tarball";
        let good = format!(
            "sha512-{}",
            base64::engine::general_purpose::STANDARD.encode(sha2::Sha512::digest(bytes))
        );
        assert!(Integrity::Sri(good).check(bytes, "u").is_ok());
        assert!(
            Integrity::Sha1(hex(&sha1::Sha1::digest(bytes)))
                .check(bytes, "u")
                .is_ok()
        );
        // An algorithm this daemon cannot compute, and a digest with no
        // algorithm at all, are refusals: the catch-all used to answer
        // "fine" and drop the download check entirely, and a catalog entry
        // may name the registry that supplies both the tarball and the
        // digest.
        for sri in ["sha999-anything", "md5-1B2M2Y8AsgTpgAmY7PhCfg==", "sha512"] {
            let err = Integrity::Sri(sri.to_string())
                .check(bytes, "https://x/t.tgz")
                .unwrap_err()
                .to_string();
            assert!(err.contains("refusing it"), "{sri}: {err}");
        }
        let wrong = Integrity::Sri("sha512-AAAA".into())
            .check(bytes, "u")
            .unwrap_err()
            .to_string();
        assert!(wrong.contains("does not match"), "{wrong}");
        assert!(Integrity::Sha1("00".into()).check(bytes, "u").is_err());
    }

    #[test]
    fn paths_inside_a_folder_stay_inside() {
        assert!(relative_inside("plugins/fmt"));
        assert!(!relative_inside("../fmt"));
        assert!(!relative_inside("a/../../b"));
        assert!(!relative_inside("/etc"));
        assert!(!relative_inside("C:/x"));
        assert!(!relative_inside("a\\b"));
    }

    #[tokio::test]
    async fn a_catalog_entry_that_climbs_out_is_refused_before_anything_is_fetched() {
        let data = tempfile::tempdir().unwrap();
        let host = PluginHost::new(data.path());
        let source = folder(&[(
            "openremote.json",
            r#"{"api": 1, "name": "tidy", "runtime": "content"}"#,
        )]);
        // Where an entry named `../../escape` would resolve from
        // plugins/<marketplace>/: an install replaces that folder whole.
        std::fs::create_dir_all(data.path().join("escape")).unwrap();
        std::fs::write(data.path().join("escape/keep.txt"), "the user's own").unwrap();
        let request = |name: &str| StageRequest {
            fetch: Fetch::Folder(source.path().to_path_buf()),
            marketplace: "official".into(),
            entry_name: Some(name.into()),
            entry: None,
            recorded: PluginSource::Folder {
                path: source.path().display().to_string(),
            },
        };
        let err = host
            .stage(request("../../escape"), builtins())
            .await
            .unwrap_err()
            .to_string();
        assert!(err.contains("not a plugin name"), "{err}");
        // Refused before the fetch, so nothing was staged and nothing
        // outside the plugins folder moved.
        assert!(!data.path().join("plugins").exists());
        assert_eq!(
            std::fs::read_to_string(data.path().join("escape/keep.txt")).unwrap(),
            "the user's own"
        );
    }

    #[test]
    fn an_install_cannot_choose_a_folder_outside_the_plugins_root() {
        let data = tempfile::tempdir().unwrap();
        let host = PluginHost::new(data.path());
        let fetched = data.path().join("plugins/.staging/x/plugin");
        std::fs::create_dir_all(&fetched).unwrap();
        std::fs::write(
            fetched.join("openremote.json"),
            r#"{"api": 1, "name": "tidy", "runtime": "content"}"#,
        )
        .unwrap();
        let manifest = read_manifest(&fetched, &builtins()).unwrap();
        std::fs::create_dir_all(data.path().join("escape")).unwrap();
        std::fs::write(data.path().join("escape/keep.txt"), "the user's own").unwrap();
        // A staged install whose name came from somewhere that skipped the
        // gate: install checks the segments itself.
        let staged = Staged {
            dir: data.path().join("plugins/.staging/x"),
            root: fetched,
            manifest,
            name: "../../escape".into(),
            marketplace: "official".into(),
            source: PluginSource::Folder { path: "x".into() },
            resolved: None,
            command: None,
        };
        let err = host.install(staged).unwrap_err().to_string();
        assert!(err.contains("not a plugin name"), "{err}");
        assert_eq!(
            std::fs::read_to_string(data.path().join("escape/keep.txt")).unwrap(),
            "the user's own"
        );
    }

    #[test]
    fn a_trailing_dot_names_the_trimmed_folder_on_windows_only() {
        // The rule that decides whether removal may look at a trimmed name,
        // checked from both sides on any machine: Windows dropped the dot
        // when it made the folder, nothing else did.
        assert_eq!(installed_as("tidy.", true), "tidy");
        assert_eq!(installed_as("tidy ", true), "tidy");
        assert_eq!(installed_as("tidy.", false), "tidy.");
        assert_eq!(installed_as("tidy ", false), "tidy ");
        // A dot or a space inside the name is part of it everywhere, and a
        // name with nothing to drop is itself.
        assert_eq!(installed_as("my.plugin", true), "my.plugin");
        assert_eq!(installed_as("My Plugin", true), "My Plugin");
        assert_eq!(installed_as("tidy", true), "tidy");
        assert_eq!(installed_as("...", true), "");
    }

    #[test]
    fn removal_takes_one_segment_inside_the_plugins_folder_and_no_more() {
        let data = tempfile::tempdir().unwrap();
        let host = PluginHost::new(data.path());
        // A folder installed before the name gate existed: it still
        // removes, so a folder a catalog once created here can be cleaned
        // up by hand.
        let odd = data.path().join("plugins/Old.Market/My Plugin");
        std::fs::create_dir_all(&odd).unwrap();
        host.uninstall("Old.Market", "My Plugin").unwrap();
        assert!(!odd.exists());
        // A record carrying a trailing dot or space is a Windows shape: Win32
        // drops both when it creates the folder, so `tidy.` was installed as
        // `tidy` and removing `tidy` is removing what the record means, the
        // user's only way to be rid of the plugin. Windows makes one folder
        // of the two names, so this is asserted on Windows only: elsewhere
        // nothing was dropped at install time, `tidy.` is a folder of its
        // own, and `tidy` belongs to a different plugin in the same
        // marketplace, which the trimmed delete would have taken with it.
        let decoy = data.path().join("plugins/official/tidy");
        std::fs::create_dir_all(decoy.join("skills")).unwrap();
        std::fs::write(decoy.join("keep.txt"), "another plugin").unwrap();
        if cfg!(windows) {
            host.uninstall("official", "tidy.").unwrap();
            assert!(!decoy.exists(), "the trimmed folder was left behind");
        } else {
            // Its own folder is there and so is the decoy: the record is
            // refused, and neither folder is touched.
            let own = data.path().join("plugins/official/tidy.");
            std::fs::create_dir_all(own.join("skills")).unwrap();
            let err = host.uninstall("official", "tidy.").unwrap_err().to_string();
            assert!(
                err.contains("is not one folder inside the plugins folder"),
                "{err}"
            );
            assert!(own.exists(), "the record's own folder was deleted");
            assert_eq!(
                std::fs::read_to_string(decoy.join("keep.txt")).unwrap(),
                "another plugin"
            );
            // A trailing space with no folder of its own removes nothing and
            // says so, and the decoy is still there afterwards.
            host.uninstall("official", "tidy ").unwrap();
            assert_eq!(
                std::fs::read_to_string(decoy.join("keep.txt")).unwrap(),
                "another plugin"
            );
        }
        // A record naming a folder that is not there removes nothing, and
        // reports nothing to remove, so the list can drop the record.
        let nested = data.path().join("plugins/official/x/keep.txt");
        std::fs::create_dir_all(nested.parent().unwrap()).unwrap();
        std::fs::write(&nested, "keep").unwrap();
        for (marketplace, name) in [("official", "x/y"), ("official", "x\\y"), ("..", "tidy")] {
            host.uninstall(marketplace, name).unwrap();
        }
        assert_eq!(std::fs::read_to_string(&nested).unwrap(), "keep");
        // A name that does resolve to something, and is not one folder
        // inside the plugins folder, is refused: nothing of it is deleted,
        // and the refusal says why in the user's terms, printing the path it
        // will not touch.
        std::fs::create_dir_all(data.path().join("keep")).unwrap();
        std::fs::write(data.path().join("keep/me.txt"), "keep").unwrap();
        // `...` is a folder of its own on Linux and the marketplace folder
        // itself on Windows; either way it resolves to something.
        std::fs::create_dir_all(data.path().join("plugins/official").join("...")).unwrap();
        for (marketplace, name) in [
            ("official", "../../keep"),
            ("official", ".."),
            ("official", "..."),
            ("official", "."),
        ] {
            let err = host.uninstall(marketplace, name).unwrap_err().to_string();
            assert!(
                err.contains("is not one folder inside the plugins folder"),
                "{name:?}: {err}"
            );
            // The instruction lands on one folder, named by its path, and
            // says it is this plugin's own: a name like `...` resolves to the
            // marketplace folder itself on Windows, where "delete that
            // folder" would read as "delete every plugin in here".
            assert!(
                err.contains("if that is this plugin's folder, delete it yourself"),
                "{name:?}: {err}"
            );
            assert!(
                err.contains(&name_path(data.path(), name)),
                "{name:?}: {err}"
            );
            // It no longer claims the name is not a plugin folder.
            assert!(!err.contains("is not a plugin folder"), "{name:?}: {err}");
        }
        // A folder whose existence cannot be checked keeps the record; what
        // `unremovable` does with that answer is the test above, and what
        // removal does to get the answer is `a_name_that_cannot_be_checked_keeps_the_record`.
        assert_eq!(
            std::fs::read_to_string(data.path().join("keep/me.txt")).unwrap(),
            "keep"
        );
    }

    #[test]
    fn a_name_that_cannot_be_checked_keeps_the_record() {
        // The question itself has to be able to fail, or the arm above is
        // never reached in the app: `Path::exists` folds a failed check into
        // "no", which drops the record and leaves the folder it names as an
        // orphan nothing looks at again. A NUL byte in the name is the
        // portable way to make the check fail - no file system can be asked
        // about such a path, so the stat errors on every platform - and this
        // is the only test that reaches the arm through the real call.
        let data = tempfile::tempdir().unwrap();
        let host = PluginHost::new(data.path());
        let said = host.uninstall("official", "a\0b").unwrap_err().to_string();
        assert!(
            said.contains("could not check whether this plugin's folder"),
            "{said}"
        );
    }

    #[test]
    fn a_folder_that_cannot_be_checked_keeps_the_record() {
        let target = Path::new("plugins").join("official").join("..");
        // Two kinds rather than one, because the arm does not look at the kind
        // and should say the same thing for every reason the check can fail.
        // Both are stable on the workspace's minimum Rust; `InvalidFilename`
        // reads better for the NUL byte the test above sends, but it is still
        // unstable there, so the minimum-version job could not build the tree.
        for kind in [
            std::io::ErrorKind::PermissionDenied,
            std::io::ErrorKind::InvalidInput,
        ] {
            let said = unremovable("official", "..", &target, Err(std::io::Error::from(kind)))
                .unwrap_err()
                .to_string();
            // It says the check failed, not that the folder is gone, and it
            // names the folder so the person can look.
            assert!(
                said.contains("could not check whether this plugin's folder"),
                "{kind:?}: {said}"
            );
            assert!(
                said.contains(&target.display().to_string()),
                "{kind:?}: {said}"
            );
            assert!(!said.contains("is not one folder"), "{kind:?}: {said}");
        }
        // Nothing there: the record may go.
        unremovable("official", "x/y", &target, Ok(false)).unwrap();
        // Something there: refused, with the path and the qualification.
        let said = unremovable("official", "...", &target, Ok(true))
            .unwrap_err()
            .to_string();
        assert!(
            said.contains("if that is this plugin's folder, delete it yourself"),
            "{said}"
        );
        assert!(said.contains(&target.display().to_string()), "{said}");
    }

    /// The path a refusal prints for a record name, spelled the way a temp
    /// directory spells it: only used to check the message carries it.
    fn name_path(data_dir: &Path, name: &str) -> String {
        plugin_dir(data_dir, "official", name).display().to_string()
    }

    #[test]
    fn a_failed_swap_puts_the_old_copy_back() {
        let root = tempfile::tempdir().unwrap();
        let plugins = root.path().join("plugins");
        let target = plugins.join("official/tidy");
        std::fs::create_dir_all(&target).unwrap();
        std::fs::write(target.join("openremote.json"), "the old copy").unwrap();
        // The incoming folder is not there, so the second rename fails the
        // way a folder a running chat still holds open makes it fail.
        swap_in(&plugins, &root.path().join("missing"), &target).unwrap_err();
        // The plugin is where the store says it is, still.
        assert_eq!(
            std::fs::read_to_string(target.join("openremote.json")).unwrap(),
            "the old copy"
        );
        // And .trash is not left holding the only copy.
        let left = std::fs::read_dir(plugins.join(".trash")).map(|it| it.count());
        assert_eq!(left.unwrap_or(0), 0);
    }

    #[tokio::test]
    async fn a_command_plugins_new_powers_are_not_swapped_in_unreviewed() {
        let data = tempfile::tempdir().unwrap();
        let data_dir = data.path();
        let installed = data_dir.join("plugins/generated/gen");
        std::fs::create_dir_all(installed.join("skills/a")).unwrap();
        std::fs::write(installed.join("skills/a/SKILL.md"), "# a").unwrap();
        std::fs::write(
            installed.join("openremote.json"),
            r#"{"api": 1, "name": "gen", "runtime": "content"}"#,
        )
        .unwrap();
        // What the approved command prints now: a changed copy of the same
        // plugin, at first with the powers the user approved.
        let printed = tempfile::tempdir().unwrap();
        let folder = printed.path().join("gen");
        std::fs::create_dir_all(folder.join("skills/a")).unwrap();
        std::fs::write(folder.join("skills/a/SKILL.md"), "# a changed").unwrap();
        std::fs::write(
            folder.join("openremote.json"),
            r#"{"api": 1, "name": "gen", "runtime": "content"}"#,
        )
        .unwrap();
        let record = |resolved: Option<String>| InstalledPlugin {
            name: "gen".into(),
            marketplace: "generated".into(),
            resolved,
            command: Some(ApprovedCommand {
                command: format!("echo {}", folder.display()),
                timeout_secs: 30,
            }),
            version: None,
            description: None,
            runtime: PluginRuntime::Content,
            powers: Vec::new(),
            source: PluginSource::Marketplace {
                marketplace: "generated".into(),
                plugin: "gen".into(),
                from: "a command".into(),
            },
            scope: openremote_core::PluginScope::Machine,
            harness: None,
            enabled: true,
            installed_at: 0,
            updated_at: 0,
        };
        assert!(
            rerun_command(data_dir, &record(None))
                .await
                .unwrap()
                .is_some()
        );
        assert_eq!(
            std::fs::read_to_string(installed.join("skills/a/SKILL.md")).unwrap(),
            "# a changed"
        );
        // The output grows an MCP server, which runs programs: a power the
        // user never saw. It is not swapped in behind their back.
        std::fs::write(folder.join(".mcp.json"), "{}").unwrap();
        let err = rerun_command(data_dir, &record(None))
            .await
            .unwrap_err()
            .to_string();
        assert!(err.contains("starting programs"), "{err}");
        assert!(!installed.join(".mcp.json").exists());
        assert_eq!(
            std::fs::read_to_string(installed.join("skills/a/SKILL.md")).unwrap(),
            "# a changed"
        );
    }

    #[tokio::test]
    async fn a_command_plugins_copy_cannot_take_a_harness_id() {
        let data = tempfile::tempdir().unwrap();
        let data_dir = data.path();
        let installed = data_dir.join("plugins/generated/gen");
        std::fs::create_dir_all(installed.join("skills/a")).unwrap();
        std::fs::write(installed.join("skills/a/SKILL.md"), "# a").unwrap();
        std::fs::write(
            installed.join("openremote.json"),
            r#"{"api": 1, "name": "gen", "runtime": "content"}"#,
        )
        .unwrap();
        // The command's output now calls itself `claude`, a harness id:
        // an install of the same folder would be refused for it, and an
        // update must not slip it in as the plugin the chats already have.
        let printed = tempfile::tempdir().unwrap();
        let folder = printed.path().join("gen");
        std::fs::create_dir_all(folder.join("skills/a")).unwrap();
        std::fs::write(folder.join("skills/a/SKILL.md"), "# a changed").unwrap();
        std::fs::write(
            folder.join("openremote.json"),
            r#"{"api": 1, "name": "claude", "runtime": "content"}"#,
        )
        .unwrap();
        let record = InstalledPlugin {
            name: "gen".into(),
            marketplace: "generated".into(),
            resolved: None,
            command: Some(ApprovedCommand {
                command: format!("echo {}", folder.display()),
                timeout_secs: 30,
            }),
            version: None,
            description: None,
            runtime: PluginRuntime::Content,
            powers: Vec::new(),
            source: PluginSource::Marketplace {
                marketplace: "generated".into(),
                plugin: "gen".into(),
                from: "a command".into(),
            },
            scope: openremote_core::PluginScope::Machine,
            harness: None,
            enabled: true,
            installed_at: 0,
            updated_at: 0,
        };
        let err = rerun_command(data_dir, &record)
            .await
            .unwrap_err()
            .to_string();
        assert!(err.contains("taken by OpenRemote itself"), "{err}");
        // The approved copy is still the one on disk.
        assert_eq!(
            std::fs::read_to_string(installed.join("skills/a/SKILL.md")).unwrap(),
            "# a"
        );
    }

    #[test]
    fn a_pinned_commit_is_a_hex_commit_id() {
        assert!(is_commit_id("a7ee8a3"));
        assert!(is_commit_id("A7EE8A3"));
        assert!(is_commit_id(&"0".repeat(40)));
        assert!(is_commit_id(&"a".repeat(64)));
        assert!(!is_commit_id("a7ee8a"));
        assert!(!is_commit_id(&"a".repeat(65)));
        assert!(!is_commit_id("main"));
        assert!(!is_commit_id("--upload-pack=C:/Windows/System32/calc.exe"));
        assert!(!is_commit_id("-a7ee8a3"));
        assert!(!is_commit_id(""));
    }

    #[test]
    fn every_git_call_is_shaped_by_the_neutral_settings_first() {
        // What this proves is the wiring - the settings are prepended to
        // every call's own words - and not that git honours them. Showing
        // that needs a repository with a submodule and a machine gitconfig
        // saying `submodule.recurse=true`, which is machine state a test
        // here is not allowed to write: see the removal note in the report.
        let argv = git_argv(&["fetch", "--depth", "1", "origin", "main"]);
        let neutral: Vec<String> = GIT_NEUTRAL.iter().map(|a| (*a).to_string()).collect();
        // Settings, then the call: never the other way round, since git
        // reads `-c` only before the subcommand.
        assert_eq!(&argv[..neutral.len()], &neutral[..]);
        assert_eq!(
            &argv[neutral.len()..],
            ["fetch", "--depth", "1", "origin", "main"]
        );
        // Both settings that would otherwise shape a plugin fetch: a
        // `.gitmodules` must stay inert, and `ext::` must not reach a
        // program.
        assert_eq!(GIT_NEUTRAL.len() % 2, 0, "each setting is a -c and a key");
        assert!(argv.iter().any(|a| a == "submodule.recurse=false"));
        assert!(argv.iter().any(|a| a == "protocol.ext.allow=never"));
        // A call with no words of its own still carries them.
        assert_eq!(git_argv(&[]), neutral);
    }

    #[tokio::test]
    async fn a_hostile_pinned_commit_is_refused_before_git_starts() {
        let to = tempfile::tempdir().unwrap().path().join("clone");
        let err = git_fetch(
            "https://example.invalid/tools.git",
            None,
            Some("--upload-pack=notepad.exe"),
            None,
            &to,
        )
        .await
        .unwrap_err()
        .to_string();
        assert!(err.contains("not a commit id"), "{err}");
        // git never started: `to` is not even made, and `git init` would
        // have left a .git in it.
        assert!(!to.exists());
    }

    #[tokio::test]
    async fn option_shaped_git_values_are_refused_before_git_starts() {
        let to = tempfile::tempdir().unwrap().path().join("clone");
        let cases: [(Option<&str>, Option<&str>); 3] = [
            (Some("--upload-pack=x"), None),
            (Some("-x"), None),
            (None, Some("--stdin")),
        ];
        for (reference, subdir) in cases {
            let err = git_fetch(
                "https://example.invalid/tools.git",
                reference,
                None,
                subdir,
                &to,
            )
            .await
            .unwrap_err()
            .to_string();
            assert!(err.contains("not a"), "{reference:?} {subdir:?}: {err}");
            assert!(!to.exists(), "{reference:?} {subdir:?} ran git");
        }
        let err = git_head("https://example.invalid/tools.git", Some("--upload-pack=x"))
            .await
            .unwrap_err()
            .to_string();
        assert!(err.contains("not a branch or tag"), "{err}");
    }

    #[test]
    fn a_folder_with_claude_parts_and_no_manifest_gets_a_bare_one() {
        let dir = folder(&[("skills/greet/SKILL.md", "# greet")]);
        // read_manifest needs a name: a direct install of a bare skills
        // folder has none, so it is refused...
        assert!(read_manifest(dir.path(), &builtins()).is_err());
        // ...while one that names itself in openremote.json gets Claude's
        // file written beside it.
        std::fs::write(
            dir.path().join("openremote.json"),
            r#"{"api": 1, "name": "greeter", "version": "1.0.0", "runtime": "content"}"#,
        )
        .unwrap();
        let m = read_manifest(dir.path(), &builtins()).unwrap();
        write_bare_claude_manifest(dir.path(), &m).unwrap();
        assert_eq!(
            plugin_json(dir.path()),
            json!({"name": "greeter", "version": "1.0.0"})
        );
    }

    // ---- harness plugins ----

    /// A minimal harness block, with the driver written where the current
    /// OS looks for it.
    fn harness_plugin(driver: &str) -> tempfile::TempDir {
        let os = std::env::consts::OS;
        let body = format!(
            r#"{{"api":1,"name":"freebuff","runtime":"process","powers":["network"],"harness":{{"api":1,"id":"freebuff","name":"Freebuff","driver":{{"{os}":"{driver}","default":"bin/freebuff-driver"}}}}}}"#
        );
        folder(&[("openremote.json", &body), (driver, "driver")])
    }

    #[test]
    fn a_plugin_can_declare_a_harness_and_its_driver_stays_inside() {
        let os = std::env::consts::OS;
        let driver = if os == "windows" {
            "bin/windows-x64/freebuff-driver.exe"
        } else {
            "bin/freebuff-driver"
        };
        let dir = harness_plugin(driver);
        let manifest = read_manifest(dir.path(), &builtins()).expect("a harness plugin reads");
        let harness = manifest.harness.expect("the harness block");
        assert_eq!(harness.id, "freebuff");
        assert_eq!(harness.name, "Freebuff");
        // A process harness runs programs, and the install screen says so.
        assert!(manifest.powers.contains(&PluginPower::RunPrograms));
        assert!(manifest.not_sandboxed);
        let resolved = harness.resolved_driver(dir.path()).expect("driver inside");
        assert!(
            resolved.ends_with(if os == "windows" {
                "freebuff-driver.exe"
            } else {
                "freebuff-driver"
            }),
            "{}",
            resolved.display()
        );

        // A path that climbs out is refused, even when a file sits there.
        let mut escaping = harness.clone();
        escaping.driver = HashMap::from([("default".to_string(), "../evil.exe".to_string())]);
        let err = escaping
            .resolved_driver(dir.path())
            .expect_err("a driver outside the folder");
        assert!(err.contains("leaves the plugin folder"), "{err}");
    }

    #[test]
    fn a_harness_id_cannot_take_a_builtin_and_a_foreign_api_is_refused() {
        let os = std::env::consts::OS;
        let dir = folder(&[(
            "openremote.json",
            &format!(
                r#"{{"api":1,"name":"x","runtime":"process","harness":{{"api":1,"id":"claude","name":"X","driver":{{"{os}":"d"}}}}}}"#
            ),
        )]);
        let err = read_manifest(dir.path(), &builtins()).expect_err("a built-in id is reserved");
        assert!(err.to_string().contains("taken by OpenRemote"), "{err}");

        let dir = folder(&[(
            "openremote.json",
            &format!(
                r#"{{"api":1,"name":"x","runtime":"process","harness":{{"api":2,"id":"x","name":"X","driver":{{"{os}":"d"}}}}}}"#
            ),
        )]);
        let err = read_manifest(dir.path(), &builtins()).expect_err("a newer seam");
        assert!(err.to_string().contains("harness api"), "{err}");
    }

    /// The plugin's own harness manifest, with `extra` keys spliced into the
    /// harness block and the driver path for this OS.
    fn harness_manifest_json(driver: &str, extra: &str) -> String {
        let os = std::env::consts::OS;
        format!(
            r#"{{"api":1,"name":"freebuff","runtime":"process","harness":{{"api":1,"id":"freebuff","name":"Freebuff","driver":{{"{os}":"{driver}"}}{extra}}}}}"#
        )
    }

    /// An installed plugin's record for a folder the test wrote at
    /// `<data dir>/plugins/local/<name>`, which is where
    /// `installed_harnesses` looks.
    fn installed_record(data_dir: &Path, name: &str, harness_id: &str) -> InstalledPlugin {
        InstalledPlugin {
            name: name.into(),
            marketplace: "local".into(),
            resolved: None,
            command: None,
            version: None,
            description: None,
            runtime: PluginRuntime::Process,
            powers: Vec::new(),
            source: openremote_core::PluginSource::Folder {
                path: data_dir
                    .join("plugins/local")
                    .join(name)
                    .display()
                    .to_string(),
            },
            scope: openremote_core::PluginScope::Machine,
            harness: Some(openremote_core::PluginHarnessRef {
                id: harness_id.into(),
                name: "Freebuff".into(),
            }),
            enabled: true,
            installed_at: 0,
            updated_at: 0,
        }
    }

    /// The driver path this OS runs in the tests' plugin folder.
    fn driver_path() -> &'static str {
        if std::env::consts::OS == "windows" {
            "bin/freebuff-driver.exe"
        } else {
            "bin/freebuff-driver"
        }
    }

    #[test]
    fn a_models_file_inside_the_plugin_folder_wins_over_the_manifest() {
        let data = tempfile::tempdir().unwrap();
        let dir = data.path().join("plugins/local/freebuff");
        std::fs::create_dir_all(dir.join("bin")).unwrap();
        std::fs::write(dir.join(driver_path()), "driver").unwrap();
        std::fs::write(
            dir.join("openremote.json"),
            harness_manifest_json(
                driver_path(),
                r#","models":[{"model":"shipped"}],"models_file":"models.json""#,
            ),
        )
        .unwrap();
        std::fs::write(
            dir.join("models.json"),
            r#"{"captured_at":1700000000000,"default":"captured","models":[{"model":"captured","display_name":"Captured"}]}"#,
        )
        .unwrap();

        let record = installed_record(data.path(), "freebuff", "freebuff");
        let specs = installed_harnesses(data.path(), &[&record]);
        let spec = specs.first().expect("one harness");
        assert_eq!(spec.models_captured_at, Some(1_700_000_000_000));
        let models = spec.models.as_ref().expect("the captured list");
        assert_eq!(models.len(), 1, "the manifest's list is replaced");
        assert_eq!(models[0].model, "captured");
        assert_eq!(models[0].display_name.as_deref(), Some("Captured"));
    }

    #[test]
    fn a_models_file_that_escapes_the_folder_is_ignored() {
        let data = tempfile::tempdir().unwrap();
        let dir = data.path().join("plugins/local/freebuff");
        std::fs::create_dir_all(dir.join("bin")).unwrap();
        std::fs::write(dir.join(driver_path()), "driver").unwrap();
        std::fs::write(
            dir.join("openremote.json"),
            harness_manifest_json(
                driver_path(),
                r#","models":[{"model":"shipped"}],"models_file":"../models.json""#,
            ),
        )
        .unwrap();
        // A real file sits one folder up: the escape is refused, not just
        // missed because nothing was there.
        std::fs::write(
            data.path().join("plugins/local/models.json"),
            r#"{"captured_at":1700000000000,"models":[{"model":"outside"}]}"#,
        )
        .unwrap();

        let record = installed_record(data.path(), "freebuff", "freebuff");
        let specs = installed_harnesses(data.path(), &[&record]);
        let spec = specs.first().expect("one harness");
        assert_eq!(spec.models_captured_at, None);
        let models = spec.models.as_ref().expect("the manifest's list");
        assert_eq!(models.len(), 1);
        assert_eq!(models[0].model, "shipped");
    }

    #[test]
    fn a_malformed_models_file_falls_back_to_the_manifest() {
        let data = tempfile::tempdir().unwrap();
        let dir = data.path().join("plugins/local/freebuff");
        std::fs::create_dir_all(dir.join("bin")).unwrap();
        std::fs::write(dir.join(driver_path()), "driver").unwrap();
        std::fs::write(
            dir.join("openremote.json"),
            harness_manifest_json(
                driver_path(),
                r#","models":[{"model":"shipped"}],"models_file":"models.json""#,
            ),
        )
        .unwrap();
        std::fs::write(dir.join("models.json"), "{not json").unwrap();

        let record = installed_record(data.path(), "freebuff", "freebuff");
        let specs = installed_harnesses(data.path(), &[&record]);
        let spec = specs.first().expect("one harness");
        assert_eq!(spec.models_captured_at, None);
        let models = spec.models.as_ref().expect("the manifest's list");
        assert_eq!(models.len(), 1);
        assert_eq!(models[0].model, "shipped");
    }

    #[test]
    fn an_unknown_settings_word_is_refused_at_install() {
        let os = std::env::consts::OS;
        let dir = folder(&[(
            "openremote.json",
            &format!(
                r#"{{"api":1,"name":"x","runtime":"process","harness":{{"api":1,"id":"x","name":"X","driver":{{"{os}":"d"}},"settings":"sometimes"}}}}"#
            ),
        )]);
        let err = read_manifest(dir.path(), &builtins()).expect_err("an unknown word");
        assert!(err.to_string().contains("live"), "{err}");
        assert!(err.to_string().contains("sometimes"), "{err}");
    }

    /// A manifest with the contract's harness block, in this OS's driver key.
    fn traits_manifest(extra: &str) -> tempfile::TempDir {
        let os = std::env::consts::OS;
        folder(&[(
            "openremote.json",
            &format!(
                r#"{{"api":1,"name":"x","runtime":"process","harness":{{"api":1,"id":"x","name":"X","driver":{{"{os}":"d"}}{extra}}}}}"#
            ),
        )])
    }

    #[test]
    fn a_harness_manifest_declares_its_permission_words_fast_mcp_menus_and_prompt_rules() {
        let dir = traits_manifest(
            r#","permission_modes":["ask",{"w":"auto","l":"Run without asking"}],"default_permission":"ask","fast":true,"mcp":true,"commands":true,"steers_mid_turn":true"#,
        );
        let manifest = read_manifest(dir.path(), &builtins()).expect("reads");
        let traits = manifest.harness.expect("harness").traits();
        assert_eq!(
            traits.permission_modes,
            vec![
                PermissionWord::new("ask", "ask"),
                PermissionWord::new("auto", "Run without asking"),
            ],
            "a bare word is its own label; a row keeps its own"
        );
        assert_eq!(traits.default_permission.as_deref(), Some("ask"));
        assert!(traits.fast && traits.mcp && traits.commands && traits.steers_mid_turn);
        assert!(
            !traits.one_prompt_at_a_time,
            "a harness that steers takes the second message inside the turn"
        );

        // A block that says nothing keeps every old answer: one prompt at a
        // time, no pickers, no MCP, no process started for a menu.
        let dir = traits_manifest("");
        let manifest = read_manifest(dir.path(), &builtins()).expect("reads");
        assert_eq!(
            manifest.harness.expect("harness").traits(),
            HarnessTraits::default()
        );
        assert!(HarnessTraits::default().one_prompt_at_a_time);
    }

    #[test]
    fn a_harness_manifest_whose_words_disagree_is_refused() {
        let unlisted =
            traits_manifest(r#","permission_modes":["ask"],"default_permission":"auto""#);
        let err = read_manifest(unlisted.path(), &builtins()).expect_err("an unlisted default");
        assert!(err.to_string().contains("'auto'"), "{err}");

        let twice = traits_manifest(r#","permission_modes":["ask",{"w":"ask","l":"Ask"}]"#);
        let err = read_manifest(twice.path(), &builtins()).expect_err("a word twice");
        assert!(err.to_string().contains("twice"), "{err}");

        let blank = traits_manifest(r#","permission_modes":[{"w":"ask","l":" "}]"#);
        assert!(
            read_manifest(blank.path(), &builtins()).is_err(),
            "a blank label"
        );

        let both = traits_manifest(r#","steers_mid_turn":true,"one_prompt_at_a_time":true"#);
        let err = read_manifest(both.path(), &builtins()).expect_err("steers and one at a time");
        assert!(err.to_string().contains("steers"), "{err}");
    }

    #[cfg(windows)]
    #[test]
    fn the_node_spelling_win32_is_accepted_for_a_hand_written_manifest() {
        let dir = folder(&[
            (
                "openremote.json",
                r#"{"api":1,"name":"x","runtime":"process","harness":{"api":1,"id":"x","name":"X","driver":{"win32":"bin/x.exe"}}}"#,
            ),
            ("bin/x.exe", "driver"),
        ]);
        let manifest = read_manifest(dir.path(), &builtins()).expect("reads");
        let harness = manifest.harness.expect("harness");
        assert_eq!(harness.driver_path(), Some("bin/x.exe"));
    }
}
