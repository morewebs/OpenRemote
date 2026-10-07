//! OpenCode CLI resolution: `opencode` on PATH (native binary in the
//! npm layout - verified locally), then the official installer's root
//! `~/.opencode/bin` (the same root OpenRemote extracts the Windows
//! release into), then npm's Windows package dir.

use std::env;
use std::path::{Path, PathBuf};

use openremote_harness::Resolution;
use openremote_harness::paths::{first_file, home_dir, under, which};

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
    let name = binary_name(cfg!(windows));
    if let Some(exe) = which(name) {
        return Resolution::Executable(exe);
    }
    let appdata = env::var_os("APPDATA").map(PathBuf::from);
    if let Some(exe) = first_file(candidates(
        cfg!(windows),
        home_dir().as_deref(),
        appdata.as_deref(),
    )) {
        return Resolution::Executable(exe);
    }
    Resolution::Unavailable
}

fn binary_name(windows: bool) -> &'static str {
    if windows { "opencode.exe" } else { "opencode" }
}

fn candidates(windows: bool, home: Option<&Path>, appdata: Option<&Path>) -> Vec<PathBuf> {
    let name = binary_name(windows);
    let mut out: Vec<PathBuf> = home
        .map(|h| under(h, &[".opencode", "bin", name]))
        .into_iter()
        .collect();
    if windows {
        // npm's global bin dir only carries sh/cmd shims; the native
        // binary lives inside the package (verified layout:
        // %APPDATA%\npm\node_modules\opencode-ai\bin\opencode.exe).
        if let Some(appdata) = appdata {
            out.push(under(
                appdata,
                &["npm", "node_modules", "opencode-ai", "bin", "opencode.exe"],
            ));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_installer_root_comes_first_on_both() {
        let home = Path::new("home");
        let appdata = Path::new("appdata");
        assert_eq!(
            candidates(false, Some(home), Some(appdata)),
            [under(home, &[".opencode", "bin", "opencode"])]
        );
        let win = candidates(true, Some(home), Some(appdata));
        assert_eq!(win[0], under(home, &[".opencode", "bin", "opencode.exe"]));
        assert_eq!(win.len(), 2);
    }
}
