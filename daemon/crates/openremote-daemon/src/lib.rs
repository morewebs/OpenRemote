//! The daemon library: registry, supervisor, HTTP API, SSE, phone pairing. The
//! binary in `main.rs` serves the same router as `app::serve` with the pairing
//! control routes merged in, because a socket decides who may call those, not a
//! header check.

pub mod adapt;
pub mod app;
pub mod browser;
pub mod cloud_http;
pub mod embedded;
pub mod files;
pub mod fsdirs;
pub mod host;
pub mod http;
pub mod images;
pub mod install;
pub mod marketplace;
pub mod mcp_chat;
pub mod pairing;
pub mod pairing_page;
pub mod plugin_driver;
pub mod plugin_runtime;
pub mod plugins;
pub mod registry;
pub mod shutdown;
pub mod signin;
pub mod supervisor;
pub mod sync;
pub mod team;

pub use app::{AppOptions, serve};
pub use registry::HarnessRegistry;
pub use supervisor::{Supervisor, SupervisorError};
