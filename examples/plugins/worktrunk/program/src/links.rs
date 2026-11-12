//! Whether a folder holds a link anywhere inside it. The plugin's own copy
//! of Worktrunk must be plain files, as a plugin's folder must be.

use std::path::Path;

pub fn has_links(dir: &Path) -> bool {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return false;
    };
    for entry in entries.flatten() {
        let Ok(meta) = std::fs::symlink_metadata(entry.path()) else {
            return true;
        };
        if meta.file_type().is_symlink() {
            return true;
        }
        if meta.is_dir() && has_links(&entry.path()) {
            return true;
        }
    }
    false
}
