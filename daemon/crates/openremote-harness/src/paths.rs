//! The filesystem lookups every resolver shares: PATH, the home
//! directory, and Windows' local app data. Each harness crate keeps its
//! own list of where its owner's installer puts the CLI; these are only
//! the primitives that list is built from.

use std::env;
use std::path::{Path, PathBuf};

/// The first file named `name` on PATH. Relative PATH entries are
/// skipped - they resolve against whatever directory the daemon happens
/// to run in, which is never where a harness lives.
pub fn which(name: &str) -> Option<PathBuf> {
    let path = env::var_os("PATH")?;
    env::split_paths(&path)
        .filter(|dir| dir.is_absolute())
        .map(|dir| dir.join(name))
        .find(|candidate| candidate.is_file())
}

/// The user's home: `USERPROFILE` on Windows, `HOME` elsewhere (either is
/// honored on both, the way the owners' installers read them).
pub fn home_dir() -> Option<PathBuf> {
    env::var_os("USERPROFILE")
        .or_else(|| env::var_os("HOME"))
        .map(PathBuf::from)
}

/// `%LOCALAPPDATA%` - where several owners' Windows installers land.
pub fn local_app_data() -> Option<PathBuf> {
    env::var_os("LOCALAPPDATA").map(PathBuf::from)
}

/// The first candidate that exists as a file.
pub fn first_file(candidates: impl IntoIterator<Item = PathBuf>) -> Option<PathBuf> {
    candidates.into_iter().find(|candidate| candidate.is_file())
}

/// `base` joined with each path segment - keeps candidate lists readable
/// without hard-coding a separator.
pub fn under(base: &Path, segments: &[&str]) -> PathBuf {
    segments.iter().fold(base.to_path_buf(), |p, s| p.join(s))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_file_skips_what_does_not_exist() {
        let exe = std::env::current_exe().unwrap();
        let missing = std::env::temp_dir().join("or-paths-no-such-file");
        assert_eq!(first_file([missing, exe.clone()]), Some(exe));
    }

    #[test]
    fn under_joins_segments_in_order() {
        let base = Path::new("root");
        assert_eq!(
            under(base, &[".grok", "bin", "grok"]),
            Path::new("root").join(".grok").join("bin").join("grok")
        );
    }
}
