//! Codex CLI resolution: PATH first (native binary on POSIX), then the
//! npm layout (`@openai/codex` ships `bin/codex.js`, a node wrapper around
//! the platform binary - verified locally: `node bin/codex.js --version`
//! → `codex-cli 0.148.0`).

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
    let name = if cfg!(windows) { "codex.exe" } else { "codex" };
    if let Some(dir) = which_dir(name) {
        return Resolution::Executable(dir);
    }
    if cfg!(windows) {
        // npm global layout: %APPDATA%\npm\node_modules\@openai\codex\bin\codex.js
        if let Some(appdata) = env::var_os("APPDATA") {
            let script = Path::new(&appdata)
                .join("npm")
                .join("node_modules")
                .join("@openai")
                .join("codex")
                .join("bin")
                .join("codex.js");
            if script.is_file() {
                if let Some(node) = which_dir("node.exe").or_else(|| which_dir("node")) {
                    return Resolution::NodeScript { node, script };
                }
            }
        }
        return Resolution::Unavailable;
    }
    Resolution::Unavailable
}

fn which_dir(name: &str) -> Option<PathBuf> {
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
