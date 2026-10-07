use std::collections::BTreeMap;
use std::fs;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::PathBuf;

use thiserror::Error;

/// Millis in half an hour - the uptime band's slice size.
const HALF_HOUR_MS: i64 = 1_800_000;

/// The grid's hostname slug: lowercase, alphanumerics and single dashes
/// (runs of separators collapse - `Build  BOX` and `build-box` are the
/// same machine).
fn slug(name: &str) -> String {
    let mut out = String::new();
    let mut dash = false;
    for c in name.trim().chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
            dash = false;
        } else if !dash && !out.is_empty() {
            out.push('-');
            dash = true;
        }
    }
    out.trim_matches('-').to_string()
}

use crate::events::{Event, EventPayload};
use crate::ids::{DecisionId, DeviceId, MachineId, RuleId, SessionId};
use crate::model::{
    AutomationRule, Decision, DecisionState, Machine, MachineStatus, MessageRole, Plugin, Receipt,
    ReceiptStatus, Session, SessionStatus,
};

#[derive(Debug, Error)]
pub enum StoreError {
    #[error("not found: {0}")]
    NotFound(String),
    #[error("conflict: {0}")]
    Conflict(String),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("bad state file: {0}")]
    BadState(String),
    /// A replicated range doesn't continue the log: the caller pulls from
    /// `have` instead.
    #[error("gap: have {have}, got {got}")]
    Gap { have: u64, got: u64 },
}

/// A synced chat deleted everywhere. Peers that still hold it delete their
/// copy instead of handing it back.
#[derive(Clone, Copy, Debug, serde::Serialize, serde::Deserialize)]
pub struct Tombstone {
    pub at: i64,
    pub by: DeviceId,
}

/// Where a synced chat's log stands, for comparing with a peer's.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct SessionSummary {
    pub id: SessionId,
    pub executor: DeviceId,
    /// The next sequence number: how many events this copy holds.
    pub head: u64,
    pub updated_at: i64,
}

/// What a replicated append did.
#[derive(Debug, PartialEq, Eq)]
pub struct ReplicaAppend {
    pub appended: usize,
    pub head: u64,
}

/// Sessions, decisions, receipts - the durable daemon state.
///
/// Persistence: per-session append-only `events.jsonl` plus an atomically
/// rewritten `state.json`. No database in slice 1. On open, sessions that
/// were alive when the daemon died go to `failed` with reason
/// `daemon restart`, and their pending decisions retire silently - the
/// console treats unresolved `decision.requested` on non-alive sessions as
/// history.
#[derive(Default, serde::Serialize, serde::Deserialize)]
struct State {
    sessions: BTreeMap<SessionId, Session>,
    decisions: BTreeMap<DecisionId, Decision>,
    receipts: BTreeMap<String, Receipt>,
    #[serde(default)]
    next_seq: BTreeMap<SessionId, u64>,
    #[serde(default)]
    machines: BTreeMap<MachineId, Machine>,
    /// Per machine: the half-hour starts (ms) the agent was present for.
    #[serde(default)]
    presence: BTreeMap<MachineId, Vec<i64>>,
    #[serde(default)]
    plugins: BTreeMap<String, Plugin>,
    #[serde(default)]
    rules: BTreeMap<RuleId, AutomationRule>,
    /// This computer's id in Cloud mode, once it has one.
    #[serde(default)]
    this_device: Option<DeviceId>,
    #[serde(default)]
    tombstones: BTreeMap<SessionId, Tombstone>,
}

pub struct Store {
    dir: PathBuf,
    state: State,
}

impl Store {
    /// Open (or create) the store under `dir`; heals torn logs and
    /// reconciles chats that were running when the daemon died.
    pub fn open(dir: PathBuf) -> Result<Self, StoreError> {
        fs::create_dir_all(&dir)?;
        let state = match fs::read_to_string(dir.join("state.json")) {
            Ok(raw) => {
                serde_json::from_str(&raw).map_err(|e| StoreError::BadState(e.to_string()))?
            }
            Err(_) => State::default(),
        };
        let mut store = Self { dir, state };
        let healed = store.heal_logs()?;
        let reconciled = store.reconcile_crashed()?;
        let titled = store.backfill_titles();
        let had_this_machine = store.machines().iter().any(|m| m.this_machine);
        let this_id = store.ensure_this_machine();
        // Opening the store means the daemon is running - present now.
        store.mark_present(&this_id).ok();
        if healed || reconciled || titled || !had_this_machine {
            store.persist()?;
        }
        Ok(store)
    }

    /// A crash can tear the last line of a log, or land a batch in the log
    /// that state.json never recorded (the log is written first). Cut the
    /// torn line, and re-apply whatever the log holds past the recorded
    /// sequence, so the next append doesn't reuse a number.
    fn heal_logs(&mut self) -> Result<bool, StoreError> {
        const TAIL: u64 = 64 * 1024;
        let ids: Vec<SessionId> = self.state.sessions.keys().copied().collect();
        let mut changed = false;
        for id in ids {
            let Ok(mut file) = fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open(self.event_log(&id))
            else {
                continue;
            };
            let len = file.metadata()?.len();
            if len == 0 {
                continue;
            }
            let start = len.saturating_sub(TAIL);
            file.seek(SeekFrom::Start(start))?;
            let mut tail = Vec::new();
            file.read_to_end(&mut tail)?;
            if tail.last() != Some(&b'\n') {
                match tail.iter().rposition(|b| *b == b'\n') {
                    Some(i) => {
                        file.set_len(start + i as u64 + 1)?;
                        tail.truncate(i + 1);
                    }
                    None if start == 0 => {
                        file.set_len(0)?;
                        tail.clear();
                    }
                    // A torn line longer than the window: readers skip it.
                    None => continue,
                }
                changed = true;
            }
            let text = String::from_utf8_lossy(&tail);
            // The window may start mid-line; that partial line is skipped.
            let lines = text.lines().skip(usize::from(start > 0));
            let recorded = self.state.next_seq.get(&id).copied().unwrap_or(0);
            let mut next = recorded;
            for line in lines {
                if let Ok(event) = serde_json::from_str::<Event>(line) {
                    if event.seq >= next {
                        self.apply(&event)?;
                        next = event.seq + 1;
                    }
                }
            }
            if next > recorded {
                self.state.next_seq.insert(id, next);
                changed = true;
            }
        }
        Ok(changed)
    }

    /// Chats that were running here when the daemon died: mid-turn deaths
    /// lose work (failed, reason recorded), quiet ones just need a resume
    /// (stopped), and their pending decisions retire silently. A private
    /// chat changes silently, as always; a synced one records the change as
    /// an event so the user's other devices learn of it. Copies of chats
    /// running on another device are that device's business.
    fn reconcile_crashed(&mut self) -> Result<bool, StoreError> {
        let crashed: Vec<(SessionId, SessionStatus, bool)> = self
            .state
            .sessions
            .values()
            .filter(|s| s.status.is_alive() && self.executes_here(s))
            .map(|s| (s.id, s.status, s.executor.is_some()))
            .collect();
        for (id, was, synced) in &crashed {
            let status = if matches!(was, SessionStatus::Working | SessionStatus::Waiting) {
                SessionStatus::Failed
            } else {
                SessionStatus::Stopped
            };
            let reason = Some("daemon restart".to_string());
            if *synced {
                self.append(*id, EventPayload::SessionStatusChanged { status, reason })?;
            } else if let Some(session) = self.state.sessions.get_mut(id) {
                session.status = status;
                session.last_error = reason;
                session.updated_at = crate::now_ms();
            }
            for decision in self.state.decisions.values_mut() {
                if decision.session_id == *id && decision.state == DecisionState::Pending {
                    decision.state = DecisionState::Retired;
                }
            }
        }
        Ok(!crashed.is_empty())
    }

    /// Chats from before titles were stored take theirs from the first
    /// prompt in their log.
    fn backfill_titles(&mut self) -> bool {
        let untitled: Vec<SessionId> = self
            .state
            .sessions
            .values()
            .filter(|s| s.title.is_none())
            .map(|s| s.id)
            .collect();
        let mut changed = false;
        for id in untitled {
            let Ok(file) = fs::File::open(self.event_log(&id)) else {
                continue;
            };
            let mut head = Vec::new();
            if file.take(64 * 1024).read_to_end(&mut head).is_err() {
                continue;
            }
            let title =
                String::from_utf8_lossy(&head).lines().find_map(
                    |line| match serde_json::from_str::<Event>(line).ok()?.payload {
                        EventPayload::MessageAdded { message }
                            if message.role == MessageRole::User =>
                        {
                            Some(crate::title_from(&message.text))
                        }
                        _ => None,
                    },
                );
            if let (Some(title), Some(session)) = (title, self.state.sessions.get_mut(&id)) {
                session.title = Some(title);
                changed = true;
            }
        }
        changed
    }

    pub fn session(&self, id: &SessionId) -> Result<&Session, StoreError> {
        self.state
            .sessions
            .get(id)
            .ok_or_else(|| StoreError::NotFound(format!("session {id}")))
    }

    pub fn sessions(&self) -> Vec<&Session> {
        self.state.sessions.values().collect()
    }

    pub fn decision(&self, id: &DecisionId) -> Result<&Decision, StoreError> {
        self.state
            .decisions
            .get(id)
            .ok_or_else(|| StoreError::NotFound(format!("decision {id}")))
    }

    /// Record a live model, effort, or fast change. A `None` field is left
    /// as it was; fast is only written when the caller changed it.
    pub fn update_session_settings(
        &mut self,
        id: &SessionId,
        model: Option<String>,
        effort: Option<String>,
        fast: Option<bool>,
    ) -> Result<Session, StoreError> {
        let session = self
            .state
            .sessions
            .get_mut(id)
            .ok_or_else(|| StoreError::NotFound(format!("session {id}")))?;
        if model.is_some() {
            session.model = model;
        }
        if effort.is_some() {
            session.effort = effort;
        }
        if let Some(fast) = fast {
            session.fast = fast;
        }
        session.updated_at = crate::now_ms();
        let updated = session.clone();
        self.persist()?;
        Ok(updated)
    }

    pub fn decisions(&self, session_id: &SessionId) -> Vec<&Decision> {
        self.state
            .decisions
            .values()
            .filter(|d| &d.session_id == session_id)
            .collect()
    }

    pub fn receipt(&self, request_id: &str) -> Option<&Receipt> {
        self.state.receipts.get(request_id)
    }

    // ---- machines ----

    /// The daemon's own machine: created on first open, one stable id for
    /// the install's lifetime.
    pub fn ensure_this_machine(&mut self) -> MachineId {
        if let Some((id, _)) = self.state.machines.iter().find(|(_, m)| m.this_machine) {
            return *id;
        }
        let id = MachineId::new();
        let machine = Machine {
            id,
            name: slug(&crate::hostname()),
            platform: std::env::consts::OS.to_string(),
            status: MachineStatus::Online,
            this_machine: true,
            enrollment_token: None,
            created_at: crate::now_ms(),
            updated_at: crate::now_ms(),
            last_seen: Some(crate::now_ms()),
        };
        self.state.machines.insert(id, machine);
        id
    }

    pub fn machines(&self) -> Vec<&Machine> {
        self.state.machines.values().collect()
    }

    pub fn machine(&self, id: &MachineId) -> Result<&Machine, StoreError> {
        self.state
            .machines
            .get(id)
            .ok_or_else(|| StoreError::NotFound(format!("machine {id}")))
    }

    /// Add a machine: it lands `waiting` - it comes online when its agent
    /// checks in (the next machines pass), never from this call. Duplicate
    /// hostnames are rejected.
    pub fn create_machine(&mut self, name: &str, platform: &str) -> Result<Machine, StoreError> {
        let name = slug(name);
        if name.is_empty() {
            return Err(StoreError::BadState("a machine needs a name".into()));
        }
        if self.state.machines.values().any(|m| m.name == name) {
            return Err(StoreError::Conflict(format!(
                "'{name}' is already in the list"
            )));
        }
        let machine = Machine {
            id: MachineId::new(),
            name,
            platform: platform.to_string(),
            status: MachineStatus::Waiting,
            this_machine: false,
            enrollment_token: Some(uuid::Uuid::new_v4().to_string()),
            created_at: crate::now_ms(),
            updated_at: crate::now_ms(),
            last_seen: None,
        };
        let id = machine.id;
        self.state.machines.insert(id, machine.clone());
        self.persist()?;
        Ok(machine)
    }

    pub fn remove_machine(&mut self, id: &MachineId) -> Result<(), StoreError> {
        let machine = self.machine(id)?;
        if machine.this_machine {
            return Err(StoreError::Conflict(
                "this machine is the daemon itself".into(),
            ));
        }
        self.state.machines.remove(id);
        self.state.presence.remove(id);
        self.persist()
    }

    /// Record the current half-hour as present for a machine. Metadata,
    /// not history - nothing renders from the raw list; `presence` folds
    /// it into the 24-hour band. Returns true when a new slice landed (it
    /// persists then and only then).
    pub fn mark_present(&mut self, id: &MachineId) -> Result<bool, StoreError> {
        if self.machine(id).is_err() {
            return Ok(false);
        }
        let now = crate::now_ms();
        let slice = now - (now % HALF_HOUR_MS);
        let entries = self.state.presence.entry(*id).or_default();
        if entries.last().is_some_and(|last| *last >= slice) {
            return Ok(false);
        }
        entries.push(slice);
        let floor = slice - HALF_HOUR_MS * 48;
        entries.retain(|start| *start >= floor);
        self.persist()?;
        Ok(true)
    }

    /// 48 half-hour slices over the last 24 hours, oldest first; true =
    /// the agent was present.
    pub fn presence(&self, id: &MachineId) -> Vec<bool> {
        let now = crate::now_ms();
        let current = now - (now % HALF_HOUR_MS);
        let up = self.state.presence.get(id);
        (0..48)
            .rev()
            .map(|offset| {
                let start = current - offset * HALF_HOUR_MS;
                up.is_some_and(|entries| entries.binary_search(&start).is_ok())
            })
            .collect()
    }

    // ---- plugins ----

    pub fn plugins(&self) -> Vec<&Plugin> {
        self.state.plugins.values().collect()
    }

    pub fn plugin(&self, id: &str) -> Result<&Plugin, StoreError> {
        self.state
            .plugins
            .get(id)
            .ok_or_else(|| StoreError::NotFound(format!("plugin {id}")))
    }

    /// Install a plugin on a machine. Marketplace installs dedup by
    /// `catalog@machine`; custom ones by name per machine. The state
    /// change only - machine reachability is the caller's ruling.
    pub fn install_plugin(&mut self, mut plugin: Plugin) -> Result<Plugin, StoreError> {
        let dup = match &plugin.catalog_id {
            Some(_) => self.state.plugins.contains_key(&plugin.id),
            None => self.state.plugins.values().any(|p| {
                p.catalog_id.is_none() && p.machine == plugin.machine && p.name == plugin.name
            }),
        };
        if dup {
            return Err(StoreError::Conflict(format!(
                "'{}' is already on that machine",
                plugin.name
            )));
        }
        plugin.updated_at = crate::now_ms();
        let id = plugin.id.clone();
        self.state.plugins.insert(id, plugin.clone());
        self.persist()?;
        Ok(plugin)
    }

    pub fn remove_plugin(&mut self, id: &str) -> Result<(), StoreError> {
        if self.state.plugins.remove(id).is_none() {
            return Err(StoreError::NotFound(format!("plugin {id}")));
        }
        self.persist()
    }

    pub fn set_plugin_enabled(&mut self, id: &str, enabled: bool) -> Result<Plugin, StoreError> {
        let plugin = self
            .state
            .plugins
            .get_mut(id)
            .ok_or_else(|| StoreError::NotFound(format!("plugin {id}")))?;
        plugin.enabled = enabled;
        plugin.updated_at = crate::now_ms();
        let out = plugin.clone();
        self.persist()?;
        Ok(out)
    }

    /// Record that the machine has the key this plugin needs - the key
    /// itself never crosses this API.
    pub fn acknowledge_plugin_key(&mut self, id: &str) -> Result<Plugin, StoreError> {
        let plugin = self
            .state
            .plugins
            .get_mut(id)
            .ok_or_else(|| StoreError::NotFound(format!("plugin {id}")))?;
        if !plugin.needs_key {
            return Err(StoreError::Conflict(format!(
                "'{}' does not need a key",
                plugin.name
            )));
        }
        plugin.has_key = true;
        plugin.updated_at = crate::now_ms();
        let out = plugin.clone();
        self.persist()?;
        Ok(out)
    }

    // ---- automations ----

    pub fn rules(&self) -> Vec<&AutomationRule> {
        self.state.rules.values().collect()
    }

    pub fn rule(&self, id: &RuleId) -> Result<&AutomationRule, StoreError> {
        self.state
            .rules
            .get(id)
            .ok_or_else(|| StoreError::NotFound(format!("rule {id}")))
    }

    pub fn save_rule(&mut self, mut rule: AutomationRule) -> Result<AutomationRule, StoreError> {
        rule.updated_at = crate::now_ms();
        let id = rule.id;
        self.state.rules.insert(id, rule.clone());
        self.persist()?;
        Ok(rule)
    }

    pub fn remove_rule(&mut self, id: &RuleId) -> Result<(), StoreError> {
        if self.state.rules.remove(id).is_none() {
            return Err(StoreError::NotFound(format!("rule {id}")));
        }
        self.persist()
    }

    /// The rule fired: its chat and time are recorded for the list's
    /// "ran Mon DD, HH:MM" and the jump to the chat.
    pub fn record_rule_run(&mut self, id: &RuleId, chat: SessionId) -> Result<(), StoreError> {
        let rule = self
            .state
            .rules
            .get_mut(id)
            .ok_or_else(|| StoreError::NotFound(format!("rule {id}")))?;
        rule.last_chat = Some(chat);
        rule.last_run = Some(crate::now_ms());
        rule.updated_at = crate::now_ms();
        self.persist()
    }

    // ---- receipts ----

    /// Record a receipt. An `accepted` receipt may progress to a terminal
    /// state (the same request completing); a terminal receipt is final -
    /// a duplicate never re-executes, the stored outcome stands.
    pub fn record_receipt(&mut self, receipt: Receipt) -> Result<bool, StoreError> {
        if let Some(existing) = self.state.receipts.get(&receipt.request_id) {
            let progressable = existing.status == ReceiptStatus::Accepted
                && receipt.status != ReceiptStatus::Accepted;
            if !progressable {
                return Ok(false);
            }
        }
        let request_id = receipt.request_id.clone();
        self.state.receipts.insert(request_id, receipt);
        self.persist()?;
        Ok(true)
    }

    /// Mark all pending decisions of a session retired - silently, by contract.
    pub fn retire_pending(&mut self, session_id: &SessionId) -> Result<usize, StoreError> {
        let mut retired = 0;
        for decision in self.state.decisions.values_mut() {
            if decision.session_id == *session_id && decision.state == DecisionState::Pending {
                decision.state = DecisionState::Retired;
                retired += 1;
            }
        }
        if retired > 0 {
            self.persist()?;
        }
        Ok(retired)
    }

    /// Record the harness's own session id (metadata, not history - resume
    /// reads it; the console never renders it).
    pub fn set_harness_ref(
        &mut self,
        session_id: &SessionId,
        harness_ref: String,
    ) -> Result<(), StoreError> {
        let session = self
            .state
            .sessions
            .get_mut(session_id)
            .ok_or_else(|| StoreError::NotFound(format!("session {session_id}")))?;
        session.harness_session_ref = Some(harness_ref);
        session.updated_at = crate::now_ms();
        self.persist()
    }

    /// Hand out the session's next turn number (monotonic, per-session).
    pub fn bump_turn(&mut self, session_id: &SessionId) -> Result<u64, StoreError> {
        let session = self
            .state
            .sessions
            .get_mut(session_id)
            .ok_or_else(|| StoreError::NotFound(format!("session {session_id}")))?;
        let turn = session.next_turn;
        session.next_turn += 1;
        session.updated_at = crate::now_ms();
        self.persist()?;
        Ok(turn)
    }

    /// Session metadata from the harness (`init` frame): its conversation
    /// id for resume, and the model / permission mode it actually runs.
    /// Metadata, not history - nothing here is an event. Returns whether
    /// anything the console renders actually changed (a same-model repeat
    /// init is not a change).
    pub fn note_session(
        &mut self,
        session_id: &SessionId,
        harness_ref: Option<String>,
        model: Option<String>,
        permission_mode: Option<String>,
    ) -> Result<bool, StoreError> {
        let session = self
            .state
            .sessions
            .get_mut(session_id)
            .ok_or_else(|| StoreError::NotFound(format!("session {session_id}")))?;
        let mut changed = false;
        if let Some(r) = harness_ref {
            if session.harness_session_ref.as_deref() != Some(r.as_str()) {
                session.harness_session_ref = Some(r);
                changed = true;
            }
        }
        if let Some(m) = model {
            if session.model.as_deref() != Some(m.as_str()) {
                session.model = Some(m);
                changed = true;
            }
        }
        if let Some(p) = permission_mode {
            if session.permission_mode.as_deref() != Some(p.as_str()) {
                session.permission_mode = Some(p);
                changed = true;
            }
        }
        if changed {
            session.updated_at = crate::now_ms();
            self.persist()?;
        }
        Ok(changed)
    }

    /// Record a tool the harness granted for the rest of the session (its
    /// own session-scope answer word). Facts, not history - the
    /// `session.updated` event that follows carries them to the console.
    pub fn note_approved_tool(
        &mut self,
        session_id: &SessionId,
        tool: &str,
    ) -> Result<(), StoreError> {
        let session = self
            .state
            .sessions
            .get_mut(session_id)
            .ok_or_else(|| StoreError::NotFound(format!("session {session_id}")))?;
        if !session.approved_tools.iter().any(|t| t == tool) {
            session.approved_tools.push(tool.to_string());
            session.updated_at = crate::now_ms();
            self.persist()?;
        }
        Ok(())
    }

    /// Append one event; see `append_batch`.
    pub fn append(
        &mut self,
        session_id: SessionId,
        payload: EventPayload,
    ) -> Result<Event, StoreError> {
        let mut events = self.append_batch(session_id, vec![payload])?;
        events
            .pop()
            .ok_or_else(|| StoreError::BadState("empty batch".to_string()))
    }

    /// Append events to a chat this device runs: assigns `seq`s and applies
    /// them, then one log write and one state write for the whole batch (a
    /// streamed reply is many events a second). Copies of chats running on
    /// another device only grow through `append_replicated`, and a chat
    /// deleted everywhere takes nothing more.
    pub fn append_batch(
        &mut self,
        session_id: SessionId,
        payloads: Vec<EventPayload>,
    ) -> Result<Vec<Event>, StoreError> {
        let Some(first) = payloads.first() else {
            return Ok(Vec::new());
        };
        if self.state.tombstones.contains_key(&session_id) {
            return Err(StoreError::Conflict(format!(
                "chat {session_id} was deleted"
            )));
        }
        // Only session.created may name a session the state doesn't know -
        // it is the event that inserts it.
        match self.state.sessions.get(&session_id) {
            None if !matches!(first, EventPayload::SessionCreated { .. }) => {
                return Err(StoreError::NotFound(format!("session {session_id}")));
            }
            Some(session) if !self.executes_here(session) => {
                return Err(StoreError::Conflict(format!(
                    "chat {session_id} runs on another device"
                )));
            }
            _ => {}
        }
        let at = crate::now_ms();
        let mut lines = String::new();
        let mut events = Vec::with_capacity(payloads.len());
        for payload in payloads {
            let seq = self.state.next_seq.entry(session_id).or_insert(0);
            let event = Event {
                seq: *seq,
                session_id,
                at: Some(at),
                payload,
            };
            *seq += 1;
            lines.push_str(
                &serde_json::to_string(&event).map_err(|e| StoreError::BadState(e.to_string()))?,
            );
            lines.push('\n');
            self.apply(&event)?;
            events.push(event);
        }
        self.write_log(&session_id, &lines)?;
        self.persist()?;
        Ok(events)
    }

    /// Extend this device's copy of a chat that runs on `runner` with
    /// events from its log, verbatim. They must continue the copy exactly:
    /// duplicates are skipped, a gap stops the append and is reported so the
    /// caller pulls the missing range. Event types this build doesn't know
    /// are kept as they are and skipped by state, so an older device never
    /// stalls a newer one.
    ///
    /// A chat that started private and was synced later opens with a
    /// `session.created` that names no runner; the copy takes `runner` from
    /// its first event, so it is never mistaken for a private chat here.
    pub fn append_replicated(
        &mut self,
        session_id: SessionId,
        runner: DeviceId,
        events: Vec<serde_json::Value>,
    ) -> Result<ReplicaAppend, StoreError> {
        if Some(runner) == self.state.this_device {
            return Err(StoreError::Conflict(format!(
                "chat {session_id} runs on this device"
            )));
        }
        if self.state.tombstones.contains_key(&session_id) {
            return Err(StoreError::Conflict(format!(
                "chat {session_id} was deleted"
            )));
        }
        if let Some(session) = self.state.sessions.get(&session_id) {
            if self.executes_here(session) || session.executor != Some(runner) {
                return Err(StoreError::Conflict(format!(
                    "chat {session_id} runs on another device"
                )));
            }
        }
        let mut head = self.state.next_seq.get(&session_id).copied().unwrap_or(0);
        let mut lines = String::new();
        let mut appended = 0;
        let mut gap = None;
        for value in events {
            let seq = value["seq"].as_u64();
            let same_chat = value["session_id"].as_str() == Some(&session_id.to_string());
            let Some(seq) = seq.filter(|_| same_chat) else {
                return Err(StoreError::BadState(
                    "a replicated event without its chat or seq".into(),
                ));
            };
            if seq < head {
                continue;
            }
            if seq > head {
                gap = Some(seq);
                break;
            }
            match serde_json::from_value::<Event>(value.clone()) {
                Ok(mut event) => {
                    let opens = matches!(event.payload, EventPayload::SessionCreated { .. });
                    if (head == 0) != opens {
                        return Err(StoreError::BadState(
                            "a copy starts with session.created, and only there".into(),
                        ));
                    }
                    if let EventPayload::SessionCreated { session } = &mut event.payload {
                        if session.executor.is_some_and(|e| e != runner) {
                            return Err(StoreError::BadState(
                                "a replicated chat names another runner".into(),
                            ));
                        }
                        session.executor = Some(runner);
                    }
                    self.apply(&event)?;
                }
                Err(_) if head == 0 => {
                    return Err(StoreError::BadState(
                        "a copy starts with session.created".into(),
                    ));
                }
                Err(_) => {}
            }
            lines.push_str(&value.to_string());
            lines.push('\n');
            head += 1;
            appended += 1;
        }
        if appended > 0 {
            self.state.next_seq.insert(session_id, head);
            // A copy never runs anything, so nothing is left to decide once
            // the chat isn't running any more.
            if self
                .state
                .sessions
                .get(&session_id)
                .is_some_and(|s| !s.status.is_alive())
            {
                for decision in self.state.decisions.values_mut() {
                    if decision.session_id == session_id && decision.state == DecisionState::Pending
                    {
                        decision.state = DecisionState::Retired;
                    }
                }
            }
            self.write_log(&session_id, &lines)?;
            self.persist()?;
        }
        match gap {
            Some(got) if appended == 0 => Err(StoreError::Gap { have: head, got }),
            _ => Ok(ReplicaAppend { appended, head }),
        }
    }

    /// Raw log lines from `from` on, about `max_bytes` of them (always at
    /// least one when there is one), and the head: what a peer pulls.
    pub fn raw_range(
        &self,
        session_id: &SessionId,
        from: u64,
        max_bytes: usize,
    ) -> Result<(Vec<String>, u64), StoreError> {
        self.session(session_id)?;
        let head = self.state.next_seq.get(session_id).copied().unwrap_or(0);
        let raw = match fs::read_to_string(self.event_log(session_id)) {
            Ok(raw) => raw,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok((Vec::new(), head)),
            Err(e) => return Err(e.into()),
        };
        #[derive(serde::Deserialize)]
        struct Seq {
            seq: u64,
        }
        let mut out = Vec::new();
        let mut size = 0;
        for line in raw.lines() {
            let Ok(Seq { seq }) = serde_json::from_str(line) else {
                continue;
            };
            if seq < from {
                continue;
            }
            if !out.is_empty() && size + line.len() > max_bytes {
                break;
            }
            size += line.len();
            out.push(line.to_string());
        }
        Ok((out, head))
    }

    /// Where every synced chat stands, and which ones were deleted.
    pub fn summaries(&self) -> (Vec<SessionSummary>, Vec<(SessionId, Tombstone)>) {
        let sessions = self
            .state
            .sessions
            .values()
            .filter_map(|s| {
                Some(SessionSummary {
                    id: s.id,
                    executor: s.executor?,
                    head: self.state.next_seq.get(&s.id).copied().unwrap_or(0),
                    updated_at: s.updated_at,
                })
            })
            .collect();
        let tombstones = self
            .state
            .tombstones
            .iter()
            .map(|(id, t)| (*id, *t))
            .collect();
        (sessions, tombstones)
    }

    /// Deletes a synced chat for good: its record, decisions and log, and
    /// remembers the deletion so no peer hands it back.
    pub fn tombstone(&mut self, session_id: SessionId, stone: Tombstone) -> Result<(), StoreError> {
        self.forget_session(&session_id)?;
        self.state.tombstones.insert(session_id, stone);
        self.persist()
    }

    /// This device was removed from the account: every synced chat goes
    /// (the user's other devices keep theirs), and private chats stay.
    pub fn purge_synced(&mut self) -> Result<usize, StoreError> {
        let synced: Vec<SessionId> = self
            .state
            .sessions
            .values()
            .filter(|s| s.executor.is_some())
            .map(|s| s.id)
            .collect();
        for id in &synced {
            self.forget_session(id)?;
        }
        self.state.tombstones.clear();
        self.state.this_device = None;
        self.persist()?;
        Ok(synced.len())
    }

    fn forget_session(&mut self, session_id: &SessionId) -> Result<(), StoreError> {
        self.state.sessions.remove(session_id);
        self.state.next_seq.remove(session_id);
        self.state
            .decisions
            .retain(|_, d| d.session_id != *session_id);
        match fs::remove_dir_all(self.dir.join("sessions").join(session_id.to_string())) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.into()),
            _ => Ok(()),
        }
    }

    /// How many events this device holds for a chat (its next seq).
    pub fn head(&self, session_id: &SessionId) -> u64 {
        self.state.next_seq.get(session_id).copied().unwrap_or(0)
    }

    pub fn this_device(&self) -> Option<DeviceId> {
        self.state.this_device
    }

    pub fn set_this_device(&mut self, device: Option<DeviceId>) -> Result<(), StoreError> {
        self.state.this_device = device;
        self.persist()
    }

    pub fn tombstoned(&self, session_id: &SessionId) -> bool {
        self.state.tombstones.contains_key(session_id)
    }

    /// Private chats and synced chats this device runs execute here; the
    /// rest are copies of chats running on the user's other devices.
    pub fn executes_here(&self, session: &Session) -> bool {
        session.executor.is_none() || session.executor == self.state.this_device
    }

    fn write_log(&self, session_id: &SessionId, lines: &str) -> Result<(), StoreError> {
        let log_dir = self.dir.join("sessions").join(session_id.to_string());
        fs::create_dir_all(&log_dir)?;
        let mut log = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(log_dir.join("events.jsonl"))?;
        log.write_all(lines.as_bytes())?;
        Ok(())
    }

    /// Events after a cursor. `None` replays from the beginning (a fresh
    /// SSE connect); `Some(seq)` returns strictly later events (a resume -
    /// the client already has `seq`).
    pub fn events_after(
        &self,
        session_id: &SessionId,
        after: Option<u64>,
    ) -> Result<Vec<Event>, StoreError> {
        self.session(session_id)?;
        let path = self.event_log(session_id);
        let raw = match fs::read_to_string(&path) {
            Ok(raw) => raw,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(e.into()),
        };
        let mut out = Vec::new();
        for line in raw.lines() {
            if line.trim().is_empty() {
                continue;
            }
            match serde_json::from_str::<Event>(line) {
                Ok(event) => {
                    if after.is_none_or(|cursor| event.seq > cursor) {
                        out.push(event);
                    }
                }
                // A torn final line (crash mid-write) is history, not a failure.
                Err(e) => {
                    eprintln!("warning: dropping unparsable event line in {session_id}: {e}");
                }
            }
        }
        Ok(out)
    }

    fn event_log(&self, session_id: &SessionId) -> PathBuf {
        self.dir
            .join("sessions")
            .join(session_id.to_string())
            .join("events.jsonl")
    }

    fn apply(&mut self, event: &Event) -> Result<(), StoreError> {
        // The event's own time, so a copy dates the chat by its history.
        let now = event.at.unwrap_or_else(crate::now_ms);
        match &event.payload {
            EventPayload::SessionCreated { session } => {
                self.state.sessions.insert(session.id, session.clone());
            }
            EventPayload::SessionUpdated { session } => {
                let mut session = session.clone();
                // Events from before a chat was synced name no runner; the
                // runner it has now stays.
                if session.executor.is_none() {
                    session.executor = self
                        .state
                        .sessions
                        .get(&session.id)
                        .and_then(|s| s.executor);
                }
                if session.title.is_none() {
                    session.title = self
                        .state
                        .sessions
                        .get(&session.id)
                        .and_then(|s| s.title.clone());
                }
                self.state.sessions.insert(session.id, session);
            }
            EventPayload::MessageAdded { message } => {
                if let Some(session) = self.state.sessions.get_mut(&event.session_id) {
                    session.updated_at = now;
                    if session.title.is_none() && message.role == MessageRole::User {
                        session.title = Some(crate::title_from(&message.text));
                    }
                }
            }
            EventPayload::SessionStatusChanged { status, reason } => {
                let session = self
                    .state
                    .sessions
                    .get_mut(&event.session_id)
                    .ok_or_else(|| StoreError::NotFound(format!("session {}", event.session_id)))?;
                session.status = *status;
                session.last_error = reason.clone();
                session.updated_at = now;
            }
            EventPayload::DecisionRequested { decision } => {
                self.state.decisions.insert(decision.id, decision.clone());
            }
            EventPayload::DecisionResponded {
                decision_id,
                choice,
            } => {
                if let Some(decision) = self.state.decisions.get_mut(decision_id) {
                    if decision.state == DecisionState::Pending {
                        decision.state = DecisionState::Answered;
                        decision.answer = Some(crate::model::DecisionAnswer {
                            choice: choice.clone(),
                            updated_input: None,
                            answered_at: now,
                        });
                    }
                }
            }
            EventPayload::TurnStarted { .. }
            | EventPayload::TurnCompleted { .. }
            | EventPayload::MessageDelta { .. }
            | EventPayload::ToolStarted { .. }
            | EventPayload::ToolResult { .. }
            | EventPayload::ContextUsed { .. }
            | EventPayload::UsageCost { .. }
            | EventPayload::ReasoningAdded { .. }
            | EventPayload::ReasoningDelta { .. }
            | EventPayload::ThinkingTokens { .. }
            | EventPayload::NoteAdded { .. }
            | EventPayload::DaemonError { .. } => {
                if let Some(session) = self.state.sessions.get_mut(&event.session_id) {
                    session.updated_at = now;
                }
            }
        }
        Ok(())
    }

    /// State holds webhook keys, so only this user may read it.
    fn persist(&self) -> Result<(), StoreError> {
        let json = serde_json::to_string_pretty(&self.state)
            .map_err(|e| StoreError::BadState(e.to_string()))?;
        crate::fsx::write_private(&self.dir.join("state.json"), json.as_bytes())?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{DecisionKind, DecisionOption};

    fn tempdir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("or-core-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn session() -> Session {
        Session {
            id: SessionId::new(),
            harness: "claude".into(),
            workspace: "/tmp/w".into(),
            status: SessionStatus::Starting,
            model: None,
            effort: None,
            permission_mode: None,
            harness_session_ref: None,
            created_at: 0,
            updated_at: 0,
            last_error: None,
            next_turn: 1,
            fast: false,
            approved_tools: Vec::new(),
            executor: None,
            title: None,
        }
    }

    fn decision(session: &Session) -> Decision {
        Decision {
            id: DecisionId::new(),
            session_id: session.id,
            turn: 1,
            kind: DecisionKind::Approval,
            state: DecisionState::Pending,
            harness_request: serde_json::json!({"tool_name": "Bash", "input": {"command": "ls"}}),
            harness_ref: Some("cr-1".into()),
            tool_name: Some("Bash".into()),
            options: vec![
                DecisionOption {
                    id: "allow".into(),
                    label: "Allow".into(),
                },
                DecisionOption {
                    id: "deny".into(),
                    label: "Deny".into(),
                },
            ],
            created_at: 0,
            answer: None,
        }
    }

    #[test]
    fn events_round_trip_through_the_log() {
        let dir = tempdir();
        let mut store = Store::open(dir.clone()).unwrap();
        let s = session();
        store
            .append(s.id, EventPayload::SessionCreated { session: s.clone() })
            .unwrap();
        store
            .append(
                s.id,
                EventPayload::SessionStatusChanged {
                    status: SessionStatus::Working,
                    reason: None,
                },
            )
            .unwrap();
        store
            .append(
                s.id,
                EventPayload::MessageAdded {
                    message: crate::model::ChatMessage {
                        id: "m1".into(),
                        turn: 1,
                        role: crate::model::MessageRole::Assistant,
                        text: "hi".into(),
                    },
                },
            )
            .unwrap();

        let store = Store::open(dir).unwrap();
        let all = store.events_after(&s.id, None).unwrap();
        assert_eq!(all.len(), 3);
        assert_eq!(all[0].kind(), "session.created");
        assert_eq!(all[1].kind(), "session.status_changed");
        assert!(
            serde_json::to_string(&all[2].payload)
                .unwrap()
                .contains("hi")
        );
        // resume boundary: a client holding seq 1 gets only the tail
        assert_eq!(store.events_after(&s.id, Some(1)).unwrap().len(), 1);
    }

    #[test]
    fn retiring_a_decision_emits_nothing_and_blocks_late_answers() {
        let dir = tempdir();
        let mut store = Store::open(dir).unwrap();
        let s = session();
        store
            .append(s.id, EventPayload::SessionCreated { session: s.clone() })
            .unwrap();
        let d = decision(&s);
        let d_id = d.id;
        store
            .append(s.id, EventPayload::DecisionRequested { decision: d })
            .unwrap();
        let before = store.events_after(&s.id, None).unwrap().len();

        let retired = store.retire_pending(&s.id).unwrap();
        assert_eq!(retired, 1);
        // The contract: no event is invented for the retirement.
        assert_eq!(store.events_after(&s.id, None).unwrap().len(), before);
        assert_eq!(store.decision(&d_id).unwrap().state, DecisionState::Retired);
    }

    #[test]
    fn a_crashed_session_fails_on_reopen_and_its_decisions_retire() {
        let dir = tempdir();
        let mut store = Store::open(dir.clone()).unwrap();
        let s = session();
        store
            .append(s.id, EventPayload::SessionCreated { session: s.clone() })
            .unwrap();
        store
            .append(
                s.id,
                EventPayload::SessionStatusChanged {
                    status: SessionStatus::Working,
                    reason: None,
                },
            )
            .unwrap();
        let d = decision(&s);
        let d_id = d.id;
        store
            .append(s.id, EventPayload::DecisionRequested { decision: d })
            .unwrap();
        drop(store);

        let reopened = Store::open(dir).unwrap();
        let loaded = reopened.session(&s.id).unwrap();
        assert_eq!(loaded.status, SessionStatus::Failed);
        assert_eq!(loaded.last_error.as_deref(), Some("daemon restart"));
        assert_eq!(
            reopened.decision(&d_id).unwrap().state,
            DecisionState::Retired
        );
    }

    fn user_message(turn: u64, text: &str) -> EventPayload {
        EventPayload::MessageAdded {
            message: crate::model::ChatMessage {
                id: format!("m{turn}"),
                turn,
                role: MessageRole::User,
                text: text.into(),
            },
        }
    }

    /// The raw log of a chat, as a peer would receive it.
    fn log_values(store: &Store, id: &SessionId) -> Vec<serde_json::Value> {
        let (lines, _) = store.raw_range(id, 0, usize::MAX).unwrap();
        lines
            .iter()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect()
    }

    #[test]
    fn a_batch_is_one_contiguous_dated_append_and_names_the_chat() {
        let mut store = Store::open(tempdir()).unwrap();
        let s = session();
        let events = store
            .append_batch(
                s.id,
                vec![
                    EventPayload::SessionCreated { session: s.clone() },
                    user_message(1, "Fix the login redirect loop\nand add a test"),
                    EventPayload::MessageDelta {
                        turn: 1,
                        text: "On it".into(),
                    },
                ],
            )
            .unwrap();
        assert_eq!(
            events.iter().map(|e| e.seq).collect::<Vec<_>>(),
            vec![0, 1, 2]
        );
        assert!(events.iter().all(|e| e.at.is_some()));
        assert_eq!(
            store.session(&s.id).unwrap().title.as_deref(),
            Some("Fix the login redirect loop")
        );
        assert_eq!(log_values(&store, &s.id).len(), 3);
        assert!(store.append_batch(s.id, vec![]).unwrap().is_empty());
    }

    #[test]
    fn a_copy_grows_only_by_contiguous_replication() {
        let elsewhere = DeviceId::new();
        let mut source = Store::open(tempdir()).unwrap();
        source.set_this_device(Some(elsewhere)).unwrap();
        let mut s = session();
        s.executor = Some(elsewhere);
        source
            .append_batch(
                s.id,
                vec![
                    EventPayload::SessionCreated { session: s.clone() },
                    user_message(1, "Deploy the site"),
                    EventPayload::MessageDelta {
                        turn: 1,
                        text: "Done".into(),
                    },
                ],
            )
            .unwrap();
        let mut log = log_values(&source, &s.id);
        // A newer device's event type rides along verbatim.
        log.push(serde_json::json!({
            "seq": 3, "session_id": s.id.to_string(), "type": "future.event", "x": 1
        }));

        let mut copy = Store::open(tempdir()).unwrap();
        copy.set_this_device(Some(DeviceId::new())).unwrap();
        // Out of order: the first event must be session.created at 0.
        assert!(matches!(
            copy.append_replicated(s.id, elsewhere, log[1..].to_vec()),
            Err(StoreError::Gap { have: 0, got: 1 })
        ));
        let done = copy
            .append_replicated(s.id, elsewhere, log[..2].to_vec())
            .unwrap();
        assert_eq!(
            done,
            ReplicaAppend {
                appended: 2,
                head: 2
            }
        );
        // Overlap is skipped; the rest continues the copy.
        let done = copy
            .append_replicated(s.id, elsewhere, log.clone())
            .unwrap();
        assert_eq!(
            done,
            ReplicaAppend {
                appended: 2,
                head: 4
            }
        );
        assert_eq!(
            log_values(&copy, &s.id),
            log,
            "the copy is the log, verbatim"
        );
        let copied = copy.session(&s.id).unwrap();
        assert_eq!(copied.title.as_deref(), Some("Deploy the site"));
        assert!(!copy.executes_here(copied));

        // A copy can't be written to as if it ran here, and the runner's own
        // log never takes replicated events.
        assert!(matches!(
            copy.append(s.id, user_message(2, "hi")),
            Err(StoreError::Conflict(_))
        ));
        assert!(matches!(
            source.append_replicated(s.id, elsewhere, log.clone()),
            Err(StoreError::Conflict(_))
        ));

        // A chat synced after it started opens with no runner named; the
        // copy takes the runner it was pulled for, never "private here".
        let mut later = session();
        later.executor = None;
        let raw = serde_json::to_value(Event {
            seq: 0,
            session_id: later.id,
            at: None,
            payload: EventPayload::SessionCreated {
                session: later.clone(),
            },
        })
        .unwrap();
        copy.append_replicated(later.id, elsewhere, vec![raw.clone()])
            .unwrap();
        let taken = copy.session(&later.id).unwrap();
        assert_eq!(taken.executor, Some(elsewhere));
        assert!(!copy.executes_here(taken));
        // And a copy can't be claimed for this device or another runner.
        let me = copy.this_device().unwrap();
        assert!(matches!(
            copy.append_replicated(SessionId::new(), me, vec![raw.clone()]),
            Err(StoreError::Conflict(_))
        ));
        assert!(matches!(
            copy.append_replicated(later.id, DeviceId::new(), vec![raw]),
            Err(StoreError::Conflict(_))
        ));
    }

    #[test]
    fn deletion_is_remembered_and_removal_purges_synced_chats_only() {
        let me = DeviceId::new();
        let mut store = Store::open(tempdir()).unwrap();
        store.set_this_device(Some(me)).unwrap();
        let private = session();
        let mut synced = session();
        synced.executor = Some(me);
        let mut other = session();
        other.executor = Some(me);
        for s in [&private, &synced, &other] {
            store
                .append(s.id, EventPayload::SessionCreated { session: s.clone() })
                .unwrap();
        }
        let (summaries, _) = store.summaries();
        assert_eq!(summaries.len(), 2, "only synced chats are summarised");

        let stone = Tombstone { at: 1, by: me };
        store.tombstone(other.id, stone).unwrap();
        assert!(store.session(&other.id).is_err());
        assert!(store.tombstoned(&other.id));
        assert!(matches!(
            store.append(
                other.id,
                EventPayload::SessionCreated {
                    session: other.clone()
                }
            ),
            Err(StoreError::Conflict(_))
        ));
        assert!(
            !store
                .dir
                .join("sessions")
                .join(other.id.to_string())
                .exists()
        );

        assert_eq!(store.purge_synced().unwrap(), 1);
        assert!(store.session(&synced.id).is_err());
        assert!(store.session(&private.id).is_ok(), "private chats stay");
        assert_eq!(store.this_device(), None);
        assert!(!store.tombstoned(&other.id));
    }

    #[test]
    fn a_restart_reconciles_only_chats_running_here_and_tells_peers() {
        let me = DeviceId::new();
        let dir = tempdir();
        let mut store = Store::open(dir.clone()).unwrap();
        store.set_this_device(Some(me)).unwrap();
        let working = EventPayload::SessionStatusChanged {
            status: SessionStatus::Working,
            reason: None,
        };
        let private = session();
        let mut synced = session();
        synced.executor = Some(me);
        for s in [&private, &synced] {
            store
                .append_batch(
                    s.id,
                    vec![
                        EventPayload::SessionCreated { session: s.clone() },
                        working.clone(),
                    ],
                )
                .unwrap();
        }
        // A copy of a chat that is working on another device.
        let elsewhere = DeviceId::new();
        let mut mirrored = session();
        mirrored.executor = Some(elsewhere);
        let log: Vec<serde_json::Value> = [
            EventPayload::SessionCreated {
                session: mirrored.clone(),
            },
            working.clone(),
        ]
        .into_iter()
        .enumerate()
        .map(|(seq, payload)| {
            serde_json::to_value(Event {
                seq: seq as u64,
                session_id: mirrored.id,
                at: Some(1),
                payload,
            })
            .unwrap()
        })
        .collect();
        store
            .append_replicated(mirrored.id, elsewhere, log)
            .unwrap();
        drop(store);

        let store = Store::open(dir).unwrap();
        assert_eq!(
            store.session(&private.id).unwrap().status,
            SessionStatus::Failed
        );
        assert_eq!(log_values(&store, &private.id).len(), 2, "private: silent");
        assert_eq!(
            store.session(&synced.id).unwrap().status,
            SessionStatus::Failed
        );
        let synced_log = log_values(&store, &synced.id);
        assert_eq!(synced_log.len(), 3, "synced: the restart is an event");
        assert_eq!(synced_log[2]["reason"], "daemon restart");
        assert_eq!(
            store.session(&mirrored.id).unwrap().status,
            SessionStatus::Working,
            "a copy waits for its runner"
        );
    }

    #[test]
    fn a_torn_log_heals_and_unrecorded_events_are_reapplied() {
        let dir = tempdir();
        let mut store = Store::open(dir.clone()).unwrap();
        let s = session();
        store
            .append(s.id, EventPayload::SessionCreated { session: s.clone() })
            .unwrap();
        let log = store.event_log(&s.id);
        drop(store);
        // The daemon died after writing an event but before state.json, and
        // again in the middle of the next line.
        let unrecorded = serde_json::to_string(&Event {
            seq: 1,
            session_id: s.id,
            at: Some(5),
            payload: user_message(1, "Rename the module"),
        })
        .unwrap();
        let mut file = fs::OpenOptions::new().append(true).open(&log).unwrap();
        write!(file, "{unrecorded}\n{{\"seq\":2,\"sess").unwrap();
        drop(file);

        let mut store = Store::open(dir).unwrap();
        assert!(
            fs::read_to_string(&log).unwrap().ends_with('\n'),
            "torn line cut"
        );
        assert_eq!(
            store.session(&s.id).unwrap().title.as_deref(),
            Some("Rename the module")
        );
        let next = store.append(s.id, user_message(2, "and test it")).unwrap();
        assert_eq!(next.seq, 2, "no sequence number is reused");
    }

    #[test]
    fn receipts_are_idempotent_by_request_id() {
        let dir = tempdir();
        let mut store = Store::open(dir).unwrap();
        let r = Receipt {
            request_id: "req-1".into(),
            session_id: None,
            status: crate::model::ReceiptStatus::Completed,
            result: Some(serde_json::json!({"ok": true})),
            error: None,
            updated_at: 1,
        };
        assert!(store.record_receipt(r).unwrap());
        let mut dup = Receipt {
            request_id: "req-1".into(),
            session_id: None,
            status: crate::model::ReceiptStatus::Failed,
            result: None,
            error: Some("late".into()),
            updated_at: 2,
        };
        assert!(!store.record_receipt(dup.clone()).unwrap());
        let stored = store.receipt("req-1").unwrap();
        // the first terminal state wins; the duplicate is not re-executed
        assert_eq!(stored.status, crate::model::ReceiptStatus::Completed);
        dup.status = crate::model::ReceiptStatus::Accepted;
        assert!(!store.record_receipt(dup).unwrap());
        assert_eq!(
            store.receipt("req-1").unwrap().status,
            crate::model::ReceiptStatus::Completed
        );
    }

    #[test]
    fn an_accepted_receipt_progresses_to_its_terminal_state() {
        let dir = tempdir();
        let mut store = Store::open(dir).unwrap();
        let accepted = Receipt {
            request_id: "req-2".into(),
            session_id: None,
            status: crate::model::ReceiptStatus::Accepted,
            result: None,
            error: None,
            updated_at: 1,
        };
        assert!(store.record_receipt(accepted).unwrap());
        let completed = Receipt {
            request_id: "req-2".into(),
            session_id: None,
            status: crate::model::ReceiptStatus::Completed,
            result: Some(serde_json::json!({"delivered": true})),
            error: None,
            updated_at: 2,
        };
        assert!(store.record_receipt(completed).unwrap());
        assert_eq!(
            store.receipt("req-2").unwrap().status,
            crate::model::ReceiptStatus::Completed
        );
        // and a second duplicate of the ORIGINAL accepted form changes nothing
        let again = Receipt {
            request_id: "req-2".into(),
            session_id: None,
            status: crate::model::ReceiptStatus::Accepted,
            result: None,
            error: None,
            updated_at: 3,
        };
        assert!(!store.record_receipt(again).unwrap());
        assert_eq!(
            store.receipt("req-2").unwrap().status,
            crate::model::ReceiptStatus::Completed
        );
    }

    #[test]
    fn this_machine_exists_from_the_first_open_and_keeps_its_id() {
        let dir = tempdir();
        let mut store = Store::open(dir.clone()).unwrap();
        let id = store.ensure_this_machine();
        let machine = store.machine(&id).unwrap().clone();
        assert!(machine.this_machine);
        assert_eq!(machine.status, MachineStatus::Online);
        assert_eq!(machine.platform, std::env::consts::OS);

        // Reopen: same machine, same id, no duplicates.
        drop(store);
        let reopened = Store::open(dir).unwrap();
        assert_eq!(reopened.machines().len(), 1);
        assert_eq!(reopened.machine(&id).unwrap().id, id);
    }

    #[test]
    fn added_machines_wait_and_duplicates_are_rejected() {
        let dir = tempdir();
        let mut store = Store::open(dir).unwrap();
        let added = store.create_machine("Build Box", "macos").unwrap();
        assert_eq!(added.status, MachineStatus::Waiting);
        assert_eq!(added.name, "build-box");
        assert!(added.enrollment_token.is_some());
        assert!(!added.this_machine);

        // slug-equal names are duplicates, whatever they were typed as
        assert!(store.create_machine("build  BOX", "linux").is_err());
        assert!(store.create_machine("  ", "linux").is_err());
        assert_eq!(store.machines().len(), 2); // this machine + the waiting one

        // waiting machines go away; this machine never does
        store.remove_machine(&added.id).unwrap();
        assert_eq!(store.machines().len(), 1);
        let this_id = store.ensure_this_machine();
        assert!(store.remove_machine(&this_id).is_err());
    }

    #[test]
    fn presence_folds_into_the_24_hour_band() {
        let dir = tempdir();
        let mut store = Store::open(dir).unwrap();
        let id = store.ensure_this_machine();

        // present right now: the last slice is up, the rest is history
        let band = store.presence(&id);
        assert_eq!(band.len(), 48);
        assert!(band[47]);

        // marking again within the same half hour changes nothing
        let before = store.presence(&id);
        store.mark_present(&id).unwrap();
        assert_eq!(store.presence(&id), before);

        // a machine that never checked in has an all-down band
        let added = store.create_machine("spare", "linux").unwrap();
        assert!(!store.presence(&added.id).iter().any(|up| *up));
    }
}
