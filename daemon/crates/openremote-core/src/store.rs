use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::PathBuf;

use thiserror::Error;

/// Millis in half an hour — the uptime band's slice size.
const HALF_HOUR_MS: i64 = 1_800_000;

/// The grid's hostname slug: lowercase, alphanumerics and single dashes
/// (runs of separators collapse — `Build  BOX` and `build-box` are the
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
use crate::ids::{DecisionId, MachineId, RuleId, SessionId};
use crate::model::{
    AutomationRule, Decision, DecisionState, Machine, MachineStatus, Plugin, Receipt,
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
}

/// Sessions, decisions, receipts — the durable daemon state.
///
/// Persistence: per-session append-only `events.jsonl` plus an atomically
/// rewritten `state.json`. No database in slice 1. On open, sessions that
/// were alive when the daemon died go to `failed` with reason
/// `daemon restart`, and their pending decisions retire silently — the
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
}

pub struct Store {
    dir: PathBuf,
    state: State,
}

impl Store {
    /// Open (or create) the store under `dir`; reconciles crashed sessions.
    pub fn open(dir: PathBuf) -> Result<Self, StoreError> {
        fs::create_dir_all(&dir)?;
        let mut state = match fs::read_to_string(dir.join("state.json")) {
            Ok(raw) => {
                serde_json::from_str(&raw).map_err(|e| StoreError::BadState(e.to_string()))?
            }
            Err(_) => State::default(),
        };
        let now = crate::now_ms();
        let crashed: Vec<(SessionId, SessionStatus)> = state
            .sessions
            .iter()
            .filter(|(_, s)| s.status.is_alive())
            .map(|(id, s)| (*id, s.status))
            .collect();
        for (id, was) in &crashed {
            // The process died with the daemon. Mid-turn deaths lose work
            // (failed, reason recorded); quiet ones just need a resume (stopped).
            let now_status = if matches!(was, SessionStatus::Working | SessionStatus::Waiting) {
                SessionStatus::Failed
            } else {
                SessionStatus::Stopped
            };
            if let Some(session) = state.sessions.get_mut(id) {
                session.status = now_status;
                session.last_error = Some("daemon restart".to_string());
                session.updated_at = now;
            }
            // Retire silently — no event is ever invented for this.
            for decision in state.decisions.values_mut() {
                if decision.session_id == *id && decision.state == DecisionState::Pending {
                    decision.state = DecisionState::Retired;
                }
            }
        }
        let mut store = Self { dir, state };
        let had_this_machine = store.machines().iter().any(|m| m.this_machine);
        let this_id = store.ensure_this_machine();
        // Opening the store means the daemon is running — present now.
        store.mark_present(&this_id).ok();
        if !crashed.is_empty() || !had_this_machine {
            store.persist()?;
        }
        Ok(store)
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
        let hostname = std::env::var("COMPUTERNAME")
            .or_else(|_| std::env::var("HOSTNAME"))
            .unwrap_or_else(|_| "this-computer".to_string());
        let machine = Machine {
            id,
            name: slug(&hostname),
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

    /// Add a machine: it lands `waiting` — it comes online when its agent
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
    /// not history — nothing renders from the raw list; `presence` folds
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
    /// change only — machine reachability is the caller's ruling.
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

    /// Record that the machine has the key this plugin needs — the key
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
    /// state (the same request completing); a terminal receipt is final —
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

    /// Mark all pending decisions of a session retired — silently, by contract.
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

    /// Record the harness's own session id (metadata, not history — resume
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
    /// Metadata, not history — nothing here is an event.
    pub fn note_session(
        &mut self,
        session_id: &SessionId,
        harness_ref: Option<String>,
        model: Option<String>,
        permission_mode: Option<String>,
    ) -> Result<(), StoreError> {
        let session = self
            .state
            .sessions
            .get_mut(session_id)
            .ok_or_else(|| StoreError::NotFound(format!("session {session_id}")))?;
        if let Some(r) = harness_ref {
            session.harness_session_ref = Some(r);
        }
        if model.is_some() {
            session.model = model;
        }
        if permission_mode.is_some() {
            session.permission_mode = permission_mode;
        }
        session.updated_at = crate::now_ms();
        self.persist()
    }

    /// Record a tool the harness granted for the rest of the session (its
    /// own session-scope answer word). Facts, not history — the
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

    /// Append an event: assigns `seq`, applies it to state, persists both.
    pub fn append(
        &mut self,
        session_id: SessionId,
        payload: EventPayload,
    ) -> Result<Event, StoreError> {
        // Only session.created may name a session the state doesn't know —
        // it is the event that inserts it.
        let creating = matches!(payload, EventPayload::SessionCreated { .. });
        if !creating && !self.state.sessions.contains_key(&session_id) {
            return Err(StoreError::NotFound(format!("session {session_id}")));
        }
        let seq = self.state.next_seq.entry(session_id).or_insert(0);
        let event = Event {
            seq: *seq,
            session_id,
            payload,
        };
        *seq += 1;

        let json =
            serde_json::to_string(&event).map_err(|e| StoreError::BadState(e.to_string()))?;
        self.apply(&event)?;
        let log_dir = self.dir.join("sessions").join(session_id.to_string());
        fs::create_dir_all(&log_dir)?;
        let mut log = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(log_dir.join("events.jsonl"))?;
        log.write_all(json.as_bytes())?;
        log.write_all(b"\n")?;
        self.persist()?;
        Ok(event)
    }

    /// Events after a cursor. `None` replays from the beginning (a fresh
    /// SSE connect); `Some(seq)` returns strictly later events (a resume —
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
        let now = crate::now_ms();
        match &event.payload {
            EventPayload::SessionCreated { session } => {
                self.state.sessions.insert(session.id, session.clone());
            }
            EventPayload::SessionUpdated { session } => {
                self.state.sessions.insert(session.id, session.clone());
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
            | EventPayload::MessageAdded { .. }
            | EventPayload::MessageDelta { .. }
            | EventPayload::ToolStarted { .. }
            | EventPayload::ToolResult { .. }
            | EventPayload::DaemonError { .. } => {
                if let Some(session) = self.state.sessions.get_mut(&event.session_id) {
                    session.updated_at = now;
                }
            }
        }
        Ok(())
    }

    fn persist(&self) -> Result<(), StoreError> {
        let tmp = self.dir.join("state.json.tmp");
        let mut f = fs::File::create(&tmp)?;
        let json = serde_json::to_string_pretty(&self.state)
            .map_err(|e| StoreError::BadState(e.to_string()))?;
        f.write_all(json.as_bytes())?;
        f.sync_all()?;
        drop(f);
        fs::rename(&tmp, self.dir.join("state.json"))?;
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
            permission_mode: None,
            harness_session_ref: None,
            created_at: 0,
            updated_at: 0,
            last_error: None,
            next_turn: 1,
            fast: false,
            approved_tools: Vec::new(),
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
