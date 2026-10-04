//! Claude Code harness driver.
//!
//! A Rust port of the `claude-agent-sdk-python` (MIT) process transport:
//! one persistent `claude` process per session speaking
//! `--input-format stream-json --output-format stream-json --verbose`,
//! with `--permission-prompt-tool stdio` so approvals surface as
//! `can_use_tool` control requests — the daemon answers them as the SDK
//! host would. The turn boundary is the `result` message; never guess
//! turn state from absence of output.

pub mod config;
pub mod driver;
pub mod resolve;

pub use openremote_harness::anthropic_wire as frames;

pub use driver::Driver;
pub use openremote_harness::{DriverError, DriverEvent, Resolution, SpawnOptions};

use std::path::Path;

/// Find the Claude Code CLI. `override` wins (tests point it at the
/// fixture agent); otherwise the native binary on PATH, then the SDK's
/// POSIX fallbacks, then the npm layouts.
pub fn resolve_claude(override_path: Option<&Path>) -> Resolution {
    resolve::resolve_impl(override_path.map(std::path::PathBuf::from))
}
