//! The daemon library: registry, supervisor, HTTP API, SSE. The binary in
//! `main.rs` is a thin shell over `app::serve`.

pub mod app;
pub mod http;
pub mod install;
pub mod plugins;
pub mod registry;
pub mod supervisor;

pub use app::{AppOptions, serve};
pub use registry::HarnessRegistry;
pub use supervisor::{Supervisor, SupervisorError};
