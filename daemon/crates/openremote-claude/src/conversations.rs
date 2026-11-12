//! Where Claude Code keeps a chat's conversation, and how the conversation
//! follows the chat to another folder.
//!
//! claude files every conversation under the folder it ran in:
//! `<config>/projects/<folder key>/<conversation id>.jsonl`, the key being
//! the folder's path with every character other than an ASCII letter or
//! digit turned into `-` (a key longer than 200 characters is cut there
//! and tagged with a hash of the whole path - claude's own rule, read from
//! its code). `--resume` looks in the current folder's directory, so a
//! chat that moves takes a copy of its conversation into the new folder's
//! directory first. claude's own files are only ever read: the copy is a
//! new file, and a file already in the new folder's directory is never
//! overwritten or extended.

use std::fs;
use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// claude cuts a folder key here and tags the rest with a hash.
const KEY_LIMIT: usize = 200;

/// claude's configuration folder: `CLAUDE_CONFIG_DIR` when set, otherwise
/// `.claude` in the home folder - the same place claude itself reads.
pub fn config_dir() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("CLAUDE_CONFIG_DIR").filter(|d| !d.is_empty()) {
        return Some(PathBuf::from(dir));
    }
    openremote_harness::paths::home_dir().map(|home| home.join(".claude"))
}

/// The directory name claude files a folder's conversations under. claude
/// works on the path as a JavaScript string, so the rule runs over its
/// UTF-16 units: each one that is not an ASCII letter or digit becomes
/// one `-`, and a long key keeps its first 200 characters plus a base-36
/// hash of the whole path.
pub fn folder_key(folder: &str) -> String {
    let units: Vec<u16> = folder.encode_utf16().collect();
    let key: String = units
        .iter()
        .map(|&unit| match u8::try_from(unit) {
            Ok(byte) if byte.is_ascii_alphanumeric() => char::from(byte),
            _ => '-',
        })
        .collect();
    if key.len() <= KEY_LIMIT {
        return key;
    }
    // Java's string hash, as claude computes it: 32-bit, wrapping.
    let mut hash: i32 = 0;
    for &unit in &units {
        hash = (hash << 5).wrapping_sub(hash).wrapping_add(i32::from(unit));
    }
    format!(
        "{}-{}",
        &key[..KEY_LIMIT],
        base36(u64::from(hash.unsigned_abs()))
    )
}

fn base36(mut n: u64) -> String {
    const DIGITS: &[u8; 36] = b"0123456789abcdefghijklmnopqrstuvwxyz";
    let mut out = Vec::new();
    loop {
        out.push(DIGITS[(n % 36) as usize]);
        n /= 36;
        if n == 0 {
            break;
        }
    }
    out.reverse();
    String::from_utf8(out).unwrap_or_default()
}

/// The folder as a running claude sees its own working directory. Windows
/// hands a process its directory as given, minus trailing separators;
/// Linux and macOS report it with links resolved. A folder that names
/// itself two ways (a link) gets both keys, so `--resume` finds the copy
/// whichever one claude settles on.
fn folder_keys(folder: &Path) -> Vec<String> {
    let given: PathBuf = folder.components().collect();
    let mut keys = vec![folder_key(&given.to_string_lossy())];
    if cfg!(unix) {
        if let Ok(resolved) = fs::canonicalize(folder) {
            let key = folder_key(&resolved.to_string_lossy());
            if !keys.contains(&key) {
                keys.push(key);
            }
        }
    }
    keys
}

/// What carrying a conversation did.
#[derive(Debug, PartialEq, Eq)]
pub enum Carried {
    /// A copy now sits in the new folder's directory.
    Copied,
    /// The new folder's directory already held the conversation, whole.
    AlreadyThere,
}

/// Put conversation `id` where claude looks for it when it runs in
/// `folder`: a copy of its newest file, in that folder's directory under
/// `config`. The original stays where claude wrote it. Refused when the
/// file can't be found, or when the new folder's directory already holds
/// a different copy - the caller then starts a fresh conversation rather
/// than touch claude's own files.
pub fn carry(config: &Path, id: &str, folder: &Path) -> Result<Carried, String> {
    // A conversation id is claude's UUID: never a path.
    if id.is_empty()
        || !id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err("claude's conversation id doesn't look like one".into());
    }
    let projects = config.join("projects");
    let name = format!("{id}.jsonl");
    let source = newest(&projects, &name)
        .ok_or_else(|| "claude's conversation for this chat wasn't found".to_string())?;
    let mut carried = Carried::AlreadyThere;
    for key in folder_keys(folder) {
        let dir = projects.join(&key);
        let target = dir.join(&name);
        if target.exists() {
            if same_bytes(&source, &target).map_err(|e| e.to_string())? {
                continue;
            }
            return Err(
                "the new folder already holds a different copy of claude's conversation".into(),
            );
        }
        fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        copy_new(&source, &target).map_err(|e| e.to_string())?;
        carried = Carried::Copied;
    }
    Ok(carried)
}

/// The most recently written copy of `name` in any folder's directory:
/// after an earlier move the conversation went on in the newest one. A
/// copy keeps its source's time on some systems, so a tie goes to the
/// longer file - claude only ever appends to a conversation.
fn newest(projects: &Path, name: &str) -> Option<PathBuf> {
    let mut best: Option<((SystemTime, u64), PathBuf)> = None;
    for entry in fs::read_dir(projects).ok()?.flatten() {
        let candidate = entry.path().join(name);
        let Ok(meta) = fs::metadata(&candidate) else {
            continue;
        };
        if !meta.is_file() {
            continue;
        }
        let rank = (
            meta.modified().unwrap_or(SystemTime::UNIX_EPOCH),
            meta.len(),
        );
        if best.as_ref().is_none_or(|(seen, _)| rank > *seen) {
            best = Some((rank, candidate));
        }
    }
    best.map(|(_, path)| path)
}

/// Whether two files hold the same bytes.
fn same_bytes(a: &Path, b: &Path) -> std::io::Result<bool> {
    if fs::metadata(a)?.len() != fs::metadata(b)?.len() {
        return Ok(false);
    }
    let mut left = BufReader::new(fs::File::open(a)?);
    let mut right = BufReader::new(fs::File::open(b)?);
    let mut x = [0u8; 64 * 1024];
    let mut y = [0u8; 64 * 1024];
    loop {
        let n = left.read(&mut x)?;
        if n == 0 {
            return Ok(true);
        }
        right.read_exact(&mut y[..n])?;
        if x[..n] != y[..n] {
            return Ok(false);
        }
    }
}

/// Copy to a name claude never reads, then rename into place: claude never
/// meets a half-written conversation, and a target that appeared meanwhile
/// is not overwritten.
fn copy_new(source: &Path, target: &Path) -> std::io::Result<()> {
    let partial = target.with_extension("jsonl.openremote-copy");
    fs::copy(source, &partial)?;
    if target.exists() {
        let _ = fs::remove_file(&partial);
        return Err(std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            "the conversation appeared there meanwhile",
        ));
    }
    fs::rename(&partial, target).inspect_err(|_| {
        let _ = fs::remove_file(&partial);
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_folder_key_is_the_path_with_every_other_character_a_dash() {
        assert_eq!(folder_key(r"C:\Users\me\work"), "C--Users-me-work");
        assert_eq!(folder_key("/home/me/my project"), "-home-me-my-project");
        // One dash per UTF-16 unit: an accented letter is one, an emoji two.
        assert_eq!(folder_key("/a/é"), "-a--");
        assert_eq!(folder_key("/a/\u{1F600}"), "-a---");
    }

    #[test]
    fn a_long_folder_key_is_cut_and_tagged_with_claudes_hash() {
        let long = format!("/{}", "a".repeat(250));
        let key = folder_key(&long);
        assert!(key.starts_with(&format!("-{}", "a".repeat(199))));
        // The tags are what claude's own JavaScript computes for these
        // paths (its hash and base-36, run in node).
        let (cut, tag) = key.split_at(KEY_LIMIT);
        assert_eq!(cut.len(), KEY_LIMIT);
        assert_eq!(tag, "-feo44x");
        let wide = format!(r"C:\Users\me\{}", "proj-\u{e9}\u{1F600}".repeat(30));
        let key = folder_key(&wide);
        assert_eq!(key.len(), 207);
        assert!(key.ends_with("-pugx0t"));
        assert_eq!(base36(0), "0");
        assert_eq!(base36(35), "z");
        assert_eq!(base36(36), "10");
    }

    #[test]
    fn a_conversation_is_copied_beside_the_new_folder_and_the_original_stays() {
        let root = std::env::temp_dir().join(format!(
            "or-claude-carry-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or_default()
        ));
        let old = root.join("projects").join("-old-folder");
        fs::create_dir_all(&old).unwrap();
        let id = "0b5f2c1e-1111-2222-3333-444455556666";
        fs::write(old.join(format!("{id}.jsonl")), "{\"a\":1}\n").unwrap();
        let folder = root.join("new folder");
        fs::create_dir_all(&folder).unwrap();

        assert_eq!(carry(&root, id, &folder), Ok(Carried::Copied));
        let keys = folder_keys(&folder);
        let copy = root
            .join("projects")
            .join(&keys[0])
            .join(format!("{id}.jsonl"));
        assert_eq!(fs::read_to_string(&copy).unwrap(), "{\"a\":1}\n");
        assert!(old.join(format!("{id}.jsonl")).is_file());
        // Again: already there, nothing written.
        assert_eq!(carry(&root, id, &folder), Ok(Carried::AlreadyThere));
        // A different copy in the way is never overwritten.
        std::thread::sleep(std::time::Duration::from_millis(20));
        fs::write(old.join(format!("{id}.jsonl")), "{\"a\":1}\n{\"b\":2}\n").unwrap();
        assert!(carry(&root, id, &folder).is_err());
        assert_eq!(fs::read_to_string(&copy).unwrap(), "{\"a\":1}\n");
        // Never a path for an id.
        assert!(carry(&root, "../x", &folder).is_err());
        let _ = fs::remove_dir_all(&root);
    }
}
