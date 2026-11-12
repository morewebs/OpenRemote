//! Finding Worktrunk's `wt` program, and running it.
//!
//! The plugin gets a cleared environment, so it never runs a bare `wt`: it
//! looks the program up itself and runs it by its absolute path. Windows
//! has a `wt.exe` of its own (Windows Terminal's alias, in `WindowsApps`),
//! which is never taken for Worktrunk; there Worktrunk is `git-wt.exe`.
//!
//! Nothing about a Worktrunk version is built in. What a version can do is
//! read from its own `--help`, once per version, so a newer `wt` with new
//! flags works the day it ships, and an older one is never handed a flag
//! it doesn't know. The answer is kept until the program file changes
//! (an update), then read again.

use std::collections::{HashMap, HashSet};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc::channel;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};

/// A Worktrunk version, `major.minor.patch`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Version(pub u64, pub u64, pub u64);

impl std::fmt::Display for Version {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}.{}", self.0, self.1, self.2)
    }
}

impl Version {
    /// The first `x.y.z` in `text` (a leading `v` allowed), as `wt
    /// --version` prints it (`wt v0.81.0`) or a tag names it (`v0.81.0`).
    pub fn find(text: &str) -> Option<Version> {
        let bytes = text.as_bytes();
        let mut i = 0;
        while i < bytes.len() {
            if bytes[i].is_ascii_digit()
                && (i == 0 || !bytes[i - 1].is_ascii_alphanumeric() || bytes[i - 1] == b'v')
            {
                if let Some(v) = Self::parse_at(&text[i..]) {
                    return Some(v);
                }
            }
            i += 1;
        }
        None
    }

    fn parse_at(text: &str) -> Option<Version> {
        let mut parts = [0u64; 3];
        let mut rest = text;
        for (n, part) in parts.iter_mut().enumerate() {
            let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
            if digits == 0 || digits > 9 {
                return None;
            }
            *part = rest[..digits].parse().ok()?;
            rest = &rest[digits..];
            if n < 2 {
                rest = rest.strip_prefix('.')?;
            }
        }
        Some(Version(parts[0], parts[1], parts[2]))
    }

    /// A tag that is exactly a release (`v1.2.3`), not a pre-release.
    pub fn release_tag(tag: &str) -> Option<Version> {
        let rest = tag.strip_prefix('v').unwrap_or(tag);
        let v = Self::parse_at(rest)?;
        (rest == v.to_string()).then_some(v)
    }
}

/// Where the program came from, which decides how it is updated.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Origin {
    /// The path the user typed in the plugin's settings.
    Setting,
    /// The copy this plugin installed into its own data folder.
    Managed,
    /// Found on PATH or in a usual install folder.
    Found,
}

#[derive(Clone, Debug)]
pub struct Located {
    pub path: PathBuf,
    pub version: Option<Version>,
    pub origin: Origin,
}

/// One run of a program.
#[derive(Clone, Debug, Default)]
pub struct Ran {
    pub code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    pub timed_out: bool,
}

impl Ran {
    pub fn ok(&self) -> bool {
        self.code == Some(0) && !self.timed_out
    }

    /// What the run said, for a person or an agent: its messages (Worktrunk
    /// writes them on stderr) and anything on stdout that isn't JSON.
    pub fn said(&self) -> String {
        let mut out = strip_ansi(self.stderr.trim()).to_string();
        let stdout = strip_ansi(self.stdout.trim());
        if !stdout.is_empty() && serde_json::from_str::<serde_json::Value>(&stdout).is_err() {
            if !out.is_empty() {
                out.push('\n');
            }
            out.push_str(&stdout);
        }
        if self.timed_out {
            out.push_str("\n(stopped: it ran past its time limit)");
        }
        out
    }
}

/// Is this the Windows Terminal alias (or anything else from WindowsApps)?
pub fn is_windows_apps(path: &Path) -> bool {
    let lower = path.to_string_lossy().to_lowercase().replace('/', "\\");
    lower.contains("\\windowsapps\\")
}

fn exe(name: &str) -> String {
    if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.to_string()
    }
}

/// The names Worktrunk goes by, in the order they are tried. On Windows the
/// binary is `git-wt` (winget installs only that name) and a bare `wt` is
/// usually Windows Terminal.
pub fn program_names() -> Vec<String> {
    if cfg!(windows) {
        vec![exe("git-wt"), exe("wt")]
    } else {
        vec![exe("wt"), exe("git-wt")]
    }
}

/// Folders Worktrunk's installers put it in, for when PATH is thin (the
/// daemon may have been started without the user's shell profile).
pub fn usual_dirs(env: &dyn Fn(&str) -> Option<String>) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    let home = env("HOME").or_else(|| env("USERPROFILE"));
    if let Some(cargo) = env("CARGO_HOME") {
        dirs.push(PathBuf::from(cargo).join("bin"));
    }
    if let Some(home) = &home {
        dirs.push(PathBuf::from(home).join(".cargo").join("bin"));
        dirs.push(PathBuf::from(home).join(".local").join("bin"));
    }
    if cfg!(windows) {
        if let Some(local) = env("LOCALAPPDATA") {
            dirs.push(
                PathBuf::from(&local)
                    .join("Microsoft")
                    .join("WinGet")
                    .join("Links"),
            );
        }
    } else {
        for dir in [
            "/opt/homebrew/bin",
            "/usr/local/bin",
            "/home/linuxbrew/.linuxbrew/bin",
            "/usr/bin",
        ] {
            dirs.push(PathBuf::from(dir));
        }
    }
    dirs
}

/// Candidate programs, best first: every folder on PATH, then the usual
/// install folders, each tried with each name. WindowsApps is never one.
pub fn candidates(env: &dyn Fn(&str) -> Option<String>) -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = env("PATH")
        .map(|p| std::env::split_paths(&p).collect())
        .unwrap_or_default();
    dirs.extend(usual_dirs(env));
    let mut out = Vec::new();
    for name in program_names() {
        for dir in &dirs {
            if !dir.is_absolute() {
                continue;
            }
            let path = dir.join(&name);
            if !is_windows_apps(&path) && !out.contains(&path) {
                out.push(path);
            }
        }
    }
    out
}

/// A program by its name, looked up on PATH and then in the usual folders,
/// as an absolute path (used for `git`, `curl` and `tar`).
pub fn find_program(name: &str, env: &dyn Fn(&str) -> Option<String>) -> Option<PathBuf> {
    let mut dirs: Vec<PathBuf> = Vec::new();
    // Windows' own tar and curl come first: the GNU tar Git for Windows
    // puts on PATH reads `C:` as a remote host and can't open a zip.
    if cfg!(windows) && matches!(name, "tar" | "curl") {
        if let Some(root) = env("SystemRoot") {
            dirs.push(PathBuf::from(root).join("System32"));
        }
    }
    dirs.extend(
        env("PATH")
            .map(|p| std::env::split_paths(&p).collect::<Vec<_>>())
            .unwrap_or_default(),
    );
    if cfg!(windows) {
        if let Some(root) = env("SystemRoot") {
            dirs.push(PathBuf::from(root).join("System32"));
        }
        for var in ["ProgramFiles", "ProgramW6432"] {
            if let Some(pf) = env(var) {
                dirs.push(PathBuf::from(pf).join("Git").join("cmd"));
            }
        }
    }
    dirs.extend(usual_dirs(env));
    dirs.into_iter()
        .filter(|d| d.is_absolute())
        .map(|d| d.join(exe(name)))
        .find(|p| p.is_file() && !is_windows_apps(p))
}

/// Run a program by its absolute path: no shell ever sits between the
/// arguments and the program, stdin is closed (so a prompt fails at once
/// instead of waiting forever), colors are off, and the run is stopped at
/// `limit`.
pub fn run(
    program: &Path,
    args: &[String],
    cwd: Option<&Path>,
    limit: Duration,
) -> std::io::Result<Ran> {
    let mut cmd = Command::new(program);
    cmd.args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env("NO_COLOR", "1")
        .env("CLICOLOR", "0")
        .env_remove("CLICOLOR_FORCE");
    if let Some(cwd) = cwd {
        cmd.current_dir(cwd);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // No console window flashing up for each run.
        cmd.creation_flags(0x0800_0000);
    }
    let mut child = cmd.spawn()?;
    let (done_tx, done_rx) = channel::<()>();
    let collect = |pipe: Option<Box<dyn Read + Send>>| {
        let buf = Arc::new(Mutex::new(Vec::new()));
        if let Some(mut pipe) = pipe {
            let into = buf.clone();
            let done = done_tx.clone();
            std::thread::spawn(move || {
                let mut chunk = [0u8; 8192];
                loop {
                    match pipe.read(&mut chunk) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => into
                            .lock()
                            .unwrap_or_else(|p| p.into_inner())
                            .extend_from_slice(&chunk[..n]),
                    }
                }
                let _ = done.send(());
            });
        } else {
            let _ = done_tx.send(());
        }
        buf
    };
    let out = collect(
        child
            .stdout
            .take()
            .map(|p| Box::new(p) as Box<dyn Read + Send>),
    );
    let err = collect(
        child
            .stderr
            .take()
            .map(|p| Box::new(p) as Box<dyn Read + Send>),
    );
    drop(done_tx);

    let start = Instant::now();
    let mut ran = Ran::default();
    loop {
        if let Some(status) = child.try_wait()? {
            ran.code = status.code();
            break;
        }
        if start.elapsed() >= limit {
            let _ = child.kill();
            let _ = child.wait();
            ran.timed_out = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(15));
    }
    // Worktrunk starts background work (post-start hooks, a background
    // removal) that may inherit the pipes: give the readers a moment, never
    // wait on a grandchild.
    for _ in 0..2 {
        if done_rx.recv_timeout(Duration::from_secs(2)).is_err() {
            break;
        }
    }
    ran.stdout =
        String::from_utf8_lossy(&out.lock().unwrap_or_else(|p| p.into_inner())).into_owned();
    ran.stderr =
        String::from_utf8_lossy(&err.lock().unwrap_or_else(|p| p.into_inner())).into_owned();
    Ok(ran)
}

/// Text without terminal color and cursor codes.
pub fn strip_ansi(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            match chars.peek() {
                Some('[') => {
                    chars.next();
                    for c in chars.by_ref() {
                        if ('@'..='~').contains(&c) {
                            break;
                        }
                    }
                }
                Some(']') => {
                    // OSC (hyperlinks): up to BEL or ESC \.
                    chars.next();
                    while let Some(c) = chars.next() {
                        if c == '\u{7}' {
                            break;
                        }
                        if c == '\u{1b}' {
                            chars.next();
                            break;
                        }
                    }
                }
                _ => {
                    chars.next();
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// Every long flag a `--help` text names (`--no-cd`, `--format`).
pub fn long_flags(help: &str) -> HashSet<String> {
    let mut out = HashSet::new();
    let text = strip_ansi(help);
    let bytes = text.as_bytes();
    let mut i = 0;
    while i + 2 < bytes.len() {
        if bytes[i] == b'-'
            && bytes[i + 1] == b'-'
            && (i == 0 || !bytes[i - 1].is_ascii_alphanumeric() && bytes[i - 1] != b'-')
        {
            let start = i + 2;
            let mut end = start;
            while end < bytes.len() && (bytes[end].is_ascii_alphanumeric() || bytes[end] == b'-') {
                end += 1;
            }
            if end > start && bytes[start].is_ascii_alphabetic() {
                out.insert(format!("--{}", &text[start..end]));
            }
            i = end;
        } else {
            i += 1;
        }
    }
    out
}

/// The subcommands a `--help` text lists under `Commands:`.
pub fn subcommands(help: &str) -> Vec<String> {
    let text = strip_ansi(help);
    let mut out = Vec::new();
    let mut inside = false;
    for line in text.lines() {
        if line.trim_end().ends_with("Commands:") && !line.starts_with(' ') {
            inside = true;
            continue;
        }
        if inside {
            if line.trim().is_empty() || !line.starts_with(' ') {
                if !out.is_empty() {
                    break;
                }
                continue;
            }
            if let Some(word) = line.split_whitespace().next() {
                if word.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') && word != "help" {
                    out.push(word.to_string());
                }
            }
        }
    }
    out
}

/// Finds Worktrunk and remembers what each version can do.
pub struct Wt {
    found: Mutex<Option<(String, Located, Option<SystemTime>)>>,
    flags: Mutex<HashMap<String, HashSet<String>>>,
}

pub const PROBE_TIME: Duration = Duration::from_secs(20);

fn modified(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}

impl Default for Wt {
    fn default() -> Self {
        Self::new()
    }
}

impl Wt {
    pub fn new() -> Self {
        Self {
            found: Mutex::new(None),
            flags: Mutex::new(HashMap::new()),
        }
    }

    /// Forget what was found: the next need looks again (after a setting
    /// changed, or an install or update).
    pub fn forget(&self) {
        *self.found.lock().unwrap_or_else(|p| p.into_inner()) = None;
        self.flags.lock().unwrap_or_else(|p| p.into_inner()).clear();
    }

    /// Whether `path` is a Worktrunk program, and its version.
    pub fn probe(path: &Path) -> Result<Option<Version>, String> {
        if is_windows_apps(path) {
            return Err(format!(
                "{} is Windows' own app alias (Windows Terminal), not Worktrunk",
                path.display()
            ));
        }
        if !path.is_file() {
            return Err(format!("{} is not a file", path.display()));
        }
        let ran = run(path, &["--version".into()], None, PROBE_TIME)
            .map_err(|e| format!("{} could not run: {e}", path.display()))?;
        let said = format!("{}\n{}", ran.stdout, ran.stderr);
        let first = said
            .trim()
            .lines()
            .next()
            .unwrap_or("")
            .trim()
            .to_lowercase();
        let looks =
            first.starts_with("wt ") || first.starts_with("git-wt ") || first.contains("worktrunk");
        if !ran.ok() || !looks {
            return Err(format!(
                "{} is not Worktrunk (it says: {})",
                path.display(),
                first
            ));
        }
        Ok(Version::find(&first))
    }

    /// Find Worktrunk: the user's setting, else the plugin's own copy, else
    /// PATH and the usual folders. `key` names the inputs (the setting and
    /// the plugin's copy) so a change to either looks again.
    pub fn locate(
        &self,
        setting: Option<&str>,
        managed: Option<PathBuf>,
        env: &dyn Fn(&str) -> Option<String>,
    ) -> Result<Located, String> {
        let key = format!("{setting:?}|{managed:?}");
        {
            let found = self.found.lock().unwrap_or_else(|p| p.into_inner());
            if let Some((k, located, stamp)) = found.as_ref() {
                if *k == key && modified(&located.path) == *stamp && located.path.is_file() {
                    return Ok(located.clone());
                }
            }
        }
        let located = Self::look(setting, managed, env)?;
        let stamp = modified(&located.path);
        let mut found = self.found.lock().unwrap_or_else(|p| p.into_inner());
        if found.as_ref().map(|(_, l, s)| (l.path.clone(), *s))
            != Some((located.path.clone(), stamp))
        {
            // A new program, or the same one updated: what it can do is
            // read again.
            self.flags.lock().unwrap_or_else(|p| p.into_inner()).clear();
        }
        *found = Some((key, located.clone(), stamp));
        Ok(located)
    }

    fn look(
        setting: Option<&str>,
        managed: Option<PathBuf>,
        env: &dyn Fn(&str) -> Option<String>,
    ) -> Result<Located, String> {
        if let Some(raw) = setting.map(str::trim).filter(|s| !s.is_empty()) {
            let path = PathBuf::from(raw);
            if !path.is_absolute() {
                return Err(format!(
                    "The Worktrunk program setting must be a full path (it is {raw}). Change it under Settings > Plugins > Worktrunk, or empty it to find Worktrunk on its own."
                ));
            }
            let version = Self::probe(&path)?;
            return Ok(Located {
                path,
                version,
                origin: Origin::Setting,
            });
        }
        if let Some(path) = managed.filter(|p| p.is_file()) {
            if let Ok(version) = Self::probe(&path) {
                return Ok(Located {
                    path,
                    version,
                    origin: Origin::Managed,
                });
            }
        }
        let mut refused = Vec::new();
        for path in candidates(env) {
            if !path.is_file() {
                continue;
            }
            match Self::probe(&path) {
                Ok(version) => {
                    return Ok(Located {
                        path,
                        version,
                        origin: Origin::Found,
                    })
                }
                Err(why) => refused.push(why),
            }
        }
        let mut msg = String::from(
            "Worktrunk is not installed on this computer, or OpenRemote can't find it. Open the Worktrunk view (Plugins in the sidebar) to install it, or set its full path under Settings > Plugins > Worktrunk.",
        );
        if !refused.is_empty() {
            msg.push_str(" Passed over: ");
            msg.push_str(&refused.join("; "));
        }
        Err(msg)
    }

    /// The long flags `wt <command...>` takes in this version, read from its
    /// own help once per version.
    pub fn flags(&self, located: &Located, command: &[&str]) -> HashSet<String> {
        let key = format!("{}|{}", located.path.display(), command.join(" "));
        if let Some(found) = self
            .flags
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .get(&key)
        {
            return found.clone();
        }
        let mut args: Vec<String> = command.iter().map(|s| s.to_string()).collect();
        args.push("--help".into());
        let found = run(&located.path, &args, None, PROBE_TIME)
            .map(|ran| long_flags(&format!("{}\n{}", ran.stdout, ran.stderr)))
            .unwrap_or_default();
        if !found.is_empty() {
            self.flags
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .insert(key, found.clone());
        }
        found
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_read_from_what_wt_and_tags_say() {
        assert_eq!(Version::find("wt v0.81.0"), Some(Version(0, 81, 0)));
        assert_eq!(Version::find("git-wt 1.2.3\n"), Some(Version(1, 2, 3)));
        assert_eq!(
            Version::find("worktrunk 10.0.12-beta"),
            Some(Version(10, 0, 12))
        );
        assert_eq!(Version::find("no version here 1.2"), None);
        assert_eq!(Version::release_tag("v0.81.0"), Some(Version(0, 81, 0)));
        assert_eq!(Version::release_tag("v0.81.0-rc.1"), None);
        assert_eq!(Version::release_tag("nightly"), None);
        assert!(
            Version(0, 81, 0) < Version(0, 81, 1)
                && Version(0, 81, 9) < Version(0, 82, 0)
                && Version(0, 99, 0) < Version(1, 0, 0)
        );
    }

    #[test]
    fn windows_terminal_is_never_worktrunk() {
        assert!(is_windows_apps(Path::new(
            r"C:\Users\me\AppData\Local\Microsoft\WindowsApps\wt.exe"
        )));
        assert!(is_windows_apps(Path::new(
            "C:/Users/me/AppData/Local/Microsoft/WindowsApps/wt.exe"
        )));
        assert!(!is_windows_apps(Path::new(
            r"C:\Users\me\.cargo\bin\wt.exe"
        )));
        let env = |k: &str| match k {
            "PATH" => Some(if cfg!(windows) {
                r"C:\Users\me\AppData\Local\Microsoft\WindowsApps;C:\tools".to_string()
            } else {
                "/usr/local/bin:relative/dir".to_string()
            }),
            _ => None,
        };
        let found = candidates(&env);
        assert!(found.iter().all(|p| !is_windows_apps(p) && p.is_absolute()));
        assert!(!found.is_empty());
        if cfg!(windows) {
            assert!(
                found[0].ends_with("git-wt.exe"),
                "git-wt first on Windows: {found:?}"
            );
        }
    }

    #[test]
    fn flags_and_commands_come_from_the_help_text() {
        let help = "Usage: wt switch [OPTIONS] [BRANCH]\n\nOptions:\n  -c, --create\n      --no-cd   Skip\n      --format <FORMAT>\n  -x, --execute <EXECUTE>\n\x1b[1m      --base\x1b[0m <BASE>\nnot-a--flag\n";
        let flags = long_flags(help);
        for f in ["--create", "--no-cd", "--format", "--execute", "--base"] {
            assert!(flags.contains(f), "{f} in {flags:?}");
        }
        assert!(!flags.contains("--flag"));
        let top = "wt - x\n\nUsage: wt [OPTIONS] [COMMAND]\n\nCommands:\n  switch  Switch\n  list    List\n  newcmd  Something new\n  help    Print this\n\nOptions:\n  -h, --help\n";
        assert_eq!(subcommands(top), vec!["switch", "list", "newcmd"]);
    }

    #[test]
    fn windows_own_tar_beats_one_on_path() {
        let dir = std::env::temp_dir().join(format!("wtp-tar-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(exe("tar")), b"").unwrap();
        std::fs::write(dir.join(exe("git")), b"").unwrap();
        let root = std::env::var("SystemRoot").ok();
        let path = dir.to_string_lossy().into_owned();
        let env = |k: &str| match k {
            "PATH" => Some(path.clone()),
            "SystemRoot" => root.clone(),
            _ => None,
        };
        let tar = find_program("tar", &env).unwrap();
        let system_tar = root
            .as_ref()
            .map(|r| PathBuf::from(r).join("System32").join("tar.exe"));
        match system_tar.filter(|t| cfg!(windows) && t.is_file()) {
            Some(t) => assert_eq!(tar, t),
            None => assert_eq!(tar, dir.join(exe("tar"))),
        }
        // Anything else is taken from PATH first.
        assert_eq!(find_program("git", &env).unwrap(), dir.join(exe("git")));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn ansi_codes_go() {
        assert_eq!(strip_ansi("main  \u{1b}[2m^\u{1b}[22m"), "main  ^");
        assert_eq!(
            strip_ansi("\u{1b}]8;;http://x\u{7}link\u{1b}]8;;\u{7}"),
            "link"
        );
    }
}
