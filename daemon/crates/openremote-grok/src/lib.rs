//! Grok Build harness driver.
//!
//! Grok's headless surface (verified live against grok 1.0.46,
//! 2026-10-01): `grok --single <prompt> --output-format
//! streaming-messages-json` - NDJSON in the Anthropic Messages wire
//! (`system/init` with the session id and model, `assistant` content
//! blocks, `result` as the turn boundary) - the same wire family Claude
//! Code speaks. Print mode: **one process per prompt**; continuation
//! respawns with `--resume <session id>`.
//!
//! Honest limitation (recorded in the map): grok print mode has no remote
//! approval channel - approvals are governed by its own
//! `--permission-mode` words (`default acceptEdits auto dontAsk
//! bypassPermissions plan`), passed through verbatim.

pub mod config;
pub mod driver;
pub mod resolve;

use std::path::Path;

pub use driver::Driver;
pub use openremote_harness::{DriverError, DriverEvent, Resolution, SpawnOptions};

/// Find the Grok Build CLI. `override` wins (tests point it at the
/// fixture); otherwise `grok` on PATH, then `~/.grok/bin/grok(.exe)`.
pub fn resolve_grok(override_path: Option<&Path>) -> Resolution {
    resolve::resolve_impl(override_path.map(std::path::PathBuf::from))
}
