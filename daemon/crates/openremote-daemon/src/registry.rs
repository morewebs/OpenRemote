//! The harness registry: what's installed and how to drive it. Probing is
//! data-driven per harness crate; e2e injects fixture binaries through the
//! overrides map so every test drives the real spawn path.

use std::collections::HashMap;
use std::path::PathBuf;

use openremote_core::Harness;
use openremote_harness::{DriverError, DriverEvent, ModelDescriptor, Resolution, SpawnOptions};
use serde_json::Value;
use tokio::sync::mpsc;

/// How the answer affects the running turn — the driver knows its own
/// semantics (codex `cancel` interrupts; claude's choices all continue).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnswerOutcome {
    /// The turn continues after this answer.
    Continues,
    /// The answer interrupts the turn; its completion event settles it.
    Interrupts,
}

/// One harness's driving backend: the resolved CLI plus the spawn it fronts.
pub enum Backend {
    Claude(openremote_claude::Resolution),
    Codex(openremote_codex::Resolution),
    Grok(openremote_grok::Resolution),
}

/// A live session's driver, dispatch-only — every method reaches the
/// concrete driver behind the same vocabulary.
pub enum SessionDriver {
    Claude(openremote_claude::Driver),
    Codex(openremote_codex::Driver),
    Grok(openremote_grok::Driver),
}

impl SessionDriver {
    pub async fn send_prompt(&mut self, text: &str) -> Result<(), DriverError> {
        match self {
            SessionDriver::Claude(driver) => driver.send_prompt(text).await,
            SessionDriver::Codex(driver) => driver.send_prompt(text).await,
            SessionDriver::Grok(driver) => driver.send_prompt(text).await,
        }
    }

    pub async fn interrupt(&mut self) -> Result<(), DriverError> {
        match self {
            SessionDriver::Claude(driver) => driver.interrupt().await,
            SessionDriver::Codex(driver) => driver.interrupt().await,
            SessionDriver::Grok(driver) => driver.interrupt().await,
        }
    }

    pub async fn answer(
        &mut self,
        harness_ref: &str,
        choice: &str,
        request: &Value,
    ) -> Result<AnswerOutcome, DriverError> {
        match self {
            SessionDriver::Claude(driver) => {
                driver.answer(harness_ref, choice, request).await?;
                Ok(AnswerOutcome::Continues)
            }
            SessionDriver::Codex(driver) => {
                driver.answer(harness_ref, choice, request).await?;
                // Codex's own semantics (schema): decline continues the
                // turn, cancel interrupts it.
                Ok(if choice == "cancel" {
                    AnswerOutcome::Interrupts
                } else {
                    AnswerOutcome::Continues
                })
            }
            SessionDriver::Grok(driver) => {
                driver.answer(harness_ref, choice, request).await?;
                Ok(AnswerOutcome::Continues)
            }
        }
    }

    pub async fn shutdown(&mut self) -> Result<Option<i32>, DriverError> {
        match self {
            SessionDriver::Claude(driver) => driver.shutdown().await,
            SessionDriver::Codex(driver) => driver.shutdown().await,
            SessionDriver::Grok(driver) => driver.shutdown().await,
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

fn harness_from(id: &str, name: &str, resolution: &Resolution) -> Harness {
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
        version: None,
        available: resolution.is_available(),
    }
}

impl HarnessRegistry {
    /// Probe every supported harness. `overrides` maps harness ids to
    /// binaries (e2e points them at fixture agents); probed CLIs win for
    /// everything not overridden.
    pub fn probe(overrides: &HashMap<String, PathBuf>) -> Self {
        let mut entries = Vec::new();

        let claude =
            openremote_claude::resolve_claude(overrides.get("claude").map(|p| p.as_path()));
        let backend = if claude.is_available() {
            Some(Backend::Claude(claude.clone()))
        } else {
            None
        };
        entries.push(RegistryEntry {
            harness: harness_from("claude", "Claude Code", &claude),
            backend,
        });

        let codex = openremote_codex::resolve_codex(overrides.get("codex").map(|p| p.as_path()));
        let backend = if codex.is_available() {
            Some(Backend::Codex(codex.clone()))
        } else {
            None
        };
        entries.push(RegistryEntry {
            harness: harness_from("codex", "Codex", &codex),
            backend,
        });

        let grok = openremote_grok::resolve_grok(overrides.get("grok").map(|p| p.as_path()));
        let backend = if grok.is_available() {
            Some(Backend::Grok(grok.clone()))
        } else {
            None
        };
        entries.push(RegistryEntry {
            harness: harness_from("grok", "Grok Build", &grok),
            backend,
        });

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
