//! The harness registry: what's installed and how to drive it. Probing is
//! data-driven per harness crate; e2e injects fixture binaries through the
//! overrides map so every test drives the real spawn path.

use std::collections::HashMap;
use std::path::PathBuf;

use openremote_core::Harness;
use openremote_harness::{
    DriverError, DriverEvent, ModelDescriptor, Resolution, SessionSettings, SpawnOptions,
};
use serde_json::Value;
use tokio::sync::mpsc;

// The harness-neutral answer semantics live with the driver vocabulary;
// the per-harness filling happens in `SessionDriver::answer` below.
pub use openremote_harness::AnswerOutcome;

/// One harness's driving backend: the resolved CLI plus the spawn it fronts.
#[derive(Clone)]
pub enum Backend {
    Claude(openremote_claude::Resolution),
    Codex(openremote_codex::Resolution),
    Grok(openremote_grok::Resolution),
    Pi(openremote_pi::Resolution),
    Opencode(openremote_opencode::Resolution),
    Agy(openremote_agy::Resolution),
}

/// A live session's driver, dispatch-only - every method reaches the
/// concrete driver behind the same vocabulary.
pub enum SessionDriver {
    Claude(openremote_claude::Driver),
    Codex(openremote_codex::Driver),
    Grok(openremote_grok::Driver),
    Pi(openremote_pi::Driver),
    Opencode(openremote_opencode::Driver),
    Agy(openremote_agy::Driver),
}

impl SessionDriver {
    pub async fn send_prompt(&mut self, text: &str) -> Result<(), DriverError> {
        match self {
            SessionDriver::Claude(driver) => driver.send_prompt(text).await,
            SessionDriver::Codex(driver) => driver.send_prompt(text).await,
            SessionDriver::Grok(driver) => driver.send_prompt(text).await,
            SessionDriver::Pi(driver) => driver.send_prompt(text).await,
            SessionDriver::Opencode(driver) => driver.send_prompt(text).await,
            SessionDriver::Agy(driver) => driver.send_prompt(text).await,
        }
    }

    /// Apply a live model, effort, or fast change in the harness's own
    /// words. A harness with no wire for a field says so - the console
    /// does not render that control.
    pub async fn apply_settings(&mut self, settings: &SessionSettings) -> Result<(), DriverError> {
        match self {
            // Claude's own slash commands (`/model`, `/effort`, `/fast`)
            // ride the user-message wire; the fresh init frame confirms.
            SessionDriver::Claude(driver) => driver.apply_settings(settings).await,
            // Codex's own wire is thread/settings/update - a turn/start
            // override is ignored once the thread adopted a model.
            SessionDriver::Codex(driver) => driver.apply_settings(settings).await,
            // Grok, OpenCode, and Antigravity take model and effort on the
            // process that starts the chat. A running process has no wire
            // for a change; resume is how it lands.
            SessionDriver::Grok(_) | SessionDriver::Opencode(_) | SessionDriver::Agy(_) => {
                Err(DriverError::Harness(
                    "this harness applies model and effort when the chat starts - stop it and resume to change them".into(),
                ))
            }
            SessionDriver::Pi(driver) => driver.apply_settings(settings).await,
        }
    }

    pub async fn interrupt(&mut self) -> Result<(), DriverError> {
        match self {
            SessionDriver::Claude(driver) => driver.interrupt().await,
            SessionDriver::Codex(driver) => driver.interrupt().await,
            SessionDriver::Grok(driver) => driver.interrupt().await,
            SessionDriver::Pi(driver) => driver.interrupt().await,
            SessionDriver::Opencode(driver) => driver.interrupt().await,
            SessionDriver::Agy(driver) => driver.interrupt().await,
        }
    }

    pub async fn answer(
        &mut self,
        harness_ref: &str,
        choice: &str,
        request: &Value,
        tool_name: Option<&str>,
    ) -> Result<AnswerOutcome, DriverError> {
        match self {
            SessionDriver::Claude(driver) => {
                driver.answer(harness_ref, choice, request).await?;
                // Claude's control protocol offers allow/deny per request
                // only - its own session-scoped grants live in its TUI, not
                // on this wire.
                Ok(AnswerOutcome::default())
            }
            SessionDriver::Codex(driver) => {
                driver.answer(harness_ref, choice, request).await?;
                // Codex's own semantics (schema): decline continues the
                // turn, cancel interrupts it, acceptForSession grants the
                // tool for the session.
                Ok(AnswerOutcome {
                    interrupts_turn: choice == "cancel",
                    session_grant: (choice == "acceptForSession")
                        .then(|| tool_name.unwrap_or("tool").to_string()),
                })
            }
            SessionDriver::Grok(driver) => {
                driver.answer(harness_ref, choice, request).await?;
                Ok(AnswerOutcome::default())
            }
            SessionDriver::Pi(driver) => {
                driver.answer(harness_ref, choice, request).await?;
                Ok(AnswerOutcome::default())
            }
            SessionDriver::Opencode(driver) => {
                driver.answer(harness_ref, choice, request).await?;
                // OpenCode's own words: `always` grants the action for the
                // session.
                Ok(AnswerOutcome {
                    interrupts_turn: false,
                    session_grant: (choice == "always")
                        .then(|| tool_name.unwrap_or("tool").to_string()),
                })
            }
            SessionDriver::Agy(driver) => {
                driver.answer(harness_ref, choice, request).await?;
                Ok(AnswerOutcome::default())
            }
        }
    }

    pub async fn shutdown(&mut self) -> Result<Option<i32>, DriverError> {
        match self {
            SessionDriver::Claude(driver) => driver.shutdown().await,
            SessionDriver::Codex(driver) => driver.shutdown().await,
            SessionDriver::Grok(driver) => driver.shutdown().await,
            SessionDriver::Pi(driver) => driver.shutdown().await,
            SessionDriver::Opencode(driver) => driver.shutdown().await,
            SessionDriver::Agy(driver) => driver.shutdown().await,
        }
    }
}

impl Backend {
    /// Codex's spawn runs its app-server handshake before returning; the
    /// claude transport boots without one. Both land on the same shape.
    pub async fn spawn(
        &self,
        opts: SpawnOptions,
    ) -> Result<(SessionDriver, mpsc::Receiver<DriverEvent>), DriverError> {
        match self {
            Backend::Claude(resolution) => {
                let (driver, rx) = openremote_claude::Driver::spawn(resolution, opts)?;
                Ok((SessionDriver::Claude(driver), rx))
            }
            Backend::Codex(resolution) => {
                let (driver, rx) = openremote_codex::Driver::spawn(resolution, opts).await?;
                Ok((SessionDriver::Codex(driver), rx))
            }
            Backend::Grok(resolution) => {
                let (driver, rx) = openremote_grok::Driver::spawn(resolution, opts)?;
                Ok((SessionDriver::Grok(driver), rx))
            }
            Backend::Pi(resolution) => {
                let (driver, rx) = openremote_pi::Driver::spawn(resolution, opts).await?;
                Ok((SessionDriver::Pi(driver), rx))
            }
            Backend::Opencode(resolution) => {
                let (driver, rx) = openremote_opencode::Driver::spawn(resolution, opts).await?;
                Ok((SessionDriver::Opencode(driver), rx))
            }
            Backend::Agy(resolution) => {
                let (driver, rx) = openremote_agy::Driver::spawn(resolution, opts)?;
                Ok((SessionDriver::Agy(driver), rx))
            }
        }
    }

    /// The models this harness advertises, in its own words. `None` = the
    /// harness doesn't advertise through us (the console's slot stays
    /// reserved).
    pub async fn models(&self) -> Option<Vec<ModelDescriptor>> {
        match self {
            Backend::Claude(resolution) => {
                // The SDK's initialize handshake - its `models` array is the
                // list the CLI's own /model picker serves.
                openremote_claude::driver::models(resolution).await.ok()
            }
            Backend::Codex(resolution) => openremote_codex::models(resolution).await.ok(),
            // Grok's own cache of its models endpoint - its names, its
            // effort words. Grok refetches the cache; we read it.
            Backend::Grok(_) => Some(openremote_grok::config::catalog()),
            // Pi's model catalog needs a signed-in provider (`pi
            // --list-models` answers "No models available" here) - the
            // slot stays reserved until that probe is honest.
            Backend::Pi(_) => None,
            // OpenCode's /api/model is a catalog without a current-model
            // marker (observed live: the model only rides step-started
            // events) - the catalog stays reserved.
            Backend::Opencode(_) => None,
            // Antigravity's `agy models` prints its own catalog (TSV).
            Backend::Agy(resolution) => openremote_agy::driver::models(resolution).await.ok(),
        }
    }

    /// The model a fresh chat on this harness will run, from the harness's
    /// own config - its words, replaced by the wire's first-turn truth.
    /// `None` = nothing honest to say pre-send (the slot stays reserved).
    pub fn default_model(&self) -> Option<(String, Option<String>)> {
        match self {
            // Claude's own priority chain (pick → env → settings.json →
            // default env), verified against its docs; a probe process
            // emits nothing before a turn (observed) so config is the only
            // pre-send fact.
            Backend::Claude(_) => {
                openremote_claude::config::configured_model(None).map(|model| (model, None))
            }
            Backend::Codex(_) => None, // the thread/start echo at create is the fact
            // Grok's config.toml [models] table: default + its own effort.
            Backend::Grok(_) => openremote_grok::config::default_model(),
            Backend::Pi(_) => None, // get_state at create is the fact
            // OpenCode's /api/model is a catalog, not a current-model
            // answer (observed live) - nothing pre-send; the first
            // step-start reports the model.
            Backend::Opencode(_) => None,
            // Antigravity's own settings file - the same source its
            // banner shows, verbatim (the effort rides inside the name).
            Backend::Agy(_) => openremote_agy::config::default_model().map(|m| (m, None)),
        }
    }
}

struct RegistryEntry {
    harness: Harness,
    backend: Option<Backend>,
}

pub struct HarnessRegistry {
    entries: Vec<RegistryEntry>,
    /// The overrides this registry was probed with - an install re-probes
    /// with the same ones.
    overrides: HashMap<String, PathBuf>,
    /// The Node.js pi would run on, by its own `--version` - whether pi's
    /// install needs a runtime set up first.
    node_version: Option<String>,
}

fn harness_from(id: &str, name: &str, resolution: &Resolution, version: Option<&str>) -> Harness {
    Harness {
        id: id.to_string(),
        name: name.to_string(),
        path: match resolution {
            Resolution::Executable(p) => Some(p.display().to_string()),
            Resolution::NodeScript { node, script } => {
                Some(format!("{} {}", node.display(), script.display()))
            }
            Resolution::Unavailable => None,
        },
        version: version.map(String::from),
        available: resolution.is_available(),
        fast_supported: fast_supported(id, version),
        signed_in: None,
        default_model: None,
        default_effort: None,
    }
}

impl HarnessRegistry {
    /// Probe every supported harness. `overrides` maps harness ids to
    /// binaries (e2e points them at fixture agents) and is authoritative
    /// where given. One bounded `--version` per harness runs in parallel -
    /// a CLI that doesn't answer just stays unprobed, never a startup
    /// gate.
    pub async fn probe(overrides: &HashMap<String, PathBuf>) -> Self {
        let claude =
            openremote_claude::resolve_claude(overrides.get("claude").map(|p| p.as_path()));
        let codex = openremote_codex::resolve_codex(overrides.get("codex").map(|p| p.as_path()));
        let grok = openremote_grok::resolve_grok(overrides.get("grok").map(|p| p.as_path()));
        let pi = openremote_pi::resolve_pi(overrides.get("pi").map(|p| p.as_path()));
        let opencode =
            openremote_opencode::resolve_opencode(overrides.get("opencode").map(|p| p.as_path()));
        let agy = openremote_agy::resolve_agy(overrides.get("agy").map(|p| p.as_path()));

        let node = crate::install::node_for_pi()
            .map(Resolution::Executable)
            .unwrap_or(Resolution::Unavailable);
        let (v_claude, v_codex, v_grok, v_pi, v_opencode, v_agy, v_node) = tokio::join!(
            probe_version(&claude),
            probe_version(&codex),
            probe_version(&grok),
            probe_version(&pi),
            probe_version(&opencode),
            probe_version(&agy),
            probe_version(&node),
        );
        // Sign-in is a separate fact from "the CLI is installed". Only a
        // harness with its own status command is asked; the others stay
        // unknown rather than guessed.
        let (s_claude, s_codex, s_grok, s_opencode) = tokio::join!(
            probe_signed_in(&claude, &["auth", "status"]),
            probe_signed_in(&codex, &["login", "status"]),
            probe_grok_signed_in(&grok),
            probe_signed_in(&opencode, &["providers", "list"]),
        );

        let mut entries = Vec::new();
        let mut push = |id: &str, name: &str, resolution: Resolution, version: Option<String>| {
            let backend = if resolution.is_available() {
                Some(match id {
                    "claude" => Backend::Claude(resolution.clone()),
                    "codex" => Backend::Codex(resolution.clone()),
                    "grok" => Backend::Grok(resolution.clone()),
                    "pi" => Backend::Pi(resolution.clone()),
                    "opencode" => Backend::Opencode(resolution.clone()),
                    _ => Backend::Agy(resolution.clone()),
                })
            } else {
                None
            };
            entries.push(RegistryEntry {
                harness: harness_from(id, name, &resolution, version.as_deref()),
                backend,
            });
        };
        push("claude", "Claude Code", claude, v_claude);
        push("codex", "Codex", codex, v_codex);
        push("grok", "Grok Build", grok, v_grok);
        push("pi", "Pi Agent", pi, v_pi);
        push("opencode", "OpenCode", opencode, v_opencode);
        push("agy", "Antigravity", agy, v_agy);
        for entry in &mut entries {
            entry.harness.signed_in = match entry.harness.id.as_str() {
                "claude" => s_claude,
                "codex" => s_codex,
                "grok" => s_grok,
                "opencode" => s_opencode,
                // Pi's check needs a provider, and Antigravity has no status
                // command. Unknown, not a guessed sign-in.
                _ => None,
            };
            // The pre-send model fact rides with the harness - the
            // harness's own config, its words (a fixture is not the
            // harness; its slots stay silent).
            if !overrides.contains_key(&entry.harness.id) {
                if let Some(backend) = entry.backend.as_ref() {
                    if let Some((model, effort)) = backend.default_model() {
                        entry.harness.default_model = Some(model);
                        entry.harness.default_effort = effort;
                    }
                }
            }
        }

        Self {
            entries,
            overrides: overrides.clone(),
            node_version: v_node,
        }
    }

    /// Re-run the probe with the same overrides - after an install
    /// changed what's on this machine.
    pub async fn reprobe(&self) -> Self {
        Self::probe(&self.overrides).await
    }

    /// The overrides this registry was probed with.
    pub fn overrides(&self) -> &HashMap<String, PathBuf> {
        &self.overrides
    }

    /// Whether a Node.js new enough for pi is on this machine.
    pub fn node_ready(&self) -> bool {
        self.node_version
            .as_deref()
            .is_some_and(|v| version_at_least(v, crate::install::PI_NODE_MIN))
    }

    pub fn harnesses(&self) -> Vec<Harness> {
        self.entries.iter().map(|e| e.harness.clone()).collect()
    }

    pub fn backend(&self, id: &str) -> Option<&Backend> {
        self.entries
            .iter()
            .find(|e| e.harness.id == id && e.harness.available)
            .and_then(|e| e.backend.as_ref())
    }

    /// Whether this harness slot is fixture-backed - the e2e suite
    /// injects binaries through the override map, and a fixture's login
    /// behavior is not a real sign-in. Production probed with no
    /// overrides; nothing there is a fixture.
    pub fn is_fixture(&self, id: &str) -> bool {
        self.overrides.contains_key(id)
    }

    /// Advertised models for a harness; empty when the harness doesn't
    /// advertise through us.
    pub async fn models(&self, id: &str) -> Vec<ModelDescriptor> {
        match self.backend(id) {
            Some(backend) => backend.models().await.unwrap_or_default(),
            None => Vec::new(),
        }
    }
}

/// Debug/testing escape hatch: `OPENREMOTE_<HARNESS>_PATH` forces a
/// harness's binary (e.g. a fixture agent) without touching PATH. The
/// override is authoritative - an injected daemon never leaks to the
/// machine's real CLIs.
pub fn env_overrides() -> HashMap<String, PathBuf> {
    let mut map = HashMap::new();
    for (key, value) in std::env::vars_os() {
        let Some(key) = key.to_str() else { continue };
        let Some(rest) = key.strip_prefix("OPENREMOTE_") else {
            continue;
        };
        let Some(harness) = rest.strip_suffix("_PATH") else {
            continue;
        };
        if !harness.is_empty() {
            map.insert(harness.to_ascii_lowercase(), PathBuf::from(value));
        }
    }
    map
}

/// Whether the harness's own fast mode is usable at the probed version:
/// claude's headless `fastMode` needs v2.1.205+, codex's `fast` service
/// tier v0.110.0+. The other harnesses have no fast mode to offer - their
/// chips never render.
fn fast_supported(id: &str, version: Option<&str>) -> bool {
    match (id, version) {
        ("claude", Some(v)) => version_at_least(v, (2, 1, 205)),
        ("codex", Some(v)) => version_at_least(v, (0, 110, 0)),
        _ => false,
    }
}

/// The first `d.d.d` run in a CLI's `--version` output, whatever else it
/// prints (`2.1.287 (Claude Code)`, `codex-cli 0.160.0`, …).
fn parse_version(text: &str) -> Option<String> {
    for token in text.split(|c: char| !(c.is_ascii_digit() || c == '.')) {
        let parts: Vec<&str> = token.split('.').collect();
        if parts.len() >= 3
            && parts
                .iter()
                .take(3)
                .all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()))
        {
            return Some(token.to_string());
        }
    }
    None
}

/// Lexicographic `d.d.d` compare, shorter versions padded with zeros.
fn version_at_least(version: &str, min: (u64, u64, u64)) -> bool {
    let parts: Vec<u64> = version
        .split('.')
        .map(|p| p.parse::<u64>().unwrap_or(0))
        .collect();
    let at = |i: usize| parts.get(i).copied().unwrap_or(0);
    (at(0), at(1), at(2)) >= min
}

/// One bounded `--version` run against the resolved CLI - a catalog fact,
/// never a startup gate.
async fn probe_version(resolution: &Resolution) -> Option<String> {
    let (program, argv): (PathBuf, Vec<std::ffi::OsString>) = match resolution {
        Resolution::Executable(path) => (path.clone(), Vec::new()),
        Resolution::NodeScript { node, script } => {
            (node.clone(), vec![script.as_os_str().to_os_string()])
        }
        Resolution::Unavailable => return None,
    };
    let probe = tokio::process::Command::new(&program)
        .args(&argv)
        .arg("--version")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .output();
    let output = tokio::time::timeout(std::time::Duration::from_secs(5), probe)
        .await
        .ok()?
        .ok()?;
    parse_version(&String::from_utf8_lossy(&output.stdout))
}

/// Grok has no status command. Its own sign-in fact is `~/.grok/auth.json`,
/// written by `grok login` and nowhere else. A fixture binary is not Grok,
/// so it stays unknown.
async fn probe_grok_signed_in(resolution: &Resolution) -> Option<bool> {
    let version = probe_version(resolution).await?;
    if !version.starts_with("1.") {
        return None;
    }
    grok_auth_file_present()
}

/// Whether the CLI reports itself signed in, in its own words.
///
/// - Claude: `auth status` JSON `loggedIn`.
/// - Codex: `login status` prints "Logged in" or "Not logged in".
/// - OpenCode: `providers list` ends with "N credentials".
///
/// `None` when the command cannot be asked (fixture binaries, a CLI that
/// did not answer). A fixture must not be reported as signed out.
async fn probe_signed_in(resolution: &Resolution, args: &[&str]) -> Option<bool> {
    let (program, mut argv): (PathBuf, Vec<std::ffi::OsString>) = match resolution {
        Resolution::Executable(path) => (path.clone(), Vec::new()),
        Resolution::NodeScript { node, script } => {
            (node.clone(), vec![script.as_os_str().to_os_string()])
        }
        Resolution::Unavailable => return None,
    };
    argv.extend(args.iter().map(|arg| (*arg).into()));
    let probe = tokio::process::Command::new(&program)
        .args(&argv)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output();
    let output = tokio::time::timeout(std::time::Duration::from_secs(8), probe)
        .await
        .ok()?
        .ok()?;
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    parse_signed_in(&text)
}

/// Grok writes `auth.json` when `grok login` succeeds and nowhere else.
/// An empty or missing file is signed out; a file we cannot read is unknown.
fn grok_auth_file_present() -> Option<bool> {
    let home = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME"))?;
    let path = PathBuf::from(home).join(".grok").join("auth.json");
    match std::fs::read_to_string(&path) {
        Ok(text) => Some(!text.trim().is_empty() && text.trim() != "{}"),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Some(false),
        Err(_) => None,
    }
}

/// The harness's own status words. Anything else - a fixture's usage
/// text, a command the binary does not have - is unknown, not signed out.
fn parse_signed_in(text: &str) -> Option<bool> {
    let lower = text.to_ascii_lowercase();
    if let Some(value) = serde_json::from_str::<serde_json::Value>(text.trim())
        .ok()
        .and_then(|v| v.get("loggedIn").and_then(|b| b.as_bool()))
    {
        return Some(value);
    }
    if lower.contains("not logged in") {
        return Some(false);
    }
    if lower.contains("logged in") {
        return Some(true);
    }
    // OpenCode: "8 credentials" / "0 credentials". The box-drawing list
    // is not the fact; the count line is.
    if let Some(rest) = lower.split("credentials").next() {
        if lower.contains("credentials") {
            let count = rest
                .split(|c: char| !c.is_ascii_digit())
                .rfind(|p| !p.is_empty())
                .and_then(|p| p.parse::<u64>().ok());
            if let Some(count) = count {
                return Some(count > 0);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_come_out_of_whatever_the_cli_prints() {
        assert_eq!(
            parse_version("2.1.287 (Claude Code)"),
            Some("2.1.287".to_string())
        );
        assert_eq!(
            parse_version("codex-cli 0.160.0"),
            Some("0.160.0".to_string())
        );
        assert_eq!(
            parse_version("opencode v0.14.105\n"),
            Some("0.14.105".to_string())
        );
        assert_eq!(parse_version("no digits here"), None);
        assert_eq!(parse_version("1.2"), None);
        // a pre-release suffix never corrupts the number
        assert_eq!(parse_version("2.1.205-beta.1"), Some("2.1.205".to_string()));
    }

    #[test]
    fn sign_in_uses_the_harnesss_own_words() {
        assert_eq!(parse_signed_in(r#"{"loggedIn": true}"#), Some(true));
        assert_eq!(parse_signed_in(r#"{"loggedIn": false}"#), Some(false));
        assert_eq!(parse_signed_in("Logged in using ChatGPT"), Some(true));
        assert_eq!(parse_signed_in("Not logged in\n"), Some(false));
        assert_eq!(parse_signed_in("└  8 credentials\n"), Some(true));
        assert_eq!(parse_signed_in("0 credentials"), Some(false));
        // A fixture's usage text is not a sign-in fact.
        assert_eq!(parse_signed_in("fixture-agent 9.9.9\nUsage: fixture"), None);
    }

    #[test]
    fn fast_mode_gates_on_each_harnesss_own_minimum() {
        // claude: headless fastMode needs 2.1.205+
        assert!(fast_supported("claude", Some("2.1.205")));
        assert!(fast_supported("claude", Some("2.1.287")));
        assert!(!fast_supported("claude", Some("2.1.204")));
        assert!(!fast_supported("claude", None));
        // codex: the fast service tier needs 0.110.0+
        assert!(fast_supported("codex", Some("0.110.0")));
        assert!(fast_supported("codex", Some("0.160.0")));
        assert!(!fast_supported("codex", Some("0.109.9")));
        // nobody else offers a fast mode
        assert!(!fast_supported("grok", Some("9.9.9")));
        assert!(!fast_supported("pi", Some("9.9.9")));
        assert!(!fast_supported("opencode", Some("9.9.9")));
        assert!(!fast_supported("agy", Some("9.9.9")));
    }
}
