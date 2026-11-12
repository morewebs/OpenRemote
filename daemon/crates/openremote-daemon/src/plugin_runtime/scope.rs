//! Install scopes: who an installed plugin is for, where its files live,
//! and which chats get it.
//!
//! - Global and Machine plugins live in `<data dir>/plugins/<marketplace>/
//!   <name>/` and ride every chat on this machine. A Global one is also on
//!   the user's other devices, each installed there by its own approval.
//! - A Project-local plugin lives in `<data dir>/plugins/.projects/
//!   <project>/<marketplace>/<name>/`, outside the repo.
//! - A Project plugin is also written into the repo, `<project folder>/
//!   .openremote/plugins/<name>/`, so whoever clones the repo has its files.
//!   Files in a repo are anyone's to change, so they never run: what runs
//!   is OpenRemote's private snapshot of the bytes the user reviewed
//!   (`<data dir>/plugins/.projects/<project>/repo/<name>/`). The repo copy
//!   only tells whether the project now carries something else, which then
//!   waits for the user's review and stops the plugin until then.
//!
//! Project plugins ride only the chats whose folder is that project's
//! folder - the rule the sidebar groups chats by. A scope never changes
//! what a plugin may do: its powers are the install record's, the same for
//! every scope.
//!
//! OpenRemote writes and deletes in a repo only through paths none of whose
//! parts (`.openremote`, `.openremote/plugins`, the plugin's folder,
//! `plugins.json`) is a link, and only inside the project's own folder.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex as StdMutex, OnceLock};

use openremote_core::{InstalledPlugin, PluginScope, PluginSource, ProjectId, Store};
use serde::Serialize;
use sha2::Digest;

use super::Active;

pub fn project_local_dir(
    data_dir: &Path,
    project: ProjectId,
    marketplace: &str,
    name: &str,
) -> PathBuf {
    data_dir
        .join("plugins")
        .join(".projects")
        .join(project.to_string())
        .join(marketplace)
        .join(name)
}

/// A Project plugin's private snapshot: the reviewed bytes, which are what
/// runs. It sits under `.repo`, beside the Project-local plugins'
/// marketplace folders.
pub fn snapshot_dir(data_dir: &Path, project: ProjectId, name: &str) -> PathBuf {
    data_dir
        .join("plugins")
        .join(".projects")
        .join(project.to_string())
        .join(".repo")
        .join(name)
}

/// A project's folder, where its repo plugins hang.
pub fn project_folder(store: &Store, project: ProjectId) -> Option<PathBuf> {
    store.project(&project).ok()?.folders.first().cloned()
}

/// Whether a path is a link: a symlink, or on Windows a junction. Rust's
/// `is_symlink` answers true there for every name-surrogate reparse point,
/// which is what a link is. Other reparse points are not refused: OneDrive's
/// Files-On-Demand and other cloud placeholders are reparse points that
/// lead nowhere else, and projects under Documents are often made of them.
/// A path that isn't there is not a link.
pub(crate) fn is_link(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok_and(|meta| meta.file_type().is_symlink())
}

/// A path under a project's folder, checked before OpenRemote writes,
/// deletes or reads through it: none of `parts` from the project folder
/// down is a link, and what exists of it resolves inside the project.
fn repo_path(project_folder: &Path, parts: &[&str]) -> Result<PathBuf, String> {
    let root = std::fs::canonicalize(project_folder)
        .map_err(|e| format!("the project's folder can't be read: {e}"))?;
    let mut path = project_folder.to_path_buf();
    for part in parts {
        path.push(part);
        if is_link(&path) {
            return Err(format!(
                "{} in the project is a link, so OpenRemote won't write or delete through it",
                path.display()
            ));
        }
        if let Ok(real) = std::fs::canonicalize(&path) {
            if !real.starts_with(&root) {
                return Err(format!("{} leads outside the project", path.display()));
            }
        }
    }
    Ok(path)
}

/// The repo's folder of one plugin, checked (see `repo_path`).
pub fn repo_dir(project_folder: &Path, name: &str) -> Result<PathBuf, String> {
    if !crate::plugins::valid_name(name) {
        return Err(format!("'{name}' is not a plugin's folder name"));
    }
    repo_path(project_folder, &[".openremote", "plugins", name])
}

/// The repo's note of where each of its plugins came from, checked.
fn repo_index(project_folder: &Path) -> Result<PathBuf, String> {
    repo_path(project_folder, &[".openremote", "plugins.json"])
}

/// Where an installed plugin's files run from; none when its project is
/// gone or a project install names no project. For a Project plugin that is
/// its private snapshot, never the repo.
pub fn folder(data_dir: &Path, store: &Store, plugin: &InstalledPlugin) -> Option<PathBuf> {
    match plugin.scope {
        PluginScope::Global | PluginScope::Machine => Some(crate::plugins::plugin_dir(
            data_dir,
            &plugin.marketplace,
            &plugin.name,
        )),
        PluginScope::ProjectLocal => Some(project_local_dir(
            data_dir,
            plugin.project?,
            &plugin.marketplace,
            &plugin.name,
        )),
        PluginScope::Project => {
            let project = plugin.project?;
            store.project(&project).ok()?;
            Some(snapshot_dir(data_dir, project, &plugin.name))
        }
    }
}

/// A Project plugin's copy in its repo, checked; none for the other scopes.
pub fn repo_copy(store: &Store, plugin: &InstalledPlugin) -> Option<Result<PathBuf, String>> {
    if plugin.scope != PluginScope::Project {
        return None;
    }
    let folder = project_folder(store, plugin.project?)?;
    Some(repo_dir(&folder, &plugin.name))
}

fn same_folder(a: &Path, b: &Path) -> bool {
    if a == b {
        return true;
    }
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

/// The registered project whose folder a chat runs in.
pub fn project_of(store: &Store, workspace: &Path) -> Option<ProjectId> {
    store
        .projects()
        .into_iter()
        .find(|p| p.folders.iter().any(|f| same_folder(f, workspace)))
        .map(|p| p.id)
}

/// Whether a plugin of `scope` (for `project`) rides a chat in
/// `chat_project`: Global and Machine ones ride every chat, a project's
/// only that project's chats - never a chat outside any project.
pub fn rides(
    scope: PluginScope,
    project: Option<ProjectId>,
    chat_project: Option<ProjectId>,
) -> bool {
    match scope {
        PluginScope::Global | PluginScope::Machine => true,
        PluginScope::ProjectLocal | PluginScope::Project => {
            project.is_some() && project == chat_project
        }
    }
}

/// Whether two installs could ever ride the same chat: two project
/// installs only when they are for the same project.
pub fn share_chats(
    a: (PluginScope, Option<ProjectId>),
    b: (PluginScope, Option<ProjectId>),
) -> bool {
    if a.0.is_project() && b.0.is_project() {
        a.1.is_some() && a.1 == b.1
    } else {
        true
    }
}

/// The plugins that may run for a chat in `workspace` (or, with none, for
/// this machine as a whole): every enabled Global and Machine plugin, and -
/// for a chat in a project - that project's enabled ones whose repo still
/// carries what the user approved. Each runs from its own folder (a Project
/// plugin from its snapshot). In id order.
pub fn active(store: &Store, data_dir: &Path, workspace: Option<&Path>) -> Vec<Active> {
    let chat_project = workspace.and_then(|w| project_of(store, w));
    let mut out = Vec::new();
    for plugin in store.installed_plugins() {
        if !plugin.enabled || !rides(plugin.scope, plugin.project, chat_project) {
            continue;
        }
        let Some(dir) = folder(data_dir, store, plugin) else {
            continue;
        };
        if !approved(store, plugin) {
            continue;
        }
        out.push(Active {
            plugin: plugin.clone(),
            dir,
        });
    }
    out.sort_by_key(|a| a.plugin.id());
    out
}

/// Whether a plugin may run: a Project plugin only while its repo copy is
/// what was approved (a changed one waits for a new review). Its code runs
/// from the snapshot either way; this is the repo's say on whether it runs
/// at all. The other scopes' folders are OpenRemote's own.
pub fn approved(store: &Store, plugin: &InstalledPlugin) -> bool {
    if plugin.scope != PluginScope::Project {
        return true;
    }
    // A project that is gone, or a repo path that is a link, approves
    // nothing.
    match repo_copy(store, plugin) {
        Some(Ok(dir)) => unchanged(&dir, plugin.resolved.as_deref()),
        _ => false,
    }
}

/// Whether a folder's files are exactly the approved ones.
pub fn unchanged(dir: &Path, approved: Option<&str>) -> bool {
    match (approved, cached_hash(dir)) {
        (Some(approved), Some(hash)) => approved == hash,
        _ => false,
    }
}

/// A cheap look at a folder: every file's path, size and change time, as one
/// digest. An equal look reuses the last content hash instead of reading
/// every byte at each chat's start. A change that keeps a file's size and
/// time can pass a look; it decides only whether a repo still carries the
/// approved plugin, never what runs (the snapshot does).
fn look(dir: &Path) -> Option<String> {
    fn walk(root: &Path, dir: &Path, files: &mut Vec<(String, u64, u128)>) -> std::io::Result<()> {
        for entry in std::fs::read_dir(dir)? {
            let entry = entry?;
            if entry.file_name() == ".git" {
                continue;
            }
            let kind = entry.file_type()?;
            if kind.is_dir() {
                walk(root, &entry.path(), files)?;
            } else if kind.is_file() {
                let meta = entry.metadata()?;
                let when = meta
                    .modified()
                    .ok()
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map_or(0, |d| d.as_nanos());
                let path = entry.path();
                let relative = path.strip_prefix(root).unwrap_or(&path);
                files.push((relative.to_string_lossy().into_owned(), meta.len(), when));
            }
        }
        Ok(())
    }
    if is_link(dir) {
        return None;
    }
    let mut files = Vec::new();
    walk(dir, dir, &mut files).ok()?;
    files.sort();
    let mut hasher = sha2::Sha256::new();
    for (path, len, when) in files {
        hasher.update(path.as_bytes());
        hasher.update([0]);
        hasher.update(len.to_le_bytes());
        hasher.update(when.to_le_bytes());
    }
    Some(crate::plugins::hex(&hasher.finalize()))
}

fn cached_hash(dir: &Path) -> Option<String> {
    static HASHES: OnceLock<StdMutex<HashMap<PathBuf, (String, String)>>> = OnceLock::new();
    let now = look(dir)?;
    let cache = HASHES.get_or_init(Default::default);
    if let Some((seen, hash)) = crate::supervisor::lock_recovering(cache).get(dir) {
        if *seen == now {
            return Some(hash.clone());
        }
    }
    let hash = crate::plugins::content_hash(dir).ok()?;
    crate::supervisor::lock_recovering(cache).insert(dir.to_path_buf(), (now, hash.clone()));
    Some(hash)
}

/// A plugin a project's repo carries that this machine hasn't approved,
/// or whose files changed since it was approved.
#[derive(Serialize, Debug, PartialEq)]
pub struct Found {
    pub project: ProjectId,
    pub name: String,
    /// `new` (never approved here) or `changed` (the repo's files differ
    /// from what was approved).
    pub why: &'static str,
    /// The installed plugin it would replace, for a changed one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
}

/// Every registered project's repo plugins that wait for review. A repo
/// whose plugins folder is a link is not read.
pub fn found(store: &Store) -> Vec<Found> {
    let mut out = Vec::new();
    for project in store.projects() {
        let Some(folder) = project.folders.first() else {
            continue;
        };
        let Ok(plugins) = repo_path(folder, &[".openremote", "plugins"]) else {
            continue;
        };
        let Ok(entries) = std::fs::read_dir(plugins) else {
            continue;
        };
        let mut names: Vec<String> = entries
            .flatten()
            .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()) && !is_link(&e.path()))
            .filter_map(|e| e.file_name().to_str().map(String::from))
            // A folder OpenRemote could never install is not offered.
            .filter(|name| crate::plugins::valid_name(name))
            .collect();
        names.sort();
        for name in names {
            let Ok(dir) = repo_dir(folder, &name) else {
                continue;
            };
            let approved = store.installed_plugins().into_iter().find(|p| {
                p.scope == PluginScope::Project && p.project == Some(project.id) && p.name == name
            });
            let why = match approved {
                None => "new",
                Some(p) if !unchanged(&dir, p.resolved.as_deref()) => "changed",
                Some(_) => continue,
            };
            out.push(Found {
                project: project.id,
                name,
                why,
                id: approved.map(InstalledPlugin::id),
            });
        }
    }
    out
}

/// The marketplace a repo's index says one of its plugins came from.
pub fn repo_marketplace(project_folder: &Path, name: &str) -> Option<String> {
    let raw = std::fs::read(repo_index(project_folder).ok()?).ok()?;
    let index: serde_json::Value = serde_json::from_slice(&raw).ok()?;
    index["plugins"][name]["marketplace"]
        .as_str()
        .map(String::from)
}

/// A source as another place may hear of it: a folder or file on this
/// machine is named only by its kind, and a link's sign-in words are left
/// out.
pub fn shareable_source(source: &PluginSource) -> PluginSource {
    match source {
        PluginSource::Folder { .. } => PluginSource::Folder {
            path: String::new(),
        },
        PluginSource::Archive { location }
            if !(location.starts_with("https://") || location.starts_with("http://")) =>
        {
            PluginSource::Archive {
                location: String::new(),
            }
        }
        PluginSource::Archive { location } => PluginSource::Archive {
            location: without_userinfo(location),
        },
        PluginSource::Git { url } => PluginSource::Git {
            url: without_userinfo(url),
        },
        other => other.clone(),
    }
}

fn without_userinfo(url: &str) -> String {
    let Some((scheme, rest)) = url.split_once("://") else {
        return url.to_string();
    };
    let host_end = rest.find('/').unwrap_or(rest.len());
    match rest[..host_end].rfind('@') {
        Some(at) => format!("{scheme}://{}", &rest[at + 1..]),
        None => url.to_string(),
    }
}

/// Write a repo file whole, through a temporary file beside it and a
/// rename, so a reader never sees half of it.
fn write_whole(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let temp = path.with_file_name(format!(".plugins.json.{}", uuid::Uuid::new_v4()));
    std::fs::write(&temp, bytes)?;
    std::fs::rename(&temp, path).inspect_err(|_| {
        let _ = std::fs::remove_file(&temp);
    })
}

/// Note in the repo's index where one of its plugins came from, so a
/// teammate's review can say it too.
pub fn note_in_repo_index(project_folder: &Path, plugin: &InstalledPlugin) -> Result<(), String> {
    let path = repo_index(project_folder)?;
    let source = match shareable_source(&plugin.source) {
        PluginSource::Folder { .. } => serde_json::json!({"kind": "folder"}),
        PluginSource::Archive { location } if location.is_empty() => {
            serde_json::json!({"kind": "archive"})
        }
        other => serde_json::to_value(other).unwrap_or_default(),
    };
    let mut index: serde_json::Value = std::fs::read(&path)
        .ok()
        .and_then(|raw| serde_json::from_slice(&raw).ok())
        .unwrap_or_else(|| serde_json::json!({"plugins": {}}));
    if !index["plugins"].is_object() {
        index["plugins"] = serde_json::json!({});
    }
    index["plugins"][&plugin.name] = serde_json::json!({
        "marketplace": plugin.marketplace,
        "version": plugin.version,
        "source": source,
    });
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    // Asked again now the folder exists, in case it was made a link
    // between the two looks.
    let path = repo_index(project_folder)?;
    write_whole(
        &path,
        &serde_json::to_vec_pretty(&index).expect("an index serializes"),
    )
    .map_err(|e| e.to_string())
}

/// Drop a removed plugin from the repo's index.
pub fn forget_in_repo_index(project_folder: &Path, name: &str) {
    let Ok(path) = repo_index(project_folder) else {
        return;
    };
    let Some(mut index) = std::fs::read(&path)
        .ok()
        .and_then(|raw| serde_json::from_slice::<serde_json::Value>(&raw).ok())
    else {
        return;
    };
    if let Some(plugins) = index["plugins"].as_object_mut() {
        plugins.remove(name);
    }
    let _ = write_whole(
        &path,
        &serde_json::to_vec_pretty(&index).expect("an index serializes"),
    );
}

/// Put a plugin's reviewed files into its repo folder: a fresh copy beside
/// it, then swapped in. Refused when any part of the path is a link.
pub fn write_repo_copy(project_folder: &Path, name: &str, from: &Path) -> Result<(), String> {
    let target = repo_dir(project_folder, name)?;
    let parent = target.parent().expect("a plugin folder has a parent");
    std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    // Made now if it wasn't there: asked again.
    let target = repo_dir(project_folder, name)?;
    let fresh = parent.join(format!(".{name}.new-{}", uuid::Uuid::new_v4()));
    crate::plugins::copy_folder(from, &fresh).map_err(|e| e.to_string())?;
    crate::plugins::place(&fresh, &target).map_err(|e| {
        let _ = std::fs::remove_dir_all(&fresh);
        e.to_string()
    })
}

/// Delete a plugin's repo folder. Refused when any part of the path is a
/// link: nothing outside the project is deleted through it.
pub fn delete_repo_copy(project_folder: &Path, name: &str) -> Result<(), String> {
    let dir = repo_dir(project_folder, name)?;
    match std::fs::remove_dir_all(&dir) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.to_string()),
        _ => Ok(()),
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use openremote_core::{PluginContributions, PluginPower, PluginRuntime as Runtime, Project};

    pub(crate) fn record(
        name: &str,
        scope: PluginScope,
        project: Option<ProjectId>,
    ) -> InstalledPlugin {
        InstalledPlugin {
            name: name.into(),
            marketplace: "local".into(),
            resolved: None,
            command: None,
            version: None,
            description: None,
            runtime: Runtime::Wasm,
            powers: vec![PluginPower::AgentTools],
            source: PluginSource::Folder {
                path: "/somewhere".into(),
            },
            scope,
            project,
            contributes: PluginContributions::default(),
            harness: None,
            enabled: true,
            installed_at: 0,
            updated_at: 0,
        }
    }

    fn project(store: &mut Store, folder: &Path) -> ProjectId {
        let id = ProjectId::new();
        store
            .save_project(Project {
                id,
                folders: vec![folder.to_path_buf()],
                created_at: 0,
                updated_at: 0,
            })
            .unwrap();
        id
    }

    /// Only a real link is refused: a plain folder or file is not one, and
    /// neither is a cloud placeholder (OneDrive's Files-On-Demand keeps
    /// Documents as reparse points that are not links; one can't be made in
    /// a test, so the rule is pinned in the source).
    #[test]
    fn only_a_real_link_is_a_link() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("file"), "x").unwrap();
        assert!(!is_link(dir.path()));
        assert!(!is_link(&dir.path().join("file")));
        assert!(!is_link(&dir.path().join("missing")));
        let outside = tempfile::tempdir().unwrap();
        #[cfg(windows)]
        {
            let made = std::process::Command::new("cmd")
                .args(["/C", "mklink", "/J"])
                .arg(dir.path().join("j"))
                .arg(outside.path())
                .output()
                .is_ok_and(|o| o.status.success());
            assert!(made);
        }
        #[cfg(unix)]
        std::os::unix::fs::symlink(outside.path(), dir.path().join("j")).unwrap();
        assert!(is_link(&dir.path().join("j")));
        let source = include_str!("scope.rs");
        let body = &source[source.find("fn is_link(").unwrap()..];
        let body = &body[..body.find("\n}").unwrap()];
        assert!(!body.contains("0x400"), "any reparse point is not a link");
        let plugins = include_str!("../plugins.rs");
        assert!(!plugins.contains("& 0x400"), "nor in place()");
    }

    #[test]
    fn a_source_told_elsewhere_names_no_local_path_or_sign_in() {
        let folder = PluginSource::Folder {
            path: "C:/Users/me/secret-work/plugin".into(),
        };
        assert_eq!(
            shareable_source(&folder),
            PluginSource::Folder {
                path: String::new()
            }
        );
        let file = PluginSource::Archive {
            location: "/home/me/p.zip".into(),
        };
        assert_eq!(
            shareable_source(&file),
            PluginSource::Archive {
                location: String::new()
            }
        );
        let git = PluginSource::Git {
            url: "https://me:token@example.test/owner/p.git".into(),
        };
        assert_eq!(
            shareable_source(&git),
            PluginSource::Git {
                url: "https://example.test/owner/p.git".into()
            }
        );
    }

    #[test]
    fn a_project_plugin_rides_only_its_own_projects_chats() {
        let (one, two) = (ProjectId::new(), ProjectId::new());
        assert!(rides(PluginScope::Machine, None, None));
        assert!(rides(PluginScope::Global, None, Some(one)));
        for scope in [PluginScope::Project, PluginScope::ProjectLocal] {
            assert!(rides(scope, Some(one), Some(one)));
            assert!(!rides(scope, Some(one), Some(two)), "another project");
            assert!(!rides(scope, Some(one), None), "no project");
            assert!(
                !rides(scope, None, None),
                "a malformed record rides nothing"
            );
        }
        assert!(!share_chats(
            (PluginScope::Project, Some(one)),
            (PluginScope::ProjectLocal, Some(two))
        ));
        assert!(share_chats(
            (PluginScope::Project, Some(one)),
            (PluginScope::Machine, None)
        ));
    }

    #[test]
    fn a_chat_gets_its_projects_plugins_and_no_other_projects() {
        let data = tempfile::tempdir().unwrap();
        let (a_dir, b_dir, loose) = (
            tempfile::tempdir().unwrap(),
            tempfile::tempdir().unwrap(),
            tempfile::tempdir().unwrap(),
        );
        let mut store = Store::open(data.path().to_path_buf()).unwrap();
        let a = project(&mut store, a_dir.path());
        let b = project(&mut store, b_dir.path());
        store
            .put_installed_plugin(record("everywhere", PluginScope::Machine, None))
            .unwrap();
        store
            .put_installed_plugin(record("for-a", PluginScope::ProjectLocal, Some(a)))
            .unwrap();
        // A repo plugin of b's, its files exactly what was approved.
        let repo = repo_dir(b_dir.path(), "for-b").unwrap();
        std::fs::create_dir_all(&repo).unwrap();
        std::fs::write(repo.join("openremote.json"), "{}").unwrap();
        let mut for_b = record("for-b", PluginScope::Project, Some(b));
        for_b.resolved = Some(crate::plugins::content_hash(&repo).unwrap());
        store.put_installed_plugin(for_b).unwrap();

        let names = |workspace: Option<&Path>| -> Vec<String> {
            active(&store, data.path(), workspace)
                .into_iter()
                .map(|a| a.plugin.name)
                .collect()
        };
        assert_eq!(names(Some(a_dir.path())), ["everywhere", "for-a"]);
        assert_eq!(names(Some(b_dir.path())), ["everywhere", "for-b"]);
        assert_eq!(names(Some(loose.path())), ["everywhere"]);
        assert_eq!(names(None), ["everywhere"]);
        // It runs from OpenRemote's snapshot, never the repo: what claude
        // loads and what a program starts from.
        let dir = active(&store, data.path(), Some(b_dir.path()))
            .into_iter()
            .find(|a| a.plugin.name == "for-b")
            .unwrap()
            .dir;
        assert_eq!(dir, snapshot_dir(data.path(), b, "for-b"));
        assert!(!dir.starts_with(b_dir.path()));

        // b's repo plugin changed after its approval: it stops riding.
        std::fs::write(repo.join("openremote.json"), r#"{"changed": 1}"#).unwrap();
        assert_eq!(names(Some(b_dir.path())), ["everywhere"]);
        let waiting = found(&store);
        assert_eq!(waiting.len(), 1);
        assert_eq!((waiting[0].why, waiting[0].project), ("changed", b));
    }

    #[test]
    fn a_repo_plugin_never_approved_here_waits_for_review() {
        let data = tempfile::tempdir().unwrap();
        let folder = tempfile::tempdir().unwrap();
        let mut store = Store::open(data.path().to_path_buf()).unwrap();
        let p = project(&mut store, folder.path());
        std::fs::create_dir_all(repo_dir(folder.path(), "teammates").unwrap()).unwrap();
        // A folder no plugin could be named is not offered.
        std::fs::create_dir_all(folder.path().join(".openremote/plugins/Not A Name")).unwrap();
        assert_eq!(
            found(&store),
            vec![Found {
                project: p,
                name: "teammates".into(),
                why: "new",
                id: None
            }]
        );
        // Nothing runs from it: no record, nothing active.
        assert!(active(&store, data.path(), Some(folder.path())).is_empty());
    }
}
