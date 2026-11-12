//! Installing an approved plugin into the scope the user chose, and
//! removing one. Staging and the review are `plugins.rs`'s; this places the
//! approved folder where its scope keeps it, records it, and tells the rest
//! of OpenRemote.
//!
//! What the user chose about who a plugin is for never adds to what it may
//! do: the record's powers and contributions are the reviewed manifest's,
//! whatever the scope, and nothing in the request can name a power.

use std::path::PathBuf;
use std::sync::Arc;

use axum::http::StatusCode;
use openremote_core::{InstalledPlugin, PluginHarnessRef, PluginScope, ProjectId};

use super::{Placing, scope, tool_name_clash};
use crate::app::App;
use crate::plugins::{Manifest, Staged};
use crate::supervisor::lock_recovering;

pub type Failure = (StatusCode, String);

/// The scopes a staged plugin may install into. A plugin a command makes
/// is made again on this machine before each chat, so it stays on this
/// machine; a harness lives in every picker on the machine, so it can't
/// belong to one project.
pub fn allowed_scopes(manifest: &Manifest, from_command: bool) -> Vec<PluginScope> {
    if from_command {
        return vec![PluginScope::Machine];
    }
    let mut scopes = vec![PluginScope::Global, PluginScope::Machine];
    if manifest.harness.is_none() {
        scopes.push(PluginScope::ProjectLocal);
        scopes.push(PluginScope::Project);
    }
    scopes
}

/// Where a scope keeps a plugin's files.
fn target(
    app: &App,
    staged: &Staged,
    scope: PluginScope,
    project: Option<ProjectId>,
) -> Result<PathBuf, Failure> {
    match (scope, project) {
        (PluginScope::Global | PluginScope::Machine, _) => Ok(crate::plugins::plugin_dir(
            &app.data_dir,
            &staged.marketplace,
            &staged.name,
        )),
        (PluginScope::ProjectLocal, Some(project)) => Ok(scope::project_local_dir(
            &app.data_dir,
            project,
            &staged.marketplace,
            &staged.name,
        )),
        (PluginScope::Project, Some(project)) => {
            let folder = scope::project_folder(&lock_recovering(&app.store), project).ok_or((
                StatusCode::CONFLICT,
                "the project has no folder".to_string(),
            ))?;
            Ok(scope::repo_dir(&folder, &staged.name))
        }
        (_, None) => Err((
            StatusCode::BAD_REQUEST,
            "say which project it is for".to_string(),
        )),
    }
}

/// Place an approved plugin in its scope and record it. An install of an
/// id already here replaces it whole and keeps it on or off as it was.
pub async fn install(
    app: &Arc<App>,
    staged: Staged,
    scope: PluginScope,
    project: Option<ProjectId>,
) -> Result<InstalledPlugin, Failure> {
    let refuse = |staged: Staged, failure: Failure| {
        app.plugins.discard(staged);
        Err(failure)
    };
    let manifest = staged.manifest.clone();
    let from_command = staged.command.is_some();
    if !allowed_scopes(&manifest, from_command).contains(&scope) {
        let why = if from_command {
            "a plugin made by a command installs for this machine only"
        } else {
            "a plugin with an App of its own installs for this machine or for every device, not for one project"
        };
        return refuse(staged, (StatusCode::BAD_REQUEST, why.into()));
    }
    // A project scope names its project, and only a project scope keeps
    // one: without it the install would take this machine's own id.
    let project = match (scope.is_project(), project) {
        (true, Some(project)) => {
            if lock_recovering(&app.store).project(&project).is_err() {
                return refuse(staged, (StatusCode::NOT_FOUND, "no such project".into()));
            }
            Some(project)
        }
        (true, None) => {
            return refuse(
                staged,
                (
                    StatusCode::BAD_REQUEST,
                    "say which project it is for".into(),
                ),
            );
        }
        (false, _) => None,
    };
    let id = openremote_core::plugin_id(&staged.name, &staged.marketplace, scope, project);
    let (name, marketplace) = (staged.name.clone(), staged.marketplace.clone());
    let placing = Placing {
        id: &id,
        name: &name,
        marketplace: &marketplace,
        scope,
        project,
        offers_tools: !manifest.contributes.tools.is_empty(),
    };
    // A plugin update replaces the folder its driver runs from. On Windows
    // a live chat holds that folder open, so refuse with the chats named
    // instead of an access-denied from the filesystem.
    let chats = app.supervisor.plugin_chats(&id).await;
    if !chats.is_empty() {
        let why = format!(
            "stop {} running on this plugin's App before updating it",
            chats.join(", ")
        );
        return refuse(staged, (StatusCode::CONFLICT, why));
    }
    // Agent tools are named `<plugin>__<tool>`: two plugins of one name that
    // share chats would answer to the same names. Asked first so nothing is
    // touched, and again under the store lock right before the record.
    let clash = {
        let store = lock_recovering(&app.store);
        tool_name_clash(&store.installed_plugins(), &placing)
    };
    if let Some(why) = clash {
        return refuse(staged, (StatusCode::CONFLICT, why));
    }
    let target = match target(app, &staged, scope, project) {
        Ok(target) => target,
        Err(failure) => return refuse(staged, failure),
    };
    let before = lock_recovering(&app.store)
        .installed_plugin(&id)
        .ok()
        .cloned();
    let old_folder = before.as_ref().and_then(|b| app.plugin_runtime.folder(b));
    let (source, resolved, command) = (
        staged.source.clone(),
        staged.resolved.clone(),
        staged.command.clone(),
    );
    // Its program (if it runs one) holds the folder too: it stops before
    // the folder is replaced, and the next need starts the new version.
    app.plugin_runtime.changed(&id).await;
    let placed = match scope {
        PluginScope::Global | PluginScope::Machine => app.plugins.install(staged),
        PluginScope::ProjectLocal | PluginScope::Project => app.plugins.install_at(staged, &target),
    };
    placed.map_err(failure)?;
    // The placed folder's content: a repo plugin runs only while its files
    // are exactly these, and a Global one is copied as exactly these.
    let hash = {
        let target = target.clone();
        tokio::task::spawn_blocking(move || crate::plugins::content_hash(&target))
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
    };
    let now = openremote_core::now_ms();
    let harness = manifest.harness.as_ref().map(|h| PluginHarnessRef {
        id: h.id.clone(),
        name: h.name.clone(),
    });
    let plugin = {
        let mut store = lock_recovering(&app.store);
        if let Some(why) = tool_name_clash(&store.installed_plugins(), &placing) {
            // Another install got in while this one was placing its files.
            // A new folder of its own goes; an update's folder is its own.
            drop(store);
            if before.is_none() {
                let _ = std::fs::remove_dir_all(&target);
            }
            return Err((StatusCode::CONFLICT, why));
        }
        // What the review showed and the user approved, read from the
        // reviewed manifest: the scope adds nothing to it.
        let plugin = InstalledPlugin {
            name: name.clone(),
            marketplace: marketplace.clone(),
            resolved: if scope == PluginScope::Project {
                Some(hash.clone())
            } else {
                resolved
            },
            command,
            version: manifest.version.clone(),
            description: manifest.description.clone(),
            runtime: manifest.runtime,
            powers: manifest.powers.clone(),
            source,
            scope,
            project,
            contributes: manifest.contributes.clone(),
            harness,
            enabled: before.as_ref().is_none_or(|p| p.enabled),
            installed_at: before.as_ref().map_or(now, |p| p.installed_at),
            updated_at: now,
        };
        store
            .put_installed_plugin(plugin.clone())
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
        plugin
    };
    // Stopped again now the new folder and record are in place: a tool call
    // during the swap may have started the old program, and it must not
    // keep running the old version.
    app.plugin_runtime.changed(&id).await;
    // The same id moved between a project's two scopes leaves its old
    // folder behind; it goes.
    if let (Some(before), Some(old)) = (&before, old_folder) {
        if before.scope != scope && old != target {
            let _ = std::fs::remove_dir_all(&old);
            forget_in_repo(app, before);
        }
    }
    match scope {
        PluginScope::Global => super::global::publish(app, &plugin, hash, false).await,
        // Moved from every device to this machine alone: the other devices
        // let their copies go.
        _ if before
            .as_ref()
            .is_some_and(|b| b.scope == PluginScope::Global) =>
        {
            super::global::publish(app, &plugin, String::new(), true).await
        }
        _ => {}
    }
    if scope == PluginScope::Project {
        if let Some(folder) =
            project.and_then(|p| scope::project_folder(&lock_recovering(&app.store), p))
        {
            let _ = scope::note_in_repo_index(&folder, &plugin);
        }
    }
    // The plugin's App appears (or changes) without a restart.
    app.supervisor.reprobe_harnesses().await;
    Ok(plugin)
}

/// A plugin folder's failure as the routes answer it.
fn failure(err: crate::plugins::PluginError) -> Failure {
    use crate::plugins::PluginError;
    let status = match &err {
        PluginError::Invalid(_) | PluginError::Source(_) => StatusCode::BAD_REQUEST,
        PluginError::NotFound(_) => StatusCode::NOT_FOUND,
        PluginError::Io(_) => StatusCode::INTERNAL_SERVER_ERROR,
    };
    (status, err.to_string())
}

fn forget_in_repo(app: &App, plugin: &InstalledPlugin) {
    if plugin.scope != PluginScope::Project {
        return;
    }
    if let Some(folder) = plugin
        .project
        .and_then(|p| scope::project_folder(&lock_recovering(&app.store), p))
    {
        scope::forget_in_repo_index(&folder, &plugin.name);
    }
}

/// Uninstall: its files go (from the project's folder too, for a Project
/// plugin - the repo then shows the removal for the user to commit), its
/// record and settings go, and with `tell_account` a Global plugin goes
/// from every device.
pub async fn remove(app: &Arc<App>, id: &str, tell_account: bool) -> Result<(), Failure> {
    let plugin = lock_recovering(&app.store)
        .installed_plugin(id)
        .cloned()
        .map_err(|_| (StatusCode::NOT_FOUND, format!("{id} is not installed")))?;
    // A removal deletes the folder its driver runs from; a live chat holds
    // that folder open on Windows. Name the chats instead of failing later
    // with an access-denied.
    let chats = app.supervisor.plugin_chats(id).await;
    if !chats.is_empty() {
        return Err((
            StatusCode::CONFLICT,
            format!(
                "stop {} running on this plugin's App before removing it",
                chats.join(", ")
            ),
        ));
    }
    // Its program stops first: it holds the folder open too.
    app.plugin_runtime.changed(id).await;
    match plugin.scope {
        PluginScope::Global | PluginScope::Machine => app
            .plugins
            .uninstall(&plugin.marketplace, &plugin.name)
            .map_err(failure)?,
        PluginScope::ProjectLocal | PluginScope::Project => {
            if let Some(dir) = app
                .plugin_runtime
                .folder(&plugin)
                .filter(|_| crate::plugins::valid_name(&plugin.name))
            {
                match std::fs::remove_dir_all(&dir) {
                    Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
                        return Err((StatusCode::INTERNAL_SERVER_ERROR, e.to_string()));
                    }
                    _ => {}
                }
            }
        }
    }
    forget_in_repo(app, &plugin);
    lock_recovering(&app.store)
        .remove_installed_plugin(id)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    // Its settings go with it, secrets too.
    app.plugin_runtime.removed(id).await;
    if tell_account && plugin.scope == PluginScope::Global {
        super::global::publish(app, &plugin, String::new(), true).await;
    }
    app.supervisor.reprobe_harnesses().await;
    Ok(())
}

/// A repo plugin read for this machine's approval: the folder the project
/// carries, staged like any other folder.
pub fn project_source(
    app: &App,
    project: &str,
    name: &str,
) -> Result<crate::plugins::StageRequest, Failure> {
    let project =
        ProjectId::parse(project).ok_or((StatusCode::BAD_REQUEST, "bad project id".to_string()))?;
    let folder = scope::project_folder(&lock_recovering(&app.store), project)
        .ok_or((StatusCode::NOT_FOUND, "no such project".to_string()))?;
    if !crate::plugins::valid_name(name) {
        return Err((StatusCode::BAD_REQUEST, "not a plugin's folder name".into()));
    }
    let dir = scope::repo_dir(&folder, name);
    if !dir.is_dir() {
        return Err((
            StatusCode::NOT_FOUND,
            format!("the project has no plugin {name}"),
        ));
    }
    let marketplace = scope::repo_marketplace(&folder, name)
        .filter(|m| crate::plugins::plugin_dir_checked(&app.data_dir, m, name).is_ok())
        .unwrap_or_else(|| openremote_core::LOCAL_MARKETPLACE.to_string());
    Ok(crate::plugins::StageRequest {
        fetch: crate::plugins::Fetch::Folder(dir.clone()),
        marketplace,
        entry_name: Some(name.to_string()),
        entry: None,
        recorded: openremote_core::PluginSource::Folder {
            path: dir.to_string_lossy().into_owned(),
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest(harness: bool) -> Manifest {
        let dir = tempfile::tempdir().unwrap();
        let mut raw = serde_json::json!({"api": 1, "name": "shout", "runtime": "content"});
        if harness {
            raw["harness"] = serde_json::json!({
                "api": 1, "id": "shout", "name": "Shout", "driver": {"default": "bin/d"}
            });
            std::fs::create_dir_all(dir.path().join("bin")).unwrap();
            std::fs::write(dir.path().join("bin/d"), "x").unwrap();
        }
        std::fs::write(dir.path().join("openremote.json"), raw.to_string()).unwrap();
        crate::plugins::read_manifest(dir.path(), &[]).expect("a manifest")
    }

    #[test]
    fn a_harness_or_a_commands_plugin_is_never_for_one_project() {
        use PluginScope::*;
        assert_eq!(
            allowed_scopes(&manifest(false), false),
            [Global, Machine, ProjectLocal, Project]
        );
        assert_eq!(allowed_scopes(&manifest(false), true), [Machine]);
        assert_eq!(allowed_scopes(&manifest(true), false), [Global, Machine]);
    }
}
