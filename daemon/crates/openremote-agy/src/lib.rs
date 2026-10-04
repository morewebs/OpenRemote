//! Antigravity CLI (`agy`) harness driver.
//!
//! Verified live against agy 1.2.9: print mode (`--print <prompt>
//! --output-format stream-json`) emits one NDJSON frame family -
//! `{event: "init", conversation_id, init}`, `{event: "step_update",
//! step_update: {state, step_type, text_delta}}`, and `{event: "result",
//! result: {status, response, usage}}` where `status` is Antigravity's
//! own word (`SUCCESS`). One process per prompt; continuation respawns
//! with `--conversation <id>`. The model catalog is `agy models` (TSV:
//! id, display name - the harness's own words).
//!
//! The antigravity-cli repository carries no license file - its docs and
//! wire shapes are used as facts only; no code is ported from it.

pub mod config;
pub mod driver;
pub mod resolve;

use std::path::Path;

pub use driver::Driver;
pub use openremote_harness::{DriverError, DriverEvent, Resolution, SpawnOptions};

/// Find the agy CLI. `override` wins (tests point it at the fixture);
/// otherwise `agy` on PATH, then `~/.agy/bin`-style install roots.
pub fn resolve_agy(override_path: Option<&Path>) -> Resolution {
    resolve::resolve_impl(override_path.map(std::path::PathBuf::from))
}
