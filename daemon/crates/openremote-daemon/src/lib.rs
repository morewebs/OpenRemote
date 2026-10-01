//! The daemon library: supervisor, HTTP API, SSE. The binary in `main.rs`
//! is a thin shell over `app::serve`.

pub mod app;
pub mod http;
pub mod supervisor;

pub use app::{AppOptions, serve};
pub use supervisor::{Supervisor, SupervisorError};
