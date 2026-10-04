//! Pi Agent harness driver.
//!
//! Pi's `--mode rpc`: one persistent node process speaking flat JSON
//! lines - requests `{id, type, …params}`, responses `{type:
//! "response", id, command, success, data|error}`, and notifications
//! (`message_update`, `tool_execution_*`, `message_end`,
//! `agent_settled`). Behavior ported from davidondrej/cloudroom-core
//! `src/runtime/pi.rs` (Apache-2.0); the dialog contract
//! (`extension_ui_request` / `extension_ui_response` with `{value}` or
//! `{cancelled}`) read from the installed pi package
//! (@earendil-works/pi-coding-agent) - cloudroom auto-cancels dialogs,
//! OpenRemote ANSWERS them.

pub mod driver;
pub mod resolve;

use std::path::Path;

pub use driver::Driver;
pub use openremote_harness::{DriverError, DriverEvent, Resolution, SpawnOptions};

/// Find the pi CLI. `override` wins (tests point it at the fixture);
/// otherwise `pi` on PATH, then the npm layout
/// (`@earendil-works/pi-coding-agent/dist/cli.js` via node - verified
/// locally).
pub fn resolve_pi(override_path: Option<&Path>) -> Resolution {
    resolve::resolve_impl(override_path.map(std::path::PathBuf::from))
}
