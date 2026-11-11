//! Pictures that ride a chat - what the user attached to a message, what
//! an agent's tool answered with - kept as files beside the store rather
//! than in the event log: one screenshot outweighs a whole transcript, and
//! every device copies the log. A blob is named by its sha256 and written
//! once. A note beside it holds its type and the chats that keep it; the
//! last of those chats to go takes the picture with it.
//!
//! The daemon hashes and checks the bytes; this module only files them.

use std::collections::BTreeSet;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use crate::ids::SessionId;

/// What the store knows about one picture besides its bytes.
#[derive(Serialize, Deserialize, Default)]
struct Note {
    mime: String,
    owners: BTreeSet<SessionId>,
}

pub struct Blobs {
    dir: PathBuf,
    /// One writer at a time: two chats keeping the same picture at once
    /// must both end up named in its note.
    write: Mutex<()>,
}

/// The file extension a harness that reads pictures from disk expects
/// for a type.
pub fn extension(mime: &str) -> &'static str {
    match mime {
        "image/png" => "png",
        "image/jpeg" => "jpg",
        "image/gif" => "gif",
        "image/webp" => "webp",
        _ => "bin",
    }
}

const EXTENSIONS: &[&str] = &["png", "jpg", "gif", "webp", "bin"];

impl Blobs {
    /// The blob store of the daemon whose data folder is `data_dir`.
    pub fn new(data_dir: &Path) -> Self {
        Self {
            dir: data_dir.join("blobs"),
            write: Mutex::new(()),
        }
    }

    /// Whether `name` can name a blob: a sha256 in lowercase hex. Checked
    /// before any name from a request reaches the filesystem, so a name is
    /// never a path.
    pub fn is_name(name: &str) -> bool {
        name.len() == 64 && name.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
    }

    fn checked(name: &str) -> io::Result<()> {
        if Self::is_name(name) {
            Ok(())
        } else {
            Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "not a blob name",
            ))
        }
    }

    fn note_path(&self, sha: &str) -> PathBuf {
        self.dir.join(format!("{sha}.json"))
    }

    fn note(&self, sha: &str) -> Option<Note> {
        let raw = fs::read(self.note_path(sha)).ok()?;
        serde_json::from_slice(&raw).ok()
    }

    fn save_note(&self, sha: &str, note: &Note) -> io::Result<()> {
        let raw = serde_json::to_vec(note).map_err(io::Error::other)?;
        crate::fsx::write_private(&self.note_path(sha), &raw)
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, ()> {
        self.write.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Keep a picture for a chat. The bytes are written once, under their
    /// hash (`sha`, which the caller computed); a picture already kept just
    /// gains the chat as a holder.
    pub fn put(&self, sha: &str, mime: &str, bytes: &[u8], owner: SessionId) -> io::Result<()> {
        Self::checked(sha)?;
        let _guard = self.lock();
        crate::fsx::create_private_dir(&self.dir)?;
        let path = self.dir.join(sha);
        if !path.is_file() {
            crate::fsx::write_private(&path, bytes)?;
        }
        let mut note = self.note(sha).unwrap_or_default();
        let fresh_type = note.mime.is_empty();
        if fresh_type {
            note.mime = mime.to_string();
        }
        if note.owners.insert(owner) || fresh_type {
            self.save_note(sha, &note)?;
        }
        Ok(())
    }

    /// A picture's type and bytes, if it is kept here.
    pub fn get(&self, sha: &str) -> io::Result<Option<(String, Vec<u8>)>> {
        Self::checked(sha)?;
        let bytes = match fs::read(self.dir.join(sha)) {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e),
        };
        let mime = self
            .note(sha)
            .map(|n| n.mime)
            .filter(|m| !m.is_empty())
            .unwrap_or_else(|| "application/octet-stream".to_string());
        Ok(Some((mime, bytes)))
    }

    /// Whether `owner` keeps this picture - what lets a request about one
    /// chat read only that chat's pictures.
    pub fn held_by(&self, sha: &str, owner: SessionId) -> bool {
        Self::is_name(sha) && self.note(sha).is_some_and(|n| n.owners.contains(&owner))
    }

    /// The picture as a file named with its extension, for a harness that
    /// reads pictures from disk and goes by the name (codex's local
    /// images). A hard link where the disk allows, a copy otherwise; it
    /// goes when the picture does.
    pub fn file(&self, sha: &str, mime: &str) -> io::Result<PathBuf> {
        Self::checked(sha)?;
        let named = self.dir.join(format!("{sha}.{}", extension(mime)));
        if !named.is_file() {
            let _guard = self.lock();
            let source = self.dir.join(sha);
            if !named.is_file() && fs::hard_link(&source, &named).is_err() {
                fs::copy(&source, &named)?;
            }
        }
        Ok(named)
    }

    /// A chat is gone: it lets go of every picture it kept, and the
    /// pictures no other chat keeps go with it. Rare (a chat deleted
    /// everywhere, a device leaving Cloud), so it reads every note rather
    /// than keep a second index that could drift.
    pub fn release(&self, owner: SessionId) -> io::Result<usize> {
        let _guard = self.lock();
        let entries = match fs::read_dir(&self.dir) {
            Ok(entries) => entries,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(0),
            Err(e) => return Err(e),
        };
        let mut removed = 0;
        for entry in entries.flatten() {
            let name = entry.file_name();
            let Some(sha) = name.to_str().and_then(|n| n.strip_suffix(".json")) else {
                continue;
            };
            if !Self::is_name(sha) {
                continue;
            }
            let Some(mut note) = self.note(sha) else {
                continue;
            };
            if !note.owners.remove(&owner) {
                continue;
            }
            if !note.owners.is_empty() {
                self.save_note(sha, &note)?;
                continue;
            }
            // Its last holder is gone: the bytes, the named copies, then
            // the note - the note last, so an interrupted removal is found
            // again by the next release instead of leaving bytes nobody
            // can trace.
            for path in std::iter::once(self.dir.join(sha)).chain(
                EXTENSIONS
                    .iter()
                    .map(|ext| self.dir.join(format!("{sha}.{ext}"))),
            ) {
                fs::remove_file(&path).or_else(|e| match e.kind() {
                    io::ErrorKind::NotFound => Ok(()),
                    _ => Err(e),
                })?;
            }
            fs::remove_file(self.note_path(sha))?;
            removed += 1;
        }
        Ok(removed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tempdir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("or-blobs-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    const SHA: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

    #[test]
    fn a_name_is_a_lowercase_sha256_and_never_a_path() {
        assert!(Blobs::is_name(SHA));
        assert!(!Blobs::is_name(&SHA.to_uppercase()));
        assert!(!Blobs::is_name("../token"));
        assert!(!Blobs::is_name(&SHA[1..]));
        let blobs = Blobs::new(&tempdir());
        assert!(blobs.get("../token").is_err());
        assert!(
            blobs
                .put("../token", "image/png", b"x", SessionId::new())
                .is_err()
        );
    }

    #[test]
    fn a_picture_is_kept_once_and_goes_with_the_last_chat_that_holds_it() {
        let blobs = Blobs::new(&tempdir());
        let (a, b) = (SessionId::new(), SessionId::new());
        blobs.put(SHA, "image/png", b"png bytes", a).unwrap();
        blobs.put(SHA, "image/png", b"png bytes", b).unwrap();
        assert!(blobs.held_by(SHA, a) && blobs.held_by(SHA, b));
        assert!(!blobs.held_by(SHA, SessionId::new()));
        let named = blobs.file(SHA, "image/png").unwrap();
        assert!(named.to_string_lossy().ends_with(".png"));
        assert_eq!(fs::read(&named).unwrap(), b"png bytes");

        assert_eq!(blobs.release(a).unwrap(), 0, "b still holds it");
        assert_eq!(
            blobs.get(SHA).unwrap(),
            Some(("image/png".to_string(), b"png bytes".to_vec()))
        );
        assert_eq!(blobs.release(b).unwrap(), 1);
        assert_eq!(blobs.get(SHA).unwrap(), None);
        assert!(!named.exists(), "the named copy goes with the picture");
    }

    #[test]
    fn a_store_with_no_pictures_releases_nothing() {
        let blobs = Blobs::new(&tempdir());
        assert_eq!(blobs.release(SessionId::new()).unwrap(), 0);
        assert_eq!(blobs.get(SHA).unwrap(), None);
    }
}
