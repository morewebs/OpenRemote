//! The daemon library: registry, supervisor, HTTP API, SSE. The binary in
//! `main.rs` is a thin shell over `app::serve`.

pub mod app;
pub mod cloud_http;
pub mod embedded;
pub mod fsdirs;
pub mod host;
pub mod http;
pub mod install;
pub mod plugins;
pub mod registry;
pub mod signin;
pub mod supervisor;
pub mod sync;

pub use app::{AppOptions, serve};
pub use registry::HarnessRegistry;
pub use supervisor::{Supervisor, SupervisorError};
