//! The harness registry: what's installed and how to drive it. Probing is
//! data-driven per harness crate; e2e injects fixture binaries through the
//! overrides map so every test drives the real spawn path.

use std::collections::HashMap;
use std::path::PathBuf;

use openremote_core::Harness;
use openremote_harness::{DriverError, DriverEvent, ModelDescriptor, Resolution, SpawnOptions};
use serde_json::Value;
use tokio::sync::mpsc;

// The harness-neutral answer semantics live with the driver vocabulary;
// the per-harness filling happens in `SessionDriver::answer` below.
pub use openremote_harness::AnswerOutcome;

/// One harness's driving backend: the resolved CLI plus the spawn it fronts.
pub enum Backend {
    Claude(openremote_claude::Resolution),
    Codex(openremote_codex::Resolution),
    Grok(openremote_grok::Resolution),
    Pi(openremote_pi::Resolution),
    Opencode(openremote_opencode::Resolution),
    Agy(openremote_agy::Resolution),
}

/// A live session's driver, dispatch-only — every method reaches the
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
                // only — its own session-scoped grants live in its TUI, not
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
            Backend::Claude(_) => None,
            Backend::Codex(resolution) => openremote_codex::models(resolution).await.ok(),
            Backend::Grok(_) => None,
            // Pi's model catalog needs a signed-in provider (`pi
            // --list-models` answers "No models available" here) — the
            // slot stays reserved until that probe is honest.
            Backend::Pi(_) => None,
            // OpenCode's /api/model is per-project (it needs a serve in
            // the workspace) — a per-session model query lands with the
            // model slot ticket; the catalog stays reserved for now.
            Backend::Opencode(_) => None,
            // Antigravity's `agy models` prints its own catalog (TSV).
            Backend::Agy(resolution) => openremote_agy::driver::models(resolution).await.ok(),
        }
    }
}

struct RegistryEntry {
    harness: Harness,
    backend: Option<Backend>,
}

pub struct HarnessRegistry {
    entries: Vec<RegistryEntry>,
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
    }
}

impl HarnessRegistry {
    /// Probe every supported harness. `overrides` maps harness ids to
    /// binaries (e2e points them at fixture agents); probed CLIs win for
    /// everything not overridden. One bounded `--version` per harness runs
    /// in parallel — a CLI that doesn't answer just stays unprobed, never
    /// a startup gate.
    pub async fn probe(overrides: &HashMap<String, PathBuf>) -> Self {
        let claude =
            openremote_claude::resolve_claude(overrides.get("claude").map(|p| p.as_path()));
        let codex = openremote_codex::resolve_codex(overrides.get("codex").map(|p| p.as_path()));
        let grok = openremote_grok::resolve_grok(overrides.get("grok").map(|p| p.as_path()));
        let pi = openremote_pi::resolve_pi(overrides.get("pi").map(|p| p.as_path()));
        let opencode =
            openremote_opencode::resolve_opencode(overrides.get("opencode").map(|p| p.as_path()));
        let agy = openremote_agy::resolve_agy(overrides.get("agy").map(|p| p.as_path()));

        let (v_claude, v_codex, v_grok, v_pi, v_opencode, v_agy) = tokio::join!(
            probe_version(&claude),
            probe_version(&codex),
            probe_version(&grok),
            probe_version(&pi),
            probe_version(&opencode),
            probe_version(&agy),
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

        Self { entries }
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

    /// Advertised models for a harness; empty when the harness doesn't
    /// advertise through us.
    pub async fn models(&self, id: &str) -> Vec<ModelDescriptor> {
        match self.backend(id) {
            Some(backend) => backend.models().await.unwrap_or_default(),
            None => Vec::new(),
        }
    }
}

/// Whether the harness's own fast mode is usable at the probed version:
/// claude's headless `fastMode` needs v2.1.205+, codex's `fast` service
/// tier v0.110.0+. The other harnesses have no fast mode to offer — their
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

/// One bounded `--version` run against the resolved CLI — a catalog fact,
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
