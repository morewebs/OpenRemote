//! Running the plugins the user installed, beyond their harnesses (those are
//! main's `plugin_driver`). Fetching, staging and the install itself live in
//! `plugins.rs`; this is everything after: the OpenRemote part of a manifest
//! (`manifest`), process plugins' programs (`process`), WASM agent tools
//! (`wasm`), the user's settings (`settings`), who each plugin is for
//! (`scope`), placing and removing an approved plugin (`install`), the
//! account's Global plugins (`global`) and the routes that reach them
//! (`routes`).
//!
//! The trust boundary: nothing runs that the user didn't approve. What a
//! plugin may do is read from its install record - the powers and
//! contributions the user approved on the install screen - never from the
//! plugin's folder at call time. A program or module starts only for an
//! approved, enabled plugin, only when a chat or a view needs it, only from
//! a file inside the plugin's own folder, and only with the powers approved
//! at install: a process program needs `run_programs`, an agent tool needs
//! `agent_tools`, a WASM module's network functions need `network`. Who a
//! plugin is for never adds to that: a project's plugin runs only for that
//! project's chats, and a Global plugin another device installed reaches
//! this one as news to review, never as an install.

pub mod global;
pub mod install;
pub mod js;
pub mod manifest;
pub mod process;
pub mod routes;
pub mod scope;
pub mod settings;
pub mod wasm;

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex as StdMutex, Weak};
use std::time::Duration;

use openremote_core::{
    InstalledPlugin, PluginPower, PluginRuntime as Runtime, PluginScope, ProjectId, SessionId,
    Store,
};
use serde_json::{Map, Value, json};

use crate::plugins::{Manifest, PluginError};
use crate::supervisor::{Supervisor, lock_recovering};
use process::{RpcError, StartSpec};

/// A tool call waits this long by default; a tool may name its own.
const TOOL_TIME: Duration = Duration::from_secs(120);
/// A call from a plugin's view to its program.
const REQUEST_TIME: Duration = Duration::from_secs(60);

/// The chat a tool call comes from, as a plugin is told it.
#[derive(Clone, Debug)]
pub struct ChatInfo {
    pub id: SessionId,
    pub workspace: Option<PathBuf>,
}

/// An installed plugin at work, and its folder.
#[derive(Clone, Debug)]
pub struct Active {
    pub plugin: InstalledPlugin,
    pub dir: PathBuf,
}

pub struct PluginRuntime {
    data_dir: PathBuf,
    store: Arc<StdMutex<Store>>,
    supervisor: Weak<Supervisor>,
    pub processes: process::ProcessHost,
    pub wasm: wasm::WasmHost,
    pub settings: settings::SettingsStore,
    /// A Global plugin's folder packed for another device's copy, kept for
    /// the slices that follow the first.
    archive: tokio::sync::Mutex<Option<global::Packed>>,
}

/// At review: a WASM plugin's module compiles, exports what OpenRemote
/// calls and imports only what its powers allow.
pub fn check_staged(root: &Path, manifest: &Manifest) -> Result<(), PluginError> {
    if manifest.runtime != Runtime::Wasm {
        return Ok(());
    }
    let Some(entry) = manifest.entry.as_deref() else {
        return Ok(());
    };
    let path = manifest::confined(root, entry, "its module").map_err(PluginError::Invalid)?;
    wasm::check(&path, &manifest.powers).map_err(PluginError::Invalid)
}

/// A plugin about to be installed, as the name-clash check sees it.
pub struct Placing<'a> {
    pub id: &'a str,
    pub name: &'a str,
    pub marketplace: &'a str,
    pub scope: PluginScope,
    pub project: Option<ProjectId>,
    pub offers_tools: bool,
}

/// Why a plugin can't be installed beside one already here: an agent calls
/// a plugin's tools `<name>__<tool>`, so two plugins of one name that ride
/// the same chats would answer to the same tool names and one would quietly
/// take the other's place. Refused when either one offers tools. Installs
/// for two different projects never meet in a chat, so they never clash.
/// The same id is an update.
pub fn tool_name_clash(installed: &[&InstalledPlugin], new: &Placing) -> Option<String> {
    installed
        .iter()
        .find(|other| {
            other.name == new.name
                && other.id() != new.id
                && (new.offers_tools || !other.contributes.tools.is_empty())
                && scope::share_chats((new.scope, new.project), (other.scope, other.project))
        })
        .map(|other| {
            if other.marketplace == new.marketplace {
                let place = match other.scope {
                    PluginScope::Global => "for every device",
                    PluginScope::Machine => "for this machine",
                    PluginScope::ProjectLocal | PluginScope::Project => "for this project",
                };
                format!(
                    "{} is already installed {place}, so this project's chats have it already. Keep one of the two: remove that one first to install it here.",
                    new.name
                )
            } else {
                format!(
                    "Another plugin named {} is already installed (from {}). Their agent tools would share names, so remove that one first.",
                    new.name, other.marketplace
                )
            }
        })
}

impl PluginRuntime {
    pub fn new(
        data_dir: &Path,
        store: Arc<StdMutex<Store>>,
        supervisor: Weak<Supervisor>,
    ) -> Arc<Self> {
        Self::with_wasm(data_dir, store, supervisor, wasm::WasmHost::default())
    }

    /// The same, with its own WASM host (the tests' runs on less fuel).
    pub fn with_wasm(
        data_dir: &Path,
        store: Arc<StdMutex<Store>>,
        supervisor: Weak<Supervisor>,
        wasm: wasm::WasmHost,
    ) -> Arc<Self> {
        let runtime = Arc::new(Self {
            data_dir: data_dir.to_path_buf(),
            store,
            supervisor,
            processes: process::ProcessHost::default(),
            wasm,
            settings: settings::SettingsStore::new(data_dir),
            archive: tokio::sync::Mutex::new(None),
        });
        let weak = Arc::downgrade(&runtime);
        runtime
            .processes
            .set_requests(Arc::new(move |plugin, method, params| {
                let weak = weak.clone();
                Box::pin(async move {
                    match weak.upgrade() {
                        Some(runtime) => runtime.answer(&plugin, &method, params).await,
                        None => Err(RpcError::new(-32000, "OpenRemote is stopping")),
                    }
                })
            }));
        runtime
    }

    /// One installed plugin by id.
    pub fn plugin(&self, id: &str) -> Option<InstalledPlugin> {
        lock_recovering(&self.store)
            .installed_plugin(id)
            .ok()
            .cloned()
    }

    /// Where an installed plugin's files are: under the data dir for a
    /// Machine, Global or Project-local plugin, in the repo for a Project
    /// one; none when its project is gone.
    pub fn folder(&self, plugin: &InstalledPlugin) -> Option<PathBuf> {
        scope::folder(&self.data_dir, &lock_recovering(&self.store), plugin)
    }

    /// The enabled plugins a chat in `workspace` gets, each with its folder,
    /// in id order: the Global and Machine ones, and that project's own.
    /// With no workspace, only those that are for every chat.
    pub fn active(&self, workspace: Option<&Path>) -> Vec<Active> {
        scope::active(&lock_recovering(&self.store), &self.data_dir, workspace)
    }

    /// A plugin's own folder for what it keeps.
    pub fn data_folder(&self, id: &str) -> PathBuf {
        self.data_dir.join("plugin-data").join(id)
    }

    /// The plugin's settings as it is handed them: declared values, set or
    /// default, secrets included.
    pub fn effective_settings(&self, plugin: &InstalledPlugin) -> Map<String, Value> {
        settings::effective(
            &plugin.contributes.settings,
            &self.settings.load(&plugin.id()),
        )
    }

    /// Refuse to run a plugin whose required settings are unset, saying
    /// which ones.
    fn ready(&self, plugin: &InstalledPlugin) -> Result<(), String> {
        let missing = settings::missing_required(
            &plugin.contributes.settings,
            &self.settings.load(&plugin.id()),
        );
        if missing.is_empty() {
            Ok(())
        } else {
            Err(format!(
                "the plugin {} needs {} set first (Settings > Plugins > {})",
                plugin.name,
                missing.join(", "),
                plugin.name
            ))
        }
    }

    /// The checked `entry` and `contributes` of a plugin's folder as it
    /// is now. Only the entry is taken from it: what the plugin may do is
    /// the install record's.
    fn section(plugin: &InstalledPlugin, root: &Path) -> Result<manifest::Section, String> {
        let raw: Value = std::fs::read(root.join("openremote.json"))
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .ok_or_else(|| format!("{}'s openremote.json is unreadable", plugin.name))?;
        manifest::read_section(&raw, root, &plugin.name, plugin.runtime).map_err(|e| e.to_string())
    }

    /// A JavaScript plugin's own page, the file its entry names: what the
    /// console's frame for its composer buttons builds. Only the path is
    /// taken from the folder; what the page may do is the install record's.
    pub fn page(&self, plugin: &InstalledPlugin) -> Option<String> {
        if plugin.runtime != Runtime::Js {
            return None;
        }
        let root = self.folder(plugin)?;
        match Self::section(plugin, &root).ok()?.entry? {
            manifest::Entry::File(path) => Some(path),
            manifest::Entry::Command { .. } => None,
        }
    }

    /// What starts a process plugin's program. Refused unless the plugin is
    /// installed, on, a process plugin, approved to run programs, and set
    /// up; its program is a file inside its own folder.
    fn spec(&self, id: &str) -> Result<StartSpec, String> {
        let plugin = self
            .plugin(id)
            .ok_or_else(|| format!("{id} is not installed"))?;
        if !plugin.enabled {
            return Err(format!("{} is turned off", plugin.name));
        }
        if plugin.runtime != Runtime::Process {
            return Err(format!("{} does not run a program", plugin.name));
        }
        if !plugin.powers.contains(&PluginPower::RunPrograms) {
            return Err(format!(
                "{} was not given the power to run programs",
                plugin.name
            ));
        }
        self.ready(&plugin)?;
        let root = self
            .folder(&plugin)
            .ok_or_else(|| format!("{}'s project is gone", plugin.name))?;
        if !self.approved(&plugin) {
            return Err(format!(
                "{}'s files in the project changed since you approved them; review it again under Plugins",
                plugin.name
            ));
        }
        let entry = Self::section(&plugin, &root)?
            .entry
            .ok_or_else(|| format!("{} names no program", plugin.name))?;
        let (program, args) = entry.resolve(&root)?;
        Ok(StartSpec {
            id: id.to_string(),
            name: plugin.name.clone(),
            version: plugin.version.clone(),
            data: self.data_folder(id),
            root,
            program,
            args,
            settings: self.effective_settings(&plugin),
            powers: plugin.powers.clone(),
            scope: plugin.scope,
        })
    }

    /// Whether a plugin may run now: a Project plugin only while its repo
    /// still carries what was approved (its code runs from the snapshot).
    pub fn approved(&self, plugin: &InstalledPlugin) -> bool {
        scope::approved(&lock_recovering(&self.store), plugin)
    }

    /// A process plugin's running program, started if it isn't. A Project
    /// plugin whose repo copy changed since its review is stopped here,
    /// even mid-run: a view's or a tool's next call never reaches it.
    pub async fn process(&self, id: &str) -> Result<Arc<process::Conn>, String> {
        if let Some(plugin) = self.plugin(id) {
            if !self.approved(&plugin) {
                self.processes.stop(id).await;
                return Err(format!(
                    "{}'s files in the project changed since you approved them; review it again under Plugins",
                    plugin.name
                ));
            }
        }
        self.processes.connection(id, || self.spec(id)).await
    }

    /// The agent tools a chat in `workspace` lists, as MCP shapes them:
    /// those of the enabled plugins it gets that the user approved to give
    /// agents tools. Two of one name would give two tools one name: the
    /// first (by id) keeps it, and the clash is noted on stderr.
    pub fn tool_listing(&self, workspace: Option<&Path>) -> Vec<Value> {
        let mut seen = std::collections::BTreeSet::new();
        let mut out = Vec::new();
        for active in self.active(workspace) {
            let plugin = &active.plugin;
            if !plugin.powers.contains(&PluginPower::AgentTools)
                || !matches!(plugin.runtime, Runtime::Process | Runtime::Wasm)
            {
                continue;
            }
            for tool in &plugin.contributes.tools {
                let name = manifest::agent_tool_name(&plugin.name, &tool.name);
                if !seen.insert(name.clone()) {
                    eprintln!(
                        "plugin {}: its tool {name} is hidden behind another plugin's of the same name",
                        plugin.id()
                    );
                    continue;
                }
                out.push(json!({
                    "name": name,
                    "description": tool.description,
                    "inputSchema": tool.input_schema,
                    "annotations": {
                        "title": format!("{} ({})", tool.name, plugin.name),
                        "readOnlyHint": tool.read_only,
                    },
                }));
            }
        }
        out
    }

    /// Run one of a chat's plugin tools. Always answers an MCP tool
    /// result: a refusal is an error the agent reads.
    pub async fn call_tool(&self, chat: &ChatInfo, name: &str, arguments: Value) -> Value {
        let Some((plugin_name, tool_name)) = name.split_once("__") else {
            return error_result(&format!("there is no tool named {name}"));
        };
        // The same plugin the chat's listing named: the first by id that
        // offers it, among the plugins this chat gets.
        let found = self
            .active(chat.workspace.as_deref())
            .into_iter()
            .find(|a| {
                a.plugin.name == plugin_name
                    && a.plugin.powers.contains(&PluginPower::AgentTools)
                    && a.plugin
                        .contributes
                        .tools
                        .iter()
                        .any(|t| t.name == tool_name)
            });
        let Some(active) = found else {
            return error_result(&format!(
                "no plugin this chat has offers {name} (it may be off, removed, for another project, or not given the power to offer agents tools)"
            ));
        };
        let plugin = active.plugin;
        if let Err(why) = self.ready(&plugin) {
            return error_result(&why);
        }
        let Some(tool) = plugin
            .contributes
            .tools
            .iter()
            .find(|t| t.name == tool_name)
            .cloned()
        else {
            return error_result(&format!("there is no tool named {name}"));
        };
        let time = tool
            .timeout_secs
            .map(Duration::from_secs)
            .unwrap_or(TOOL_TIME);
        let chat_json = json!({"id": chat.id, "workspace": chat.workspace});
        match plugin.runtime {
            Runtime::Process => {
                let conn = match self.process(&plugin.id()).await {
                    Ok(conn) => conn,
                    Err(e) => {
                        return error_result(&format!("{} could not start: {e}", plugin.name));
                    }
                };
                match conn
                    .call(
                        "tools/call",
                        json!({"name": tool.name, "arguments": arguments, "chat": chat_json}),
                        time,
                    )
                    .await
                {
                    Ok(result) => tool_result(result),
                    Err(e) => error_result(&format!("{}: {e}", plugin.name)),
                }
            }
            Runtime::Wasm => {
                let path = match Self::section(&plugin, &active.dir).and_then(|s| match s.entry {
                    Some(manifest::Entry::File(path)) => {
                        manifest::confined(&active.dir, &path, "its module")
                    }
                    _ => Err("names no module".to_string()),
                }) {
                    Ok(path) => path,
                    Err(why) => return error_result(&format!("{}: {why}", plugin.name)),
                };
                let input = json!({
                    "tool": tool.name,
                    "arguments": arguments,
                    "settings": self.effective_settings(&plugin),
                    "chat": chat_json,
                });
                self.wasm
                    .call(&plugin.id(), &path, &plugin.powers, input, time)
                    .await
            }
            _ => error_result(&format!("{} can't run tools", plugin.name)),
        }
    }

    /// A call from one of a plugin's views (through the console) to its
    /// own program: the view's way to ask the plugin for its data.
    pub async fn request(&self, id: &str, name: &str, params: Value) -> Result<Value, String> {
        let plugin = self
            .plugin(id)
            .ok_or_else(|| format!("{id} is not installed"))?;
        if plugin.runtime != Runtime::Process {
            return Err(format!("{} has no program to ask", plugin.name));
        }
        let conn = self.process(id).await?;
        conn.call(
            "ui/request",
            json!({"name": name, "params": params}),
            REQUEST_TIME,
        )
        .await
        .map_err(|e| e.message)
    }

    /// Something about a plugin changed - installed, updated, turned on or
    /// off: its program stops, and the next need starts the new one.
    pub async fn changed(&self, id: &str) {
        self.processes.stop(id).await;
    }

    /// A removed plugin's program stops and its values go - secrets too.
    pub async fn removed(&self, id: &str) {
        self.processes.stop(id).await;
        self.settings.forget(id);
    }

    /// New settings reach a running program at once; a stopped one gets
    /// them when it starts.
    pub async fn settings_changed(&self, plugin: &InstalledPlugin) {
        if let Some(conn) = self.processes.running(&plugin.id()).await {
            let _ = conn
                .notify(
                    "settings/changed",
                    json!({"settings": self.effective_settings(plugin)}),
                )
                .await;
        }
    }

    /// The daemon is exiting: plugin programs go with it.
    pub async fn shutdown(&self) {
        self.processes.stop_all().await;
    }

    /// Whether a plugin may reach a chat in `workspace`: a Global or
    /// Machine plugin every chat, a project's plugin only that project's
    /// (the same rule that decides which chats it rides).
    fn reaches(&self, plugin: &InstalledPlugin, workspace: Option<&Path>) -> bool {
        if !plugin.scope.is_project() {
            return true;
        }
        let store = lock_recovering(&self.store);
        let chat_project = workspace.and_then(|w| scope::project_of(&store, w));
        scope::rides(plugin.scope, plugin.project, chat_project)
    }

    /// The chat a call names, refused when the plugin may not reach it.
    fn reachable_chat(&self, plugin: &InstalledPlugin, id: &SessionId) -> Result<(), RpcError> {
        let workspace = lock_recovering(&self.store)
            .session(id)
            .map(|s| s.workspace.clone())
            .map_err(|_| RpcError::new(-32602, "there is no such chat"))?;
        if self.reaches(plugin, workspace.as_deref()) {
            Ok(())
        } else {
            Err(RpcError::new(
                -32003,
                format!(
                    "{} is installed for one project, and that chat is outside it",
                    plugin.name
                ),
            ))
        }
    }

    /// A plugin's own call to OpenRemote: a process plugin's program over
    /// its pipe, a JavaScript plugin's frame through the console. Each
    /// method needs a power the user approved at install, read from the
    /// install record, and a project's plugin reaches only that project's
    /// chats.
    pub async fn answer(&self, id: &str, method: &str, params: Value) -> Result<Value, RpcError> {
        let plugin = self
            .plugin(id)
            .filter(|p| p.enabled)
            .ok_or_else(|| RpcError::new(-32000, "the plugin is not installed or is off"))?;
        let needs = |power: PluginPower, what: &str| {
            if plugin.powers.contains(&power) {
                Ok(())
            } else {
                Err(RpcError::new(
                    -32003,
                    format!("{what} needs a power this plugin wasn't given"),
                ))
            }
        };
        // Check the power before anything else, so a refusal says so
        // whatever else is wrong with the call.
        match method {
            "chats/list" | "chats/read" => needs(PluginPower::ReadChats, "reading chats")?,
            "chats/start" | "chats/prompt" | "chats/stop" => needs(
                PluginPower::AgentTools,
                "starting, steering or stopping chats",
            )?,
            other => {
                return Err(RpcError::new(
                    -32601,
                    format!("OpenRemote offers no method {other}"),
                ));
            }
        }
        let supervisor = self
            .supervisor
            .upgrade()
            .ok_or_else(|| RpcError::new(-32000, "OpenRemote is stopping"))?;
        let chat_id = |params: &Value| {
            params
                .get("chat")
                .and_then(Value::as_str)
                .and_then(SessionId::parse)
                .ok_or_else(|| RpcError::new(-32602, "name the chat by its id"))
        };
        let failed = |e: crate::supervisor::SupervisorError| RpcError::new(-32000, e.to_string());
        match method {
            "chats/list" => {
                let chats: Vec<Value> = supervisor
                    .sessions()
                    .into_iter()
                    .filter(|s| self.reaches(&plugin, s.workspace.as_deref()))
                    .map(|s| {
                        json!({
                            "id": s.id,
                            "title": s.title,
                            "harness": s.harness,
                            "model": s.model,
                            "status": s.status,
                            "workspace": s.workspace,
                            "updated_at": s.updated_at,
                        })
                    })
                    .collect();
                Ok(json!({"chats": chats}))
            }
            "chats/read" => {
                let id = chat_id(&params)?;
                self.reachable_chat(&plugin, &id)?;
                let text = supervisor.condensed_chat(&id).map_err(failed)?;
                Ok(json!({"text": text}))
            }
            "chats/start" => {
                let harness = params
                    .get("harness")
                    .and_then(Value::as_str)
                    .ok_or_else(|| RpcError::new(-32602, "name the harness"))?;
                let mut workspace = params
                    .get("workspace")
                    .and_then(Value::as_str)
                    .map(PathBuf::from);
                // A project's plugin starts its chats in its project.
                if workspace.is_none() && plugin.scope.is_project() {
                    workspace = plugin
                        .project
                        .and_then(|p| scope::project_folder(&lock_recovering(&self.store), p));
                }
                if let Some(w) = &workspace {
                    if !w.is_absolute() || !w.is_dir() {
                        return Err(RpcError::new(
                            -32602,
                            "the workspace must be a folder on this machine",
                        ));
                    }
                }
                if !self.reaches(&plugin, workspace.as_deref()) {
                    return Err(RpcError::new(
                        -32003,
                        format!(
                            "{} is installed for one project, and starts chats only there",
                            plugin.name
                        ),
                    ));
                }
                let text = |key: &str| params.get(key).and_then(Value::as_str).map(String::from);
                // A chat a plugin starts runs with the harness's own
                // default permissions - the plugin can't grant itself
                // more than a chat the user starts gets.
                let session = supervisor
                    .create_session(
                        harness,
                        workspace,
                        text("model"),
                        text("effort"),
                        None,
                        false,
                        None,
                        None,
                    )
                    .await
                    .map_err(failed)?;
                if let Some(prompt) = text("prompt") {
                    supervisor
                        .prompt(&session.id, &prompt)
                        .await
                        .map_err(failed)?;
                }
                Ok(json!({"chat": session.id}))
            }
            "chats/prompt" => {
                let id = chat_id(&params)?;
                self.reachable_chat(&plugin, &id)?;
                let text = params
                    .get("text")
                    .and_then(Value::as_str)
                    .ok_or_else(|| RpcError::new(-32602, "send some text"))?;
                supervisor.queue_add(&id, text).await.map_err(failed)?;
                Ok(json!({"queued": true}))
            }
            "chats/stop" => {
                let id = chat_id(&params)?;
                self.reachable_chat(&plugin, &id)?;
                supervisor.stop(&id).await.map_err(failed)?;
                Ok(json!({"stopped": true}))
            }
            _ => unreachable!("every method was matched above"),
        }
    }
}

/// A tool's answer as MCP shapes it: an MCP result passes as it is; a
/// bare string becomes its text.
pub fn tool_result(answer: Value) -> Value {
    match answer {
        Value::Object(ref fields) if fields.get("content").is_some_and(Value::is_array) => answer,
        Value::String(text) => json!({"content": [{"type": "text", "text": text}]}),
        other => json!({"content": [{"type": "text", "text": other.to_string()}]}),
    }
}

pub fn error_result(text: &str) -> Value {
    json!({"content": [{"type": "text", "text": text}], "isError": true})
}

#[cfg(test)]
mod tests {
    use super::*;
    use openremote_core::{PluginContributions, PluginSource, ToolContribution};

    /// A data dir whose store holds one installed process plugin, its folder
    /// with `openremote.json` and a program file.
    fn installed(
        powers: Vec<PluginPower>,
        scope: PluginScope,
        entry: Value,
    ) -> (tempfile::TempDir, Arc<PluginRuntime>, String) {
        let project = scope.is_project().then(ProjectId::new);
        let data = tempfile::tempdir().unwrap();
        let dir = crate::plugins::plugin_dir(data.path(), "local", "tools");
        std::fs::create_dir_all(dir.join("bin")).unwrap();
        std::fs::write(dir.join("bin/plugin"), "program").unwrap();
        std::fs::write(
            dir.join("openremote.json"),
            json!({
                "api": 1, "name": "tools", "runtime": "process", "entry": entry,
                "contributes": {"tools": [{"name": "t", "description": "d"}]}
            })
            .to_string(),
        )
        .unwrap();
        let mut store = Store::open(data.path().to_path_buf()).unwrap();
        let plugin = InstalledPlugin {
            name: "tools".into(),
            marketplace: "local".into(),
            resolved: None,
            command: None,
            version: None,
            description: None,
            runtime: Runtime::Process,
            powers,
            source: PluginSource::Folder {
                path: dir.to_string_lossy().into_owned(),
            },
            scope,
            project,
            contributes: PluginContributions {
                tools: vec![ToolContribution {
                    name: "t".into(),
                    description: "d".into(),
                    input_schema: json!({"type": "object"}),
                    read_only: false,
                    timeout_secs: None,
                }],
                ..Default::default()
            },
            harness: None,
            enabled: true,
            installed_at: 0,
            updated_at: 0,
        };
        let id = plugin.id();
        store.put_installed_plugin(plugin).unwrap();
        let runtime = PluginRuntime::new(data.path(), Arc::new(StdMutex::new(store)), Weak::new());
        (data, runtime, id)
    }

    fn program() -> Value {
        json!({"command": "bin/plugin", "args": []})
    }

    #[test]
    fn a_program_starts_only_with_the_approved_power_to_run_programs() {
        let all = vec![PluginPower::RunPrograms, PluginPower::AgentTools];
        let (_data, runtime, id) = installed(all.clone(), PluginScope::Machine, program());
        let spec = runtime.spec(&id).expect("an approved program starts");
        assert!(spec.program.ends_with("plugin"));
        // The install record, not the manifest, says what it may do: a
        // record without run_programs runs no program.
        let (_data, runtime, id) = installed(
            vec![PluginPower::AgentTools],
            PluginScope::Machine,
            program(),
        );
        let err = runtime.spec(&id).err().expect("refused");
        assert!(err.contains("power to run programs"), "{err}");
        // Off runs nothing.
        let (_data, runtime, id) = installed(all.clone(), PluginScope::Machine, program());
        lock_recovering(&runtime.store)
            .set_installed_plugin_enabled(&id, false)
            .unwrap();
        assert!(runtime.spec(&id).err().unwrap().contains("turned off"));
        // A project install whose project is gone runs nothing.
        let (_data, runtime, id) = installed(all, PluginScope::Project, program());
        let err = runtime.spec(&id).err().expect("refused");
        assert!(err.contains("project is gone"), "{err}");
    }

    #[test]
    fn a_folder_edited_after_install_cannot_point_its_program_outside() {
        let all = vec![PluginPower::RunPrograms, PluginPower::AgentTools];
        for entry in [
            json!({"command": "../../../../outside", "args": []}),
            json!({"command": "node", "args": []}),
            json!({"command": "/bin/sh", "args": []}),
        ] {
            let (_data, runtime, id) = installed(all.clone(), PluginScope::Machine, entry.clone());
            let err = runtime.spec(&id).err().expect("refused");
            assert!(err.contains("its program"), "{entry}: {err}");
        }
    }

    #[test]
    fn only_tools_of_plugins_approved_for_agent_tools_are_listed() {
        let (_data, runtime, _) = installed(
            vec![PluginPower::RunPrograms, PluginPower::AgentTools],
            PluginScope::Machine,
            program(),
        );
        let listed = runtime.tool_listing(None);
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0]["name"], "tools__t");
        let (_data, runtime, _) = installed(
            vec![PluginPower::RunPrograms],
            PluginScope::Machine,
            program(),
        );
        assert!(runtime.tool_listing(None).is_empty());
    }

    fn placing<'a>(
        id: &'a str,
        name: &'a str,
        marketplace: &'a str,
        scope: PluginScope,
        project: Option<ProjectId>,
        offers_tools: bool,
    ) -> Placing<'a> {
        Placing {
            id,
            name,
            marketplace,
            scope,
            project,
            offers_tools,
        }
    }

    #[test]
    fn a_second_plugin_of_the_same_name_with_tools_is_refused() {
        let (_data, runtime, id) = installed(
            vec![PluginPower::RunPrograms, PluginPower::AgentTools],
            PluginScope::Machine,
            program(),
        );
        let store = lock_recovering(&runtime.store);
        let installed = store.installed_plugins();
        let machine = PluginScope::Machine;
        // The same name from another marketplace: refused, naming the one
        // already here, whether the new one offers tools or not (the one
        // here does).
        for offers_tools in [true, false] {
            let new = placing("tools@other", "tools", "other", machine, None, offers_tools);
            let why = tool_name_clash(&installed, &new).expect("refused");
            assert!(why.contains("already installed (from local)"), "{why}");
        }
        // An update of the same plugin, or another name: fine.
        let update = placing(&id, "tools", "local", PluginScope::Global, None, true);
        assert!(tool_name_clash(&installed, &update).is_none());
        let other = placing("else@other", "else", "other", machine, None, true);
        assert!(tool_name_clash(&installed, &other).is_none());
        // Neither offers tools: side by side is fine.
        let mut bare = installed[0].clone();
        bare.contributes.tools.clear();
        let new = placing("tools@other", "tools", "other", machine, None, false);
        assert!(tool_name_clash(&[&bare], &new).is_none());
    }

    #[test]
    fn a_name_clash_is_judged_by_the_chats_the_two_would_share() {
        let (_data, runtime, _) = installed(
            vec![PluginPower::RunPrograms, PluginPower::AgentTools],
            PluginScope::Machine,
            program(),
        );
        let (one, two) = (ProjectId::new(), ProjectId::new());
        let mut store = lock_recovering(&runtime.store);
        // The same plugin from the same marketplace, for one project, while
        // this machine has it for every chat: refused, saying so in its own
        // words rather than as another marketplace's plugin.
        let for_one = format!("tools@local~{one}");
        let new = placing(
            &for_one,
            "tools",
            "local",
            PluginScope::Project,
            Some(one),
            true,
        );
        let why = tool_name_clash(&store.installed_plugins(), &new).expect("refused");
        assert!(!why.contains("from local"), "{why}");
        assert!(why.contains("already installed for this machine"), "{why}");
        // Installed for project one only: project two's install never
        // meets it in a chat, so both may stay.
        store.remove_installed_plugin("tools@local").unwrap();
        let mut first = super::scope::tests::record("tools", PluginScope::ProjectLocal, Some(one));
        first.contributes.tools = vec![ToolContribution {
            name: "t".into(),
            description: "d".into(),
            input_schema: json!({"type": "object"}),
            read_only: false,
            timeout_secs: None,
        }];
        store.put_installed_plugin(first).unwrap();
        let installed = store.installed_plugins();
        let for_two = format!("tools@other~{two}");
        let new = placing(
            &for_two,
            "tools",
            "other",
            PluginScope::Project,
            Some(two),
            true,
        );
        assert!(tool_name_clash(&installed, &new).is_none());
        // ...but the same project's chats would get both.
        let new = placing("x", "tools", "other", PluginScope::Project, Some(one), true);
        assert!(tool_name_clash(&installed, &new).is_some());
        // And so would every chat, for a Machine install.
        let new = placing(
            "tools@other",
            "tools",
            "other",
            PluginScope::Machine,
            None,
            true,
        );
        assert!(tool_name_clash(&installed, &new).is_some());
    }

    #[tokio::test]
    async fn a_plugins_own_calls_need_the_power_each_names() {
        let (_data, runtime, id) = installed(
            vec![PluginPower::RunPrograms],
            PluginScope::Machine,
            program(),
        );
        for method in [
            "chats/list",
            "chats/read",
            "chats/start",
            "chats/prompt",
            "chats/stop",
        ] {
            let err = runtime.answer(&id, method, json!({})).await.unwrap_err();
            assert_eq!(err.code, -32003, "{method}: {}", err.message);
        }
        let err = runtime.answer(&id, "fs/read", json!({})).await.unwrap_err();
        assert_eq!(err.code, -32601);
        // With the power the gate opens (this test has no supervisor, so
        // the call itself then stops there).
        let (_data, runtime, id) = installed(
            vec![PluginPower::RunPrograms, PluginPower::ReadChats],
            PluginScope::Machine,
            program(),
        );
        let err = runtime
            .answer(&id, "chats/list", json!({}))
            .await
            .unwrap_err();
        assert_eq!(err.code, -32000, "{}", err.message);
        let err = runtime
            .answer(&id, "chats/stop", json!({}))
            .await
            .unwrap_err();
        assert_eq!(err.code, -32003);
    }
}
