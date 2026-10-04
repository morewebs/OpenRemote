//! OpenCode CLI resolution: `opencode` on PATH (native binary in the
//! npm layout - verified locally).

use std::env;
use std::path::PathBuf;

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
    let name = if cfg!(windows) {
        "opencode.exe"
    } else {
        "opencode"
    };
    if let Some(path) = env::var_os("PATH") {
        for dir in env::split_paths(&path) {
            if !dir.is_absolute() {
                continue;
            }
            let candidate = dir.join(name);
            if candidate.is_file() {
                return Resolution::Executable(candidate);
            }
        }
    }
    if cfg!(windows) {
        // npm's global bin dir only carries sh/cmd shims; the native
        // binary lives inside the package (verified layout:
        // %APPDATA%\npm\node_modules\opencode-ai\bin\opencode.exe).
        if let Some(appdata) = env::var_os("APPDATA") {
            let exe = std::path::Path::new(&appdata)
                .join("npm")
                .join("node_modules")
                .join("opencode-ai")
                .join("bin")
                .join("opencode.exe");
            if exe.is_file() {
                return Resolution::Executable(exe);
            }
        }
    }
    Resolution::Unavailable
}
