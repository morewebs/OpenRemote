//! Grok Build harness driver.
//!
//! Grok's headless surface, as its own headless guide documents it
//! (`~/.grok/docs/user-guide/14-headless-mode.md`, checked against grok
//! 1.0.77): `grok --output-format streaming-messages-json --prompt-file
//! <path>` - NDJSON in the Anthropic Messages wire (`system/init` with the
//! session id, model and slash commands; `assistant` content blocks of
//! text, thinking and `tool_use`; `user` frames of `tool_result`; a
//! `compact_boundary` system frame; `result` as the turn boundary) - the
//! same wire family Claude Code speaks. The prompt rides a file so chat
//! text never stands in an argv slot. Print mode: **one process per
//! prompt**; continuation respawns with `--resume <session id>`.
//!
//! Honest limitation: grok print mode has no remote
//! approval channel - approvals are governed by its own
//! `--permission-mode` words (`default acceptEdits auto dontAsk
//! bypassPermissions plan`), passed through verbatim.

pub mod auth;
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
