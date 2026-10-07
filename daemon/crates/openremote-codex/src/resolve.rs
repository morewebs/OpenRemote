//! Codex CLI resolution: PATH first, then the official installer's bin
//! directory (`~/.local/bin/codex`, `%LOCALAPPDATA%\Programs\OpenAI\Codex\bin`),
//! then the npm layout (`@openai/codex` ships `bin/codex.js`, a node
//! wrapper around the platform binary - verified locally: `node
//! bin/codex.js --version` → `codex-cli 0.148.0`).

use std::env;
use std::path::{Path, PathBuf};

use openremote_harness::Resolution;
use openremote_harness::paths::{first_file, home_dir, local_app_data, under, which};

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
    if let Some(exe) = which(name) {
        return Resolution::Executable(exe);
    }
    if let Some(exe) = first_file(installer_candidates(
        cfg!(windows),
        home_dir().as_deref(),
        local_app_data().as_deref(),
    )) {
        return Resolution::Executable(exe);
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
                if let Some(node) = which("node.exe").or_else(|| which("node")) {
                    return Resolution::NodeScript { node, script };
                }
            }
        }
    }
    Resolution::Unavailable
}

/// Where the official installer leaves the visible `codex`: a symlink in
/// `~/.local/bin` on Unix, a junction-backed bin dir under local app data
/// on Windows. A GUI-launched daemon often lacks either on PATH.
fn installer_candidates(windows: bool, home: Option<&Path>, local: Option<&Path>) -> Vec<PathBuf> {
    if windows {
        local
            .map(|l| under(l, &["Programs", "OpenAI", "Codex", "bin", "codex.exe"]))
            .into_iter()
            .collect()
    } else {
        home.map(|h| under(h, &[".local", "bin", "codex"]))
            .into_iter()
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_installer_roots_per_os() {
        let home = Path::new("home");
        let local = Path::new("local");
        assert_eq!(
            installer_candidates(false, Some(home), Some(local)),
            [under(home, &[".local", "bin", "codex"])]
        );
        assert_eq!(
            installer_candidates(true, Some(home), Some(local)),
            [under(
                local,
                &["Programs", "OpenAI", "Codex", "bin", "codex.exe"]
            )]
        );
    }
}
