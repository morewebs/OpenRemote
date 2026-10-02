//! Pi CLI resolution: PATH first, then the npm layout (dist/cli.js is a
//! node script — verified locally: `node dist/cli.js --version` → 0.87.1).

use std::env;
use std::path::{Path, PathBuf};

use openremote_harness::Resolution;

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
    let name = if cfg!(windows) { "pi.exe" } else { "pi" };
    if let Some(exe) = which(name) {
        return Resolution::Executable(exe);
    }
    if cfg!(windows) {
        if let Some(appdata) = env::var_os("APPDATA") {
            let script = Path::new(&appdata)
                .join("npm")
                .join("node_modules")
                .join("@earendil-works")
                .join("pi-coding-agent")
                .join("dist")
                .join("cli.js");
            if script.is_file() {
                if let Some(node) = which("node.exe").or_else(|| which("node")) {
                    return Resolution::NodeScript { node, script };
                }
            }
        }
    } else if let Some(home) = home_dir() {
        // POSIX npm layout
        let script =
            home.join(".npm-global/lib/node_modules/@earendil-works/pi-coding-agent/dist/cli.js");
        if script.is_file() {
            if let Some(node) = which("node") {
                return Resolution::NodeScript { node, script };
            }
        }
    }
    Resolution::Unavailable
}

fn which(name: &str) -> Option<PathBuf> {
    let path = env::var_os("PATH")?;
    for dir in env::split_paths(&path) {
        if !dir.is_absolute() {
            continue;
        }
        let candidate = dir.join(name);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

fn home_dir() -> Option<PathBuf> {
    env::var_os("USERPROFILE")
        .or_else(|| env::var_os("HOME"))
        .map(PathBuf::from)
}
