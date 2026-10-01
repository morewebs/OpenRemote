//! OpenCode CLI resolution: `opencode` on PATH (native binary in the
//! npm layout — verified locally).

use std::env;
use std::path::PathBuf;

use openremote_harness::Resolution;

pub(crate) fn resolve_impl(override_path: Option<PathBuf>) -> Resolution {
    if let Some(path) = override_path {
        if path.is_file() {
            return Resolution::Executable(path);
        }
    }
    let name = if cfg!(windows) {
        "opencode.exe"
    } else {
        "opencode"
    };
    let Some(path) = env::var_os("PATH") else {
        return Resolution::Unavailable;
    };
    for dir in env::split_paths(&path) {
        if !dir.is_absolute() {
            continue;
        }
        let candidate = dir.join(name);
        if candidate.is_file() {
            return Resolution::Executable(candidate);
        }
    }
    Resolution::Unavailable
}
