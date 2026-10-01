//! Claude Code harness driver.
//!
//! A Rust port of the `claude-agent-sdk-python` (MIT) process transport:
//! one persistent `claude` process per session speaking
//! `--input-format stream-json --output-format stream-json --verbose`,
//! with `--permission-prompt-tool stdio` so approvals surface as
//! `can_use_tool` control requests — the daemon answers them as the SDK
//! host would. The turn boundary is the `result` message; never guess
//! turn state from absence of output.

pub mod driver;
pub mod frames;
pub mod resolve;

pub use driver::{Driver, DriverError, DriverEvent, DriverOptions};
pub use resolve::{Resolution, resolve_claude};
