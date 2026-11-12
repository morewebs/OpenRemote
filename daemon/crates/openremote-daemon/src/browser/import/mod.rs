//! Bringing bookmarks and history in from another browser on this computer:
//! Chrome, Edge, Brave, Vivaldi, Opera and Chromium ([`chromium`]), Firefox
//! ([`firefox`]) and Safari ([`safari`]).
//!
//! What may be read is decided here, once, for every reader:
//!
//! - **Where.** A profile is one found in a browser's own folder on this
//!   computer ([`sources`]), or a folder the user picked themselves
//!   ([`source_at`]). No path comes from anywhere else, and the route that
//!   runs an import is the console's alone: it is not a peer route.
//! - **Which files.** Only the names in [`READABLE`], each opened read-only
//!   through a [`Profile`]. A reader cannot name any other file: the files
//!   that hold sign-ins, saved passwords, payment details or site storage are
//!   never opened, and a test reads every reader's source to keep it so.
//! - **How.** A database the browser may hold open is first copied, within a
//!   size cap, into a private scratch folder ([`Scratch`]) and read from
//!   there; the folder is deleted when the import ends, whatever happened.
//!   Nothing in the browser's own folder is written, locked or created.
//! - **What comes out.** Only `http` and `https` addresses, of bounded
//!   length, with titles stripped of control characters and cut short.
//!   `javascript:`, `file:`, `data:` and every other scheme are skipped and
//!   counted. Bookmarks are capped per import below the sync ceiling (see
//!   [`super::store::IMPORT_BOOKMARKS`]); history is capped too, covers the
//!   last [`super::store::HISTORY_DAYS`] days, and never leaves this device.

pub mod chromium;
pub mod firefox;
pub mod plist;
pub mod safari;
pub mod sqlite;

use std::cell::RefCell;
use std::fs::{File, OpenOptions};
use std::io::{self, Read};
use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use super::store::{Bookmark, HISTORY_DAYS, IMPORT_VISITS, Visit};

/// Every file an import may open, by name, in a profile (or, for
/// `profiles.ini`, in Firefox's own folder). A database's write-ahead log
/// (`<name>-wal`) is copied with it.
pub const READABLE: &[&str] = &[
    // Chromium browsers
    "Bookmarks",
    "History",
    // Firefox
    "places.sqlite",
    "profiles.ini",
    // Safari
    "Bookmarks.plist",
    "History.db",
];

/// A bookmarks file or a profile list larger than this is not a browser's.
pub const MAX_TEXT: u64 = 32 << 20;
/// The largest database an import copies. Years of heavy browsing make a
/// history database of a few hundred megabytes at most.
pub const MAX_DATABASE: u64 = 512 << 20;
/// The most bookmark entries a reader collects before the store's own cap
/// is applied (most of a large tree is duplicates and folders).
pub const READ_BOOKMARKS: usize = 10_000;
/// The longest address an import keeps. A bookmark row then stays well
/// under the sync note's row limit.
pub const MAX_URL: usize = 2048;
/// The longest title an import keeps, in characters.
pub const MAX_TITLE: usize = 200;
/// The longest key (a guid) a reader keeps from a row, in characters.
pub const MAX_KEY: usize = 32;

/// The browser family a profile belongs to: it decides the reader.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Chromium,
    Firefox,
    Safari,
}

/// One profile that can be imported from.
#[derive(Clone, Debug, PartialEq)]
pub struct Source {
    /// Stable for this profile: imported rows are keyed by it, so importing
    /// the same profile again adds nothing twice.
    pub id: String,
    /// The browser's name, as people say it.
    pub browser: String,
    /// The profile's name within the browser.
    pub profile: String,
    pub kind: Kind,
    pub dir: PathBuf,
}

impl Source {
    /// What the console lists: no paths, only names and what is there.
    pub fn listing(&self) -> Value {
        let profile = Profile::new(&self.dir);
        let (bookmarks, history) = match self.kind {
            Kind::Chromium => (profile.has("Bookmarks"), profile.has("History")),
            Kind::Firefox => (profile.has("places.sqlite"), profile.has("places.sqlite")),
            Kind::Safari => (profile.has("Bookmarks.plist"), profile.has("History.db")),
        };
        json!({
            "id": self.id,
            "browser": self.browser,
            "profile": self.profile,
            "kind": self.kind,
            "bookmarks": bookmarks,
            "history": history,
        })
    }
}

/// Which halves to bring in.
#[derive(Clone, Copy, Debug)]
pub struct Want {
    pub bookmarks: bool,
    pub history: bool,
}

/// What a reader hands back, before it is checked: an entry's own key (a
/// guid or row id, untrusted), its address and title, and a time in unix
/// milliseconds.
#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    pub key: String,
    pub url: String,
    pub title: String,
    pub at: i64,
}

/// The moment of the import, and how far back history goes.
#[derive(Clone, Copy, Debug)]
pub struct Ctx {
    pub now: i64,
    pub horizon: i64,
}

impl Ctx {
    pub fn at(now: i64) -> Self {
        Self {
            now,
            horizon: now - HISTORY_DAYS * 86_400_000,
        }
    }

    /// Whether a visit at `at` is one to keep: inside the history window,
    /// and not in the future (a day's slack for clocks).
    pub fn keeps(&self, at: i64) -> bool {
        at >= self.horizon && at <= self.now + 86_400_000
    }
}

/// What one import read, checked and ready for the store.
#[derive(Debug, Default)]
pub struct Found {
    pub bookmarks: Vec<Bookmark>,
    pub visits: Vec<Visit>,
    /// Entries left out for their address (`javascript:`, `file:`, `data:`,
    /// too long, not a web page).
    pub refused: usize,
    /// Every file opened, for the tests that pin what is read.
    pub opened: Vec<PathBuf>,
}

/// A browser profile's folder, through which every file is opened: by a
/// name in [`READABLE`] only, read-only, with a size cap.
pub struct Profile {
    dir: PathBuf,
    opened: RefCell<Vec<PathBuf>>,
}

fn readable(name: &str) -> io::Result<()> {
    if READABLE.contains(&name) {
        Ok(())
    } else {
        Err(refused("not a file an import reads"))
    }
}

/// A name a profile file may have: one in [`READABLE`], or its write-ahead
/// log beside it.
fn allowed_name(name: &str) -> bool {
    READABLE.contains(&name.strip_suffix("-wal").unwrap_or(name))
}

fn refused(why: &str) -> io::Error {
    io::Error::new(io::ErrorKind::PermissionDenied, why.to_string())
}

/// Opens a file for reading only (never for writing, never created), and
/// without following a link: on Unix a symbolic link fails to open, and on
/// Windows the link itself is opened, so [`Profile::open`] can look at it.
fn open_no_follow(path: &Path) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        /// FILE_FLAG_OPEN_REPARSE_POINT: open a link, not what it names.
        const OPEN_REPARSE_POINT: u32 = 0x0020_0000;
        options.custom_flags(OPEN_REPARSE_POINT);
    }
    options.open(path)
}

/// Whether an open file is a Windows reparse point that names another file
/// (a symbolic link, a junction, a mount point): its tag's name-surrogate
/// bit. Other reparse points (deduplicated or cloud files) are the file
/// itself.
#[cfg(windows)]
#[allow(unsafe_code)]
fn names_another_file(file: &File) -> io::Result<bool> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_ATTRIBUTE_REPARSE_POINT, FILE_ATTRIBUTE_TAG_INFO, FileAttributeTagInfo,
        GetFileInformationByHandleEx,
    };
    let mut info = FILE_ATTRIBUTE_TAG_INFO {
        FileAttributes: 0,
        ReparseTag: 0,
    };
    // SAFETY: the handle is the open file's own, and `info` is a buffer of
    // exactly the size passed for the class asked for.
    let ok = unsafe {
        GetFileInformationByHandleEx(
            file.as_raw_handle() as _,
            FileAttributeTagInfo,
            (&mut info as *mut FILE_ATTRIBUTE_TAG_INFO).cast(),
            std::mem::size_of::<FILE_ATTRIBUTE_TAG_INFO>() as u32,
        )
    };
    if ok == 0 {
        return Err(io::Error::last_os_error());
    }
    const NAME_SURROGATE: u32 = 0x2000_0000;
    Ok(info.FileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0
        && info.ReparseTag & NAME_SURROGATE != 0)
}

impl Profile {
    pub fn new(dir: &Path) -> Self {
        Self {
            dir: dir.to_path_buf(),
            opened: RefCell::new(Vec::new()),
        }
    }

    /// Whether the profile holds this file (a name in [`READABLE`]) as a
    /// plain file of its own.
    pub fn has(&self, name: &str) -> bool {
        readable(name).is_ok() && self.checked(name).is_ok()
    }

    /// Where a profile file is, once it is known to be what its name says: a
    /// plain file, not a link, whose real place is directly in the real
    /// profile folder under an allowed name. A name alone proves nothing
    /// when `Bookmarks` can be a link to another file.
    fn checked(&self, name: &str) -> io::Result<PathBuf> {
        if !allowed_name(name) {
            return Err(refused("not a file an import reads"));
        }
        let path = self.dir.join(name);
        let meta = std::fs::symlink_metadata(&path)?;
        if meta.file_type().is_symlink() || !meta.is_file() {
            return Err(refused("the file is a link, not a browser's own file"));
        }
        let dir = std::fs::canonicalize(&self.dir)?;
        let real = std::fs::canonicalize(&path)?;
        let name_ok = real
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(allowed_name);
        if real.parent() != Some(dir.as_path()) || !name_ok {
            return Err(refused("the file is not where its name says"));
        }
        Ok(path)
    }

    /// Opens a profile file after [`Self::checked`], and checks the open
    /// file again, so a link put in its place in between is caught too.
    fn open(&self, name: &str) -> io::Result<File> {
        let path = self.checked(name)?;
        // Only Windows opens a second time; elsewhere the first file stands.
        #[allow(unused_mut)]
        let mut file = open_no_follow(&path)?;
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            if names_another_file(&file)? {
                return Err(refused("the file is a link, not a browser's own file"));
            }
            // A reparse point that names no other file (a deduplicated or
            // cloud file) is read through its filter, so it is opened again
            // the ordinary way.
            if file.metadata()?.file_attributes() & 0x400 != 0 {
                file = OpenOptions::new().read(true).open(&path)?;
            }
        }
        if !file.metadata()?.is_file() {
            return Err(refused("the file is a link, not a browser's own file"));
        }
        self.opened.borrow_mut().push(path);
        Ok(file)
    }

    /// A whole small file (bookmarks, a profile list), at most `cap` bytes.
    pub fn read(&self, name: &str, cap: u64) -> io::Result<Vec<u8>> {
        readable(name)?;
        let file = self.open(name)?;
        let mut bytes = Vec::new();
        file.take(cap + 1).read_to_end(&mut bytes)?;
        if bytes.len() as u64 > cap {
            return Err(io::Error::other("the file is larger than a browser's"));
        }
        Ok(bytes)
    }

    /// A database copied into `scratch`, its write-ahead log with it: the
    /// browser may hold the original open and change it while it is read.
    /// Answers the copy's path. A database already copied into this scratch
    /// folder (Firefox's one file, for bookmarks and then history) is read
    /// again from that copy.
    pub fn copy(&self, name: &str, scratch: &Scratch) -> io::Result<PathBuf> {
        readable(name)?;
        let to = scratch.dir.join(name);
        if to.is_file() {
            return Ok(to);
        }
        self.copy_one(name, &to, MAX_DATABASE)?;
        let wal = format!("{name}-wal");
        // Looked at without following it: a log that is a link fails the
        // import in `copy_one` rather than being skipped or followed.
        if std::fs::symlink_metadata(self.dir.join(&wal)).is_ok() {
            self.copy_one(&wal, &scratch.dir.join(&wal), sqlite::MAX_WAL)?;
        }
        Ok(to)
    }

    fn copy_one(&self, name: &str, to: &Path, cap: u64) -> io::Result<()> {
        let source = self.open(name)?;
        let mut out = private_file(to)?;
        let copied = io::copy(&mut source.take(cap + 1), &mut out)?;
        if copied > cap {
            return Err(io::Error::other("the file is larger than a browser's"));
        }
        Ok(())
    }
}

/// Whether a profile folder `profiles.ini` names is one to read: a real
/// folder, not a link or junction to somewhere else, and, when it is named
/// relative to Firefox's own folder (`inside`), really inside it.
pub fn plain_folder(dir: &Path, inside: Option<&Path>) -> bool {
    let Ok(meta) = std::fs::symlink_metadata(dir) else {
        return false;
    };
    if meta.file_type().is_symlink() || !meta.is_dir() {
        return false;
    }
    match inside {
        None => true,
        Some(root) => match (std::fs::canonicalize(dir), std::fs::canonicalize(root)) {
            (Ok(dir), Ok(root)) => dir.starts_with(&root) && dir != root,
            _ => false,
        },
    }
}

/// A new file only this user can read.
fn private_file(path: &Path) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path)
}

/// The import's private folder for the copies it reads, inside the
/// browser's own data folder. Removed, with everything in it, when dropped:
/// at the end of an import, a failed one included.
pub struct Scratch {
    dir: PathBuf,
}

impl Scratch {
    pub fn new(parent: &Path) -> io::Result<Self> {
        let dir = parent.join(format!("import-{}", uuid::Uuid::new_v4().simple()));
        // Only Unix has a mode to set; elsewhere the builder is used as is.
        #[allow(unused_mut)]
        let mut builder = std::fs::DirBuilder::new();
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        builder.create(&dir)?;
        Ok(Self { dir })
    }

    pub fn path(&self) -> &Path {
        &self.dir
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// At most `max` characters of `text`, copied: what a reader keeps of a
/// row's text while it reads, so one oversized row (or the same row named
/// many times by a damaged file) is never copied whole.
pub fn cut(text: &str, max: usize) -> String {
    text.chars().take(max).collect()
}

/// Whether an address is short enough to keep reading: a longer one is
/// left in the file rather than copied out and refused later.
pub fn fits(url: &str) -> bool {
    url.len() <= MAX_URL
}

/// Unicode's format characters (General_Category Cf): bidirectional
/// controls, zero-width characters, the byte-order mark and the like. They
/// draw nothing and can make a title or an address read as something else.
pub fn is_format(c: char) -> bool {
    matches!(
        c as u32,
        0x00AD
            | 0x0600..=0x0605
            | 0x061C
            | 0x06DD
            | 0x070F
            | 0x0890..=0x0891
            | 0x08E2
            | 0x180E
            | 0x200B..=0x200F
            | 0x202A..=0x202E
            | 0x2060..=0x2064
            | 0x2066..=0x206F
            | 0xFEFF
            | 0xFFF9..=0xFFFB
            | 0x110BD
            | 0x110CD
            | 0x13430..=0x1343F
            | 0x1BCA0..=0x1BCA3
            | 0x1D173..=0x1D17A
            | 0xE0001
            | 0xE0020..=0xE007F
    )
}

/// An address an import keeps: `http` or `https` with a host, no
/// whitespace or control characters, at most [`MAX_URL`] bytes. Anything
/// else (`javascript:`, `file:`, `data:`, a browser's own pages) is None.
pub fn web_address(url: &str) -> Option<String> {
    let url = url.trim();
    if url.is_empty() || url.len() > MAX_URL {
        return None;
    }
    if url
        .chars()
        .any(|c| c.is_control() || c.is_whitespace() || is_format(c))
    {
        return None;
    }
    let (scheme, rest) = url.split_once("://")?;
    if !(scheme.eq_ignore_ascii_case("http") || scheme.eq_ignore_ascii_case("https")) {
        return None;
    }
    let host = rest.split(['/', '?', '#']).next().unwrap_or("");
    let host = host.rsplit('@').next().unwrap_or("");
    if host.is_empty() || host.starts_with(':') {
        return None;
    }
    Some(url.to_string())
}

/// A title as it is kept: control characters become spaces, format
/// characters ([`is_format`]) are dropped, runs of space fold to one, and it
/// is cut at [`MAX_TITLE`] characters.
pub fn clean_title(title: &str) -> String {
    let spaced: String = title
        .chars()
        .filter(|c| !is_format(*c))
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    spaced
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(MAX_TITLE)
        .collect()
}

/// A key safe to put in an id: the entry's own when it is plain (a guid, a
/// row number), otherwise a digest of it.
pub fn safe_key(raw: &str) -> String {
    let plain = !raw.is_empty()
        && raw.len() <= 64
        && raw
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_');
    if plain { raw.to_string() } else { digest(raw) }
}

fn digest(text: &str) -> String {
    let hash = Sha256::digest(text.as_bytes());
    hash.iter().take(12).map(|b| format!("{b:02x}")).collect()
}

/// Reads one source: the halves wanted, checked and keyed for the store.
/// Copies are made in a scratch folder inside `scratch_parent`, which is
/// gone when this returns.
pub fn read(source: &Source, want: Want, now: i64, scratch_parent: &Path) -> Result<Found, String> {
    let profile = Profile::new(&source.dir);
    let scratch = Scratch::new(scratch_parent)
        .map_err(|e| format!("The import couldn't make its scratch folder: {e}"))?;
    let ctx = Ctx::at(now);
    let name = source.browser.as_str();
    let (marks, seen) = match source.kind {
        Kind::Chromium => (
            if want.bookmarks && profile.has("Bookmarks") {
                chromium::bookmarks(&profile, &ctx).map_err(|e| said(name, "bookmarks", &e))?
            } else {
                Vec::new()
            },
            if want.history && profile.has("History") {
                chromium::history(&profile, &scratch, &ctx)
                    .map_err(|e| said(name, "history", &e))?
            } else {
                Vec::new()
            },
        ),
        Kind::Firefox => (
            if want.bookmarks {
                firefox::bookmarks(&profile, &scratch, &ctx)
                    .map_err(|e| said(name, "bookmarks", &e))?
            } else {
                Vec::new()
            },
            if want.history {
                firefox::history(&profile, &scratch, &ctx).map_err(|e| said(name, "history", &e))?
            } else {
                Vec::new()
            },
        ),
        Kind::Safari => (
            if want.bookmarks && profile.has("Bookmarks.plist") {
                safari::bookmarks(&profile, &ctx).map_err(|e| said(name, "bookmarks", &e))?
            } else {
                Vec::new()
            },
            if want.history && profile.has("History.db") {
                safari::history(&profile, &scratch, &ctx).map_err(|e| said(name, "history", &e))?
            } else {
                Vec::new()
            },
        ),
    };
    drop(scratch);
    let mut found = Found::default();
    for entry in marks.into_iter().take(READ_BOOKMARKS) {
        let Some(url) = web_address(&entry.url) else {
            found.refused += 1;
            continue;
        };
        found.bookmarks.push(Bookmark {
            id: format!("imp-{}-{}", source.id, safe_key(&entry.key)),
            url,
            title: clean_title(&entry.title),
            folder: "other".into(),
            position: 0.0,
            added: if entry.at > 0 && entry.at <= ctx.now + 86_400_000 {
                entry.at
            } else {
                now
            },
        });
    }
    for entry in seen.into_iter().take(IMPORT_VISITS) {
        let Some(url) = web_address(&entry.url) else {
            found.refused += 1;
            continue;
        };
        found.visits.push(Visit {
            id: format!("imp-{}-v{}", source.id, safe_key(&entry.key)),
            url,
            title: clean_title(&entry.title),
            at: entry.at,
        });
    }
    found.opened = profile.opened.into_inner();
    Ok(found)
}

/// A reader's failure, as the console shows it.
fn said(browser: &str, what: &str, error: &Failure) -> String {
    match error {
        Failure::Io(e) if e.kind() == io::ErrorKind::PermissionDenied => {
            if cfg!(target_os = "macos") {
                format!(
                    "{browser}'s {what} couldn't be opened: macOS keeps them from other apps. Give OpenRemote Full Disk Access in System Settings > Privacy & Security, then try again."
                )
            } else {
                format!(
                    "{browser}'s {what} couldn't be opened: the file is locked or not yours to read. Close {browser} and try again."
                )
            }
        }
        Failure::Io(e) if e.kind() == io::ErrorKind::NotFound => {
            format!("{browser} has no {what} in that profile.")
        }
        Failure::Io(e) => format!("{browser}'s {what} couldn't be read: {e}."),
        Failure::Damaged(why) => format!("{browser}'s {what} couldn't be read: {why}."),
    }
}

/// Why a reader stopped.
#[derive(Debug)]
pub enum Failure {
    Io(io::Error),
    /// The file is there but not what the browser writes.
    Damaged(String),
}

impl From<io::Error> for Failure {
    fn from(e: io::Error) -> Self {
        Failure::Io(e)
    }
}

impl From<sqlite::Error> for Failure {
    fn from(e: sqlite::Error) -> Self {
        match e {
            sqlite::Error::Io(e) => Failure::Io(e),
            other => Failure::Damaged(other.to_string()),
        }
    }
}

/// The newest visits a reader saw, at most `cap` of them, without holding
/// every row of a long history at once.
pub struct Newest {
    cap: usize,
    rows: Vec<(i64, i64, i64)>,
}

impl Newest {
    pub fn new(cap: usize) -> Self {
        Self {
            cap,
            rows: Vec::new(),
        }
    }

    /// One visit: its row id, the page it names, and when.
    pub fn push(&mut self, id: i64, page: i64, at: i64) {
        self.rows.push((id, page, at));
        if self.rows.len() >= self.cap.saturating_mul(2).max(1024) {
            self.trim();
        }
    }

    fn trim(&mut self) {
        self.rows
            .sort_by(|a, b| b.2.cmp(&a.2).then_with(|| b.0.cmp(&a.0)));
        self.rows.truncate(self.cap);
    }

    /// The kept visits, newest first.
    pub fn done(mut self) -> Vec<(i64, i64, i64)> {
        self.trim();
        self.rows
    }
}

// ---- where browsers keep their profiles ----

/// The folders a person's apps keep their data in, from the environment.
#[derive(Clone, Debug, Default)]
pub struct Places {
    /// `%LOCALAPPDATA%` on Windows.
    pub local: Option<PathBuf>,
    /// `%APPDATA%` on Windows.
    pub roaming: Option<PathBuf>,
    /// The home folder.
    pub home: Option<PathBuf>,
}

impl Places {
    pub fn here() -> Self {
        let var = |name: &str| {
            std::env::var_os(name)
                .filter(|v| !v.is_empty())
                .map(PathBuf::from)
        };
        Self {
            local: var("LOCALAPPDATA"),
            roaming: var("APPDATA"),
            home: var("HOME").or_else(|| var("USERPROFILE")),
        }
    }
}

/// A browser this computer may have: its name, the id its imports are keyed
/// by, its family and its data folder.
struct Known {
    browser: &'static str,
    id: &'static str,
    kind: Kind,
    root: PathBuf,
}

/// Where each browser keeps its data on this operating system.
fn known(places: &Places) -> Vec<Known> {
    let mut out = Vec::new();
    let mut add = |browser, id, kind, root: Option<PathBuf>| {
        if let Some(root) = root {
            out.push(Known {
                browser,
                id,
                kind,
                root,
            });
        }
    };
    let local = |p: &str| places.local.as_ref().map(|d| d.join(p));
    let roaming = |p: &str| places.roaming.as_ref().map(|d| d.join(p));
    let home = |p: &str| places.home.as_ref().map(|d| d.join(p));
    if cfg!(windows) {
        add(
            "Chrome",
            "chrome",
            Kind::Chromium,
            local("Google/Chrome/User Data"),
        );
        add(
            "Edge",
            "edge",
            Kind::Chromium,
            local("Microsoft/Edge/User Data"),
        );
        add(
            "Brave",
            "brave",
            Kind::Chromium,
            local("BraveSoftware/Brave-Browser/User Data"),
        );
        add(
            "Vivaldi",
            "vivaldi",
            Kind::Chromium,
            local("Vivaldi/User Data"),
        );
        add(
            "Chromium",
            "chromium",
            Kind::Chromium,
            local("Chromium/User Data"),
        );
        add(
            "Opera",
            "opera",
            Kind::Chromium,
            roaming("Opera Software/Opera Stable"),
        );
        add(
            "Firefox",
            "firefox",
            Kind::Firefox,
            roaming("Mozilla/Firefox"),
        );
    } else if cfg!(target_os = "macos") {
        let support = "Library/Application Support/";
        let at = |p: &str| home(&format!("{support}{p}"));
        add("Chrome", "chrome", Kind::Chromium, at("Google/Chrome"));
        add("Edge", "edge", Kind::Chromium, at("Microsoft Edge"));
        add(
            "Brave",
            "brave",
            Kind::Chromium,
            at("BraveSoftware/Brave-Browser"),
        );
        add("Vivaldi", "vivaldi", Kind::Chromium, at("Vivaldi"));
        add("Chromium", "chromium", Kind::Chromium, at("Chromium"));
        add(
            "Opera",
            "opera",
            Kind::Chromium,
            at("com.operasoftware.Opera"),
        );
        add("Firefox", "firefox", Kind::Firefox, at("Firefox"));
        add("Safari", "safari", Kind::Safari, home("Library/Safari"));
    } else if cfg!(target_os = "linux") {
        add(
            "Chrome",
            "chrome",
            Kind::Chromium,
            home(".config/google-chrome"),
        );
        add(
            "Edge",
            "edge",
            Kind::Chromium,
            home(".config/microsoft-edge"),
        );
        add(
            "Brave",
            "brave",
            Kind::Chromium,
            home(".config/BraveSoftware/Brave-Browser"),
        );
        add(
            "Vivaldi",
            "vivaldi",
            Kind::Chromium,
            home(".config/vivaldi"),
        );
        add(
            "Chromium",
            "chromium",
            Kind::Chromium,
            home(".config/chromium"),
        );
        add("Opera", "opera", Kind::Chromium, home(".config/opera"));
        add(
            "Firefox",
            "firefox",
            Kind::Firefox,
            home(".mozilla/firefox"),
        );
        add(
            "Firefox",
            "firefox-snap",
            Kind::Firefox,
            home("snap/firefox/common/.mozilla/firefox"),
        );
    }
    out
}

/// A part of an id from a folder name: lower case letters, digits and
/// dashes, with a digest when the name has anything else.
fn slug(name: &str) -> String {
    let plain: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    let plain = plain.trim_matches('-').to_string();
    if !plain.is_empty() && plain.len() <= 40 && name.is_ascii() {
        plain
    } else {
        digest(name)
    }
}

/// Every profile the browsers on this computer keep, found in their own
/// folders only.
pub fn sources(places: &Places) -> Vec<Source> {
    let mut out = Vec::new();
    for known in known(places) {
        match known.kind {
            Kind::Chromium => {
                let at = |dir: &Path| {
                    let p = Profile::new(dir);
                    p.has("Bookmarks") || p.has("History")
                };
                // Opera keeps its one profile in the data folder itself.
                if at(&known.root) {
                    out.push(Source {
                        id: known.id.to_string(),
                        browser: known.browser.into(),
                        profile: "Default".into(),
                        kind: Kind::Chromium,
                        dir: known.root.clone(),
                    });
                    continue;
                }
                let Ok(entries) = std::fs::read_dir(&known.root) else {
                    continue;
                };
                let mut names: Vec<String> = entries
                    .flatten()
                    .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
                    .map(|e| e.file_name().to_string_lossy().into_owned())
                    .filter(|n| n == "Default" || n.starts_with("Profile "))
                    .collect();
                names.sort_by_key(|n| (n != "Default", n.len(), n.clone()));
                for name in names {
                    let dir = known.root.join(&name);
                    if at(&dir) {
                        out.push(Source {
                            id: format!("{}-{}", known.id, slug(&name)),
                            browser: known.browser.into(),
                            profile: name,
                            kind: Kind::Chromium,
                            dir,
                        });
                    }
                }
            }
            Kind::Firefox => {
                for (dir_name, shown, dir) in firefox::profiles(&known.root) {
                    if Profile::new(&dir).has("places.sqlite") {
                        out.push(Source {
                            id: format!("{}-{}", known.id, slug(&dir_name)),
                            browser: known.browser.into(),
                            profile: shown,
                            kind: Kind::Firefox,
                            dir,
                        });
                    }
                }
            }
            Kind::Safari => {
                let p = Profile::new(&known.root);
                if p.has("Bookmarks.plist") || p.has("History.db") {
                    out.push(Source {
                        id: known.id.to_string(),
                        browser: known.browser.into(),
                        profile: "Safari".into(),
                        kind: Kind::Safari,
                        dir: known.root.clone(),
                    });
                }
            }
        }
    }
    out
}

/// The profile in a folder the user picked, told by the files in it.
pub fn source_at(path: &Path) -> Result<Source, String> {
    const NOT_ONE: &str = "That folder isn't a browser profile OpenRemote can read. Pick the folder that holds Bookmarks or History (Chrome, Edge, Brave), places.sqlite (Firefox), or Bookmarks.plist (Safari).";
    if !path.is_absolute() {
        return Err(NOT_ONE.into());
    }
    let dir = std::fs::canonicalize(path).map_err(|_| NOT_ONE.to_string())?;
    if !dir.is_dir() {
        return Err(NOT_ONE.into());
    }
    let id = format!("picked-{}", digest(&dir.to_string_lossy()));
    let name = dir
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let p = Profile::new(&dir);
    let source = |browser: &str, kind, dir: PathBuf, profile: String| Source {
        id: id.clone(),
        browser: browser.into(),
        profile,
        kind,
        dir,
    };
    if p.has("places.sqlite") {
        return Ok(source("Firefox", Kind::Firefox, dir, name));
    }
    if p.has("Bookmarks.plist") || p.has("History.db") {
        return Ok(source("Safari", Kind::Safari, dir, name));
    }
    if p.has("Bookmarks") || p.has("History") {
        return Ok(source("A Chromium browser", Kind::Chromium, dir, name));
    }
    // A browser's whole data folder: its first profile.
    let default = dir.join("Default");
    let d = Profile::new(&default);
    if d.has("Bookmarks") || d.has("History") {
        return Ok(source(
            "A Chromium browser",
            Kind::Chromium,
            default,
            "Default".into(),
        ));
    }
    Err(NOT_ONE.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Moments in the fixtures are relative to this one (make.py's NOW).
    pub const NOW: i64 = 1_790_812_800_000;

    pub fn fixtures() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/browser-import")
    }

    /// The files no import may name: sign-ins, saved passwords and their
    /// keys, payment and form data, and every kind of site storage.
    const NEVER: &[&str] = &[
        "Cookies",
        "cookies.sqlite",
        "Cookies.binarycookies",
        "Login Data",
        "logins.json",
        "key4.db",
        "key3.db",
        "Web Data",
        "formhistory.sqlite",
        "Local State",
        "Local Storage",
        "Session Storage",
        "IndexedDB",
        "storage",
        "webappsstore.sqlite",
    ];

    /// The import code a person ships, without its tests.
    fn shipped(source: &str) -> &str {
        source.split("#[cfg(test)]").next().unwrap_or(source)
    }

    #[test]
    fn no_reader_names_a_file_that_holds_sign_ins_or_site_data() {
        let sources = [
            ("mod.rs", include_str!("mod.rs")),
            ("chromium.rs", include_str!("chromium.rs")),
            ("firefox.rs", include_str!("firefox.rs")),
            ("safari.rs", include_str!("safari.rs")),
            ("sqlite.rs", include_str!("sqlite.rs")),
            ("plist.rs", include_str!("plist.rs")),
        ];
        for (file, text) in sources {
            let text = shipped(text);
            for name in NEVER {
                assert!(
                    !text.contains(&format!("\"{name}\"")),
                    "{file} names {name}"
                );
            }
            // Files are opened through a Profile, which checks the name, and
            // never by a path a reader builds itself.
            if file != "mod.rs" && file != "sqlite.rs" {
                for opener in ["File::open", "fs::read", "OpenOptions", "fs::copy"] {
                    assert!(
                        !text.contains(opener),
                        "{file} opens files itself ({opener})"
                    );
                }
            }
        }
        for name in NEVER {
            assert!(!READABLE.contains(name), "{name} is readable");
            let profile = Profile::new(&fixtures().join("chromium/Default"));
            assert!(!profile.has(name));
            assert_eq!(
                profile.read(name, 1).unwrap_err().kind(),
                io::ErrorKind::PermissionDenied
            );
            let parent = tempfile::tempdir().unwrap();
            let scratch = Scratch::new(parent.path()).unwrap();
            assert_eq!(
                profile.copy(name, &scratch).unwrap_err().kind(),
                io::ErrorKind::PermissionDenied
            );
        }
    }

    fn copy_tree(from: &Path, to: &Path) {
        std::fs::create_dir_all(to).unwrap();
        for entry in std::fs::read_dir(from).unwrap().flatten() {
            let target = to.join(entry.file_name());
            if entry.file_type().unwrap().is_dir() {
                copy_tree(&entry.path(), &target);
            } else {
                std::fs::copy(entry.path(), &target).unwrap();
            }
        }
    }

    fn snapshot(dir: &Path) -> Vec<(PathBuf, Vec<u8>, Option<std::time::SystemTime>)> {
        let mut out = Vec::new();
        let mut stack = vec![dir.to_path_buf()];
        while let Some(at) = stack.pop() {
            for entry in std::fs::read_dir(&at).unwrap().flatten() {
                let path = entry.path();
                if path.is_dir() {
                    stack.push(path.clone());
                    out.push((path, Vec::new(), None));
                } else {
                    let modified = entry.metadata().unwrap().modified().ok();
                    out.push((path.clone(), std::fs::read(&path).unwrap(), modified));
                }
            }
        }
        out.sort_by(|a, b| a.0.cmp(&b.0));
        out
    }

    fn set_read_only(dir: &Path, on: bool) {
        for entry in std::fs::read_dir(dir).unwrap().flatten() {
            if entry.file_type().unwrap().is_dir() {
                set_read_only(&entry.path(), on);
            } else {
                let mut perms = entry.metadata().unwrap().permissions();
                #[allow(clippy::permissions_set_readonly_false)]
                perms.set_readonly(on);
                std::fs::set_permissions(entry.path(), perms).unwrap();
            }
        }
    }

    /// Each fixture profile, copied with decoys of every forbidden file
    /// beside its own, and made read-only: the import reads only what it
    /// may, writes nothing in the profile, and leaves no copy behind.
    #[test]
    fn a_profile_is_read_only_and_only_its_bookmarks_and_history_are_opened() {
        for (fixture, kind, dir_in) in [
            ("chromium", Kind::Chromium, "Default"),
            (
                "firefox",
                Kind::Firefox,
                "Profiles/abcd1234.default-release",
            ),
            ("safari", Kind::Safari, ""),
        ] {
            let home = tempfile::tempdir().unwrap();
            let root = home.path().join("profile");
            copy_tree(&fixtures().join(fixture), &root);
            let dir = root.join(dir_in);
            for name in NEVER {
                std::fs::write(dir.join(name), b"decoy: never read").unwrap();
            }
            std::fs::create_dir_all(dir.join("IndexedDB-dir")).unwrap();
            set_read_only(&root, true);
            let before = snapshot(&root);
            let data = home.path().join("data");
            std::fs::create_dir_all(&data).unwrap();
            let source = Source {
                id: fixture.into(),
                browser: fixture.into(),
                profile: "p".into(),
                kind,
                dir: dir.clone(),
            };
            let found = read(
                &source,
                Want {
                    bookmarks: true,
                    history: true,
                },
                NOW,
                &data,
            )
            .unwrap();
            assert!(!found.bookmarks.is_empty(), "{fixture}");
            assert!(!found.visits.is_empty(), "{fixture}");
            for path in &found.opened {
                let name = path.file_name().unwrap().to_string_lossy();
                let base = name.strip_suffix("-wal").unwrap_or(&name);
                assert!(READABLE.contains(&base), "{fixture} opened {name}");
                assert!(
                    path.starts_with(&dir),
                    "{fixture} opened {}",
                    path.display()
                );
            }
            // Nothing in the profile changed, and nothing was added to it
            // (no journal, no lock, no shared-memory file).
            assert_eq!(snapshot(&root), before, "{fixture}");
            // The scratch copies are gone.
            assert_eq!(std::fs::read_dir(&data).unwrap().count(), 0, "{fixture}");
            set_read_only(&root, false);
        }
    }

    /// `link` made a symbolic link to the file `target`, where this system
    /// lets a test make one (Windows needs Developer Mode for it).
    fn file_link(target: &Path, link: &Path) -> bool {
        #[cfg(unix)]
        return std::os::unix::fs::symlink(target, link).is_ok();
        #[cfg(windows)]
        return std::os::windows::fs::symlink_file(target, link).is_ok();
    }

    /// `link` made a link to the folder `target`: a junction on Windows (no
    /// privilege needed), a symbolic link elsewhere.
    pub fn folder_link(target: &Path, link: &Path) -> bool {
        #[cfg(unix)]
        return std::os::unix::fs::symlink(target, link).is_ok();
        #[cfg(windows)]
        return std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            // cmd reads only backslashes as separators.
            .arg(link.to_string_lossy().replace('/', "\\"))
            .arg(target.to_string_lossy().replace('/', "\\"))
            .output()
            .is_ok_and(|o| o.status.success());
    }

    /// A `Bookmarks` that is a link to the profile's `Login Data`, or to a
    /// file outside the profile, is refused, though its name is allowed. The
    /// decoy is a valid bookmarks file, so following the link would import.
    #[test]
    fn a_profile_file_that_is_a_link_is_not_followed() {
        let home = tempfile::tempdir().unwrap();
        let dir = home.path().join("Default");
        std::fs::create_dir_all(&dir).unwrap();
        let decoy = std::fs::read(fixtures().join("chromium/Default/Bookmarks")).unwrap();
        std::fs::write(dir.join("Login Data"), &decoy).unwrap();
        std::fs::write(home.path().join("elsewhere"), &decoy).unwrap();
        let profile = Profile::new(&dir);
        if !file_link(&dir.join("Login Data"), &dir.join("Bookmarks")) {
            eprintln!("this system does not let a test make a file link; skipped");
            return;
        }
        assert!(!profile.has("Bookmarks"));
        assert_eq!(
            profile.read("Bookmarks", MAX_TEXT).unwrap_err().kind(),
            io::ErrorKind::PermissionDenied
        );
        std::fs::remove_file(dir.join("Bookmarks")).unwrap();
        assert!(file_link(
            &home.path().join("elsewhere"),
            &dir.join("History")
        ));
        let parent = tempfile::tempdir().unwrap();
        let scratch = Scratch::new(parent.path()).unwrap();
        assert_eq!(
            profile.copy("History", &scratch).unwrap_err().kind(),
            io::ErrorKind::PermissionDenied
        );
        assert!(profile.opened.borrow().is_empty());
    }

    /// A file of the profile named by a link to a folder (a junction on
    /// Windows) is not a file the profile holds.
    #[test]
    fn a_profile_name_that_is_a_folder_link_is_not_a_file() {
        let home = tempfile::tempdir().unwrap();
        let dir = home.path().join("Default");
        std::fs::create_dir_all(dir.join("target")).unwrap();
        assert!(folder_link(&dir.join("target"), &dir.join("Bookmarks")));
        let profile = Profile::new(&dir);
        assert!(!profile.has("Bookmarks"));
        assert!(profile.read("Bookmarks", MAX_TEXT).is_err());
    }

    #[test]
    fn the_scratch_copy_is_deleted_when_a_read_fails() {
        let home = tempfile::tempdir().unwrap();
        let dir = home.path().join("p");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("History"), b"SQLite format 3\0 but damaged").unwrap();
        let data = home.path().join("data");
        std::fs::create_dir_all(&data).unwrap();
        let source = Source {
            id: "x".into(),
            browser: "Chrome".into(),
            profile: "p".into(),
            kind: Kind::Chromium,
            dir,
        };
        let failed = read(
            &source,
            Want {
                bookmarks: false,
                history: true,
            },
            NOW,
            &data,
        )
        .unwrap_err();
        assert!(
            failed.starts_with("Chrome's history couldn't be read"),
            "{failed}"
        );
        assert_eq!(std::fs::read_dir(&data).unwrap().count(), 0);
    }

    #[test]
    fn a_copy_is_made_in_a_private_folder_and_holds_the_log_too() {
        let parent = tempfile::tempdir().unwrap();
        let scratch = Scratch::new(parent.path()).unwrap();
        let profile = Profile::new(&fixtures().join("firefox/Profiles/abcd1234.default-release"));
        let copy = profile.copy("places.sqlite", &scratch).unwrap();
        assert!(copy.starts_with(scratch.path()));
        assert!(scratch.path().join("places.sqlite-wal").is_file());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(scratch.path())
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o700);
            let mode = std::fs::metadata(&copy).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
        let at = scratch.path().to_path_buf();
        drop(scratch);
        assert!(!at.exists());
    }

    #[test]
    fn only_web_addresses_are_kept() {
        for url in [
            "javascript:alert(1)",
            "JavaScript:alert(1)",
            " javascript:alert(1)",
            "file:///C:/notes.txt",
            "data:text/html,<b>hi</b>",
            "chrome://settings",
            "about:blank",
            "place:sort=8",
            "vbscript:x",
            "https://",
            "https:///path",
            "https://a b.example/",
            "https://example.com/\nx",
            "http://user@:80/",
            "ftp://example.com/",
        ] {
            assert_eq!(web_address(url), None, "{url}");
        }
        assert_eq!(
            web_address("https://example.com/a?b#c").as_deref(),
            Some("https://example.com/a?b#c")
        );
        assert_eq!(
            web_address("HTTP://Example.com").as_deref(),
            Some("HTTP://Example.com")
        );
        let long = format!("https://example.com/{}", "a".repeat(MAX_URL));
        assert_eq!(web_address(&long), None);
    }

    /// Only the console runs an import. A chat's MCP server asks the daemon
    /// for its own fixed routes (the chat's tool call and its mentions), and
    /// the browser tools an agent has are a fixed list that never reaches
    /// the import: no tool names it, and neither does the code that runs
    /// them. (A program running as this user could read the browsers' files
    /// itself; the route gives an agent nothing it does not have.)
    #[test]
    fn no_agent_tool_or_mcp_server_reaches_the_import() {
        for (file, text) in [
            ("mcp_chat.rs", include_str!("../../mcp_chat.rs")),
            ("browser/tools.rs", include_str!("../tools.rs")),
            ("browser/mod.rs", include_str!("../mod.rs")),
        ] {
            assert!(
                !text.contains("/browser/import"),
                "{file} names the import route"
            );
            assert!(!text.contains("import::"), "{file} calls the importer");
        }
        for tool in super::super::tools::TOOLS {
            assert!(!tool.name.contains("import"), "{} is a tool", tool.name);
        }
    }

    /// Bidirectional controls, zero-width characters and the byte-order
    /// mark draw nothing and can make a title or an address read as another:
    /// dropped from titles, and an address holding one is not kept.
    #[test]
    fn format_characters_leave_titles_and_refuse_addresses() {
        assert_eq!(
            clean_title("\u{202E}gpj.exe\u{202C} \u{200B}Bank\u{FEFF}\u{2066}"),
            "gpj.exe Bank"
        );
        for url in [
            "https://exa\u{200B}mple.com/",
            "https://example.com/\u{202E}fdp",
            "\u{FEFF}https://example.com/",
            "https://example.com/\u{2066}",
            "https://example.com/\u{E0041}",
        ] {
            assert_eq!(web_address(url), None, "{url:?}");
        }
        assert!(is_format('\u{00AD}'));
        assert!(!is_format('a'));
        assert!(!is_format('\u{00E9}'));
    }

    #[test]
    fn titles_are_cleaned_and_cut() {
        assert_eq!(clean_title("Tabs\tand\nnewlines\u{0}"), "Tabs and newlines");
        assert_eq!(clean_title(&"é".repeat(500)).chars().count(), MAX_TITLE);
    }

    #[test]
    fn a_key_from_the_file_is_kept_only_when_plain() {
        assert_eq!(safe_key("0b5c1c0e-0000-4000"), "0b5c1c0e-0000-4000");
        let odd = safe_key("../../not-a-guid");
        assert_eq!(odd.len(), 24);
        assert!(odd.bytes().all(|b| b.is_ascii_hexdigit()));
        assert_ne!(safe_key(""), "");
    }

    #[test]
    fn an_imported_row_fits_the_sync_notes_row_limit() {
        let mark = Bookmark {
            id: format!("imp-{}-{}", "x".repeat(64), "y".repeat(64)),
            url: format!("https://example.com/{}", "a".repeat(MAX_URL - 20)),
            title: "\u{10FFFF}".repeat(MAX_TITLE),
            folder: "other".into(),
            position: 1e9,
            added: i64::MAX,
        };
        let row = serde_json::to_string(&mark).unwrap();
        assert!(
            row.len() < super::super::sync::MAX_ROW_BYTES,
            "{}",
            row.len()
        );
    }

    #[test]
    fn browsers_are_found_in_their_own_folders() {
        let home = tempfile::tempdir().unwrap();
        let places = Places {
            local: Some(home.path().join("local")),
            roaming: Some(home.path().join("roaming")),
            home: Some(home.path().join("home")),
        };
        let found = known(&places);
        let chrome = found.iter().find(|k| k.id == "chrome").unwrap();
        let firefox = found.iter().find(|k| k.id == "firefox").unwrap();
        copy_tree(&fixtures().join("chromium"), &chrome.root);
        // A second profile, and a folder that is not one.
        copy_tree(
            &fixtures().join("chromium/Default"),
            &chrome.root.join("Profile 2"),
        );
        std::fs::create_dir_all(chrome.root.join("System Profile")).unwrap();
        std::fs::create_dir_all(chrome.root.join("Profile 3")).unwrap();
        copy_tree(&fixtures().join("firefox"), &firefox.root);
        let mut listed = sources(&places);
        listed.sort_by(|a, b| a.id.cmp(&b.id));
        let ids: Vec<&str> = listed.iter().map(|s| s.id.as_str()).collect();
        assert_eq!(
            ids,
            [
                "chrome-default",
                "chrome-profile-2",
                "firefox-abcd1234-default-release"
            ]
        );
        let ff = listed.iter().find(|s| s.kind == Kind::Firefox).unwrap();
        assert_eq!(ff.profile, "default-release");
        assert_eq!(ff.listing()["bookmarks"], true);
        // A listing names no path.
        assert!(
            !ff.listing()
                .to_string()
                .contains("abcd1234.default-release\"")
        );
        assert!(ff.listing().get("dir").is_none());
    }

    #[test]
    fn a_picked_folder_is_told_by_its_files() {
        let fx = fixtures();
        let firefox = source_at(&fx.join("firefox/Profiles/abcd1234.default-release")).unwrap();
        assert_eq!(firefox.kind, Kind::Firefox);
        assert_eq!(source_at(&fx.join("safari")).unwrap().kind, Kind::Safari);
        let chrome = source_at(&fx.join("chromium")).unwrap();
        assert_eq!(chrome.kind, Kind::Chromium);
        assert!(chrome.dir.ends_with("Default"));
        assert!(chrome.id.starts_with("picked-"));
        // The same folder is the same source, so a second import adds nothing.
        assert_eq!(source_at(&fx.join("chromium")).unwrap().id, chrome.id);
        assert!(source_at(&fx).is_err());
        assert!(source_at(Path::new("relative/path")).is_err());
        assert!(source_at(&fx.join("make.py")).is_err());
        assert!(source_at(&fx.join("nowhere")).is_err());
    }

    #[test]
    fn newest_keeps_the_latest_visits_only() {
        let mut newest = Newest::new(3);
        for n in 0..5000 {
            newest.push(n, n % 7, n * 10);
        }
        let kept = newest.done();
        assert_eq!(
            kept.iter().map(|r| r.0).collect::<Vec<_>>(),
            [4999, 4998, 4997]
        );
    }
}
