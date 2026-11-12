//! Plugin marketplaces (Plugins > Discover, Marketplaces). A marketplace is
//! Claude Code's own catalog - `.claude-plugin/marketplace.json` - read as
//! it is, so any Claude marketplace works here and one catalog serves both
//! apps. OpenRemote's own facts about an entry (its runtime, the powers it
//! asks for) ride in the entry's `metadata.openremote`, a field Claude Code
//! leaves alone. A catalog is fetched with git (or downloaded, for a bare
//! marketplace.json link) and cached under `<data dir>/marketplaces/`, so
//! Discover works offline from the last refresh. Nothing here installs a
//! plugin: an entry resolves to a fetch, and the install goes through the
//! same review as any other. A catalog in the sibling layout `adapt`
//! rewrites - marketplace.json at the top of its repo, a `pluginRoot`
//! under every relative source, archive downloads pinned by sha256 - is
//! read too.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use openremote_core::{
    ApprovedCommand, InstalledPlugin, LOCAL_MARKETPLACE, MarketplaceRecord, MarketplaceSource,
    PluginPower, PluginRuntime,
};
use serde::Serialize;
use serde_json::Value;

use crate::plugins::{self, Fetch, PluginError};

/// OpenRemote's own marketplace, and where it lives.
pub const OFFICIAL: &str = "openremote";
pub const OFFICIAL_REPO: &str = "morewebs/OpenRemote-plugins";
/// Anthropic's directory of Claude Code plugins, added out of the box.
pub const ANTHROPIC: &str = "claude-plugins-official";
pub const ANTHROPIC_REPO: &str = "anthropics/claude-plugins-official";

/// A bare marketplace.json link is refused past this size.
const MAX_CATALOG_BYTES: u64 = 5 * 1024 * 1024;

/// The names Claude Code keeps for Anthropic's own marketplaces, honored
/// only from a repository under github.com/anthropics/ - here as there -
/// so a look-alike catalog can't pose as one.
const ANTHROPIC_NAMES: &[&str] = &[
    "claude-code-marketplace",
    "claude-code-plugins",
    "claude-plugins-official",
    "anthropic-marketplace",
    "anthropic-plugins",
    "agent-skills",
    "anthropic-agent-skills",
    "life-sciences",
    "knowledge-work-plugins",
    "claude-for-legal",
    "claude-for-financial-services",
    "financial-services-plugins",
    "first-party-plugins",
    "claude-tag-plugins",
    "claude-community",
    "claude-plugins-community",
    "healthcare",
    "anthropic-plugin-directory",
    "claude-plugin-directory",
];

/// The name the sibling layout's publisher keeps for its official catalog
/// (its own app refuses a copy under that name), honored here only from
/// that publisher's GitHub organization or its own hosts - as Anthropic's
/// names are above.
const PUBLISHER_CATALOG: &str = "zcode-plugins-official";
const PUBLISHER_GITHUB: &str = "zai-org/";
const PUBLISHER_HOSTS: &[&str] = &["z.ai", "zai-org.github.io"];

/// The marketplaces a machine starts with. Each is added once, ever: one
/// the user removes stays removed.
pub fn presets(now: i64) -> Vec<MarketplaceRecord> {
    let preset = |name: &str, repo: &str, removable: bool| MarketplaceRecord {
        name: name.to_string(),
        source: MarketplaceSource::Github {
            repo: repo.to_string(),
            reference: None,
        },
        enabled: true,
        removable,
        owner: None,
        description: None,
        plugin_count: 0,
        added_at: now,
        refreshed_at: None,
        error: None,
        heads: BTreeMap::new(),
    };
    vec![
        preset(OFFICIAL, OFFICIAL_REPO, false),
        preset(ANTHROPIC, ANTHROPIC_REPO, true),
    ]
}

/// What the user typed to add a marketplace, read the way Claude Code's
/// own `marketplace add` reads it: `owner/repo` (with `#ref` or `@ref`), a
/// git URL, a folder on this machine, or a link to a marketplace.json.
pub fn parse_source(input: &str) -> Result<MarketplaceSource, PluginError> {
    let input = input.trim();
    if input.is_empty() {
        return Err(PluginError::Invalid("say where the marketplace is".into()));
    }
    let (base, reference) = match input.rsplit_once('#') {
        Some((base, r)) if !r.is_empty() && !base.is_empty() => (base, Some(r.to_string())),
        _ => (input, None),
    };
    let lower = base.to_ascii_lowercase();
    if base.starts_with("git@") || lower.starts_with("ssh://") || lower.starts_with("file://") {
        return Ok(MarketplaceSource::Git {
            url: base.to_string(),
            reference,
        });
    }
    if lower.starts_with("https://") || lower.starts_with("http://") {
        if lower.ends_with(".git") || lower.contains("/_git/") || is_hosted_repo(&lower) {
            return Ok(MarketplaceSource::Git {
                url: base.to_string(),
                reference,
            });
        }
        return Ok(MarketplaceSource::Url {
            url: input.to_string(),
        });
    }
    let path = Path::new(input);
    if path.is_dir() {
        return Ok(MarketplaceSource::Directory {
            path: absolute(path),
        });
    }
    if path.is_file() && lower.ends_with(".json") {
        // A marketplace.json on disk: its root is two levels up in Claude
        // Code's place (`<root>/.claude-plugin/marketplace.json`), one up
        // at the top of a folder, where the sibling layout keeps it.
        let root = match path.parent() {
            Some(dir) if dir.file_name().is_some_and(|n| n == ".claude-plugin") => dir.parent(),
            other => other,
        };
        if let Some(root) = root {
            return Ok(MarketplaceSource::Directory {
                path: absolute(root),
            });
        }
    }
    let (repo, reference) = match base.split_once('@') {
        Some((repo, r)) if !r.is_empty() => (repo, Some(r.to_string())),
        _ => (base, reference),
    };
    if is_github_shorthand(repo) {
        return Ok(MarketplaceSource::Github {
            repo: repo.to_string(),
            reference,
        });
    }
    Err(PluginError::Invalid(format!(
        "{input} is not owner/repo, a git URL, a folder on this computer or a link to a marketplace.json"
    )))
}

fn absolute(path: &Path) -> String {
    std::path::absolute(path)
        .unwrap_or_else(|_| path.to_path_buf())
        .to_string_lossy()
        .into_owned()
}

/// `https://github.com/owner/repo` (or gitlab.com): a repository page,
/// which git clones as it is.
fn is_hosted_repo(lower: &str) -> bool {
    let rest = lower
        .trim_start_matches("https://")
        .trim_start_matches("http://");
    let Some((host, path)) = rest.split_once('/') else {
        return false;
    };
    matches!(host, "github.com" | "www.github.com" | "gitlab.com")
        && path.trim_end_matches('/').split('/').count() == 2
}

fn is_github_shorthand(s: &str) -> bool {
    let ok = |part: &str| {
        !part.is_empty()
            && part
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
    };
    matches!(s.split_once('/'), Some((owner, repo)) if ok(owner) && ok(repo) && !repo.contains('/'))
}

/// A catalog's name: Claude Code's rule (letters, digits, `.`, `_`, `-`,
/// starting with a letter or digit, no `..`), not `local`, and not a name
/// that belongs to someone else's marketplace unless it comes from there.
pub fn check_name(name: &str, source: &MarketplaceSource) -> Result<(), PluginError> {
    let valid = !name.is_empty()
        && name.len() <= 128
        && name
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphanumeric())
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
        && !name.contains("..");
    if !valid {
        return Err(PluginError::Invalid(format!(
            "'{name}' is not a marketplace name: letters, digits, '.', '_' and '-'"
        )));
    }
    let canonical = canonical(name);
    if canonical == LOCAL_MARKETPLACE {
        return Err(PluginError::Invalid(
            "'local' is the name OpenRemote files direct installs under".into(),
        ));
    }
    let home = github_path(source);
    if canonical == OFFICIAL
        && !home
            .as_deref()
            .is_some_and(|h| h.eq_ignore_ascii_case(OFFICIAL_REPO))
    {
        return Err(PluginError::Invalid(format!(
            "the name '{name}' belongs to OpenRemote's own marketplace ({OFFICIAL_REPO})"
        )));
    }
    if ANTHROPIC_NAMES.contains(&canonical.as_str())
        && !home
            .as_deref()
            .is_some_and(|h| h.starts_with("anthropics/"))
    {
        return Err(PluginError::Invalid(format!(
            "the name '{name}' belongs to one of Anthropic's own marketplaces"
        )));
    }
    if canonical == PUBLISHER_CATALOG && !from_publisher(source, home.as_deref()) {
        return Err(PluginError::Invalid(format!(
            "the name '{name}' belongs to another publisher's own marketplace"
        )));
    }
    Ok(())
}

/// Whether a catalog comes from the sibling layout's publisher: its GitHub
/// organization, or one of its own hosts.
fn from_publisher(source: &MarketplaceSource, home: Option<&str>) -> bool {
    if home.is_some_and(|h| h.starts_with(PUBLISHER_GITHUB)) {
        return true;
    }
    let url = match source {
        MarketplaceSource::Url { url } | MarketplaceSource::Git { url, .. } => url,
        _ => return false,
    };
    url_host(url).is_some_and(|host| {
        PUBLISHER_HOSTS
            .iter()
            .any(|h| host == *h || host.ends_with(&format!(".{h}")))
    })
}

/// Spellings that read as the same name: case, a trailing dot, a dot for
/// a dash.
fn canonical(name: &str) -> String {
    name.to_ascii_lowercase()
        .trim_end_matches('.')
        .replace('.', "-")
}

/// `owner/repo` (lowercase) when the source is a GitHub repository.
fn github_path(source: &MarketplaceSource) -> Option<String> {
    let path = match source {
        MarketplaceSource::Github { repo, .. } => repo.clone(),
        MarketplaceSource::Git { url, .. } => {
            let lower = url.to_ascii_lowercase();
            let at = lower
                .find("github.com/")
                .map(|i| i + "github.com/".len())
                .or_else(|| lower.find("github.com:").map(|i| i + "github.com:".len()))?;
            lower[at..].to_string()
        }
        _ => return None,
    };
    Some(
        path.to_ascii_lowercase()
            .trim_end_matches('/')
            .trim_end_matches(".git")
            .to_string(),
    )
}

/// A catalog as read from its marketplace.json.
pub struct Catalog {
    pub name: String,
    pub owner: Option<String>,
    pub description: Option<String>,
    /// `metadata.pluginRoot`: where bare entry names resolve.
    pub plugin_root: Option<String>,
    pub entries: Vec<Value>,
}

impl Catalog {
    pub fn entry(&self, name: &str) -> Option<&Value> {
        self.entries
            .iter()
            .find(|e| e["name"].as_str() == Some(name))
    }
}

/// Read `<root>/.claude-plugin/marketplace.json` - or, where there is none,
/// the `marketplace.json` at the top of the folder, where the sibling
/// layout keeps its catalog.
pub fn read_catalog(root: &Path) -> Result<Catalog, PluginError> {
    let claude = root.join(".claude-plugin").join("marketplace.json");
    let top = root.join("marketplace.json");
    let (path, at_top) = if claude.is_file() || !top.is_file() {
        (claude, false)
    } else {
        (top, true)
    };
    let raw = std::fs::read(&path).map_err(|_| {
        PluginError::Source(format!(
            "{} has no .claude-plugin/marketplace.json",
            root.display()
        ))
    })?;
    let v: Value = serde_json::from_slice(&raw)
        .map_err(|e| PluginError::Invalid(format!("marketplace.json is not valid: {e}")))?;
    let name = v["name"]
        .as_str()
        .filter(|n| !n.is_empty())
        .ok_or_else(|| PluginError::Invalid("marketplace.json names no marketplace".into()))?
        .to_string();
    let mut entries = v["plugins"]
        .as_array()
        .cloned()
        .ok_or_else(|| PluginError::Invalid("marketplace.json lists no plugins".into()))?;
    let text = |v: &Value| v.as_str().map(String::from);
    if at_top {
        if let Some(base) = text(&v["pluginRoot"]) {
            under_plugin_root(&mut entries, &base);
        }
    }
    Ok(Catalog {
        name,
        owner: text(&v["owner"]["name"]),
        description: text(&v["description"]).or_else(|| text(&v["metadata"]["description"])),
        plugin_root: text(&v["metadata"]["pluginRoot"]),
        entries,
    })
}

/// The sibling layout's top-level `pluginRoot` puts every relative source
/// under it, `./hello` included - unlike Claude Code's
/// `metadata.pluginRoot`, which only places bare names. Written into the
/// entries as read, so the rest reads one form.
fn under_plugin_root(entries: &mut [Value], base: &str) {
    let base = base.trim_start_matches("./").trim_end_matches('/');
    if base.is_empty() || base == "." {
        return;
    }
    for entry in entries {
        let Some(source) = entry.get("source").and_then(Value::as_str) else {
            continue;
        };
        let placed = if source == "." {
            format!("./{base}")
        } else if let Some(rest) = source.strip_prefix("./") {
            format!("./{base}/{rest}")
        } else if !source.is_empty() && !source.contains('/') {
            format!("./{base}/{source}")
        } else {
            continue;
        };
        entry["source"] = Value::String(placed);
    }
}

/// Where an entry's plugin comes from - every source type a Claude Code
/// catalog can name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EntrySource {
    /// A folder inside the marketplace itself.
    Relative(String),
    Github {
        repo: String,
        reference: Option<String>,
        sha: Option<String>,
    },
    Url {
        url: String,
        reference: Option<String>,
        sha: Option<String>,
    },
    GitSubdir {
        url: String,
        path: String,
        reference: Option<String>,
        sha: Option<String>,
    },
    Npm {
        package: String,
        version: Option<String>,
        registry: Option<String>,
    },
    /// A .zip or .tar.gz; `path` is the one folder inside it that is the
    /// plugin.
    Archive {
        url: String,
        sha256: Option<String>,
        path: Option<String>,
    },
    Command {
        command: String,
        timeout_secs: u64,
    },
}

/// Read an entry's `source`; the error says why it can't be installed.
pub fn entry_source(entry: &Value, plugin_root: Option<&str>) -> Result<EntrySource, String> {
    let text = |v: &Value| v.as_str().map(String::from);
    // The entry's name is what an install is filed under, so a catalog is
    // held to the same gate a plugin's own name passes - read here, so
    // Discover says why rather than offering an install that could only be
    // refused, and checked again when the install is staged.
    if let Some(name) = entry["name"].as_str() {
        if !plugins::valid_name(name) {
            return Err(format!(
                "its name '{name}' is not a plugin name: lowercase letters, digits and dashes, up to 64"
            ));
        }
    }
    match &entry["source"] {
        Value::String(s) => {
            let relative = if s == "." {
                ".".to_string()
            } else if let Some(rest) = s.strip_prefix("./") {
                rest.trim_end_matches('/').to_string()
            } else if !s.contains('/') && !s.is_empty() && plugin_root.is_some() {
                let root = plugin_root
                    .unwrap_or_default()
                    .trim_start_matches("./")
                    .trim_end_matches('/');
                if root.is_empty() || root == "." {
                    s.clone()
                } else {
                    format!("{root}/{s}")
                }
            } else {
                return Err(format!("its source path '{s}' must start with ./"));
            };
            if relative != "." && !plugins::relative_inside(&relative) {
                return Err(format!("its source path '{s}' leaves the marketplace"));
            }
            Ok(EntrySource::Relative(relative))
        }
        Value::Object(o) => {
            let field = |key: &str| o.get(key).and_then(text);
            let need = |key: &str| field(key).ok_or_else(|| format!("its source has no {key}"));
            // A commit and a branch reach git's command line, which git
            // reads for options anywhere: a pinned `sha` of
            // `--upload-pack=<program>` would start that program. Each is
            // checked here so Discover can say why, and again in
            // `plugins::git_fetch`, which passes what it was given after
            // `--`.
            let commit = |value: Option<String>| match value {
                Some(sha) if plugins::is_commit_id(&sha) => Ok(Some(sha)),
                Some(sha) => Err(format!(
                    "its pinned sha '{sha}' is not a commit id: hex, 7 to 64 characters"
                )),
                None => Ok(None),
            };
            let branch = |value: Option<String>| match value {
                Some(reference) if reference.starts_with('-') => {
                    Err(format!("'{reference}' is not a branch or tag"))
                }
                other => Ok(other),
            };
            // The url becomes a positional argument of `git fetch` and
            // `git ls-remote`, both of which read a leading dash as an
            // option: refused here so Discover does not offer an install
            // that the fetch step could only refuse.
            let address = |value: Option<String>| match value {
                Some(url) if url.starts_with('-') => Err(format!("'{url}' is not a web address")),
                other => Ok(other),
            };
            // A folder inside the repository, when the source names one.
            let folder = || -> Result<Option<String>, String> {
                let Some(path) = field("path")
                    .map(|p| p.trim_matches('/').to_string())
                    .filter(|p| !p.is_empty() && p != ".")
                else {
                    return Ok(None);
                };
                if !plugins::relative_inside(&path) {
                    return Err(format!("its folder '{path}' leaves the repository"));
                }
                Ok(Some(path))
            };
            match o.get("source").and_then(Value::as_str) {
                // The sibling layout names a repository's subfolder with
                // `path` here; Claude Code's catalogs use git-subdir.
                Some("github") => match folder()? {
                    Some(path) => Ok(EntrySource::GitSubdir {
                        url: need("repo")?,
                        path,
                        reference: branch(field("ref"))?,
                        sha: commit(field("sha"))?,
                    }),
                    None => Ok(EntrySource::Github {
                        repo: need("repo")?,
                        reference: branch(field("ref"))?,
                        sha: commit(field("sha"))?,
                    }),
                },
                // A `type` makes it a download (the sibling layout's);
                // without one it is a git repository, as in Claude Code.
                Some("url") if o.contains_key("type") => download(o),
                // The sibling layout's name for any git repository.
                Some("git") => {
                    let url = address(field("url"))?
                        .ok_or_else(|| "its source has no url".to_string())?;
                    match folder()? {
                        Some(path) => Ok(EntrySource::GitSubdir {
                            url,
                            path,
                            reference: branch(field("ref"))?,
                            sha: commit(field("sha"))?,
                        }),
                        None => Ok(EntrySource::Url {
                            url,
                            reference: branch(field("ref"))?,
                            sha: commit(field("sha"))?,
                        }),
                    }
                }
                Some("url") => Ok(EntrySource::Url {
                    url: address(field("url"))?
                        .ok_or_else(|| "its source has no url".to_string())?,
                    reference: branch(field("ref"))?,
                    sha: commit(field("sha"))?,
                }),
                Some("git-subdir") => Ok(EntrySource::GitSubdir {
                    url: address(field("url"))?
                        .ok_or_else(|| "its source has no url".to_string())?,
                    path: need("path")?.trim_matches('/').to_string(),
                    reference: branch(field("ref"))?,
                    sha: commit(field("sha"))?,
                }),
                Some("npm") => Ok(EntrySource::Npm {
                    package: need("package")?,
                    version: field("version"),
                    registry: field("registry"),
                }),
                Some("archive") => Ok(EntrySource::Archive {
                    url: need("url")?,
                    sha256: field("sha256"),
                    path: archive_folder(field("path"))?,
                }),
                Some("command") => Ok(EntrySource::Command {
                    command: need("command")?,
                    timeout_secs: o.get("timeout").and_then(Value::as_u64).unwrap_or(60),
                }),
                Some(other) => Err(format!("its source type '{other}' is unknown here")),
                None => Err("its source names no type".into()),
            }
        }
        _ => Err("it names no source".into()),
    }
}

/// A download the sibling layout pins: `{"source": "url", "type": "zip",
/// "url", "sha256", "path"}`. Its sha256 is required - that layout stops
/// an install without one - and the link is https (or this machine's own
/// address, for a local mirror).
fn download(o: &serde_json::Map<String, Value>) -> Result<EntrySource, String> {
    let text = |key: &str| o.get(key).and_then(Value::as_str).map(String::from);
    match text("type").as_deref() {
        Some("zip") => {}
        Some(other) => return Err(format!("its download type '{other}' is unknown here")),
        None => return Err("its download names no type".into()),
    }
    let url = text("url").ok_or("its source has no url")?;
    if !secure_link(&url) {
        return Err("its download is not an https link".into());
    }
    let sha256 = text("sha256").ok_or("its download names no sha256 to check it against")?;
    if sha256.len() != 64 || !sha256.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err("its download's sha256 is not 64 hex digits".into());
    }
    Ok(EntrySource::Archive {
        url,
        sha256: Some(sha256),
        path: archive_folder(text("path"))?,
    })
}

/// The folder inside an archive that is the plugin; none for the whole.
fn archive_folder(path: Option<String>) -> Result<Option<String>, String> {
    let Some(path) = path
        .map(|p| p.trim_matches('/').to_string())
        .filter(|p| !p.is_empty() && p != ".")
    else {
        return Ok(None);
    };
    if !plugins::relative_inside(&path) {
        return Err(format!("its folder '{path}' leaves the download"));
    }
    Ok(Some(path))
}

/// https, or plain http to this machine itself.
fn secure_link(url: &str) -> bool {
    let lower = url.to_ascii_lowercase();
    lower.starts_with("https://")
        || (lower.starts_with("http://")
            && url_host(&lower)
                .is_some_and(|host| matches!(host.as_str(), "127.0.0.1" | "localhost" | "::1")))
}

/// The host a URL names, lowercase, without its user, port or brackets.
fn url_host(url: &str) -> Option<String> {
    let lower = url.to_ascii_lowercase();
    let rest = lower.split_once("://")?.1;
    let authority = rest.split(['/', '?', '#']).next()?;
    let authority = authority
        .rsplit_once('@')
        .map_or(authority, |(_, host)| host);
    let host = match authority.strip_prefix('[') {
        Some(v6) => v6.split(']').next()?,
        None => authority.split(':').next()?,
    };
    (!host.is_empty()).then(|| host.to_string())
}

/// `owner/repo` shorthand to a clone URL; a URL stays as it is.
fn clone_url(url: &str) -> String {
    if is_github_shorthand(url) {
        format!("https://github.com/{url}.git")
    } else {
        url.to_string()
    }
}

impl EntrySource {
    pub fn kind(&self) -> &'static str {
        match self {
            EntrySource::Relative(_) => "relative",
            EntrySource::Github { .. } => "github",
            EntrySource::Url { .. } => "url",
            EntrySource::GitSubdir { .. } => "git-subdir",
            EntrySource::Npm { .. } => "npm",
            EntrySource::Archive { .. } => "archive",
            EntrySource::Command { .. } => "command",
        }
    }

    /// Where it comes from, in a line.
    pub fn describe(&self) -> String {
        match self {
            EntrySource::Relative(path) => format!("the marketplace's own folder ./{path}"),
            EntrySource::Github { repo, .. } => format!("github.com/{repo}"),
            EntrySource::Url { url, .. } => url.clone(),
            EntrySource::GitSubdir { url, path, .. } => format!("{path} in {url}"),
            EntrySource::Npm { package, .. } => format!("npm package {package}"),
            EntrySource::Archive { url, .. } => url.clone(),
            EntrySource::Command { command, .. } => format!("the command {command}"),
        }
    }

    /// A git source that follows a branch rather than a pinned commit:
    /// (clone URL, branch or tag).
    pub fn followed_branch(&self) -> Option<(String, Option<String>)> {
        match self {
            EntrySource::Github {
                repo,
                reference,
                sha: None,
            } => Some((clone_url(repo), reference.clone())),
            EntrySource::Url {
                url,
                reference,
                sha: None,
            }
            | EntrySource::GitSubdir {
                url,
                reference,
                sha: None,
                ..
            } => Some((clone_url(url), reference.clone())),
            _ => None,
        }
    }

    /// What to fetch. `files` is the marketplace's folder on this machine;
    /// a bare marketplace.json link has none, so its folder entries can't
    /// be fetched.
    pub fn fetch(&self, files: Option<&Path>) -> Result<Fetch, String> {
        Ok(match self {
            EntrySource::Relative(path) => {
                let root = files.ok_or(
                    "its marketplace is a bare catalog link, so a folder inside it can't be fetched",
                )?;
                Fetch::Folder(if path == "." {
                    root.to_path_buf()
                } else {
                    root.join(path)
                })
            }
            EntrySource::Github {
                repo,
                reference,
                sha,
            } => Fetch::Git {
                url: clone_url(repo),
                reference: reference.clone(),
                sha: sha.clone(),
                subdir: None,
            },
            EntrySource::Url {
                url,
                reference,
                sha,
            } => Fetch::Git {
                url: url.clone(),
                reference: reference.clone(),
                sha: sha.clone(),
                subdir: None,
            },
            EntrySource::GitSubdir {
                url,
                path,
                reference,
                sha,
            } => Fetch::Git {
                url: clone_url(url),
                reference: reference.clone(),
                sha: sha.clone(),
                subdir: Some(path.clone()),
            },
            EntrySource::Npm {
                package,
                version,
                registry,
            } => Fetch::Npm {
                package: package.clone(),
                version: version.clone(),
                registry: registry.clone(),
            },
            EntrySource::Archive { url, sha256, path } => Fetch::Archive {
                location: url.clone(),
                sha256: sha256.clone(),
                path: path.clone(),
            },
            EntrySource::Command {
                command,
                timeout_secs,
            } => Fetch::Command(ApprovedCommand {
                command: command.clone(),
                timeout_secs: *timeout_secs,
            }),
        })
    }
}

/// The marketplaces' cached catalogs on this machine.
pub struct MarketplaceHost {
    root: PathBuf,
}

/// A catalog fetched and read, not yet put in place.
pub struct Fetched {
    incoming: Option<PathBuf>,
    pub catalog: Catalog,
}

impl MarketplaceHost {
    pub fn new(data_dir: &Path) -> Self {
        let root = data_dir.join("marketplaces");
        let _ = std::fs::remove_dir_all(root.join(".incoming"));
        let _ = std::fs::remove_dir_all(root.join(".trash"));
        Self { root }
    }

    /// The folder its catalog is read from.
    pub fn catalog_dir(&self, record: &MarketplaceRecord) -> PathBuf {
        match &record.source {
            MarketplaceSource::Directory { path } => PathBuf::from(path),
            _ => self.root.join(&record.name),
        }
    }

    /// The folder its plugins' own folders sit in; none for a bare link.
    pub fn files(&self, record: &MarketplaceRecord) -> Option<PathBuf> {
        match &record.source {
            MarketplaceSource::Url { .. } => None,
            _ => Some(self.catalog_dir(record)),
        }
    }

    /// Fetch a catalog and read it, without touching the cached copy.
    pub async fn fetch(&self, source: &MarketplaceSource) -> Result<Fetched, PluginError> {
        if let MarketplaceSource::Directory { path } = source {
            return Ok(Fetched {
                incoming: None,
                catalog: read_catalog(Path::new(path))?,
            });
        }
        let incoming = self
            .root
            .join(".incoming")
            .join(uuid::Uuid::new_v4().to_string());
        let read = async {
            match source {
                MarketplaceSource::Github { repo, reference } => {
                    plugins::git_fetch(
                        &clone_url(repo),
                        reference.as_deref(),
                        None,
                        None,
                        &incoming,
                    )
                    .await?;
                }
                MarketplaceSource::Git { url, reference } => {
                    plugins::git_fetch(url, reference.as_deref(), None, None, &incoming).await?;
                }
                MarketplaceSource::Url { url } => {
                    let body =
                        plugins::http_get(url, "application/json", MAX_CATALOG_BYTES).await?;
                    let dir = incoming.join(".claude-plugin");
                    std::fs::create_dir_all(&dir)?;
                    std::fs::write(dir.join("marketplace.json"), body)?;
                }
                MarketplaceSource::Directory { .. } => unreachable!("read in place above"),
            }
            read_catalog(&incoming)
        }
        .await;
        match read {
            Ok(catalog) => Ok(Fetched {
                incoming: Some(incoming),
                catalog,
            }),
            Err(e) => {
                let _ = std::fs::remove_dir_all(&incoming);
                Err(e)
            }
        }
    }

    /// Put a fetched catalog in place of the cached one, whole.
    pub fn settle(&self, name: &str, fetched: Fetched) -> Result<Catalog, PluginError> {
        if let Some(incoming) = &fetched.incoming {
            let target = self.root.join(name);
            let moved = plugins::swap_in(&self.root, incoming, &target);
            let _ = std::fs::remove_dir_all(incoming);
            moved?;
        }
        Ok(fetched.catalog)
    }

    /// Drop a removed marketplace's cached catalog (a folder marketplace
    /// is the user's own folder and stays).
    pub fn forget(&self, record: &MarketplaceRecord) {
        if !matches!(record.source, MarketplaceSource::Directory { .. }) {
            let _ = std::fs::remove_dir_all(self.root.join(&record.name));
        }
    }
}

/// An installed plugin as Discover shows it beside its entry.
#[derive(Serialize, Debug)]
pub struct InstalledView {
    pub version: Option<String>,
    pub enabled: bool,
}

/// One catalog entry, as Discover shows it.
#[derive(Serialize, Debug)]
pub struct EntryView {
    pub id: String,
    pub name: String,
    pub marketplace: String,
    pub display_name: Option<String>,
    pub description: Option<String>,
    pub version: Option<String>,
    pub author: Option<String>,
    pub category: Option<String>,
    pub tags: Vec<String>,
    pub homepage: Option<String>,
    pub source_kind: &'static str,
    pub from: String,
    pub installable: bool,
    pub why_not: Option<String>,
    pub runtime: Option<PluginRuntime>,
    /// None: not known until the review reads the plugin.
    pub powers: Option<Vec<PluginPower>>,
    pub not_sandboxed: bool,
    pub claude_parts: Vec<String>,
    pub installed: Option<InstalledView>,
    pub update: bool,
}

/// What the entry itself says it carries, for an entry whose folder can't
/// be read before install.
fn entry_parts(entry: &Value) -> (Vec<String>, bool) {
    let mut parts = Vec::new();
    for part in ["skills", "agents", "commands"] {
        if entry.get(part).is_some() {
            parts.push(part.to_string());
        }
    }
    let hooks = entry.get("hooks").is_some();
    let mcp = entry.get("mcpServers").is_some();
    if hooks {
        parts.push("hooks".into());
    }
    if mcp {
        parts.push("mcp".into());
    }
    (parts, hooks || mcp || entry.get("lspServers").is_some())
}

/// The catalog's homepage when it carries an http(s) prefix, or none.
///
/// `homepage` is catalog text and the console hands it to `window.open` or
/// to the platform's `open_url`, which is to say to the operating system's
/// handler for whatever scheme it names. Anything without that prefix - a
/// `file:` link, a custom protocol some installed program claims - is not
/// something a review dialog should hand over with one click, so it never
/// leaves the daemon.
fn web_link(value: Option<String>) -> Option<String> {
    value.filter(|url| url.starts_with("https://") || url.starts_with("http://"))
}

/// The entries of one marketplace's catalog, as Discover shows them: what
/// each is, whether OpenRemote can install it, what it asks for when that
/// is knowable before install, and whether an installed copy is behind.
pub fn entry_views(
    record: &MarketplaceRecord,
    catalog: &Catalog,
    files: Option<&Path>,
    installed: &[InstalledPlugin],
    builtin_ids: &[String],
) -> Vec<EntryView> {
    let text = |v: &Value| v.as_str().map(String::from);
    let mut views = Vec::new();
    for entry in &catalog.entries {
        let Some(name) = entry["name"].as_str() else {
            continue;
        };
        let id = format!("{name}@{}", record.name);
        let source = entry_source(entry, catalog.plugin_root.as_deref());
        let mut why_not = source.as_ref().err().cloned();
        let runtime;
        let mut powers = None;
        let mut claude_parts = Vec::new();
        let mut version = text(&entry["version"]);
        // The sibling layout gives text per language beside the plain field
        // (`description_i18n`); the app speaks English.
        let english = |entry: &Value, key: &str| {
            text(&entry[format!("{key}_i18n")]["en"]).or_else(|| text(&entry[key]))
        };
        let mut description = english(entry, "description");

        // A folder inside the marketplace can be read now, as Claude Code
        // reads it: its own manifest says exactly what it is.
        let folder = match (&source, files) {
            (Ok(EntrySource::Relative(path)), Some(root)) => Some(if path == "." {
                root.to_path_buf()
            } else {
                root.join(path)
            }),
            (Ok(EntrySource::Relative(_)), None) => {
                why_not = Some(
                    "its marketplace is a bare catalog link, so a folder inside it can't be fetched"
                        .into(),
                );
                None
            }
            _ => None,
        };
        let read = folder
            .as_deref()
            .and_then(|dir| plugins::read_manifest(dir, builtin_ids).ok());
        if let Some(m) = &read {
            // A plugin written for another app whose every part needs that
            // app (its publisher's account, its panels) has nothing to
            // install here; said now rather than at the review.
            if why_not.is_none() {
                why_not = m.nothing_left();
            }
            runtime = Some(m.runtime);
            powers = Some(m.powers.clone());
            claude_parts = m.claude_parts.clone();
            version = m.version.clone().or(version);
            description = description.or(m.description.clone());
        } else if let Some(ours) = entry["metadata"]["openremote"].as_object() {
            // What the catalog declares for OpenRemote: shown, and checked
            // against the plugin itself at review.
            let rt = ours
                .get("runtime")
                .and_then(Value::as_str)
                .and_then(plugins::runtime);
            let mut declared: Vec<PluginPower> = ours
                .get("powers")
                .and_then(Value::as_array)
                .map(|ws| {
                    ws.iter()
                        .filter_map(|w| w.as_str().and_then(plugins::power))
                        .collect()
                })
                .unwrap_or_default();
            if rt == Some(PluginRuntime::Process) {
                declared.push(PluginPower::RunPrograms);
            }
            declared.sort();
            declared.dedup();
            runtime = rt;
            powers = Some(declared);
        } else {
            let (parts, runs_programs) = entry_parts(entry);
            claude_parts = parts;
            runtime = Some(PluginRuntime::Content);
            let command = matches!(source, Ok(EntrySource::Command { .. }));
            if runs_programs || command {
                powers = Some(vec![PluginPower::RunPrograms]);
            }
        }
        let not_sandboxed = powers
            .as_ref()
            .is_some_and(|p| p.contains(&PluginPower::RunPrograms))
            && matches!(
                runtime,
                Some(PluginRuntime::Process | PluginRuntime::Content)
            );

        let have = installed.iter().find(|p| p.id() == id);
        let update = match (have, &source) {
            (Some(have), Ok(source)) => is_behind(
                have,
                version.as_deref(),
                source,
                folder.as_deref(),
                &record.heads,
            ),
            _ => false,
        };
        let tags = ["tags", "keywords"]
            .iter()
            .flat_map(|k| entry[*k].as_array().cloned().unwrap_or_default())
            .filter_map(|t| t.as_str().map(String::from))
            .collect();
        let author = text(&entry["author"]["name"]).or_else(|| text(&entry["author"]));
        views.push(EntryView {
            id,
            name: name.to_string(),
            marketplace: record.name.clone(),
            display_name: english(entry, "displayName"),
            description,
            version,
            author,
            category: text(&entry["category"]),
            tags,
            homepage: web_link(text(&entry["homepage"])),
            source_kind: source.as_ref().map(EntrySource::kind).unwrap_or("unknown"),
            from: source
                .as_ref()
                .map(EntrySource::describe)
                .unwrap_or_default(),
            installable: why_not.is_none(),
            why_not,
            runtime,
            powers,
            not_sandboxed,
            claude_parts,
            installed: have.map(|p| InstalledView {
                version: p.version.clone(),
                enabled: p.enabled,
            }),
            update,
        });
    }
    views
}

/// Whether the marketplace now offers something other than what is
/// installed: a different version when both name one; else the pinned
/// commit or digest; else the folder's content; else, for a followed
/// branch, the commit it pointed at on the last refresh.
fn is_behind(
    have: &InstalledPlugin,
    offered: Option<&str>,
    source: &EntrySource,
    folder: Option<&Path>,
    heads: &BTreeMap<String, String>,
) -> bool {
    if let (Some(have), Some(offered)) = (have.version.as_deref(), offered) {
        return have != offered;
    }
    let resolved = have.resolved.as_deref().unwrap_or_default();
    match source {
        EntrySource::Github { sha: Some(sha), .. }
        | EntrySource::Url { sha: Some(sha), .. }
        | EntrySource::GitSubdir { sha: Some(sha), .. }
        | EntrySource::Archive {
            sha256: Some(sha), ..
        } => !resolved.eq_ignore_ascii_case(sha),
        EntrySource::Relative(_) => folder
            .and_then(|dir| plugins::content_hash(dir).ok())
            .is_some_and(|hash| hash != resolved),
        EntrySource::Npm {
            version: Some(v), ..
        } if semver::Version::parse(v).is_ok() => resolved != v,
        _ => match source.followed_branch() {
            Some(_) => heads.get(&have.name).is_some_and(|head| head != resolved),
            None => false,
        },
    }
}

/// On a refresh: for the installed plugins of this marketplace whose entry
/// follows a branch and names no version, the commit each branch points
/// at now - what `is_behind` compares against.
pub async fn followed_heads(
    catalog: &Catalog,
    installed: &[InstalledPlugin],
) -> BTreeMap<String, String> {
    let mut heads = BTreeMap::new();
    for plugin in installed {
        let Some(entry) = catalog.entry(&plugin.name) else {
            continue;
        };
        if entry.get("version").is_some() {
            continue;
        }
        let Ok(source) = entry_source(entry, catalog.plugin_root.as_deref()) else {
            continue;
        };
        if let Some((url, reference)) = source.followed_branch() {
            if let Ok(head) = plugins::git_head(&url, reference.as_deref()).await {
                heads.insert(plugin.name.clone(), head);
            }
        }
    }
    heads
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn the_add_forms_read_as_claude_code_reads_them() {
        let github = |repo: &str, reference: Option<&str>| MarketplaceSource::Github {
            repo: repo.into(),
            reference: reference.map(String::from),
        };
        assert_eq!(
            parse_source("acme/tools").unwrap(),
            github("acme/tools", None)
        );
        assert_eq!(
            parse_source("acme/tools#stable").unwrap(),
            github("acme/tools", Some("stable"))
        );
        assert_eq!(
            parse_source("acme/tools@v2").unwrap(),
            github("acme/tools", Some("v2"))
        );
        assert_eq!(
            parse_source("https://gitlab.example.com/team/plugins.git").unwrap(),
            MarketplaceSource::Git {
                url: "https://gitlab.example.com/team/plugins.git".into(),
                reference: None
            }
        );
        assert_eq!(
            parse_source("https://github.com/acme/tools").unwrap(),
            MarketplaceSource::Git {
                url: "https://github.com/acme/tools".into(),
                reference: None
            }
        );
        assert_eq!(
            parse_source("https://plugins.example.com/marketplace.json").unwrap(),
            MarketplaceSource::Url {
                url: "https://plugins.example.com/marketplace.json".into()
            }
        );
        let dir = tempfile::tempdir().unwrap();
        assert!(matches!(
            parse_source(&dir.path().to_string_lossy()).unwrap(),
            MarketplaceSource::Directory { .. }
        ));
        assert!(parse_source("not a place").is_err());
        assert!(parse_source("").is_err());
    }

    #[test]
    fn a_look_alike_cannot_take_a_name_that_belongs_elsewhere() {
        let elsewhere = MarketplaceSource::Github {
            repo: "someone/plugins".into(),
            reference: None,
        };
        for name in [
            "openremote",
            "OpenRemote",
            "openremote.",
            "claude-plugins-official",
            "claude.plugins.official",
            "local",
        ] {
            assert!(check_name(name, &elsewhere).is_err(), "{name}");
        }
        assert!(check_name("bad name", &elsewhere).is_err());
        assert!(check_name("-dash", &elsewhere).is_err());
        assert!(check_name("my-tools", &elsewhere).is_ok());

        let official = MarketplaceSource::Git {
            url: "https://github.com/morewebs/OpenRemote-plugins.git".into(),
            reference: None,
        };
        assert!(check_name("openremote", &official).is_ok());
        let anthropic = MarketplaceSource::Github {
            repo: "anthropics/claude-plugins-official".into(),
            reference: None,
        };
        assert!(check_name("claude-plugins-official", &anthropic).is_ok());
    }

    #[test]
    fn every_source_type_reads_and_unknown_ones_say_why() {
        let read = |v: Value| entry_source(&json!({"name": "x", "source": v}), None);
        assert_eq!(
            read(json!("./plugins/fmt")).unwrap(),
            EntrySource::Relative("plugins/fmt".into())
        );
        assert_eq!(read(json!(".")).unwrap(), EntrySource::Relative(".".into()));
        assert!(read(json!("./../out")).unwrap_err().contains("leaves"));
        assert!(read(json!("plugins/fmt")).unwrap_err().contains("./"));
        assert_eq!(
            entry_source(&json!({"name": "x", "source": "fmt"}), Some("./plugins")).unwrap(),
            EntrySource::Relative("plugins/fmt".into())
        );
        assert_eq!(
            read(json!({"source": "github", "repo": "a/b", "ref": "v1"})).unwrap(),
            EntrySource::Github {
                repo: "a/b".into(),
                reference: Some("v1".into()),
                sha: None
            }
        );
        assert!(matches!(
            read(json!({"source": "git-subdir", "url": "a/b", "path": "/tools/fmt/"})).unwrap(),
            EntrySource::GitSubdir { ref path, .. } if path == "tools/fmt"
        ));
        assert!(matches!(
            read(json!({"source": "npm", "package": "@a/fmt"})).unwrap(),
            EntrySource::Npm { .. }
        ));
        assert!(matches!(
            read(json!({"source": "archive", "url": "https://x/fmt.zip", "sha256": "ab"})).unwrap(),
            EntrySource::Archive { .. }
        ));
        assert_eq!(
            read(json!({"source": "command", "command": "tool path"})).unwrap(),
            EntrySource::Command {
                command: "tool path".into(),
                timeout_secs: 60
            }
        );
        assert!(
            read(json!({"source": "pip", "package": "x"}))
                .unwrap_err()
                .contains("'pip'")
        );
        assert!(
            read(json!({"source": "github"}))
                .unwrap_err()
                .contains("repo")
        );

        // owner/repo shorthand in a subfolder source clones from GitHub.
        let fetch = read(json!({"source": "git-subdir", "url": "a/b", "path": "p"}))
            .unwrap()
            .fetch(None)
            .unwrap();
        assert!(
            matches!(fetch, Fetch::Git { ref url, ref subdir, .. } if url == "https://github.com/a/b.git" && subdir.as_deref() == Some("p"))
        );
        // A bare catalog link has no folders to fetch from.
        assert!(EntrySource::Relative("x".into()).fetch(None).is_err());
    }

    #[test]
    fn a_catalog_entry_is_held_to_the_plugin_name_and_commit_gates() {
        let read = |v: Value| entry_source(&v, None);
        assert!(
            read(json!({"name": "../../escape", "source": "./x"}))
                .unwrap_err()
                .contains("not a plugin name")
        );
        assert!(
            read(json!({"name": "a/b", "source": {"source": "github", "repo": "a/b"}}))
                .unwrap_err()
                .contains("not a plugin name")
        );
        assert!(
            read(
                json!({"name": "tools", "source": {"source": "github", "repo": "a/b",
                      "sha": "--upload-pack=C:/Windows/System32/calc.exe"}})
            )
            .unwrap_err()
            .contains("not a commit id")
        );
        assert!(
            read(
                json!({"name": "tools", "source": {"source": "url", "url": "https://x/y",
                      "ref": "--upload-pack=calc.exe"}})
            )
            .unwrap_err()
            .contains("not a branch or tag")
        );
        // A url becomes an argument of git fetch and ls-remote, which
        // read a leading dash as an option; Discover refuses it up front
        // rather than offering an install the fetch could only refuse.
        assert!(
            read(json!({"name": "tools", "source": {"source": "url",
                  "url": "--upload-pack=C:/Windows/System32/calc.exe"}}))
            .unwrap_err()
            .contains("not a web address")
        );
        assert!(
            read(json!({"name": "tools", "source": {"source": "git-subdir",
                  "url": "-x", "path": "sub"}}))
            .unwrap_err()
            .contains("not a web address")
        );
        // A pinned commit of the usual short or full length still reads.
        for sha in ["a7ee8a3", "a7ee8a3ab1234567890123456789012345678901f"] {
            assert!(matches!(
                read(json!({"name": "tools", "source": {"source": "github", "repo": "a/b", "sha": sha}})).unwrap(),
                EntrySource::Github { sha: Some(_), .. }
            ));
        }
    }

    #[test]
    fn discover_offers_only_http_links_to_open() {
        let record = MarketplaceRecord {
            name: "claude-plugins-official".into(),
            source: MarketplaceSource::Github {
                repo: "anthropics/claude-plugins-official".into(),
                reference: None,
            },
            enabled: true,
            removable: true,
            owner: None,
            description: None,
            plugin_count: 3,
            added_at: 0,
            refreshed_at: None,
            error: None,
            heads: BTreeMap::new(),
        };
        let catalog = Catalog {
            name: "claude-plugins-official".into(),
            owner: None,
            description: None,
            plugin_root: None,
            entries: vec![
                json!({"name": "web", "source": {"source": "github", "repo": "a/web"},
                       "homepage": "https://example.com/web"}),
                json!({"name": "local", "source": {"source": "github", "repo": "a/local"},
                       "homepage": "file:///C:/Windows/System32/calc.exe"}),
                json!({"name": "custom", "source": {"source": "github", "repo": "a/custom"},
                       "homepage": "vscode://open?x"}),
            ],
        };
        let views = entry_views(&record, &catalog, None, &[], &[]);
        let by = |n: &str| views.iter().find(|v| v.name == n).unwrap();
        // A homepage the console would hand to window.open or the OS
        // handler: http(s) only, so a `file:` link or a custom protocol
        // never reaches the review dialog.
        assert_eq!(
            by("web").homepage.as_deref(),
            Some("https://example.com/web")
        );
        assert_eq!(by("local").homepage, None);
        assert_eq!(by("custom").homepage, None);
    }

    #[test]
    fn discover_says_what_an_entry_asks_for_when_it_can() {
        let record = &presets(0)[1];
        let catalog = Catalog {
            name: record.name.clone(),
            owner: None,
            description: None,
            plugin_root: None,
            entries: vec![
                json!({"name": "linter", "source": {"source": "github", "repo": "a/linter"}, "hooks": {"Stop": []}}),
                json!({"name": "notes", "source": {"source": "github", "repo": "a/notes"}}),
                json!({"name": "dictation", "source": {"source": "github", "repo": "m/d"},
                       "metadata": {"openremote": {"runtime": "js", "powers": ["microphone", "network"]}}}),
                json!({"name": "odd", "source": {"source": "pip", "package": "x"}}),
            ],
        };
        let views = entry_views(record, &catalog, None, &[], &[]);
        let by = |n: &str| views.iter().find(|v| v.name == n).unwrap();
        assert_eq!(by("linter").powers, Some(vec![PluginPower::RunPrograms]));
        assert!(by("linter").not_sandboxed);
        assert_eq!(by("notes").powers, None, "unknown until the review");
        assert_eq!(by("dictation").runtime, Some(PluginRuntime::Js));
        assert_eq!(
            by("dictation").powers,
            Some(vec![PluginPower::Network, PluginPower::Microphone])
        );
        assert!(!by("dictation").not_sandboxed);
        assert!(!by("odd").installable);
        assert_eq!(by("odd").id, "odd@claude-plugins-official");
    }

    #[test]
    fn a_catalog_at_the_top_of_a_folder_reads_with_its_plugin_root_above_every_source() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("marketplace.json"),
            json!({"name": "layout-tools", "pluginRoot": "plugins", "plugins": [
                {"name": "a", "source": "./a"},
                {"name": "b", "source": "b"},
                {"name": "c", "source": {"source": "github", "repo": "x/y", "path": "sub"}},
            ]})
            .to_string(),
        )
        .unwrap();
        let catalog = read_catalog(dir.path()).unwrap();
        assert_eq!(catalog.name, "layout-tools");
        let source = |i: usize| entry_source(&catalog.entries[i], None).unwrap();
        assert_eq!(source(0), EntrySource::Relative("plugins/a".into()));
        assert_eq!(source(1), EntrySource::Relative("plugins/b".into()));
        assert!(matches!(source(2), EntrySource::GitSubdir { ref path, .. } if path == "sub"));

        // Claude Code's own place wins where a folder has both.
        std::fs::create_dir_all(dir.path().join(".claude-plugin")).unwrap();
        std::fs::write(
            dir.path().join(".claude-plugin").join("marketplace.json"),
            json!({"name": "claude-side", "plugins": []}).to_string(),
        )
        .unwrap();
        assert_eq!(read_catalog(dir.path()).unwrap().name, "claude-side");
        // A marketplace.json given on its own names the folder it sits in.
        let top = dir.path().join("marketplace.json");
        assert!(matches!(
            parse_source(&top.to_string_lossy()).unwrap(),
            MarketplaceSource::Directory { ref path } if Path::new(path) == dir.path()
        ));
    }

    #[test]
    fn a_pinned_download_reads_and_an_unsafe_one_says_why() {
        let read = |v: Value| entry_source(&json!({"name": "x", "source": v}), None);
        let sha = "ab".repeat(32);
        assert_eq!(
            read(json!({"source": "url", "type": "zip", "url": "https://x/p.zip", "sha256": sha, "path": "p"}))
                .unwrap(),
            EntrySource::Archive {
                url: "https://x/p.zip".into(),
                sha256: Some(sha.clone()),
                path: Some("p".into())
            }
        );
        // This machine's own address may serve a mirror over plain http.
        assert!(
            read(json!({"source": "url", "type": "zip", "url": "http://127.0.0.1:9/p.zip", "sha256": sha}))
                .is_ok()
        );
        for (source, says) in [
            (
                json!({"source": "url", "type": "zip", "url": "https://x/p.zip"}),
                "sha256",
            ),
            (
                json!({"source": "url", "type": "zip", "url": "https://x/p.zip", "sha256": "ab"}),
                "64 hex",
            ),
            (
                json!({"source": "url", "type": "zip", "url": "http://example.com/p.zip", "sha256": sha}),
                "https",
            ),
            (
                json!({"source": "url", "type": "zip", "url": "https://x/p.zip", "sha256": sha, "path": "../p"}),
                "leaves",
            ),
            (
                json!({"source": "url", "type": "tar", "url": "https://x/p.tar", "sha256": sha}),
                "'tar'",
            ),
        ] {
            let why = read(source).unwrap_err();
            assert!(why.contains(says), "{why}");
        }
        // A git source without a type stays a git repository.
        assert!(matches!(
            read(json!({"source": "url", "url": "https://x/p.git"})).unwrap(),
            EntrySource::Url { .. }
        ));
    }

    #[test]
    fn the_publishers_own_catalog_name_is_honored_only_from_its_own_places() {
        let elsewhere = MarketplaceSource::Github {
            repo: "someone/plugins".into(),
            reference: None,
        };
        assert!(check_name(PUBLISHER_CATALOG, &elsewhere).is_err());
        let own_repo = MarketplaceSource::Github {
            repo: format!("{PUBLISHER_GITHUB}plugins"),
            reference: None,
        };
        assert!(check_name(PUBLISHER_CATALOG, &own_repo).is_ok());
        let own_host = MarketplaceSource::Url {
            url: format!(
                "https://cdn.{}/official/marketplace.json",
                PUBLISHER_HOSTS[0]
            ),
        };
        assert!(check_name(PUBLISHER_CATALOG, &own_host).is_ok());
        // A look-alike host is not the publisher's.
        let lookalike = MarketplaceSource::Url {
            url: format!(
                "https://{}.example.com/marketplace.json",
                PUBLISHER_HOSTS[0]
            ),
        };
        assert!(check_name(PUBLISHER_CATALOG, &lookalike).is_err());
    }
}
