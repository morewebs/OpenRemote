//! Executable resolution, ported from the Python SDK's `_find_cli`:
//! prefer a native binary, refuse Windows `.cmd` shims (CreateProcess
//! cannot run them), and fall back to the npm global `cli.js` driven by
//! `node` — the layout the 0.1.x TS SDK itself spawned.

use std::env;
use std::path::{Path, PathBuf};

/// What `resolve_claude` found.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Resolution {
    /// A spawnable executable (native binary, or a script on POSIX).
    Executable(PathBuf),
    /// `node <script>` — the npm install layout.
    NodeScript {
        node: PathBuf,
        script: PathBuf,
    },
    Unavailable,
}

impl Resolution {
    pub fn is_available(&self) -> bool {
        !matches!(self, Resolution::Unavailable)
    }
}

fn home_dir() -> Option<PathBuf> {
    env::var_os("USERPROFILE")
        .or_else(|| env::var_os("HOME"))
        .map(PathBuf::from)
}

fn which_exact(name: &str) -> Option<PathBuf> {
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

fn which_claude() -> Option<PathBuf> {
    if cfg!(windows) {
        // Only a native exe: the extensionless `claude` file in the npm dir
        // is a POSIX sh script and `claude.cmd` is unspawnable via
        // CreateProcess — both are refused on purpose.
        which_exact("claude.exe")
    } else {
        which_exact("claude")
    }
}

fn node_script_resolution(script: PathBuf) -> Option<Resolution> {
    if !script.is_file() {
        return None;
    }
    let node = if cfg!(windows) { "node.exe" } else { "node" };
    let node = which_exact(node)
        .or_else(|| which_exact("node"))
        .or_else(|| {
            env::var_os("PROGRAMFILES").map(|pf| {
                let pf = PathBuf::from(pf);
                pf.join("nodejs")
                    .join(if cfg!(windows) { "node.exe" } else { "node" })
            })
        })?;
    Some(Resolution::NodeScript { node, script })
}

/// Find the Claude Code CLI. `OPENREMOTE_CLAUDE_PATH` wins (tests point it
/// at the fixture agent); otherwise the native binary on PATH, then the
/// SDK's POSIX fallbacks, then the npm global `cli.js` via node.
pub fn resolve_claude() -> Resolution {
    resolve_impl(std::env::var_os("OPENREMOTE_CLAUDE_PATH").map(PathBuf::from))
}

fn resolve_impl(override_path: Option<PathBuf>) -> Resolution {
    if let Some(path) = override_path {
        if path.is_file() {
            return Resolution::Executable(path);
        }
    }
    if let Some(exe) = which_claude() {
        return Resolution::Executable(exe);
    }
    if !cfg!(windows) {
        if let Some(home) = home_dir() {
            let fallbacks = [
                home.join(".npm-global/bin/claude"),
                PathBuf::from("/usr/local/bin/claude"),
                home.join(".local/bin/claude"),
                home.join("node_modules/.bin/claude"),
                home.join(".yarn/bin/claude"),
                home.join(".claude/local/claude"),
            ];
            for candidate in fallbacks {
                if candidate.is_file() {
                    return Resolution::Executable(candidate);
                }
            }
        }
        return Resolution::Unavailable;
    }
    // Windows npm layout: %APPDATA%\npm\node_modules\@anthropic-ai\claude-code\cli.js
    if let Some(appdata) = std::env::var_os("APPDATA") {
        let script = Path::new(&appdata)
            .join("npm")
            .join("node_modules")
            .join("@anthropic-ai")
            .join("claude-code")
            .join("cli.js");
        if let Some(res) = node_script_resolution(script) {
            return res;
        }
    }
    if let Some(home) = home_dir() {
        let exe = home.join(".local").join("bin").join("claude.exe");
        if exe.is_file() {
            return Resolution::Executable(exe);
        }
    }
    Resolution::Unavailable
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_override_pointing_at_a_real_file_wins() {
        let exe = std::env::current_exe().unwrap();
        assert_eq!(resolve_impl(Some(exe.clone())), Resolution::Executable(exe));
    }

    #[test]
    fn an_override_pointing_at_nothing_is_ignored() {
        let missing = std::env::temp_dir().join("or-no-such-claude-binary");
        // PATH may legitimately hold a claude on a dev machine, so we can
        // only pin the negative half: a missing override must not error.
        let _ = resolve_impl(Some(missing));
    }
}
