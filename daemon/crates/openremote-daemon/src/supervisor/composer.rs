//! The composer's side of the supervisor: what `/` and `@` offer for a
//! chat - the harness's own commands and agents (asked once per workspace
//! and cached, asked again when the plugins change or a running chat
//! reports a different list), the installed plugins' parts, the MCP
//! servers that ride - plus the chats a chat was pointed at with @, and
//! the read-a-chat MCP server those reads go through.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use openremote_core::{Event, EventPayload, MessageRole, Session, SessionId};
use openremote_harness::CommandCatalog;
use serde::Serialize;

use super::{Supervisor, SupervisorError, lock_recovering, now_ms, store_error};

/// Why a plugin's command or agent is shown but can't be picked.
const NOT_HERE: &str = "needs a harness that runs plugins";

/// A read-a-chat answer is cut to this many characters, keeping its end.
const READ_LIMIT: usize = 40_000;

/// One harness's command list for one workspace, as last asked.
pub struct CachedCommands {
    catalog: Option<CommandCatalog>,
    /// What the list depends on: the harness's version and the installed
    /// plugins. A different stamp asks again.
    stamp: String,
    /// A running chat reported a different list, or the last ask failed.
    stale: bool,
    error: Option<String>,
}

/// Where the read-a-chat MCP server comes from: this binary, and the
/// API's loopback port it calls back on.
#[derive(Clone)]
pub struct Controls {
    exe: PathBuf,
    port: u16,
}

/// A command the / menu offers.
#[derive(Serialize, Debug)]
pub struct CommandEntry {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub argument_hint: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub aliases: Vec<String>,
    /// `harness` (its own) or `plugin` (an installed plugin's).
    pub source: &'static str,
    /// `command` or `skill`: a harness lists the user's whole skill library
    /// beside its commands, and the / menu keeps the two apart.
    pub kind: &'static str,
    /// The plugin's id, for a plugin's command.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plugin: Option<String>,
    pub available: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub why: Option<String>,
}

/// An agent the @ menu offers.
#[derive(Serialize, Debug)]
pub struct AgentEntry {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub source: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plugin: Option<String>,
    pub available: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub why: Option<String>,
}

/// An MCP server the @ menu offers by name (`@server:`).
#[derive(Serialize, Debug, PartialEq, Eq)]
pub struct McpServerName {
    pub name: String,
    /// `settings` (set up by hand) or `plugin`.
    pub source: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plugin: Option<String>,
}

/// What the composer's menus offer for one harness in one workspace.
#[derive(Serialize, Debug)]
pub struct CommandsView {
    pub harness: String,
    pub runs_plugins: bool,
    pub commands: Vec<CommandEntry>,
    pub agents: Vec<AgentEntry>,
    pub mcp_servers: Vec<McpServerName>,
    /// Why the harness's own list couldn't be asked; what an earlier ask
    /// found still shows.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// The tools a harness runs without asking. Codex pre-approves by tool
/// (`approval_mode = "approve"`) and its chats are asked before a note goes
/// to their lead (`agents_report`), which changes another chat.
pub(super) fn allowed_for(harness: &str, names: Vec<String>) -> Vec<String> {
    if harness != "codex" {
        return names;
    }
    names
        .into_iter()
        .filter(|n| !n.ends_with("__agents_report"))
        .collect()
}

/// Whether a harness's MCP client re-lists tools on
/// `notifications/tools/list_changed`, checked against each App's client:
/// Claude Code does, for a server that declares `tools.listChanged`.
pub(super) fn stages_tools(harness: &str) -> bool {
    harness == "claude"
}

/// The name a plugin's commands carry: its Claude manifest's own name.
fn manifest_name(dir: &Path) -> Option<String> {
    let raw = std::fs::read(dir.join(".claude-plugin").join("plugin.json")).ok()?;
    let value: serde_json::Value = serde_json::from_slice(&raw).ok()?;
    value.get("name")?.as_str().map(String::from)
}

impl Supervisor {
    /// Chats get OpenRemote's read-a-chat MCP server from here on.
    pub fn set_controls(&self, exe: PathBuf, port: u16) {
        let _ = self.controls.set(Controls { exe, port });
    }

    /// The MCP servers a chat starts with: the hand-written ones that ride
    /// this machine, and OpenRemote's read-a-chat server for this chat.
    /// The server reads the token from the data folder itself, so the
    /// token never appears on a command line. `--port` is the port this
    /// daemon is on now, kept as the fallback for a one-release mismatch:
    /// the server prefers the port the daemon publishes in that same data
    /// folder, because a restart moves it and the chat outlives us.
    ///
    /// Claude Code's server is `--staged`: it lists tools as the chat comes
    /// to need them and says when the list grew, because Claude Code lists
    /// again on `notifications/tools/list_changed`. Codex holds that behind a
    /// feature still under development and off by default, and a plugin's
    /// App may ignore it, so they are listed everything from the start: a
    /// tool they could never be told about would be a feature lost.
    pub(super) fn mcp_servers_for(
        &self,
        id: &SessionId,
        harness: &str,
    ) -> Vec<openremote_harness::McpServer> {
        let mut servers = self.mcp_servers();
        if let Some(controls) = self.controls.get() {
            let data_dir = self.with_store(|s| s.dir().to_path_buf());
            let mut args = vec![
                "mcp-chat".into(),
                "--session".into(),
                id.to_string(),
                "--port".into(),
                controls.port.to_string(),
                "--data-dir".into(),
                data_dir.to_string_lossy().into_owned(),
            ];
            if stages_tools(harness) {
                args.push("--staged".into());
            }
            servers.push(openremote_harness::McpServer {
                id: "openremote".into(),
                command: controls.exe.to_string_lossy().into_owned(),
                args,
            });
        }
        servers
    }

    /// The tools a chat runs without asking: OpenRemote's browser tools
    /// that only read (reading, hovering, scrolling, switching tabs), named
    /// as its MCP server's tools.
    pub(super) fn allowed_tools(&self, harness: &str) -> Vec<String> {
        if self.controls.get().is_some() {
            let mut names = crate::browser::tools::reading_tool_names("openremote");
            names.extend(crate::team::reading_tool_names("openremote"));
            allowed_for(harness, names)
        } else {
            Vec::new()
        }
    }

    /// What `/` and `@` offer for a chat on `harness` in `workspace`.
    pub async fn commands(
        &self,
        harness: &str,
        workspace: Option<PathBuf>,
    ) -> Result<CommandsView, SupervisorError> {
        let (backend, version) = {
            let registry = self.registry.read().expect("registry lock");
            let Some(known) = registry
                .harnesses()
                .iter()
                .find(|h| h.id == harness)
                .cloned()
            else {
                return Err(SupervisorError::Harness(format!(
                    "harness '{harness}' is not known"
                )));
            };
            (registry.backend(harness).cloned(), known.version)
        };
        let Some(backend) = backend else {
            return Err(SupervisorError::Harness(format!(
                "'{harness}' is not installed on this machine"
            )));
        };
        let cwd = workspace.unwrap_or_else(Self::home_dir);
        let (plugins, data_dir) = self.with_store(|s| {
            let plugins: Vec<openremote_core::InstalledPlugin> = s
                .installed_plugins()
                .into_iter()
                .filter(|p| p.enabled)
                .cloned()
                .collect();
            (plugins, s.dir().to_path_buf())
        });
        // Each enabled plugin: its id, its folder, the name its commands carry.
        let folders: Vec<(String, PathBuf, String)> = plugins
            .iter()
            .map(|p| {
                let dir = crate::plugins::plugin_dir(&data_dir, &p.marketplace, &p.name);
                let prefix = manifest_name(&dir).unwrap_or_else(|| p.name.clone());
                (p.id(), dir, prefix)
            })
            .collect();
        let runs_plugins = backend.runs_plugins();
        let plugin_dirs: Vec<PathBuf> = if runs_plugins {
            folders
                .iter()
                .map(|(_, dir, _)| dir.clone())
                .filter(|dir| dir.join(".claude-plugin").join("plugin.json").is_file())
                .collect()
        } else {
            Vec::new()
        };
        let stamp = format!(
            "{}|{}",
            version.unwrap_or_default(),
            plugins
                .iter()
                .map(|p| format!("{}:{}", p.id(), p.resolved.clone().unwrap_or_default()))
                .collect::<Vec<_>>()
                .join(",")
        );
        let key = (harness.to_string(), cwd.clone());
        let listed = lock_recovering(&self.listed_commands).get(&key).cloned();
        let hit = lock_recovering(&self.command_cache)
            .get(&key)
            .filter(|c| c.stamp == stamp && !c.stale)
            .map(|c| (c.catalog.clone(), c.error.clone()));
        let (catalog, error) = match hit {
            Some(hit) => hit,
            None => {
                let (catalog, error) = match backend.commands(&cwd, &plugin_dirs).await {
                    Ok(catalog) => (catalog, None),
                    // A failed ask keeps what an earlier one found: an old
                    // list beats none. Marked stale, the next ask retries.
                    Err(e) => {
                        let earlier = lock_recovering(&self.command_cache)
                            .get(&key)
                            .and_then(|c| c.catalog.clone());
                        (earlier, Some(e.to_string()))
                    }
                };
                lock_recovering(&self.command_cache).insert(
                    key,
                    CachedCommands {
                        catalog: catalog.clone(),
                        stamp,
                        stale: error.is_some(),
                        error: error.clone(),
                    },
                );
                (catalog, error)
            }
        };
        // A harness with no list of its own still names its commands when a
        // chat starts (grok's init). The names a chat in this folder listed
        // last stand in, without descriptions, so / offers what the harness
        // said it runs instead of nothing.
        let catalog = catalog.or_else(|| listed.map(|names| listed_catalog(&names)));
        let settings_servers: Vec<String> = self.mcp_servers().into_iter().map(|s| s.id).collect();
        Ok(build_view(
            harness,
            runs_plugins,
            catalog.unwrap_or_default(),
            &folders,
            settings_servers,
            error,
            &local_skills(&cwd),
        ))
    }

    /// A running chat listed its commands as it started: a name the cached
    /// list doesn't have means the list is behind, so the next ask asks
    /// the harness again. The names are also kept, for a harness that has
    /// no list of its own to ask.
    pub(super) fn note_command_names(&self, id: &SessionId, names: &[String]) {
        let Ok(session) = self.session(id) else {
            return;
        };
        let key = (
            session.harness.clone(),
            session.workspace.clone().unwrap_or_else(Self::home_dir),
        );
        lock_recovering(&self.listed_commands).insert(key.clone(), names.to_vec());
        let mut cache = lock_recovering(&self.command_cache);
        let Some(entry) = cache.get_mut(&key) else {
            return;
        };
        let known: BTreeSet<&str> = entry
            .catalog
            .iter()
            .flat_map(|c| c.commands.iter())
            .flat_map(|c| {
                std::iter::once(c.name.as_str()).chain(c.aliases.iter().map(String::as_str))
            })
            .collect();
        if names.iter().any(|n| !known.contains(n.as_str())) {
            entry.stale = true;
        }
    }

    /// The user pointed this chat at other chats with @: remember them, so
    /// its agent may read those - and only those.
    pub fn note_mentions(
        &self,
        id: &SessionId,
        chats: &[SessionId],
    ) -> Result<(), SupervisorError> {
        // A mention note is a write like any other. On a copy it would take
        // the sequence number the machine that runs the chat will use for its
        // next event, and the copy's pull starts after that number, so the
        // copy would never fetch it: the guard sits above the empty-list
        // shortcut because the refusal must not depend on whether the user
        // typed an @ this time.
        self.ensure_here(id)?;
        if chats.is_empty() {
            return Ok(());
        }
        let mut session = self.session(id)?;
        let mut changed = false;
        for chat in chats {
            if chat != id && !session.mentioned_chats.contains(chat) {
                session.mentioned_chats.push(*chat);
                changed = true;
            }
        }
        if changed {
            session.updated_at = now_ms();
            self.emit_all(id, vec![EventPayload::SessionUpdated { session }])?;
        }
        Ok(())
    }

    /// Rename a chat: the name every device shows, in place of the first
    /// line of its first prompt.
    pub fn rename(&self, id: &SessionId, title: &str) -> Result<Session, SupervisorError> {
        // The name is an event, so a rename on a copy is the one write a copy
        // could have taken as its own: it would sit at the sequence number the
        // machine that runs the chat uses for its next turn, the copy's pull
        // would start after it and never fetch that turn, and the copy would
        // stop being a prefix of the runner's log. Every sibling write path
        // asks this first; so does the store, which is the second net under
        // it, but the answer there is a store error rather than the sentence
        // the console knows how to show.
        self.ensure_here(id)?;
        let title = title.trim();
        if title.is_empty() {
            return Err(SupervisorError::Conflict(
                "a chat's name can't be empty".into(),
            ));
        }
        let mut session = self.session(id)?;
        session.title = Some(title.chars().take(200).collect());
        session.updated_at = now_ms();
        self.emit_all(
            id,
            vec![EventPayload::SessionUpdated {
                session: session.clone(),
            }],
        )?;
        self.session(id)
    }

    /// The read-a-chat tool's answer: a chat this one was pointed at,
    /// named by its id (or the start of it), condensed to its messages.
    pub fn read_mentioned(
        &self,
        reader: &SessionId,
        wanted: &str,
    ) -> Result<String, SupervisorError> {
        let session = self.session(reader)?;
        let wanted = wanted.trim().trim_start_matches("chat:");
        let target = session
            .mentioned_chats
            .iter()
            .find(|c| {
                let id = c.to_string();
                id == wanted || (wanted.len() >= 6 && id.starts_with(wanted))
            })
            .copied()
            .ok_or_else(|| {
                SupervisorError::Conflict(format!(
                    "this chat was not pointed at {wanted}; the user points a chat at another with @"
                ))
            })?;
        self.condensed_chat(&target)
    }

    /// A chat condensed to its messages, as an agent or a plugin with the
    /// power to read chats is handed it.
    pub fn condensed_chat(&self, id: &SessionId) -> Result<String, SupervisorError> {
        let chat = self.session(id)?;
        let events = self
            .with_store(|s| s.events_after(id, None))
            .map_err(store_error)?;
        Ok(condensed(&chat, &events, None))
    }
}

/// The skills a harness lists as commands, by folder name: the user's own
/// (`~/.claude/skills`) and the project's (`.claude/skills`). Plugins'
/// skills are known from their own folders.
fn local_skills(cwd: &std::path::Path) -> BTreeSet<String> {
    let mut found = BTreeSet::new();
    let roots = [
        Supervisor::home_dir().join(".claude").join("skills"),
        cwd.join(".claude").join("skills"),
    ];
    for root in roots {
        let Ok(entries) = std::fs::read_dir(&root) else {
            continue;
        };
        for entry in entries.flatten() {
            if entry.path().join("SKILL.md").is_file() {
                found.insert(entry.file_name().to_string_lossy().into_owned());
            }
        }
    }
    found
}

/// The / and @ lists: the harness's own entries (a plugin's when its name
/// carries an installed plugin's prefix), then each enabled plugin's own
/// parts the harness didn't list - dimmed where the harness runs no
/// plugins - and the MCP servers that ride.
/// A catalog of names alone, from the list a running chat reported. The
/// harness said nothing more about them, so nothing more is claimed.
fn listed_catalog(names: &[String]) -> CommandCatalog {
    CommandCatalog {
        commands: names
            .iter()
            .map(|name| openremote_harness::CommandDescriptor {
                name: name.clone(),
                description: None,
                argument_hint: None,
                aliases: Vec::new(),
            })
            .collect(),
        agents: Vec::new(),
    }
}

fn build_view(
    harness: &str,
    runs_plugins: bool,
    catalog: CommandCatalog,
    folders: &[(String, PathBuf, String)],
    settings_servers: Vec<String>,
    error: Option<String>,
    skills: &BTreeSet<String>,
) -> CommandsView {
    // Every plugin's own parts, read once: which of its names are skills.
    let parts: Vec<(String, crate::plugins::PluginParts)> = folders
        .iter()
        .map(|(id, dir, prefix)| (id.clone(), crate::plugins::plugin_parts(dir, prefix)))
        .collect();
    let plugin_skills: BTreeSet<&str> = parts
        .iter()
        .flat_map(|(_, p)| p.skills.iter().map(String::as_str))
        .collect();
    let plugin_commands: BTreeSet<&str> = parts
        .iter()
        .flat_map(|(_, p)| p.commands.iter().map(|c| c.name.as_str()))
        .filter(|n| !plugin_skills.contains(n))
        .collect();
    // A plugin-scoped name the harness lists from a plugin of its own is a
    // skill unless an installed plugin says it's a command file.
    let kind = |name: &str| {
        if skills.contains(name)
            || plugin_skills.contains(name)
            || (name.contains(':') && !plugin_commands.contains(name))
        {
            "skill"
        } else {
            "command"
        }
    };
    let owner = |name: &str| {
        folders
            .iter()
            .find(|(_, _, prefix)| name.starts_with(&format!("{prefix}:")))
            .map(|(id, _, _)| id.clone())
    };
    let unavailable = (!runs_plugins).then(|| NOT_HERE.to_string());
    let mut seen_commands = BTreeSet::new();
    let mut commands = Vec::new();
    for c in catalog.commands {
        seen_commands.insert(c.name.clone());
        let plugin = owner(&c.name);
        commands.push(CommandEntry {
            source: if plugin.is_some() {
                "plugin"
            } else {
                "harness"
            },
            kind: kind(&c.name),
            plugin,
            available: true,
            why: None,
            name: c.name,
            description: c.description,
            argument_hint: c.argument_hint,
            aliases: c.aliases,
        });
    }
    let mut seen_agents = BTreeSet::new();
    let mut agents = Vec::new();
    for a in catalog.agents {
        seen_agents.insert(a.name.clone());
        let plugin = owner(&a.name);
        agents.push(AgentEntry {
            source: if plugin.is_some() {
                "plugin"
            } else {
                "harness"
            },
            plugin,
            available: true,
            why: None,
            name: a.name,
            description: a.description,
        });
    }
    let mut mcp_servers: Vec<McpServerName> = settings_servers
        .into_iter()
        .filter(|name| name != "openremote")
        .map(|name| McpServerName {
            name,
            source: "settings",
            plugin: None,
        })
        .collect();
    for (id, parts) in parts {
        for c in parts.commands {
            if seen_commands.insert(c.name.clone()) {
                commands.push(CommandEntry {
                    source: "plugin",
                    kind: if parts.skills.contains(&c.name) {
                        "skill"
                    } else {
                        "command"
                    },
                    plugin: Some(id.clone()),
                    available: runs_plugins,
                    why: unavailable.clone(),
                    name: c.name,
                    description: c.description,
                    argument_hint: c.argument_hint,
                    aliases: c.aliases,
                });
            }
        }
        for a in parts.agents {
            if seen_agents.insert(a.name.clone()) {
                agents.push(AgentEntry {
                    source: "plugin",
                    plugin: Some(id.clone()),
                    available: runs_plugins,
                    why: unavailable.clone(),
                    name: a.name,
                    description: a.description,
                });
            }
        }
        for name in parts.mcp_servers {
            mcp_servers.push(McpServerName {
                name,
                source: "plugin",
                plugin: Some(id.clone()),
            });
        }
    }
    CommandsView {
        harness: harness.to_string(),
        runs_plugins,
        commands,
        agents,
        mcp_servers,
        error,
    }
}

/// A chat as the read-a-chat tool hands it to an agent: its name and
/// harness, then its messages in order, each tool call a one-line note.
/// A message another agent sent says so. With `tail`, only the last that
/// many messages (the tool notes between them ride along). Past the limit
/// the oldest part is cut - the end of a chat is the part an agent
/// pointed at it most likely needs.
pub(super) fn condensed(chat: &Session, events: &[Event], tail: Option<usize>) -> String {
    format!(
        "Chat \"{}\" ({}{}), id {}\n{}",
        chat.title.as_deref().unwrap_or("untitled"),
        chat.harness,
        chat.model
            .as_deref()
            .map(|m| format!(", {m}"))
            .unwrap_or_default(),
        chat.id,
        transcript(events, tail, READ_LIMIT)
    )
}

/// A chat's messages in order, each tool call a one-line note, a message
/// another agent sent saying so. With `tail`, only the last that many
/// messages. Past `limit` bytes the oldest part is cut - the end of a chat
/// is the part a reader most likely needs. Empty when there are none. The
/// read-a-chat tool and a moved chat's carried context both read it.
pub(super) fn transcript(events: &[Event], tail: Option<usize>, limit: usize) -> String {
    // Each message starts its own part; a tool note belongs to the part
    // before it, so a tail keeps the notes that came with its messages.
    let mut parts: Vec<String> = Vec::new();
    for event in events {
        match &event.payload {
            EventPayload::MessageAdded { message } => {
                let who = match (message.role, &message.origin) {
                    (MessageRole::User, Some(origin)) => {
                        format!("Agent \"{}\" (not the user)", origin.title)
                    }
                    (MessageRole::User, None) => "User".to_string(),
                    (MessageRole::Assistant, _) => "Assistant".to_string(),
                };
                parts.push(format!("\n{who}: {}\n", message.text.trim()));
            }
            EventPayload::ToolStarted { name, .. } => match parts.last_mut() {
                Some(last) => last.push_str(&format!("[tool: {name}]\n")),
                None => parts.push(format!("[tool: {name}]\n")),
            },
            _ => {}
        }
    }
    if let Some(tail) = tail.filter(|t| *t > 0) {
        let from = parts.len().saturating_sub(tail);
        parts.drain(..from);
    }
    let mut body = parts.concat();
    if body.len() > limit {
        let mut cut = body.len() - limit;
        while !body.is_char_boundary(cut) {
            cut += 1;
        }
        body = format!("[earlier messages cut]\n{}", &body[cut..]);
    }
    body
}

#[cfg(test)]
mod tests {
    use super::*;
    use openremote_harness::{AgentDescriptor, CommandDescriptor};

    #[test]
    fn codex_runs_reads_without_asking_but_asks_before_a_note_to_the_lead() {
        let names = |harness| {
            let mut names = crate::browser::tools::reading_tool_names("openremote");
            names.extend(crate::team::reading_tool_names("openremote"));
            allowed_for(harness, names)
        };
        let report = "mcp__openremote__agents_report".to_string();
        assert!(names("claude").contains(&report));
        let codex = names("codex");
        assert!(!codex.contains(&report), "{codex:?}");
        assert!(codex.contains(&"mcp__openremote__browser_snapshot".to_string()));
        assert!(codex.contains(&"mcp__openremote__agents_wait".to_string()));
    }

    #[test]
    fn only_an_app_that_relists_on_a_change_is_listed_tools_in_stages() {
        assert!(stages_tools("claude"));
        for harness in ["codex", "freebuff", "opencode", "pi", "grok", "agy"] {
            assert!(!stages_tools(harness), "{harness}");
        }
    }

    fn plugin_folder() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let write = |path: &str, body: &str| {
            let p = dir.path().join(path);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, body).unwrap();
        };
        write(".claude-plugin/plugin.json", r#"{"name": "notes"}"#);
        write(
            "skills/jot/SKILL.md",
            "---\nname: jot\ndescription: Jot a note\n---\n# jot",
        );
        write("commands/clean.md", "---\ndescription: Clean up\n---\nbody");
        write(
            "agents/scribe.md",
            "---\nname: scribe\ndescription: Writes\n---\n",
        );
        write(
            ".mcp.json",
            r#"{"mcpServers": {"notes-db": {"command": "x"}}}"#,
        );
        dir
    }

    #[test]
    fn a_harness_that_runs_plugins_lists_theirs_and_one_that_cannot_dims_them() {
        let folder = plugin_folder();
        let folders = vec![(
            "notes@local".to_string(),
            folder.path().to_path_buf(),
            "notes".to_string(),
        )];
        // claude listed the plugin's skill itself (with its description);
        // the plugin's other parts fill in.
        let catalog = CommandCatalog {
            commands: vec![
                CommandDescriptor {
                    name: "compact".into(),
                    description: Some("Compact".into()),
                    argument_hint: None,
                    aliases: vec![],
                },
                CommandDescriptor {
                    name: "notes:jot".into(),
                    description: Some("(notes) Jot a note".into()),
                    argument_hint: None,
                    aliases: vec![],
                },
            ],
            agents: vec![AgentDescriptor {
                name: "Explore".into(),
                description: None,
            }],
        };
        let view = build_view(
            "claude",
            true,
            catalog,
            &folders,
            vec!["docs@m".into()],
            None,
            &BTreeSet::new(),
        );
        let by = |n: &str| view.commands.iter().find(|c| c.name == n).unwrap();
        assert_eq!(by("compact").source, "harness");
        assert_eq!(by("notes:jot").source, "plugin");
        assert_eq!(by("notes:jot").plugin.as_deref(), Some("notes@local"));
        assert_eq!(
            by("notes:jot").description.as_deref(),
            Some("(notes) Jot a note")
        );
        assert!(by("notes:clean").available);
        assert!(
            view.agents
                .iter()
                .any(|a| a.name == "notes:scribe" && a.available)
        );
        assert_eq!(
            view.mcp_servers,
            vec![
                McpServerName {
                    name: "docs@m".into(),
                    source: "settings",
                    plugin: None
                },
                McpServerName {
                    name: "notes-db".into(),
                    source: "plugin",
                    plugin: Some("notes@local".into())
                },
            ]
        );

        let dimmed = build_view(
            "codex",
            false,
            CommandCatalog::default(),
            &folders,
            vec![],
            None,
            &BTreeSet::new(),
        );
        let jot = dimmed
            .commands
            .iter()
            .find(|c| c.name == "notes:jot")
            .unwrap();
        assert!(!jot.available);
        assert_eq!(jot.why.as_deref(), Some(NOT_HERE));
    }
}
