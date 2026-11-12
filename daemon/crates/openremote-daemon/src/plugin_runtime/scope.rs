//! Install scopes: who an installed plugin is for, where its files live,
//! and which chats get it.
//!
//! - Global and Machine plugins live in `<data dir>/plugins/<marketplace>/
//!   <name>/` and ride every chat on this machine. A Global one is also on
//!   the user's other devices, each installed there by its own approval.
//! - A Project-local plugin lives in `<data dir>/plugins/.projects/
//!   <project>/<marketplace>/<name>/`, outside the repo.
//! - A Project plugin lives in the repo, `<project folder>/.openremote/
//!   plugins/<name>/`, so whoever clones the repo has its files. Files are
//!   not approval: this machine runs exactly the content it approved (by
//!   content hash), and a plugin the repo brings, or changes, waits for the
//!   user's review.
//!
//! Project plugins ride only the chats whose folder is that project's
//! folder - the rule the sidebar groups chats by. A scope never changes
//! what a plugin may do: its powers are the install record's, the same for
//! every scope.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex as StdMutex, OnceLock};
use std::time::SystemTime;

use openremote_core::{InstalledPlugin, PluginScope, PluginSource, ProjectId, Store};
use serde::Serialize;

use super::Active;

/// The folder a project's own plugins sit in, inside the repo.
fn repo_plugins(project_folder: &Path) -> PathBuf {
    project_folder.join(".openremote").join("plugins")
}

/// The repo's note of where each of its plugins came from.
fn repo_index(project_folder: &Path) -> PathBuf {
    project_folder.join(".openremote").join("plugins.json")
}

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

pub fn repo_dir(project_folder: &Path, name: &str) -> PathBuf {
    repo_plugins(project_folder).join(name)
}

/// A project's folder, where its repo plugins hang.
pub fn project_folder(store: &Store, project: ProjectId) -> Option<PathBuf> {
    store.project(&project).ok()?.folders.first().cloned()
}

/// Where an installed plugin's files are; none when its project is gone or
/// a project install names no project.
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
        PluginScope::Project => Some(repo_dir(
            &project_folder(store, plugin.project?)?,
            &plugin.name,
        )),
    }
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
/// for a chat in a project - that project's enabled ones whose files are
/// still what the user approved. In id order.
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
        if !approved_files(plugin, &dir) {
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

/// Whether a plugin's folder holds what was approved: a repo plugin's
/// files are anyone's to change, so it runs only while they are exactly
/// the approved bytes. The other scopes' folders are OpenRemote's own.
pub fn approved_files(plugin: &InstalledPlugin, dir: &Path) -> bool {
    plugin.scope != PluginScope::Project || unchanged(dir, plugin.resolved.as_deref())
}

/// Whether a folder's files are exactly the approved ones.
pub fn unchanged(dir: &Path, approved: Option<&str>) -> bool {
    match (approved, cached_hash(dir)) {
        (Some(approved), Some(hash)) => approved == hash,
        _ => false,
    }
}

/// A cheap look at a folder: how many files, how many bytes, the newest
/// change. Equal looks reuse the last content hash instead of reading
/// every byte at each chat's start.
#[derive(PartialEq, Eq, Clone, Copy)]
struct Look {
    files: u64,
    bytes: u64,
    newest: Option<SystemTime>,
}

fn look(dir: &Path) -> Option<Look> {
    fn walk(dir: &Path, look: &mut Look) -> std::io::Result<()> {
        for entry in std::fs::read_dir(dir)? {
            let entry = entry?;
            if entry.file_name() == ".git" {
                continue;
            }
            let kind = entry.file_type()?;
            if kind.is_dir() {
                walk(&entry.path(), look)?;
            } else if kind.is_file() {
                let meta = entry.metadata()?;
                look.files += 1;
                look.bytes += meta.len();
                let modified = meta.modified().ok();
                if modified > look.newest {
                    look.newest = modified;
                }
            }
        }
        Ok(())
    }
    let mut out = Look {
        files: 0,
        bytes: 0,
        newest: None,
    };
    walk(dir, &mut out).ok()?;
    Some(out)
}

fn cached_hash(dir: &Path) -> Option<String> {
    static HASHES: OnceLock<StdMutex<HashMap<PathBuf, (Look, String)>>> = OnceLock::new();
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

/// Every registered project's repo plugins that wait for review.
pub fn found(store: &Store) -> Vec<Found> {
    let mut out = Vec::new();
    for project in store.projects() {
        let Some(folder) = project.folders.first() else {
            continue;
        };
        let Ok(entries) = std::fs::read_dir(repo_plugins(folder)) else {
            continue;
        };
        let mut dirs: Vec<PathBuf> = entries
            .flatten()
            .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
            .map(|e| e.path())
            .collect();
        dirs.sort();
        for dir in dirs {
            let Some(name) = dir.file_name().and_then(|n| n.to_str()).map(String::from) else {
                continue;
            };
            // A folder OpenRemote could never install is not offered.
            if !crate::plugins::valid_name(&name) {
                continue;
            }
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
    let raw = std::fs::read(repo_index(project_folder)).ok()?;
    let index: serde_json::Value = serde_json::from_slice(&raw).ok()?;
    index["plugins"][name]["marketplace"]
        .as_str()
        .map(String::from)
}

/// Note in the repo's index where one of its plugins came from, so a
/// teammate's review can say it too. A folder or file on this machine is
/// named only by its kind: a local path has no business in a repo.
pub fn note_in_repo_index(project_folder: &Path, plugin: &InstalledPlugin) -> std::io::Result<()> {
    let path = repo_index(project_folder);
    let source = match &plugin.source {
        PluginSource::Folder { .. } => serde_json::json!({"kind": "folder"}),
        PluginSource::Archive { location }
            if !(location.starts_with("https://") || location.starts_with("http://")) =>
        {
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
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(
        &path,
        serde_json::to_vec_pretty(&index).expect("an index serializes"),
    )
}

/// Drop a removed plugin from the repo's index.
pub fn forget_in_repo_index(project_folder: &Path, name: &str) {
    let path = repo_index(project_folder);
    let Some(mut index) = std::fs::read(&path)
        .ok()
        .and_then(|raw| serde_json::from_slice::<serde_json::Value>(&raw).ok())
    else {
        return;
    };
    if let Some(plugins) = index["plugins"].as_object_mut() {
        plugins.remove(name);
    }
    let _ = std::fs::write(
        &path,
        serde_json::to_vec_pretty(&index).expect("an index serializes"),
    );
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
        let repo = repo_dir(b_dir.path(), "for-b");
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
        std::fs::create_dir_all(repo_dir(folder.path(), "teammates")).unwrap();
        // A folder no plugin could be named is not offered.
        std::fs::create_dir_all(repo_dir(folder.path(), "Not A Name")).unwrap();
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
