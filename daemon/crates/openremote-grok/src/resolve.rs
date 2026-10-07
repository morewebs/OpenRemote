//! Grok CLI resolution: PATH first, then the documented install root
//! `~/.grok/bin` (verified on this machine).

use std::path::PathBuf;

use openremote_harness::Resolution;
use openremote_harness::paths::{home_dir, which};

pub(crate) fn resolve_impl(override_path: Option<PathBuf>) -> Resolution {
    // An override is authoritative: an injected daemon never leaks to the
    // machine's real CLIs, and an install test can make this harness
    // appear by materializing exactly this path.
    if let Some(path) = override_path {
        if path.is_file() {
            return Resolution::Executable(path);
        }
        return Resolution::Unavailable;
    }
    let name = if cfg!(windows) { "grok.exe" } else { "grok" };
    if let Some(exe) = which(name) {
        return Resolution::Executable(exe);
    }
    if let Some(home) = home_dir() {
        let exe = home.join(".grok").join("bin").join(name);
        if exe.is_file() {
            return Resolution::Executable(exe);
        }
    }
    Resolution::Unavailable
}
