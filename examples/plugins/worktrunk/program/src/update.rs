//! Keeping Worktrunk current: what the newest release is, how the copy on
//! this computer was installed, and installing or updating it.
//!
//! - The newest release is read from the project's own git tags (`git
//!   ls-remote`), so no web API, token or rate limit is involved, and it is
//!   asked at most every few hours.
//! - A copy a package manager installed (winget, Homebrew, cargo) is
//!   updated by that manager, so the two never disagree about what is
//!   installed. A manager that needs a password or a prompt (pacman, a
//!   system package) is named, not run.
//! - Otherwise the plugin keeps a copy of its own: the release's archive
//!   for this system, downloaded over https from the project's GitHub
//!   releases, checked against the release's published SHA-256, and
//!   unpacked into the plugin's private data folder. Updating it puts the
//!   new version beside the old one and moves a pointer, so a failed
//!   update leaves the working copy alone.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use crate::wt::{self, Located, Origin, Version};

pub const REPO: &str = "https://github.com/max-sixty/worktrunk";
pub const CHECK_EVERY: Duration = Duration::from_secs(6 * 60 * 60);
const NET_TIME: Duration = Duration::from_secs(60);
const DOWNLOAD_TIME: Duration = Duration::from_secs(600);
const BUILD_TIME: Duration = Duration::from_secs(1800);

/// The newest release among `git ls-remote --tags` lines.
pub fn newest_tag(ls_remote: &str) -> Option<Version> {
    ls_remote
        .lines()
        .filter_map(|line| line.split_whitespace().nth(1))
        .filter_map(|r| r.strip_prefix("refs/tags/"))
        .filter_map(Version::release_tag)
        .max()
}

/// The release archive for this system, as the project names them.
pub fn asset(os: &str, arch: &str) -> Option<&'static str> {
    Some(match (os, arch) {
        ("windows", "x86_64") => "worktrunk-x86_64-pc-windows-msvc.zip",
        ("macos", "aarch64") => "worktrunk-aarch64-apple-darwin.tar.xz",
        ("macos", "x86_64") => "worktrunk-x86_64-apple-darwin.tar.xz",
        ("linux", "aarch64") => "worktrunk-aarch64-unknown-linux-musl.tar.xz",
        ("linux", "x86_64") => "worktrunk-x86_64-unknown-linux-musl.tar.xz",
        _ => return None,
    })
}

pub fn this_asset() -> Option<&'static str> {
    asset(std::env::consts::OS, std::env::consts::ARCH)
}

/// How a copy of Worktrunk was installed, from where it lives.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Method {
    /// The plugin's own copy, in its data folder.
    Plugin,
    Winget,
    Homebrew,
    Cargo,
    /// cargo-dist's install script, which also uses ~/.cargo/bin.
    Script,
    Pacman,
    Conda,
    Pixi,
    /// A path the user set, or a place we don't recognise.
    Unknown,
}

impl Method {
    pub fn name(self) -> &'static str {
        match self {
            Method::Plugin => "OpenRemote (this plugin's own copy)",
            Method::Winget => "winget",
            Method::Homebrew => "Homebrew",
            Method::Cargo => "cargo install",
            Method::Script => "the Worktrunk install script",
            Method::Pacman => "pacman",
            Method::Conda => "conda",
            Method::Pixi => "pixi",
            Method::Unknown => "an unknown installer",
        }
    }
}

/// `crates_toml`: the text of ~/.cargo/.crates.toml, which lists what
/// `cargo install` put in ~/.cargo/bin.
pub fn method_for(located: &Located, crates_toml: Option<&str>) -> Method {
    if located.origin == Origin::Managed {
        return Method::Plugin;
    }
    let p = located
        .path
        .to_string_lossy()
        .replace('\\', "/")
        .to_lowercase();
    if p.contains("/winget/") {
        Method::Winget
    } else if p.contains("/cellar/") || p.contains("/homebrew/") || p.contains("/linuxbrew/") {
        Method::Homebrew
    } else if p.contains("/.pixi/") {
        Method::Pixi
    } else if p.contains("conda") || p.contains("/miniforge") || p.contains("/mambaforge") {
        Method::Conda
    } else if p.contains("/.cargo/bin/") {
        if crates_toml.is_some_and(|t| {
            t.lines()
                .any(|l| l.trim_start_matches('"').starts_with("worktrunk "))
        }) {
            Method::Cargo
        } else {
            Method::Script
        }
    } else if p.starts_with("/usr/bin/") {
        Method::Pacman
    } else {
        Method::Unknown
    }
}

/// The update, as a program and its arguments this plugin may run on the
/// user's click (`runs`), and as the words a person would type.
pub struct Plan {
    pub program: Option<&'static str>,
    pub args: Vec<&'static str>,
    pub shown: String,
}

pub fn plan(method: Method) -> Plan {
    let run = |program, args: &[&'static str], shown: &str| Plan {
        program: Some(program),
        args: args.to_vec(),
        shown: shown.into(),
    };
    let say = |shown: &str| Plan {
        program: None,
        args: vec![],
        shown: shown.into(),
    };
    match method {
        Method::Plugin => say("Update in the Worktrunk view"),
        Method::Winget => run(
            "winget",
            &[
                "upgrade",
                "--id",
                "max-sixty.worktrunk",
                "--exact",
                "--disable-interactivity",
            ],
            "winget upgrade --id max-sixty.worktrunk --exact",
        ),
        Method::Homebrew => run("brew", &["upgrade", "worktrunk"], "brew upgrade worktrunk"),
        Method::Cargo => run(
            "cargo",
            &["install", "worktrunk", "--locked"],
            "cargo install worktrunk --locked",
        ),
        Method::Script => say(if cfg!(windows) {
            "powershell -c \"irm https://github.com/max-sixty/worktrunk/releases/latest/download/worktrunk-installer.ps1 | iex\""
        } else {
            "curl --proto '=https' --tlsv1.2 -LsSf https://github.com/max-sixty/worktrunk/releases/latest/download/worktrunk-installer.sh | sh"
        }),
        Method::Pacman => say("sudo pacman -Syu worktrunk"),
        Method::Conda => say("conda update -c conda-forge worktrunk"),
        Method::Pixi => say("pixi global update worktrunk"),
        Method::Unknown => {
            say("Update Worktrunk the way you installed it (see https://worktrunk.dev)")
        }
    }
}

/// Ways to install Worktrunk, for a computer without it.
pub fn install_choices() -> Vec<String> {
    if cfg!(windows) {
        vec![
            "winget install max-sixty.worktrunk".into(),
            "cargo install worktrunk --locked".into(),
        ]
    } else if cfg!(target_os = "macos") {
        vec![
            "brew install worktrunk".into(),
            "cargo install worktrunk --locked".into(),
        ]
    } else {
        vec![
            "brew install worktrunk".into(),
            "cargo install worktrunk --locked".into(),
            "sudo pacman -S worktrunk (Arch)".into(),
        ]
    }
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// The newest release: from the cache when it is fresh, else asked again.
/// `force` asks again whatever the cache says.
pub fn latest(data: &Path, git: Option<&Path>, force: bool) -> Result<(Version, u64), String> {
    let cache = data.join("update-check.json");
    if !force {
        if let Some((v, at)) = std::fs::read_to_string(&cache)
            .ok()
            .and_then(|t| serde_json::from_str::<Value>(&t).ok())
            .and_then(|c| {
                Some((
                    Version::release_tag(c["latest"].as_str()?)?,
                    c["checked_at"].as_u64()?,
                ))
            })
        {
            if now().saturating_sub(at) < CHECK_EVERY.as_secs() {
                return Ok((v, at));
            }
        }
    }
    let git = git.ok_or(
        "git is not installed or not on PATH, so new Worktrunk releases can't be looked up",
    )?;
    let ran = wt::run(
        git,
        &[
            "ls-remote".into(),
            "--tags".into(),
            "--refs".into(),
            format!("{REPO}.git"),
        ],
        None,
        NET_TIME,
    )
    .map_err(|e| format!("git could not run: {e}"))?;
    if !ran.ok() {
        return Err(format!(
            "Worktrunk's releases could not be read: {}",
            ran.said()
        ));
    }
    let v = newest_tag(&ran.stdout).ok_or("Worktrunk's repository lists no releases")?;
    let at = now();
    let _ = std::fs::create_dir_all(data);
    let _ = std::fs::write(
        &cache,
        json!({"latest": format!("v{v}"), "checked_at": at}).to_string(),
    );
    Ok((v, at))
}

/// Where the plugin's own copy lives, and the program in it now.
pub fn managed_root(data: &Path) -> PathBuf {
    data.join("worktrunk")
}

pub fn managed_program(data: &Path) -> Option<PathBuf> {
    let root = managed_root(data);
    let current = std::fs::read_to_string(root.join("current")).ok()?;
    let dir = root.join(current.trim());
    find_in(&dir, 3)
}

/// Worktrunk's program in an unpacked release (at its top or in a folder).
fn find_in(dir: &Path, depth: u32) -> Option<PathBuf> {
    for name in wt::program_names() {
        let p = dir.join(&name);
        if p.is_file() {
            return Some(p);
        }
    }
    if depth == 0 {
        return None;
    }
    let mut subdirs: Vec<PathBuf> = std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    subdirs.sort();
    subdirs.into_iter().find_map(|d| find_in(&d, depth - 1))
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// The hash a `.sha256` file names (`<hex> *name` or `<hex>  name`).
pub fn published_hash(text: &str) -> Option<String> {
    let first = text.split_whitespace().next()?.to_lowercase();
    (first.len() == 64 && first.bytes().all(|b| b.is_ascii_hexdigit())).then_some(first)
}

/// Install release `version` as the plugin's own copy. `say` hears each
/// step. Answers the new program's path.
pub fn install_release(
    data: &Path,
    version: Version,
    curl: &Path,
    tar: &Path,
    say: &mut dyn FnMut(&str),
) -> Result<PathBuf, String> {
    let asset = this_asset().ok_or_else(|| {
        format!(
            "Worktrunk publishes no ready-made program for {} on {}: install it with cargo (cargo install worktrunk --locked)",
            std::env::consts::ARCH,
            std::env::consts::OS
        )
    })?;
    let root = managed_root(data);
    let tag = format!("v{version}");
    let work = root.join(format!(".download-{tag}"));
    let _ = std::fs::remove_dir_all(&work);
    std::fs::create_dir_all(&work).map_err(|e| format!("{}: {e}", work.display()))?;
    let url = format!("{REPO}/releases/download/{tag}/{asset}");
    let archive = work.join(asset);
    let fetch = |url: &str, to: &Path, limit: Duration| -> Result<(), String> {
        let ran = wt::run(
            curl,
            &[
                "--fail".into(),
                "--silent".into(),
                "--show-error".into(),
                "--location".into(),
                "--proto".into(),
                "=https".into(),
                "--proto-redir".into(),
                "=https".into(),
                "--output".into(),
                to.to_string_lossy().into_owned(),
                url.into(),
            ],
            None,
            limit,
        )
        .map_err(|e| format!("curl could not run: {e}"))?;
        if ran.ok() {
            Ok(())
        } else {
            Err(format!("download of {url} failed: {}", ran.said()))
        }
    };
    say(&format!("Downloading {asset} ({tag})"));
    fetch(&format!("{url}.sha256"), &work.join("sum"), NET_TIME)?;
    let expected = std::fs::read_to_string(work.join("sum"))
        .ok()
        .and_then(|t| published_hash(&t))
        .ok_or("the release's checksum file could not be read")?;
    fetch(&url, &archive, DOWNLOAD_TIME)?;
    let bytes = std::fs::read(&archive).map_err(|e| e.to_string())?;
    let got = sha256_hex(&bytes);
    if got != expected {
        let _ = std::fs::remove_dir_all(&work);
        return Err(format!(
            "the download's SHA-256 is {got}, the release says {expected}: nothing was installed"
        ));
    }
    say("Checksum matches; unpacking");
    let unpacked = work.join("unpacked");
    std::fs::create_dir_all(&unpacked).map_err(|e| e.to_string())?;
    let ran = wt::run(
        tar,
        &[
            "-xf".into(),
            archive.to_string_lossy().into_owned(),
            "-C".into(),
            unpacked.to_string_lossy().into_owned(),
        ],
        None,
        NET_TIME,
    )
    .map_err(|e| format!("tar could not run: {e}"))?;
    if !ran.ok() {
        return Err(format!("the archive could not be unpacked: {}", ran.said()));
    }
    if crate::links::has_links(&unpacked) {
        let _ = std::fs::remove_dir_all(&work);
        return Err("the archive holds links; nothing was installed".into());
    }
    let program = find_in(&unpacked, 3).ok_or("the archive holds no wt program")?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755));
    }
    let probed = wt::Wt::probe(&program)?;
    if probed != Some(version) {
        return Err(format!(
            "the unpacked program says it is {probed:?}, not {version}"
        ));
    }
    let dest = root.join(&tag);
    let _ = std::fs::remove_dir_all(&dest);
    std::fs::rename(&unpacked, &dest).map_err(|e| format!("{}: {e}", dest.display()))?;
    std::fs::write(root.join("current"), &tag).map_err(|e| e.to_string())?;
    let _ = std::fs::remove_dir_all(&work);
    // Older copies go once the new one is in place; a running one (on
    // Windows) stays until the next update.
    if let Ok(entries) = std::fs::read_dir(&root) {
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if name.starts_with('v') && name != tag && e.path().is_dir() {
                let _ = std::fs::remove_dir_all(e.path());
            }
        }
    }
    say(&format!("Installed Worktrunk {version}"));
    find_in(&dest, 3).ok_or_else(|| "the installed copy went missing".into())
}

/// Run a package manager's update, by its absolute path. Builds from
/// source (cargo) get a long time limit.
pub fn run_manager(program: &Path, args: &[&str]) -> Result<String, String> {
    let limit = if program.to_string_lossy().contains("cargo") {
        BUILD_TIME
    } else {
        DOWNLOAD_TIME
    };
    let ran = wt::run(
        program,
        &args.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
        None,
        limit,
    )
    .map_err(|e| format!("{} could not run: {e}", program.display()))?;
    if ran.ok() {
        Ok(ran.said())
    } else {
        Err(ran.said())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn located(path: &str, origin: Origin) -> Located {
        Located {
            path: PathBuf::from(path),
            version: None,
            origin,
        }
    }

    #[test]
    fn the_newest_release_tag_wins_and_prereleases_dont_count() {
        let out = "aaa\trefs/tags/v0.9.0\nbbb\trefs/tags/v0.81.0\nccc\trefs/tags/v0.100.0-rc.1\nddd\trefs/tags/v0.80.2\neee\trefs/tags/latest\n";
        assert_eq!(newest_tag(out), Some(Version(0, 81, 0)));
        assert_eq!(newest_tag(""), None);
    }

    #[test]
    fn each_system_gets_its_archive() {
        assert_eq!(
            asset("windows", "x86_64"),
            Some("worktrunk-x86_64-pc-windows-msvc.zip")
        );
        assert_eq!(
            asset("macos", "aarch64"),
            Some("worktrunk-aarch64-apple-darwin.tar.xz")
        );
        assert_eq!(
            asset("linux", "x86_64"),
            Some("worktrunk-x86_64-unknown-linux-musl.tar.xz")
        );
        assert_eq!(asset("windows", "aarch64"), None);
    }

    #[test]
    fn the_install_method_is_read_from_where_the_program_lives() {
        let cases = [
            (
                r"C:\Users\me\AppData\Local\Microsoft\WinGet\Links\git-wt.exe",
                Method::Winget,
            ),
            (
                "/opt/homebrew/Cellar/worktrunk/0.81.0/bin/wt",
                Method::Homebrew,
            ),
            ("/home/linuxbrew/.linuxbrew/bin/wt", Method::Homebrew),
            ("/home/me/.pixi/bin/wt", Method::Pixi),
            ("/home/me/miniforge3/bin/wt", Method::Conda),
            ("/usr/bin/wt", Method::Pacman),
            ("/somewhere/else/wt", Method::Unknown),
        ];
        for (path, method) in cases {
            assert_eq!(
                method_for(&located(path, Origin::Found), None),
                method,
                "{path}"
            );
        }
        let cargo = located("/home/me/.cargo/bin/wt", Origin::Found);
        assert_eq!(method_for(&cargo, Some("[v1]\n\"worktrunk 0.81.0 (registry+https://github.com/rust-lang/crates.io-index)\" = [\"wt\"]\n")), Method::Cargo);
        assert_eq!(method_for(&cargo, Some("[v1]\n")), Method::Script);
        assert_eq!(
            method_for(&located("/data/worktrunk/v1.0.0/wt", Origin::Managed), None),
            Method::Plugin
        );
    }

    #[test]
    fn managers_that_need_a_person_are_named_not_run() {
        assert!(plan(Method::Winget).program.is_some());
        assert!(plan(Method::Homebrew).program.is_some());
        assert!(plan(Method::Cargo).program.is_some());
        for m in [
            Method::Pacman,
            Method::Conda,
            Method::Script,
            Method::Unknown,
            Method::Plugin,
        ] {
            assert!(plan(m).program.is_none(), "{m:?}");
        }
    }

    #[test]
    fn checksums_read_and_compare() {
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(
            published_hash(
                "389B595CB67482085419968939E329D61901CCA9E959E4B6C288F01387B768B8 *worktrunk.zip\n"
            )
            .as_deref(),
            Some("389b595cb67482085419968939e329d61901cca9e959e4b6c288f01387b768b8")
        );
        assert_eq!(published_hash("not a hash"), None);
    }

    #[test]
    fn a_fresh_cache_answers_without_asking() {
        let dir = std::env::temp_dir().join(format!("wtp-cache-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("update-check.json"),
            json!({"latest": "v9.9.9", "checked_at": now()}).to_string(),
        )
        .unwrap();
        assert_eq!(latest(&dir, None, false).unwrap().0, Version(9, 9, 9));
        // Forced, it asks again, and with no git it says why.
        assert!(latest(&dir, None, true).unwrap_err().contains("git"));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
