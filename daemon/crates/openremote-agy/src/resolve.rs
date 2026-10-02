//! Antigravity CLI resolution: PATH first, then the documented install
//! root (`~/.local/bin/agy` / `%LOCALAPPDATA%\agy\bin`).

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
    for name in ["agy.exe", "agy"] {
        if let Some(exe) = which(name) {
            return Resolution::Executable(exe);
        }
    }
    if let Some(local) = env::var_os("LOCALAPPDATA").map(PathBuf::from) {
        let exe = local.join("agy").join("bin").join("agy.exe");
        if exe.is_file() {
            return Resolution::Executable(exe);
        }
    }
    if let Some(home) = home_dir() {
        let exe = home.join(".local").join("bin").join("agy");
        if exe.is_file() {
            return Resolution::Executable(exe);
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
