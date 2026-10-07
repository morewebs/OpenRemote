//! Folder browsing for choosing where a chat runs: directories only, from a
//! starting path or the home folder. The console picks a folder on another
//! machine with it (a typed path is never asked for), and a local browser
//! console without the native dialog uses it too.

use std::path::{Path, PathBuf};

use serde_json::{Value, json};

const MAX_ENTRIES: usize = 2000;

fn home() -> PathBuf {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/"))
}

/// Where browsing can start: the filesystem root, or each drive on Windows.
fn roots() -> Vec<String> {
    if cfg!(windows) {
        (b'A'..=b'Z')
            .map(|d| format!("{}:\\", d as char))
            .filter(|d| Path::new(d).is_dir())
            .collect()
    } else {
        vec!["/".to_string()]
    }
}

/// The subfolders of `path` (default: home), hidden ones left out.
pub fn list(path: Option<&str>) -> Result<Value, String> {
    let dir = match path.filter(|p| !p.trim().is_empty()) {
        Some(p) => PathBuf::from(p),
        None => home(),
    };
    if !dir.is_absolute() {
        return Err("choose a folder from the list".into());
    }
    let read = std::fs::read_dir(&dir).map_err(|e| format!("can't open {}: {e}", dir.display()))?;
    let mut dirs: Vec<(String, String)> = read
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_ok_and(|t| t.is_dir()))
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            (!name.starts_with('.')).then(|| (name, entry.path().to_string_lossy().into_owned()))
        })
        .collect();
    dirs.sort_by_key(|(name, _)| name.to_lowercase());
    let truncated = dirs.len() > MAX_ENTRIES;
    dirs.truncate(MAX_ENTRIES);
    Ok(json!({
        "path": dir.to_string_lossy(),
        "parent": dir.parent().map(|p| p.to_string_lossy().into_owned()),
        "home": home().to_string_lossy(),
        "roots": roots(),
        "dirs": dirs.into_iter().map(|(name, path)| json!({"name": name, "path": path})).collect::<Vec<_>>(),
        "truncated": truncated,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_visible_subfolders_only() {
        let dir = std::env::temp_dir().join(format!("or-fsdirs-{}", uuid::Uuid::new_v4()));
        for sub in ["beta", "Alpha", ".hidden"] {
            std::fs::create_dir_all(dir.join(sub)).unwrap();
        }
        std::fs::write(dir.join("file.txt"), "x").unwrap();
        let listing = list(Some(dir.to_str().unwrap())).unwrap();
        let names: Vec<&str> = listing["dirs"]
            .as_array()
            .unwrap()
            .iter()
            .map(|d| d["name"].as_str().unwrap())
            .collect();
        assert_eq!(names, vec!["Alpha", "beta"]);
        assert!(list(Some("relative/path")).is_err());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
