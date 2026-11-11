//! Agent teams: a chat's agent starts other chats as workers - on any
//! installed harness - and works with them. A team is a tree of ordinary
//! chats: a worker is a chat in the sidebar like any other, and the user can
//! open, steer, stop or take it over.
//!
//! The daemon owns the facts, not the agents: who started whom (the
//! session's `parent`), what each may do (the user's limits), and where a
//! worker's words go (its report, pushed to its lead when its turn ends).
//! An agent sees and controls only its own descendants and reports only to
//! its own parent; everything else is "no such agent in your team".

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex as StdMutex};
use std::time::{Duration, Instant};

use openremote_core::{
    EventPayload, Harness, MessageRole, Origin, ParentLink, Session, SessionId, SessionStatus,
    TurnOutcome, now_ms, title_from,
};
use openremote_harness::{DriverEvent, ModelDescriptor};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use tokio::sync::Notify;

use super::{Supervisor, SupervisorError, lock_recovering};

/// A worker's final message is cut to this many characters, keeping its end.
const REPORT_LIMIT: usize = 40_000;
/// A report inside a wait's answer is cut shorter: the lead keeps its
/// context, and `agents_read` has the rest.
const WAIT_EXCERPT: usize = 6_000;
/// The longest note or message one agent sends another, and the longest brief.
const NOTE_LIMIT: usize = 20_000;
const BRIEF_LIMIT: usize = 30_000;
/// Mid-way notes closer than this are refused: each one can wake an idle lead.
const NOTE_GAP: Duration = Duration::from_secs(15);
/// A wait answers within this, by default and at most (pi and codex time an
/// MCP call out at 60 seconds).
const WAIT_DEFAULT: u64 = 30;
const WAIT_MAX: u64 = 40;
/// How long a harness's model list is trusted.
const MODELS_FRESH: Duration = Duration::from_secs(600);
const MODELS_PATIENCE: Duration = Duration::from_secs(8);

/// What the user lets teams do. Kept in `agents.json` in the data folder,
/// per computer: the daemon that runs a lead enforces them.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TeamSettings {
    /// Whether agents are given the team tools at all.
    pub tools: bool,
    /// How deep a team goes: the user's chat is depth 0, its workers 1.
    pub max_depth: u32,
    /// Running workers one agent may have.
    pub max_children: u32,
    /// Running agents in one team, the whole tree.
    pub max_live: u32,
    /// Stopping a lead stops the workers still running under it.
    pub stop_with_lead: bool,
    /// The harnesses a worker may use; none named means every installed one.
    pub harnesses: Vec<String>,
    /// The permission word workers run with, per harness. A harness not
    /// named runs workers with its own default.
    pub permissions: BTreeMap<String, String>,
    /// Workers under one lead may message each other.
    pub sideways: bool,
}

impl Default for TeamSettings {
    fn default() -> Self {
        Self {
            tools: true,
            max_depth: 2,
            max_children: 4,
            max_live: 8,
            stop_with_lead: true,
            harnesses: Vec::new(),
            permissions: BTreeMap::new(),
            sideways: false,
        }
    }
}

impl TeamSettings {
    /// Change what the console named; `null` puts a setting back to its
    /// default. A value of the wrong kind refuses the whole change.
    fn change(&mut self, patch: &Map<String, Value>) -> Result<(), String> {
        let mut next = self.clone();
        let defaults = Self::default();
        let number = |key: &str, value: &Value, lo: u32, hi: u32| {
            value
                .as_u64()
                .map(|n| (n as u32).clamp(lo, hi))
                .ok_or_else(|| format!("{key} is a whole number from {lo} to {hi}"))
        };
        let flag = |key: &str, value: &Value| {
            value
                .as_bool()
                .ok_or_else(|| format!("{key} is true or false"))
        };
        for (key, value) in patch {
            let reset = value.is_null();
            match key.as_str() {
                "tools" => {
                    next.tools = if reset {
                        defaults.tools
                    } else {
                        flag(key, value)?
                    }
                }
                "stop_with_lead" => {
                    next.stop_with_lead = if reset {
                        defaults.stop_with_lead
                    } else {
                        flag(key, value)?
                    }
                }
                "sideways" => {
                    next.sideways = if reset {
                        defaults.sideways
                    } else {
                        flag(key, value)?
                    }
                }
                "max_depth" => {
                    next.max_depth = if reset {
                        defaults.max_depth
                    } else {
                        number(key, value, 1, 4)?
                    }
                }
                "max_children" => {
                    next.max_children = if reset {
                        defaults.max_children
                    } else {
                        number(key, value, 1, 8)?
                    }
                }
                "max_live" => {
                    next.max_live = if reset {
                        defaults.max_live
                    } else {
                        number(key, value, 1, 16)?
                    }
                }
                "harnesses" => {
                    next.harnesses = if reset {
                        Vec::new()
                    } else {
                        value
                            .as_array()
                            .ok_or("harnesses is a list of harness ids")?
                            .iter()
                            .map(|h| {
                                h.as_str()
                                    .map(str::to_string)
                                    .ok_or("harnesses is a list of harness ids")
                            })
                            .collect::<Result<_, _>>()?
                    }
                }
                "permissions" => {
                    next.permissions = if reset {
                        BTreeMap::new()
                    } else {
                        let named = value
                            .as_object()
                            .ok_or("permissions names a word per harness")?;
                        named
                            .iter()
                            .filter(|(_, word)| !word.is_null())
                            .map(|(harness, word)| {
                                word.as_str()
                                    .map(|w| (harness.clone(), w.to_string()))
                                    .ok_or("permissions names a word per harness")
                            })
                            .collect::<Result<_, _>>()?
                    }
                }
                _ => {}
            }
        }
        // A deeper team than the limit on its whole tree is no deeper.
        next.max_live = next.max_live.max(next.max_children);
        *self = next;
        Ok(())
    }
}

/// A report on its way to a lead that is not reading it yet.
#[derive(Clone, Debug)]
struct Pending {
    from: SessionId,
    title: String,
    /// The words as the lead reads them: a line saying whose report it is,
    /// then the body.
    text: String,
    /// May wake an idle lead. A failure may not: it is for the user to see.
    queueable: bool,
}

#[derive(Default)]
struct Book {
    /// `agents_wait` calls in flight, per lead.
    waiting: HashMap<SessionId, usize>,
    /// Reports for a lead's next look: those handed to a wait that is
    /// running, and failures, which nobody queues.
    mailbox: HashMap<SessionId, Vec<Pending>>,
    notifiers: HashMap<SessionId, Arc<Notify>>,
    /// When each member last reported (a finished turn or a note), ms.
    last_report: HashMap<SessionId, i64>,
    /// When each worker last sent a mid-way note.
    last_note: HashMap<SessionId, Instant>,
}

/// What a harness said about its models, and when.
type Seen = (Instant, Option<Vec<ModelDescriptor>>);

/// The team machinery's own state: the user's limits, the reports in
/// flight, and what is known about the harnesses' models.
pub(super) struct Team {
    file: PathBuf,
    settings: StdMutex<TeamSettings>,
    book: StdMutex<Book>,
    models: StdMutex<HashMap<String, Seen>>,
    /// Starting a worker is one step at a time: the limits are checked and
    /// the worker exists before the next start is checked.
    spawn_gate: tokio::sync::Mutex<()>,
}

impl Team {
    pub(super) fn open(dir: &Path) -> Self {
        let file = dir.join("agents.json");
        let settings = std::fs::read_to_string(&file)
            .ok()
            .and_then(|raw| serde_json::from_str(&raw).ok())
            .unwrap_or_default();
        Self {
            file,
            settings: StdMutex::new(settings),
            book: StdMutex::new(Book::default()),
            models: StdMutex::new(HashMap::new()),
            spawn_gate: tokio::sync::Mutex::new(()),
        }
    }
}

/// What an agent asks for when it starts a worker.
#[derive(Debug, Default, Deserialize)]
pub struct SpawnRequest {
    #[serde(default)]
    pub harness: String,
    #[serde(default)]
    pub brief: String,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub effort: Option<String>,
    #[serde(default)]
    pub permission: Option<String>,
    #[serde(default)]
    pub workspace: Option<String>,
}

/// The chats of one team, as a snapshot: who is whose.
struct Tree {
    by_id: HashMap<SessionId, Session>,
    children: HashMap<SessionId, Vec<SessionId>>,
}

impl Tree {
    fn new(sessions: Vec<Session>) -> Self {
        let mut children: HashMap<SessionId, Vec<(i64, SessionId)>> = HashMap::new();
        for session in &sessions {
            if let Some(parent) = &session.parent {
                children
                    .entry(parent.chat)
                    .or_default()
                    .push((parent.spawned_at, session.id));
            }
        }
        let children = children
            .into_iter()
            .map(|(parent, mut list)| {
                list.sort();
                (parent, list.into_iter().map(|(_, id)| id).collect())
            })
            .collect();
        Self {
            by_id: sessions.into_iter().map(|s| (s.id, s)).collect(),
            children,
        }
    }

    fn get(&self, id: &SessionId) -> Option<&Session> {
        self.by_id.get(id)
    }

    fn depth(&self, id: &SessionId) -> u32 {
        self.get(id)
            .and_then(|s| s.parent.as_ref())
            .map_or(0, |p| p.depth)
    }

    /// Everything under a chat, parents before their own workers.
    fn descendants(&self, id: &SessionId) -> Vec<&Session> {
        let mut seen = HashSet::from([*id]);
        let mut order = Vec::new();
        let mut frontier = vec![*id];
        while !frontier.is_empty() {
            let mut next = Vec::new();
            for parent in frontier {
                for child in self.children.get(&parent).into_iter().flatten() {
                    if seen.insert(*child) {
                        if let Some(session) = self.get(child) {
                            order.push(session);
                            next.push(*child);
                        }
                    }
                }
            }
            frontier = next;
        }
        order
    }

    /// The chat at the top of the tree this one is in.
    fn root(&self, id: &SessionId) -> SessionId {
        let mut at = *id;
        for _ in 0..16 {
            match self
                .get(&at)
                .and_then(|s| s.parent.as_ref())
                .filter(|p| self.by_id.contains_key(&p.chat))
            {
                Some(parent) => at = parent.chat,
                None => break,
            }
        }
        at
    }
}

/// A chat's name as the team knows it.
fn name_of(session: &Session) -> String {
    session
        .title
        .as_deref()
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .unwrap_or("Untitled chat")
        .to_string()
}

/// The word for where an agent stands. A worker whose approval is pending
/// is waiting for the user, not for its lead.
pub fn state_word(status: SessionStatus) -> &'static str {
    match status {
        SessionStatus::Starting | SessionStatus::Working => "working",
        SessionStatus::Waiting => "waiting for you",
        SessionStatus::Idle => "idle",
        SessionStatus::Failed => "failed",
        SessionStatus::Stopped => "stopped",
    }
}

/// The last `limit` characters of a text, said so when something went.
fn keep_end(text: &str, limit: usize) -> String {
    let count = text.chars().count();
    if count <= limit {
        return text.to_string();
    }
    let kept: String = text.chars().skip(count - limit).collect();
    format!("[earlier text cut]\n{kept}")
}

/// A rendered report cut to `limit`: its first line (whose it is) stays, and
/// the end of the body, where a conclusion is.
fn excerpt(text: &str, limit: usize) -> String {
    if text.chars().count() <= limit {
        return text.to_string();
    }
    match text.split_once('\n') {
        Some((head, body)) => format!(
            "{head}\n{}\n[Cut here for length; agents_read has the rest.]",
            keep_end(body.trim_start(), limit)
        ),
        None => keep_end(text, limit),
    }
}

/// One short line, for a title or a status.
fn one_line(text: &str, limit: usize) -> String {
    let line = text.trim().lines().next().unwrap_or("").trim();
    line.chars().take(limit).collect()
}

/// How a worker's turn ended, as its lead hears of it.
#[derive(Clone, Debug)]
pub(super) enum End {
    /// The harness closed the turn: completed, interrupted, or failed.
    Turn {
        turn: u64,
        coarse: TurnOutcome,
        reason: Option<String>,
    },
    /// The process died under an open turn.
    Died { turn: u64, reason: String },
    /// The chat was stopped under an open turn.
    Stopped { turn: u64 },
}

impl Supervisor {
    // ---- the user's limits ----

    pub fn team_settings(&self) -> TeamSettings {
        lock_recovering(&self.team.settings).clone()
    }

    /// Apply a change from the console and keep it.
    pub fn change_team_settings(
        &self,
        patch: &Map<String, Value>,
    ) -> Result<TeamSettings, SupervisorError> {
        let mut settings = lock_recovering(&self.team.settings);
        let mut next = settings.clone();
        next.change(patch).map_err(SupervisorError::Conflict)?;
        let raw = serde_json::to_vec_pretty(&next).expect("settings serialize");
        openremote_core::fsx::write_private(&self.team.file, &raw)
            .map_err(|e| SupervisorError::Conflict(format!("settings were not saved: {e}")))?;
        *settings = next.clone();
        Ok(next)
    }

    // ---- what a chat is offered ----

    /// Which of the team tools a chat gets. A chat that can start workers
    /// gets all of them; one that has workers but can start no more, or has
    /// a lead, gets only what it can use.
    pub(crate) fn team_offer(&self, id: &SessionId) -> Offer {
        let settings = self.team_settings();
        if !settings.tools {
            return Offer::default();
        }
        let tree = Tree::new(self.sessions());
        let Some(session) = tree.get(id) else {
            return Offer::default();
        };
        let spawn = tree.depth(id) < settings.max_depth;
        let has_workers = tree.children.contains_key(id);
        Offer {
            spawn,
            manage: spawn || has_workers,
            report: session.parent.is_some(),
        }
    }

    // ---- the tools ----

    /// `agents_harnesses`: what a worker can run on.
    pub async fn team_harnesses(&self, caller: &SessionId) -> Result<String, String> {
        self.live_caller(caller)?;
        let settings = self.team_settings();
        let mut lines = Vec::new();
        for harness in self.worker_harnesses(&settings) {
            let models = self.team_models(&harness.id).await;
            let mut line = format!("- {} ({})", harness.id, harness.name);
            match &models {
                Some(list) if !list.is_empty() => {
                    let shown: Vec<String> = list
                        .iter()
                        .map(|m| {
                            let efforts = if m.reasoning_efforts.is_empty() {
                                String::new()
                            } else {
                                format!(" [effort: {}]", m.reasoning_efforts.join(", "))
                            };
                            let default = if m.is_default { " (default)" } else { "" };
                            format!("{}{default}{efforts}", m.model)
                        })
                        .collect();
                    line.push_str(&format!("\n  models: {}", shown.join("; ")));
                }
                _ => line.push_str("\n  models: it picks its own default"),
            }
            match self.worker_permission(&harness, &settings) {
                Some(word) => line.push_str(&format!(
                    "\n  permission: workers run with \"{word}\", as the user set; you cannot change it"
                )),
                None => line.push_str(match harness.default_permission.as_deref() {
                    Some(word) => format!(
                        "\n  permission: workers run with its default (\"{word}\")"
                    ),
                    None => "\n  permission: workers run with its default".to_string(),
                }.as_str()),
            }
            lines.push(line);
        }
        if lines.is_empty() {
            return Err(
                "No harness is available for workers: none is installed and signed in, or the user has allowed none in Settings > Agents."
                    .into(),
            );
        }
        Ok(format!(
            "Harnesses you can start a worker on:\n{}",
            lines.join("\n")
        ))
    }

    /// `agents_spawn`: start a worker. Returns at once with its id.
    pub async fn team_spawn(
        self: &Arc<Self>,
        caller: &SessionId,
        request: SpawnRequest,
    ) -> Result<String, String> {
        let settings = self.team_settings();
        if !settings.tools {
            return Err("Team tools are turned off in Settings > Agents.".into());
        }
        let _one_at_a_time = self.team.spawn_gate.lock().await;
        let lead = self.live_caller(caller)?;
        let tree = Tree::new(self.sessions());
        let depth = tree.depth(caller);
        if depth >= settings.max_depth {
            return Err(format!(
                "You cannot start workers: you are {} here, and the user's limit is {} deep (Settings > Agents). Do the work yourself, or say in your report what you need.",
                if depth == 0 {
                    "at the top of a team".to_string()
                } else {
                    format!("{depth} step(s) below the user's own chat")
                },
                settings.max_depth
            ));
        }
        if request.brief.trim().is_empty() {
            return Err("A worker needs a brief: write what it should do and what to put in its final message.".into());
        }
        if request.brief.chars().count() > BRIEF_LIMIT {
            return Err(format!(
                "The brief is longer than {BRIEF_LIMIT} characters. Write what the worker needs to start, and point it at files for the rest."
            ));
        }
        // Where the worker runs: only the lead's own folder for now.
        match request.workspace.as_deref().map(str::trim) {
            None | Some("") | Some("inherit") => {}
            Some(other) => {
                return Err(format!(
                    "workspace \"{other}\" is not available yet. Only \"inherit\" - the worker shares your folder - works for now; separate worktrees arrive later. Give each worker different files to touch."
                ));
            }
        }
        let harness = self.worker_harness(&request.harness, &settings)?;
        // The limits, each saying what to do instead.
        let alive = |s: &&Session| s.status.is_alive();
        let mine = tree
            .children
            .get(caller)
            .into_iter()
            .flatten()
            .filter_map(|c| tree.get(c))
            .filter(alive)
            .count() as u32;
        if mine >= settings.max_children {
            return Err(format!(
                "You already have {mine} workers running, which is the limit ({}). Stop one you are done with (agents_stop), or give an idle one more work (agents_message).",
                settings.max_children
            ));
        }
        let root = tree.root(caller);
        let running = tree.descendants(&root).into_iter().filter(alive).count() as u32;
        if running >= settings.max_live {
            return Err(format!(
                "This team already has {running} agents running, which is the limit ({}). Wait for some to finish (agents_wait), or stop one you are done with (agents_stop).",
                settings.max_live
            ));
        }
        // A worker runs with the word the user set for its harness; a lead
        // can name it but never change it.
        let permission = self.worker_permission(&harness, &settings);
        if let Some(asked) = request
            .permission
            .as_deref()
            .map(str::trim)
            .filter(|p| !p.is_empty())
        {
            let allowed = permission
                .clone()
                .or_else(|| harness.default_permission.clone());
            if allowed.as_deref() != Some(asked) {
                return Err(match allowed {
                    Some(word) => format!(
                        "Workers on {} run with \"{word}\", which the user chose; you cannot give one \"{asked}\". Leave permission out.",
                        harness.id
                    ),
                    None => format!(
                        "{} has no permission words to choose from. Leave permission out.",
                        harness.name
                    ),
                });
            }
        }
        let (model, effort) = self
            .check_model(
                &harness,
                request.model.as_deref(),
                request.effort.as_deref(),
            )
            .await?;

        let title = request
            .title
            .as_deref()
            .map(|t| one_line(t, 60))
            .filter(|t| !t.is_empty())
            .unwrap_or_else(|| title_from(&request.brief));
        let parent = ParentLink {
            chat: *caller,
            depth: depth + 1,
            role: Some("worker".into()),
            brief: request.brief.trim().chars().take(200).collect(),
            spawned_at: now_ms(),
        };
        let worker = self
            .create_session_in(
                &harness.id,
                lead.workspace.clone(),
                model,
                effort,
                permission,
                false,
                // A synced lead's workers sync with it, and run here.
                lead.executor,
                // A worker never borrows its lead's model endpoint: it runs
                // on the harness the lead named, with that harness's own login.
                None,
                Some(parent),
                Some(title.clone()),
            )
            .await
            .map_err(|e| format!("The worker could not be created: {e}"))?;
        if !worker.status.is_alive() {
            return Err(format!(
                "The {} worker could not start: {}. It is in the user's chat list as a failed chat.",
                harness.name,
                worker
                    .last_error
                    .as_deref()
                    .unwrap_or("its harness did not start")
            ));
        }
        let rides_mcp = match self.entry(&worker.id).await {
            Ok(entry) => entry.driver.lock().await.rides_mcp(),
            Err(_) => false,
        };
        let words = worker_brief(
            &name_of(&lead),
            &self.harness_name(&lead.harness),
            lead.workspace.as_deref(),
            // What this worker really has: the team tools ride only the
            // harnesses that take the chat's MCP server, and what it may
            // do with them depends on how deep it is and on the user's
            // setting for messages between workers.
            BriefTools {
                report: rides_mcp && settings.tools,
                spawn: rides_mcp && settings.tools && depth + 1 < settings.max_depth,
                sideways: rides_mcp && settings.tools && settings.sideways,
            },
            request.brief.trim(),
        );
        let origin = Origin {
            kind: "agent".into(),
            chat: *caller,
            title: name_of(&lead),
            report: false,
        };
        if let Err(e) = self.prompt_from(&worker.id, &words, &origin).await {
            // A worker with no brief is no worker.
            let _ = self.stop(&worker.id).await;
            return Err(format!(
                "The worker started but could not take its brief ({e}); it was stopped."
            ));
        }
        Ok(format!(
            "{}\nStarted a {} worker. It runs on its own; its final message reaches you as its report, behind your current turn if you are busy. Use agents_wait to wait for reports, or carry on.",
            json!({"agent": worker.id.to_string(), "title": title, "harness": harness.id}),
            harness.name
        ))
    }

    /// `agents_list`: the caller's own team.
    pub fn team_list(&self, caller: &SessionId) -> Result<String, String> {
        self.live_caller(caller)?;
        let settings = self.team_settings();
        let tree = Tree::new(self.sessions());
        let me = tree.get(caller).ok_or("this chat is gone")?;
        let depth = tree.depth(caller);
        let mut out = Vec::new();
        out.push(format!(
            "You are \"{}\", {} (depth {depth} of a team that goes {} deep).",
            name_of(me),
            self.harness_name(&me.harness),
            settings.max_depth
        ));
        if let Some(parent) = me.parent.as_ref().and_then(|p| tree.get(&p.chat)) {
            out.push(format!(
                "You were started by {} - \"{}\" ({}, {}). Your final message is your report to it.",
                parent.id,
                name_of(parent),
                self.harness_name(&parent.harness),
                state_word(parent.status)
            ));
        }
        let team = tree.descendants(caller);
        if team.is_empty() {
            out.push("You have no workers.".into());
        } else {
            out.push("Your workers:".into());
            for member in team {
                out.push(self.member_line(member, &tree, depth));
            }
        }
        Ok(out.join("\n"))
    }

    /// `agents_read`: what one of the caller's workers did.
    pub fn team_read(
        &self,
        caller: &SessionId,
        agent: &str,
        tail: Option<usize>,
    ) -> Result<String, String> {
        self.live_caller(caller)?;
        let settings = self.team_settings();
        let tree = Tree::new(self.sessions());
        let member = self.member(&tree, caller, agent, &settings, false)?;
        let events = self
            .with_store(|s| s.events_after(&member.id, None))
            .map_err(|e| e.to_string())?;
        // The same condensed chat the read-a-chat tool gives, with the
        // worker's state ahead of it.
        Ok(format!(
            "{} - {}
{}",
            self.harness_name(&member.harness),
            state_word(member.status),
            super::composer::condensed(member, &events, tail)
        ))
    }

    /// `agents_wait`: until a worker reports or the time is up.
    pub async fn team_wait(
        self: &Arc<Self>,
        caller: &SessionId,
        timeout_s: Option<u64>,
    ) -> Result<String, String> {
        let lead = self.live_caller(caller)?;
        let tree = Tree::new(self.sessions());
        let team: HashSet<SessionId> = tree.descendants(caller).iter().map(|s| s.id).collect();
        if team.is_empty() {
            return Ok("You have no workers to wait for. Start one with agents_spawn.".into());
        }
        let timeout = Duration::from_secs(timeout_s.unwrap_or(WAIT_DEFAULT).clamp(1, WAIT_MAX));
        // What already waits: reports queued behind this very turn, and
        // failures nobody queued.
        let mut got = self.take_queued_reports(caller, &team).await;
        got.extend(self.take_mailbox(caller));
        if !got.is_empty() {
            return Ok(self.wait_answer(caller, got, None));
        }
        // A worker has a turn open while it works or waits on the user.
        let active = |tree: &Tree| {
            tree.descendants(caller).iter().any(|s| {
                matches!(
                    s.status,
                    SessionStatus::Working | SessionStatus::Starting | SessionStatus::Waiting
                )
            })
        };
        if !active(&tree) {
            return Ok(self.wait_answer(
                caller,
                Vec::new(),
                Some(
                    "None of your workers has a turn running, so there is nothing to wait for."
                        .into(),
                ),
            ));
        }
        let turn = self.current_turn(caller);
        let guard = WaitGuard::new(self, *caller);
        let deadline = Instant::now() + timeout;
        let _ = lead;
        loop {
            let notified = guard.notify.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            let got = self.take_mailbox(caller);
            if !got.is_empty() {
                return Ok(self.wait_answer(caller, got, None));
            }
            // The lead's own turn ended meanwhile (the user interrupted it):
            // this answer would go nowhere, so nothing is taken for it.
            let in_turn = self
                .session(caller)
                .map(|s| {
                    matches!(
                        s.status,
                        SessionStatus::Working | SessionStatus::Waiting | SessionStatus::Starting
                    )
                })
                .unwrap_or(false)
                && self.current_turn(caller) == turn;
            if !in_turn {
                return Ok("Your turn ended while you were waiting; any report that arrives reaches you as a message.".into());
            }
            // Everyone under the lead ended without a report for this wait (the
            // user stopped them): a report may still be a moment behind the
            // status that made it.
            if !active(&Tree::new(self.sessions())) {
                tokio::time::sleep(Duration::from_millis(400)).await;
                let got = self.take_mailbox(caller);
                return Ok(self.wait_answer(
                    caller,
                    got,
                    Some("None of your workers has a turn running any more.".into()),
                ));
            }
            let now = Instant::now();
            if now >= deadline {
                return Ok(self.wait_answer(
                    caller,
                    Vec::new(),
                    Some(format!(
                        "Nothing new in {} seconds. Call agents_wait again to keep waiting, or carry on: a report that arrives while you are busy reaches you when your turn ends.",
                        timeout.as_secs()
                    )),
                ));
            }
            let slice = (deadline - now).min(Duration::from_secs(1));
            tokio::select! {
                _ = &mut notified => {}
                _ = tokio::time::sleep(slice) => {}
            }
        }
    }

    /// `agents_report`: a note from a worker to the chat that started it.
    pub async fn team_report(
        &self,
        caller: &SessionId,
        text: &str,
        status: Option<&str>,
    ) -> Result<String, String> {
        let worker = self.live_caller(caller)?;
        let Some(parent) = worker.parent.as_ref() else {
            return Err("You have no lead to report to: this chat was started by the user. Say it in your reply instead.".into());
        };
        let lead = self
            .session(&parent.chat)
            .ok()
            .filter(|l| l.status.is_alive())
            .ok_or("The chat that started you is not running any more. Finish your task and say the result in your final message.")?;
        let text = text.trim();
        if text.is_empty() {
            return Err("A note needs words.".into());
        }
        if text.chars().count() > NOTE_LIMIT {
            return Err(format!(
                "A note is at most {NOTE_LIMIT} characters. Keep it short; your final message is the full report."
            ));
        }
        {
            let mut book = lock_recovering(&self.team.book);
            if let Some(last) = book.last_note.get(caller) {
                if last.elapsed() < NOTE_GAP {
                    return Err(format!(
                        "You sent a note a moment ago. Wait at least {} seconds between notes, and put the rest in your final message.",
                        NOTE_GAP.as_secs()
                    ));
                }
            }
            book.last_note.insert(*caller, Instant::now());
            book.last_report.insert(*caller, now_ms());
        }
        let status = status.map(|s| one_line(s, 40)).filter(|s| !s.is_empty());
        let head = format!(
            "Note from your worker \"{}\" ({}, id {}){}:",
            name_of(&worker),
            self.harness_name(&worker.harness),
            worker.id,
            status
                .as_deref()
                .map(|s| format!(" - {s}"))
                .unwrap_or_default()
        );
        self.deliver(
            &lead.id,
            Pending {
                from: worker.id,
                title: name_of(&worker),
                text: format!("{head}\n\n{text}"),
                queueable: true,
            },
        )
        .await;
        Ok(format!("Sent to \"{}\".", name_of(&lead)))
    }

    /// `agents_message`: words from the caller to a worker (or, where the
    /// user allows it, a sibling).
    pub async fn team_message(
        self: &Arc<Self>,
        caller: &SessionId,
        agent: &str,
        text: &str,
        steer: bool,
    ) -> Result<String, String> {
        let sender = self.live_caller(caller)?;
        let settings = self.team_settings();
        let tree = Tree::new(self.sessions());
        let target = self.member(&tree, caller, agent, &settings, true)?.clone();
        let text = text.trim();
        if text.is_empty() {
            return Err("A message needs words.".into());
        }
        if text.chars().count() > NOTE_LIMIT {
            return Err(format!("A message is at most {NOTE_LIMIT} characters."));
        }
        if !target.status.is_alive() {
            return Err(format!(
                "\"{}\" is {} and takes no messages; start a new worker with agents_spawn if the work goes on.",
                name_of(&target),
                state_word(target.status)
            ));
        }
        let origin = Origin {
            kind: "agent".into(),
            chat: *caller,
            title: name_of(&sender),
            report: false,
        };
        let busy = matches!(
            target.status,
            SessionStatus::Working | SessionStatus::Waiting
        );
        let sent = if steer && busy {
            self.steer_from(&target.id, text, &origin).await
        } else {
            self.queue_add_from(&target.id, text, origin)
                .await
                .map(|_| ())
        };
        sent.map_err(|e| format!("The message was not delivered: {e}"))?;
        Ok(if steer && busy {
            format!("Sent into \"{}\"'s turn that is running.", name_of(&target))
        } else if busy {
            format!(
                "\"{}\" is working; it reads your message when its turn ends.",
                name_of(&target)
            )
        } else {
            format!(
                "\"{}\" was idle; it started on your message.",
                name_of(&target)
            )
        })
    }

    /// `agents_stop`: stop one of the caller's workers and what it started.
    pub async fn team_stop(&self, caller: &SessionId, agent: &str) -> Result<String, String> {
        self.live_caller(caller)?;
        let settings = self.team_settings();
        let tree = Tree::new(self.sessions());
        let target = self.member(&tree, caller, agent, &settings, false)?.clone();
        let stopped = self.stop_branch(&target.id, true).await;
        Ok(match stopped {
            0 => format!("\"{}\" was not running.", name_of(&target)),
            1 => format!("Stopped \"{}\".", name_of(&target)),
            n => format!(
                "Stopped \"{}\" and the {} workers under it.",
                name_of(&target),
                n - 1
            ),
        })
    }

    // ---- the user's side ----

    /// The user stops everyone under a chat (the team bar's Stop all).
    /// Returns how many were running.
    pub async fn stop_team_of(&self, lead: &SessionId) -> usize {
        let tree = Tree::new(self.sessions());
        let running: Vec<SessionId> = tree
            .descendants(lead)
            .iter()
            .filter(|member| member.status.is_alive())
            .map(|member| member.id)
            .collect();
        self.stop_all(running).await
    }

    /// Stop these chats together: each stop waits for its chat's last words
    /// to land, and a team stopped one by one would outlast the console's
    /// patience. Returns how many stopped.
    async fn stop_all(&self, ids: Vec<SessionId>) -> usize {
        futures_util::future::join_all(ids.iter().map(|id| self.stop_only(id)))
            .await
            .into_iter()
            .filter(Result::is_ok)
            .count()
    }

    /// Stop a chat, with the workers it still has running when the user's
    /// setting says a lead takes its team down with it, and tell the lead
    /// it was stopped when it was in the middle of a turn.
    pub async fn stop(&self, id: &SessionId) -> Result<Session, SupervisorError> {
        let working = self
            .session(id)
            .map(|s| matches!(s.status, SessionStatus::Working | SessionStatus::Waiting))
            .unwrap_or(false);
        let turn = self.current_turn(id);
        let session = self.stop_only(id).await?;
        if working {
            self.team_turn_ended(id, End::Stopped { turn }).await;
        }
        if self.team_settings().stop_with_lead {
            self.stop_branch(id, false).await;
        }
        Ok(session)
    }

    /// Stop a chat and everything under it that runs. `include_self` false
    /// leaves the chat itself alone (it is already stopped). Returns how
    /// many were running.
    async fn stop_branch(&self, id: &SessionId, include_self: bool) -> usize {
        let tree = Tree::new(self.sessions());
        let mut order: Vec<SessionId> = Vec::new();
        if include_self {
            order.push(*id);
        }
        order.extend(tree.descendants(id).iter().map(|s| s.id));
        order.retain(|member| tree.get(member).is_some_and(|s| s.status.is_alive()));
        self.stop_all(order).await
    }

    // ---- reports ----

    /// What the lead of a worker hears when its turn ends, worked out
    /// before the event is applied (the death reason is in the stderr
    /// ring only until the chat settles). `None` for a chat with no lead.
    pub(super) fn team_ending(
        &self,
        id: &SessionId,
        event: &DriverEvent,
        stopping: bool,
    ) -> Option<End> {
        // Only the two events that can end a turn look at the store: this
        // runs for every event a harness streams.
        if !matches!(
            event,
            DriverEvent::TurnCompleted { .. } | DriverEvent::StdoutClosed
        ) {
            return None;
        }
        let session = self.session(id).ok()?;
        session.parent.as_ref()?;
        let turn = self.current_turn(id);
        match event {
            DriverEvent::TurnCompleted {
                coarse,
                error_message,
                subtype,
                ..
            } => Some(End::Turn {
                turn,
                coarse: *coarse,
                reason: error_message
                    .clone()
                    .or_else(|| (*coarse == TurnOutcome::Failed).then(|| subtype.clone())),
            }),
            DriverEvent::StdoutClosed
                if !stopping
                    && matches!(
                        session.status,
                        SessionStatus::Working | SessionStatus::Waiting
                    ) =>
            {
                Some(End::Died {
                    turn,
                    reason: self.death_reason(id),
                })
            }
            _ => None,
        }
    }

    /// A worker's turn ended: its report goes to the chat that started it.
    /// A finished, interrupted or stopped turn is a report that can wake an
    /// idle lead; a failure only answers a wait that is running - the user
    /// sees it in the worker's own chat, and nothing is retried for them.
    pub(super) async fn team_turn_ended(&self, id: &SessionId, end: End) {
        let Ok(worker) = self.session(id) else { return };
        let Some(parent) = worker.parent.as_ref() else {
            return;
        };
        let turn = match &end {
            End::Turn { turn, .. } | End::Died { turn, .. } | End::Stopped { turn } => *turn,
        };
        let (word, reason, queueable) = match &end {
            End::Turn {
                coarse: TurnOutcome::Completed,
                ..
            } => ("finished its turn".to_string(), None, true),
            End::Turn {
                coarse: TurnOutcome::Interrupted,
                ..
            } => ("was interrupted before it finished".to_string(), None, true),
            End::Turn { reason, .. } => ("failed".to_string(), reason.clone(), false),
            End::Died { reason, .. } => ("failed".to_string(), Some(reason.clone()), false),
            End::Stopped { .. } => ("was stopped before it finished".to_string(), None, true),
        };
        let last = self.last_assistant_text(id, turn);
        let mut head = format!(
            "Report from your worker \"{}\" ({}, id {}): {word}",
            name_of(&worker),
            self.harness_name(&worker.harness),
            worker.id
        );
        if let Some(reason) = reason {
            head.push_str(&format!(" - {}", one_line(&reason, 300)));
        }
        head.push('.');
        let body = match last {
            Some(text) => keep_end(text.trim(), REPORT_LIMIT),
            None => "(it left no final message)".to_string(),
        };
        lock_recovering(&self.team.book)
            .last_report
            .insert(*id, now_ms());
        self.deliver(
            &parent.chat,
            Pending {
                from: *id,
                title: name_of(&worker),
                text: format!("{head}\n\n{body}"),
                queueable,
            },
        )
        .await;
    }

    /// The last thing a chat's agent said in a turn.
    fn last_assistant_text(&self, id: &SessionId, turn: u64) -> Option<String> {
        let events = self.with_store(|s| s.events_after(id, None)).ok()?;
        events
            .into_iter()
            .rev()
            .find_map(|event| match event.payload {
                EventPayload::MessageAdded { message }
                    if message.role == MessageRole::Assistant
                        && message.turn == turn
                        && !message.text.trim().is_empty() =>
                {
                    Some(message.text)
                }
                _ => None,
            })
    }

    /// Hand a report to a lead: to the wait it has running, else queued
    /// behind its turn (an idle lead starts on it at once), else - a
    /// lead that is not running is never woken - left to be read in the
    /// worker's own chat.
    async fn deliver(&self, lead: &SessionId, pending: Pending) {
        {
            let mut book = lock_recovering(&self.team.book);
            let waiting = book.waiting.get(lead).copied().unwrap_or(0) > 0;
            if waiting || !pending.queueable {
                let list = book.mailbox.entry(*lead).or_default();
                // The newer report replaces the older one it was not read for.
                list.retain(|p| p.from != pending.from);
                list.push(pending);
                if let Some(notify) = book.notifiers.get(lead) {
                    notify.notify_waiters();
                }
                return;
            }
        }
        self.queue_report(lead, pending).await;
    }

    async fn queue_report(&self, lead: &SessionId, pending: Pending) {
        let alive = self
            .session(lead)
            .map(|s| s.status.is_alive())
            .unwrap_or(false)
            && self.entry(lead).await.is_ok();
        if !alive {
            return;
        }
        let origin = Origin {
            kind: "agent".into(),
            chat: pending.from,
            title: pending.title,
            report: true,
        };
        if let Err(e) = self.queue_add_from(lead, &pending.text, origin).await {
            eprintln!("team: a report for chat {lead} was not delivered: {e}");
        }
    }

    fn take_mailbox(&self, lead: &SessionId) -> Vec<Pending> {
        lock_recovering(&self.team.book)
            .mailbox
            .remove(lead)
            .unwrap_or_default()
    }

    /// The reports of a lead's team that are waiting in its queue: taken
    /// out of it, for a wait that is about to give them to it directly.
    async fn take_queued_reports(
        &self,
        lead: &SessionId,
        team: &HashSet<SessionId>,
    ) -> Vec<Pending> {
        let order = self.order_of(lead).await;
        let _guard = match &order {
            Some(o) => Some(o.lock().await),
            None => None,
        };
        let Ok(session) = self.session(lead) else {
            return Vec::new();
        };
        let (taken, kept): (Vec<_>, Vec<_>) = session.queue.into_iter().partition(|m| {
            m.origin
                .as_ref()
                .is_some_and(|o| o.report && team.contains(&o.chat))
        });
        if taken.is_empty() {
            return Vec::new();
        }
        if self.emit_queue(lead, kept, session.queue_paused).is_err() {
            return Vec::new();
        }
        taken
            .into_iter()
            .filter_map(|m| {
                let origin = m.origin?;
                Some(Pending {
                    from: origin.chat,
                    title: origin.title,
                    text: m.text,
                    queueable: true,
                })
            })
            .collect()
    }

    /// Put reports a wait never read back where an idle or busy lead gets
    /// them: queued, as if no wait had run.
    async fn flush_mailbox(&self, lead: &SessionId) {
        let waiting = lock_recovering(&self.team.book)
            .waiting
            .get(lead)
            .copied()
            .unwrap_or(0)
            > 0;
        if waiting {
            return;
        }
        let (queueable, failures): (Vec<_>, Vec<_>) = self
            .take_mailbox(lead)
            .into_iter()
            .partition(|p| p.queueable);
        // Failures stay for the next wait.
        if !failures.is_empty() {
            lock_recovering(&self.team.book)
                .mailbox
                .entry(*lead)
                .or_default()
                .extend(failures);
        }
        for pending in queueable {
            self.queue_report(lead, pending).await;
        }
    }

    /// What a wait answers: the reports it holds (cut short), then where
    /// the team stands.
    fn wait_answer(&self, caller: &SessionId, got: Vec<Pending>, note: Option<String>) -> String {
        let tree = Tree::new(self.sessions());
        let depth = tree.depth(caller);
        let mut out = Vec::new();
        if got.is_empty() {
            if let Some(note) = note {
                out.push(note);
            }
        } else {
            out.push(format!("{} report(s):", got.len()));
            for pending in &got {
                out.push(excerpt(&pending.text, WAIT_EXCERPT));
            }
        }
        let team = tree.descendants(caller);
        if !team.is_empty() {
            out.push("Your workers now:".into());
            for member in team {
                out.push(self.member_line(member, &tree, depth));
            }
        }
        out.join("\n\n")
    }

    // ---- who is who ----

    /// The caller's own chat, which must be running here.
    fn live_caller(&self, id: &SessionId) -> Result<Session, String> {
        let session = self
            .session(id)
            .map_err(|_| "This chat is gone.".to_string())?;
        if !session.status.is_alive() {
            return Err("This chat is not running.".into());
        }
        self.ensure_here(id).map_err(|e| e.to_string())?;
        Ok(session)
    }

    /// An agent the caller may act on, named by its id or the start of it.
    /// Anything else is "no such agent in your team" - an agent cannot
    /// learn whether a chat outside its team exists.
    fn member<'t>(
        &self,
        tree: &'t Tree,
        caller: &SessionId,
        wanted: &str,
        settings: &TeamSettings,
        sibling_ok: bool,
    ) -> Result<&'t Session, String> {
        let wanted = wanted.trim().trim_start_matches("chat:");
        let mut reach: Vec<&Session> = tree.descendants(caller);
        if sibling_ok && settings.sideways {
            if let Some(parent) = tree.get(caller).and_then(|s| s.parent.as_ref()) {
                for id in tree.children.get(&parent.chat).into_iter().flatten() {
                    if id != caller {
                        reach.extend(tree.get(id));
                    }
                }
            }
        }
        let matches: Vec<&Session> = reach
            .into_iter()
            .filter(|s| {
                let id = s.id.to_string();
                !wanted.is_empty()
                    && (id == wanted || (wanted.len() >= 6 && id.starts_with(wanted)))
            })
            .collect();
        match matches.as_slice() {
            [one] => Ok(one),
            [] => Err("no such agent in your team".into()),
            _ => Err("More than one agent starts with that - use its full id.".into()),
        }
    }

    fn member_line(&self, member: &Session, tree: &Tree, from_depth: u32) -> String {
        let depth = tree.depth(&member.id);
        let indent = "  ".repeat(depth.saturating_sub(from_depth + 1) as usize);
        let reported = {
            let book = lock_recovering(&self.team.book);
            book.last_report.get(&member.id).copied()
        }
        .or_else(|| {
            matches!(
                member.status,
                SessionStatus::Idle | SessionStatus::Stopped | SessionStatus::Failed
            )
            .then_some(member.updated_at)
        });
        let ago = match reported {
            Some(at) => format!(", last reported {}", ago(now_ms() - at)),
            None => ", no report yet".to_string(),
        };
        let why = match (member.status, member.last_error.as_deref()) {
            (SessionStatus::Failed, Some(reason)) => format!(" ({})", one_line(reason, 120)),
            _ => String::new(),
        };
        format!(
            "{indent}- {} \"{}\" - {}, {}{why}, depth {depth}{ago}",
            member.id,
            name_of(member),
            self.harness_name(&member.harness),
            state_word(member.status),
        )
    }

    // ---- harnesses for workers ----

    /// The harnesses a worker may run on: installed, on, signed in where
    /// that is known, and allowed by the user.
    fn worker_harnesses(&self, settings: &TeamSettings) -> Vec<Harness> {
        self.harnesses()
            .into_iter()
            .filter(|h| h.available && h.enabled && h.signed_in != Some(false))
            .filter(|h| settings.harnesses.is_empty() || settings.harnesses.contains(&h.id))
            .collect()
    }

    fn worker_harness(&self, wanted: &str, settings: &TeamSettings) -> Result<Harness, String> {
        let wanted = wanted.trim();
        let allowed = self.worker_harnesses(settings);
        allowed
            .iter()
            .find(|h| h.id.eq_ignore_ascii_case(wanted))
            .cloned()
            .ok_or_else(|| {
                let names: Vec<&str> = allowed.iter().map(|h| h.id.as_str()).collect();
                if names.is_empty() {
                    "No harness is available for workers right now (see agents_harnesses)."
                        .to_string()
                } else {
                    format!(
                        "\"{wanted}\" is not a harness you can start a worker on. You can use: {}.",
                        names.join(", ")
                    )
                }
            })
    }

    /// The word workers on a harness run with, when the user set one the
    /// harness knows.
    fn worker_permission(&self, harness: &Harness, settings: &TeamSettings) -> Option<String> {
        settings
            .permissions
            .get(&harness.id)
            .filter(|word| harness.permission_modes.iter().any(|w| &w.word == *word))
            .cloned()
    }

    /// A harness's models, from the last look (ten minutes) or a fresh
    /// one; `None` where it advertises none or could not be asked.
    async fn team_models(&self, harness: &str) -> Option<Vec<ModelDescriptor>> {
        {
            let models = lock_recovering(&self.team.models);
            if let Some((at, list)) = models.get(harness) {
                if at.elapsed() < MODELS_FRESH {
                    return list.clone();
                }
            }
        }
        let asked = tokio::time::timeout(MODELS_PATIENCE, self.models(harness)).await;
        let list = match asked {
            Ok(Ok(list)) => list,
            // Not asked in time, or asking failed: no word on it, and no
            // memory of the failure, so the next ask tries again.
            _ => return None,
        };
        lock_recovering(&self.team.models)
            .insert(harness.to_string(), (Instant::now(), list.clone()));
        list
    }

    /// A model and effort an agent named, held against what the harness
    /// offers when it says. Without a list they pass as named: the harness
    /// has its own word if they are wrong.
    async fn check_model(
        &self,
        harness: &Harness,
        model: Option<&str>,
        effort: Option<&str>,
    ) -> Result<(Option<String>, Option<String>), String> {
        let model = model.map(str::trim).filter(|m| !m.is_empty());
        let effort = effort.map(str::trim).filter(|e| !e.is_empty());
        if model.is_none() && effort.is_none() {
            return Ok((None, None));
        }
        let Some(list) = self
            .team_models(&harness.id)
            .await
            .filter(|l| !l.is_empty())
        else {
            return Ok((model.map(String::from), effort.map(String::from)));
        };
        let chosen = match model {
            Some(wanted) => Some(
                list.iter()
                    .find(|m| {
                        m.model.eq_ignore_ascii_case(wanted)
                            || m.display_name
                                .as_deref()
                                .is_some_and(|d| d.eq_ignore_ascii_case(wanted))
                            || m.resolved_model
                                .as_deref()
                                .is_some_and(|r| r.eq_ignore_ascii_case(wanted))
                    })
                    .ok_or_else(|| {
                        let names: Vec<&str> = list.iter().map(|m| m.model.as_str()).collect();
                        format!(
                            "{} has no model named \"{wanted}\". Its models: {}.",
                            harness.name,
                            names.join(", ")
                        )
                    })?,
            ),
            None => list.iter().find(|m| m.is_default),
        };
        if let (Some(wanted), Some(chosen)) = (effort, chosen) {
            if !chosen.reasoning_efforts.is_empty()
                && !chosen.reasoning_efforts.iter().any(|e| e == wanted)
            {
                return Err(format!(
                    "{} takes these effort levels: {}.",
                    chosen.model,
                    chosen.reasoning_efforts.join(", ")
                ));
            }
        }
        Ok((
            chosen.filter(|_| model.is_some()).map(|m| m.model.clone()),
            effort.map(String::from),
        ))
    }
}

/// Which team tools a chat is given.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Offer {
    /// It may start workers: `agents_spawn` and `agents_harnesses`.
    pub spawn: bool,
    /// It has workers, or may have: `agents_list`, `agents_read`,
    /// `agents_wait`, `agents_message`, `agents_stop`.
    pub manage: bool,
    /// It has a lead to report to: `agents_report`.
    pub report: bool,
}

/// A wait in flight: counted while it runs, and anything it was handed but
/// never read goes back to the lead's queue when it ends.
struct WaitGuard {
    supervisor: Arc<Supervisor>,
    lead: SessionId,
    notify: Arc<Notify>,
}

impl WaitGuard {
    fn new(supervisor: &Arc<Supervisor>, lead: SessionId) -> Self {
        let notify = {
            let mut book = lock_recovering(&supervisor.team.book);
            *book.waiting.entry(lead).or_insert(0) += 1;
            Arc::clone(
                book.notifiers
                    .entry(lead)
                    .or_insert_with(|| Arc::new(Notify::new())),
            )
        };
        Self {
            supervisor: Arc::clone(supervisor),
            lead,
            notify,
        }
    }
}

impl Drop for WaitGuard {
    fn drop(&mut self) {
        {
            let mut book = lock_recovering(&self.supervisor.team.book);
            if let Some(count) = book.waiting.get_mut(&self.lead) {
                *count = count.saturating_sub(1);
                if *count == 0 {
                    book.waiting.remove(&self.lead);
                }
            }
        }
        let supervisor = Arc::clone(&self.supervisor);
        let lead = self.lead;
        tokio::spawn(async move { supervisor.flush_mailbox(&lead).await });
    }
}

/// How long ago, in the words an agent reads.
fn ago(ms: i64) -> String {
    let seconds = (ms / 1000).max(0);
    match seconds {
        0..=59 => format!("{seconds} s ago"),
        60..=3599 => format!("{} min ago", seconds / 60),
        _ => format!("{} h ago", seconds / 3600),
    }
}

/// What a worker is told it can do with the team tools - only what is so.
struct BriefTools {
    /// It can send its lead a note while it works.
    report: bool,
    /// It can start workers of its own.
    spawn: bool,
    /// It can message the other workers under its lead.
    sideways: bool,
}

/// A worker's first message: the plain facts of its situation, then the
/// brief. Nothing here is a role to play - each line is true.
fn worker_brief(
    lead: &str,
    lead_harness: &str,
    workspace: Option<&Path>,
    tools: BriefTools,
    brief: &str,
) -> String {
    let mut text = format!(
        "You are a worker in an OpenRemote team. The chat \"{lead}\" ({lead_harness}) started you with the task below.          When your turn ends, your final message is sent to the lead as your report, so put the result in it.          You work for the lead, so do not ask the user questions; if you are blocked, say so in your final message."
    );
    if tools.report {
        text.push_str(
            " You can send the lead a note without ending your turn with the agents_report tool.",
        );
    }
    if tools.spawn {
        text.push_str(
            " You can start workers of your own with agents_spawn, within the limits the user set.",
        );
    }
    if tools.sideways {
        text.push_str(" You can message the other workers under your lead with agents_message.");
    }
    if let Some(folder) = workspace {
        text.push_str(&format!(
            " You share the folder {} with the lead and any other workers: keep to the files your task names.",
            folder.display()
        ));
    }
    text.push_str(
        "

Task:
",
    );
    text.push_str(brief);
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_change_by_name_and_keep_to_their_range() {
        let mut settings = TeamSettings::default();
        let patch = json!({"max_depth": 9, "max_children": 3, "sideways": true, "harnesses": ["codex"], "permissions": {"codex": "never"}});
        settings.change(patch.as_object().unwrap()).unwrap();
        assert_eq!(settings.max_depth, 4, "past the range, held at its end");
        assert_eq!(settings.max_children, 3);
        assert!(settings.sideways);
        assert_eq!(settings.harnesses, ["codex"]);
        assert_eq!(settings.permissions["codex"], "never");
        // null puts one back; a wrong kind refuses the whole change.
        settings
            .change(json!({"max_depth": null}).as_object().unwrap())
            .unwrap();
        assert_eq!(settings.max_depth, 2);
        let before = settings.clone();
        assert!(
            settings
                .change(
                    json!({"sideways": false, "tools": "yes"})
                        .as_object()
                        .unwrap()
                )
                .is_err()
        );
        assert_eq!(settings, before);
    }

    #[test]
    fn a_report_keeps_its_first_line_and_the_end_of_its_body() {
        let body = format!("{}\nthe conclusion", "x".repeat(200));
        let text = format!("Report from your worker \"w\":\n\n{body}");
        let cut = excerpt(&text, 40);
        assert!(cut.starts_with("Report from your worker"));
        assert!(cut.contains("the conclusion"));
        assert!(cut.contains("agents_read"));
        assert_eq!(excerpt("short", 40), "short");
    }
}
