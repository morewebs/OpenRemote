//! Pi CLI resolution: PATH first, then the official installer's managed
//! launcher (`~/.pi/agent/bin`), then the npm layouts. Pi is a node
//! program; on Windows its launcher is `pi-launcher.js` (beside the
//! `pi.cmd` shim CreateProcess can't run), driven by node from PATH or
//! the standalone Node pi's installer keeps in `%LOCALAPPDATA%\pi-node`.

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
    let home = home_dir();
    let local = local_app_data();
    if cfg!(windows) {
        if let Some(exe) = which("pi.exe") {
            return Resolution::Executable(exe);
        }
        let appdata = env::var_os("APPDATA").map(PathBuf::from);
        let script = first_file(windows_scripts(home.as_deref(), appdata.as_deref()));
        if let Some(script) = script {
            if let Some(node) = windows_node(local.as_deref()) {
                return Resolution::NodeScript { node, script };
            }
        }
        return Resolution::Unavailable;
    }
    if let Some(exe) = which("pi") {
        return Resolution::Executable(exe);
    }
    if let Some(exe) = first_file(unix_launchers(home.as_deref())) {
        return Resolution::Executable(exe);
    }
    // A bare npm global install under a user prefix: the package's bin
    // script, driven by node.
    if let Some(script) = first_file(unix_npm_scripts(home.as_deref())) {
        if let Some(node) = which("node") {
            return Resolution::NodeScript { node, script };
        }
    }
    Resolution::Unavailable
}

/// The managed launcher pi's installer writes (a shell script that finds
/// its own Node), and the user bin dir it symlinks into.
fn unix_launchers(home: Option<&Path>) -> Vec<PathBuf> {
    let Some(home) = home else {
        return Vec::new();
    };
    vec![
        under(home, &[".pi", "agent", "bin", "pi"]),
        under(home, &[".local", "bin", "pi"]),
    ]
}

fn unix_npm_scripts(home: Option<&Path>) -> Vec<PathBuf> {
    let Some(home) = home else {
        return Vec::new();
    };
    let package = under(
        home,
        &[
            ".npm-global",
            "lib",
            "node_modules",
            "@earendil-works",
            "pi-coding-agent",
        ],
    );
    npm_entries(&package)
}

/// The scripts node runs on Windows: the managed launcher first, then
/// npm's global layout.
fn windows_scripts(home: Option<&Path>, appdata: Option<&Path>) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Some(home) = home {
        out.push(under(home, &[".pi", "agent", "bin", "pi-launcher.js"]));
    }
    if let Some(appdata) = appdata {
        let package = under(
            appdata,
            &["npm", "node_modules", "@earendil-works", "pi-coding-agent"],
        );
        out.extend(npm_entries(&package));
    }
    out
}

/// The package's bin script: `dist/bundle/cli.js` since 1.0 (npm's `bin`
/// field), `dist/cli.js` before it.
fn npm_entries(package: &Path) -> Vec<PathBuf> {
    vec![
        under(package, &["dist", "bundle", "cli.js"]),
        under(package, &["dist", "cli.js"]),
    ]
}

/// Node for pi on Windows: PATH first, then the standalone Node pi's
/// installer extracts flat into `%LOCALAPPDATA%\pi-node\current`.
fn windows_node(local: Option<&Path>) -> Option<PathBuf> {
    which("node.exe").or_else(|| {
        local
            .map(|l| under(l, &["pi-node", "current", "node.exe"]))
            .filter(|p| p.is_file())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_managed_launcher_comes_before_npm() {
        let home = Path::new("home");
        assert_eq!(
            unix_launchers(Some(home))[0],
            under(home, &[".pi", "agent", "bin", "pi"])
        );
        let scripts = windows_scripts(Some(home), Some(Path::new("appdata")));
        assert_eq!(
            scripts[0],
            under(home, &[".pi", "agent", "bin", "pi-launcher.js"])
        );
        assert!(scripts[1].ends_with(Path::new("dist").join("bundle").join("cli.js")));
        assert!(scripts[2].ends_with(Path::new("dist").join("cli.js")));
    }

    #[test]
    fn no_home_means_no_candidates() {
        assert!(unix_launchers(None).is_empty());
        assert!(unix_npm_scripts(None).is_empty());
        assert!(windows_scripts(None, None).is_empty());
    }
}
