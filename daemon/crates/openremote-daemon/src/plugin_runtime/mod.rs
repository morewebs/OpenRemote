//! Running the plugins the user installed, beyond their harnesses (those are
//! main's `plugin_driver`). Fetching, staging and the install itself live in
//! `plugins.rs`; this is everything after: the OpenRemote part of a manifest
//! (`manifest`), process plugins' programs (`process`), WASM agent tools
//! (`wasm`), the user's settings (`settings`) and the routes that reach them
//! (`routes`).
//!
//! The trust boundary: nothing runs that the user didn't approve. What a
//! plugin may do is read from its install record - the powers and
//! contributions the user approved on the install screen - never from the
//! plugin's folder at call time. A program or module starts only for an
//! approved, enabled plugin, only when a chat or a view needs it, only from
//! a file inside the plugin's own folder, and only with the powers approved
//! at install: a process program needs `run_programs`, an agent tool needs
//! `agent_tools`, a WASM module's network functions need `network`.

pub mod manifest;
pub mod process;
pub mod routes;
pub mod settings;
pub mod wasm;

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex as StdMutex, Weak};
use std::time::Duration;

use openremote_core::{
    InstalledPlugin, PluginPower, PluginRuntime as Runtime, PluginScope, SessionId, Store,
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

/// Why a plugin can't be installed beside one already here: an agent calls
/// a plugin's tools `<name>__<tool>`, so two plugins of one name from
/// different sources would answer to the same tool names and one would
/// quietly take the other's place. Refused when either one offers tools.
/// `id` is the plugin being installed; the same id is an update.
pub fn tool_name_clash(
    installed: &[&InstalledPlugin],
    id: &str,
    name: &str,
    offers_tools: bool,
) -> Option<String> {
    installed
        .iter()
        .find(|other| {
            other.name == name
                && other.id() != id
                && (offers_tools || !other.contributes.tools.is_empty())
        })
        .map(|other| {
            format!(
                "Another plugin named {name} is already installed (from {}). Their agent tools would share names, so remove that one first.",
                other.marketplace
            )
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

    /// Where an installed plugin's files are. A Machine (or Global) plugin
    /// lives under the data dir; the project scopes' folders arrive with
    /// the scopes (plan step 5), so until then such a plugin runs nothing.
    pub fn folder(&self, plugin: &InstalledPlugin) -> Option<PathBuf> {
        match plugin.scope {
            PluginScope::Machine | PluginScope::Global => Some(crate::plugins::plugin_dir(
                &self.data_dir,
                &plugin.marketplace,
                &plugin.name,
            )),
            PluginScope::ProjectLocal | PluginScope::Project => None,
        }
    }

    /// The enabled plugins a chat gets, each with its folder, in id order.
    pub fn active(&self) -> Vec<Active> {
        let plugins: Vec<InstalledPlugin> = lock_recovering(&self.store)
            .installed_plugins()
            .into_iter()
            .filter(|p| p.enabled)
            .cloned()
            .collect();
        let mut out: Vec<Active> = plugins
            .into_iter()
            .filter_map(|plugin| {
                let dir = self.folder(&plugin)?;
                Some(Active { plugin, dir })
            })
            .collect();
        out.sort_by_key(|a| a.plugin.id());
        out
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
            .ok_or_else(|| format!("{} runs nothing for a project yet", plugin.name))?;
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

    /// A process plugin's running program, started if it isn't.
    pub async fn process(&self, id: &str) -> Result<Arc<process::Conn>, String> {
        self.processes.connection(id, || self.spec(id)).await
    }

    /// The agent tools a chat lists, as MCP shapes them: those of the
    /// enabled plugins the user approved to give agents tools. Two enabled
    /// plugins of one name would give two tools one name: the first (by
    /// id) keeps it, and the clash is noted on stderr.
    pub fn tool_listing(&self) -> Vec<Value> {
        let mut seen = std::collections::BTreeSet::new();
        let mut out = Vec::new();
        for active in self.active() {
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
        // The same plugin the listing named: the first by id that offers it.
        let found = self.active().into_iter().find(|a| {
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
                "no plugin this chat has offers {name} (it may be off, removed, or not given the power to offer agents tools)"
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

    /// A plugin program's own call to OpenRemote. Each method needs a power
    /// the user approved at install, read from the install record.
    async fn answer(&self, id: &str, method: &str, params: Value) -> Result<Value, RpcError> {
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
                let text = supervisor.condensed_chat(&id).map_err(failed)?;
                Ok(json!({"text": text}))
            }
            "chats/start" => {
                let harness = params
                    .get("harness")
                    .and_then(Value::as_str)
                    .ok_or_else(|| RpcError::new(-32602, "name the harness"))?;
                let workspace = params
                    .get("workspace")
                    .and_then(Value::as_str)
                    .map(PathBuf::from);
                if let Some(w) = &workspace {
                    if !w.is_absolute() || !w.is_dir() {
                        return Err(RpcError::new(
                            -32602,
                            "the workspace must be a folder on this machine",
                        ));
                    }
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
                let text = params
                    .get("text")
                    .and_then(Value::as_str)
                    .ok_or_else(|| RpcError::new(-32602, "send some text"))?;
                supervisor.queue_add(&id, text).await.map_err(failed)?;
                Ok(json!({"queued": true}))
            }
            "chats/stop" => {
                let id = chat_id(&params)?;
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
            project: None,
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
        // A project install has no folder here until the scopes land.
        let (_data, runtime, _) = installed(all, PluginScope::Project, program());
        let id = lock_recovering(&runtime.store).installed_plugins()[0].id();
        assert!(runtime.spec(&id).is_err());
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
        let listed = runtime.tool_listing();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0]["name"], "tools__t");
        let (_data, runtime, _) = installed(
            vec![PluginPower::RunPrograms],
            PluginScope::Machine,
            program(),
        );
        assert!(runtime.tool_listing().is_empty());
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
        // The same name from another marketplace: refused, naming the one
        // already here, whether the new one offers tools or not (the one
        // here does).
        for offers_tools in [true, false] {
            let why =
                tool_name_clash(&installed, "tools@other", "tools", offers_tools).expect("refused");
            assert!(why.contains("already installed (from local)"), "{why}");
        }
        // An update of the same plugin, or another name: fine.
        assert!(tool_name_clash(&installed, &id, "tools", true).is_none());
        assert!(tool_name_clash(&installed, "else@other", "else", true).is_none());
        // Neither offers tools: side by side is fine.
        let mut bare = installed[0].clone();
        bare.contributes.tools.clear();
        assert!(tool_name_clash(&[&bare], "tools@other", "tools", false).is_none());
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
