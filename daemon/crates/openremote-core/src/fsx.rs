//! Files only this user may read: the bearer token, the store's state (it
//! holds webhook keys), and Cloud mode's device keys and credentials.

use std::ffi::OsString;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

/// Writes `bytes` to `path` atomically (temp file, fsync, rename), readable
/// only by this user on Unix. On Windows the profile directory's own
/// permissions already keep other users out.
pub fn write_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let mut tmp: OsString = path.as_os_str().to_owned();
    tmp.push(".tmp");
    let tmp = PathBuf::from(tmp);
    {
        let mut options = fs::OpenOptions::new();
        options.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&tmp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
    }
    // The mode above only applies when the file is created; a temp file
    // left behind by an older build keeps its old mode otherwise.
    restrict(&tmp)?;
    fs::rename(&tmp, path)
}

/// Creates a directory only this user may enter (0700 on Unix).
pub fn create_private_dir(path: &Path) -> std::io::Result<()> {
    fs::create_dir_all(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

/// Narrows an existing file to this user (0600 on Unix).
pub fn restrict(path: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    }
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn private_files_are_owner_only_and_replace_atomically() {
        let dir = std::env::temp_dir().join(format!("or-fsx-{}", uuid::Uuid::new_v4()));
        create_private_dir(&dir).unwrap();
        let file = dir.join("token");
        write_private(&file, b"one").unwrap();
        write_private(&file, b"two").unwrap();
        assert_eq!(fs::read(&file).unwrap(), b"two");
        let mode = |p: &Path| fs::metadata(p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&file), 0o600);
        assert_eq!(mode(&dir), 0o700);
        assert!(!dir.join("token.tmp").exists());
        fs::remove_dir_all(&dir).unwrap();
    }
}
