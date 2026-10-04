//! OpenCode harness driver.
//!
//! OpenCode is an HTTP service, not a stdio protocol: the driver spawns
//! `opencode serve --port 0` per session (basic auth via
//! `OPENCODE_SERVER_PASSWORD`, username `opencode` - verified live
//! against opencode 1.18.21 and its OpenAPI 3.1 at `/doc`), creates the
//! session with `POST /api/session`, prompts with `POST …/prompt`
//! (async - `SessionInputAdmitted`), and consumes `GET /api/event` (the
//! global SSE bus, filtered by `properties.sessionID`). Permissions and
//! questions arrive as events and are answered with OpenCode's own
//! words (`once | always | reject`; question answers as selected
//! labels); the turn boundary is `session.next.step.ended` with its own
//! `finish` string.

pub mod driver;
pub mod http;
pub mod resolve;

use std::path::Path;

pub use driver::Driver;
pub use openremote_harness::{DriverError, DriverEvent, Resolution, SpawnOptions};

/// Find the opencode CLI. `override` wins (tests point it at the
/// fixture); otherwise `opencode` on PATH.
pub fn resolve_opencode(override_path: Option<&Path>) -> Resolution {
    resolve::resolve_impl(override_path.map(std::path::PathBuf::from))
}
