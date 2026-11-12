use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::PathBuf;

use thiserror::Error;

/// Millis in half an hour - the uptime band's slice size.
const HALF_HOUR_MS: i64 = 1_800_000;

/// Receipts buy idempotency for client retries, and retries come back
/// within minutes, not years - so the table is bounded. Past the cap the
/// oldest go; a request older than the window replaying executes again,
/// exactly as it would on a machine that never saw the first attempt.
const RECEIPT_CAP: usize = 10_000;

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
use crate::ids::{DecisionId, DeviceId, MachineId, ProjectId, RuleId, SessionId};
use crate::model::{
    AutomationRule, Decision, DecisionState, GlobalPluginRecord, InstalledPlugin, Machine,
    MachineStatus, MarketplaceRecord, McpServerEntry, MessageRole, ModelEndpoint, Project, Receipt,
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

/// Just a line's sequence number: enough to follow a log's continuity
/// without parsing (or this build knowing) the whole event.
#[derive(serde::Deserialize)]
struct SeqLine {
    seq: u64,
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
    /// Read under its old name too: these were "plugins" before the
    /// plugin system took that name.
    #[serde(default, alias = "plugins")]
    mcp_servers: BTreeMap<String, McpServerEntry>,
    /// Custom model endpoints on this machine, by endpoint id. Secrets
    /// ride inside the owner-only state file and never leave it through
    /// views, events, receipts, or errors.
    #[serde(default)]
    model_endpoints: BTreeMap<String, ModelEndpoint>,
    /// The plugins the user installed on this machine, by id
    /// (`name@marketplace`). The first builds keyed them by bare name; open
    /// re-keys those.
    #[serde(default)]
    installed_plugins: BTreeMap<String, InstalledPlugin>,
    /// The built-in harness plugins the user turned off.
    #[serde(default)]
    disabled_builtins: BTreeSet<String>,
    /// The account's Global plugins as this device last heard of them, by
    /// id - removed ones included, so a removal reaches every device. A
    /// record installs nothing (see `GlobalPluginRecord`).
    #[serde(default)]
    global_plugins: BTreeMap<String, GlobalPluginRecord>,
    /// The plugin marketplaces on this machine, by catalog name.
    #[serde(default)]
    marketplaces: BTreeMap<String, MarketplaceRecord>,
    /// The marketplaces OpenRemote added on its own, ever - so one the
    /// user removed is not added back on the next start.
    #[serde(default)]
    seeded_marketplaces: BTreeSet<String>,
    #[serde(default)]
    rules: BTreeMap<RuleId, AutomationRule>,
    #[serde(default)]
    projects: BTreeMap<ProjectId, Project>,
    /// This computer's id in Cloud mode, once it has one.
    #[serde(default)]
    this_device: Option<DeviceId>,
    #[serde(default)]
    tombstones: BTreeMap<SessionId, Tombstone>,
}

pub struct Store {
    dir: PathBuf,
    state: State,
    /// The chats' pictures, beside the store (`<data_dir>/blobs`): shared
    /// out so a picture is read and written without the store's lock.
    blobs: std::sync::Arc<crate::blobs::Blobs>,
}

impl Store {
    /// Open (or create) the store under `dir`; heals torn logs and
    /// reconciles chats that were running when the daemon died. An
    /// unparseable `state.json` is quarantined beside the store (never
    /// deleted) and the session logs are replayed to re-derive what the
    /// events carry - the logs are the history, the state was the
    /// derived copy.
    pub fn open(dir: PathBuf) -> Result<Self, StoreError> {
        fs::create_dir_all(&dir)?;
        let state = match fs::read_to_string(dir.join("state.json")) {
            Ok(raw) => match serde_json::from_str(&raw) {
                Ok(state) => state,
                Err(e) => {
                    // Booting into a panic loop until the user
                    // hand-deletes the file (losing everything) is the
                    // worse failure. Move the bad file aside - if even
                    // that fails, say so loudly instead of overwriting
                    // the user's data with fresh state.
                    let bad = dir.join(format!("state.json.bad-{}", crate::now_ms()));
                    fs::rename(dir.join("state.json"), &bad).map_err(|io| {
                        StoreError::BadState(format!(
                            "{e}; quarantining it to {} failed: {io}",
                            bad.display()
                        ))
                    })?;
                    eprintln!(
                        "warning: state.json was corrupt ({e}); it is quarantined at {} and the session logs replay",
                        bad.display()
                    );
                    State::default()
                }
            },
            Err(_) => State::default(),
        };
        let blobs = std::sync::Arc::new(crate::blobs::Blobs::new(&dir));
        let mut store = Self { dir, state, blobs };
        let rekeyed = store.rekey_installed_plugins();
        let contributed = store.backfill_plugin_contributions();
        let healed = store.heal_logs()?;
        let discovered = store.discover_logs()?;
        let pruned = store.prune_receipts();
        let reconciled = store.reconcile_crashed()?;
        let titled = store.backfill_titles();
        let had_this_machine = store.machines().iter().any(|m| m.this_machine);
        let this_id = store.ensure_this_machine();
        // Opening the store means the daemon is running - present now.
        store.mark_present(&this_id).ok();
        if rekeyed
            || contributed
            || healed
            || discovered
            || pruned
            || reconciled
            || titled
            || !had_this_machine
        {
            store.persist()?;
        }
        Ok(store)
    }

    /// The folder the store lives in - the daemon's data dir.
    pub fn dir(&self) -> &std::path::Path {
        &self.dir
    }

    /// The chats' pictures. A chat's go when the chat does.
    pub fn blobs(&self) -> std::sync::Arc<crate::blobs::Blobs> {
        std::sync::Arc::clone(&self.blobs)
    }

    /// The first plugin builds keyed installs by bare name, before plugins
    /// had marketplaces: file each under its id (`name@local`, the serde
    /// default), so the same name from a marketplace can sit beside it.
    fn rekey_installed_plugins(&mut self) -> bool {
        if self
            .state
            .installed_plugins
            .iter()
            .all(|(key, plugin)| *key == plugin.id())
        {
            return false;
        }
        let plugins = std::mem::take(&mut self.state.installed_plugins);
        self.state.installed_plugins = plugins.into_values().map(|p| (p.id(), p)).collect();
        true
    }

    /// An install saved before plugins had `contributes` has an empty one,
    /// though its record names the harness it provides: carry that harness
    /// over, so the review and the list say what the plugin adds. Only that
    /// one summary is derived - from what the user approved at install, never
    /// from the plugin's folder. A record that already lists its harnesses is
    /// left as it is.
    fn backfill_plugin_contributions(&mut self) -> bool {
        let mut changed = false;
        for plugin in self.state.installed_plugins.values_mut() {
            if let Some(harness) = &plugin.harness {
                if plugin.contributes.harnesses.is_empty() {
                    plugin.contributes.harnesses.push(harness.clone());
                    changed = true;
                }
            }
        }
        changed
    }

    /// A crash can tear the last line of a log, or land a batch in the log
    /// that state.json never recorded (the log is written first). Power
    /// loss can do the reverse - state.json's rewrite reached the disk
    /// while the log's append didn't - leaving `next_seq` past what the
    /// log really holds. A chat whose log skips sequence numbers wedges
    /// every peer pull on it forever (gap, pull, same gap, repeat), so a
    /// log must stay contiguous from its first line to its last: the torn
    /// tail is cut, anything after a discontinuity is cut (the discarded
    /// bytes move to an `events.jsonl.bad-<unix-ms>` sibling, never
    /// deleted), and `next_seq` settles on the log's true tail whichever
    /// side it was on.
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
            let recorded = self.state.next_seq.get(&id).copied().unwrap_or(0);
            let len = file.metadata()?.len();
            if len == 0 {
                // A durably empty log that state credits with events is
                // the state-ahead case at its extreme: everything was lost.
                if recorded > 0 {
                    self.state.next_seq.insert(id, 0);
                    changed = true;
                }
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
            let (run_next, cut_at) = self.heal_log_window(&id, &tail, start, recorded)?;
            // `run_next` is the seq after the log's contiguous run - the
            // replay target when the state lagged, the rewind target when
            // it didn't.
            if let Some(run_next) = run_next.filter(|next| *next != recorded) {
                self.state.next_seq.insert(id, run_next);
                changed = true;
            }
            if let Some(cut_at) = cut_at {
                self.salvage_cut(&id, &mut file, cut_at)?;
                changed = true;
            }
        }
        Ok(changed)
    }

    /// Walk a healed window of one chat's log: apply to the state every
    /// event of the contiguous run at or past `recorded`, and report
    /// `(the seq after the run's last line, the file offset just past
    /// it)`. The offset is set only when something follows the run that
    /// it disowns - a line without a parsable seq, one that breaks the
    /// sequence, or one naming another chat - so the caller cuts there.
    /// `start` is the window's offset in the file; a window that opens
    /// mid-line skips that partial first line.
    fn heal_log_window(
        &mut self,
        id: &SessionId,
        window: &[u8],
        start: u64,
        recorded: u64,
    ) -> Result<(Option<u64>, Option<u64>), StoreError> {
        // An empty window that starts at 0 sees a whole empty log;
        // otherwise the log's tail is simply out of sight.
        if window.is_empty() {
            return Ok(((start == 0).then_some(0), None));
        }
        let mut expected: Option<u64> = None; // the seq the run wants next
        let mut run_next = None; // the seq after the run's last line so far
        let mut run_end = None; // the file offset just past that line
        let mut cut_at = None;
        let mut pos = start;
        let mut partial = start > 0;
        for segment in window.split_inclusive(|b| *b == b'\n') {
            let after = pos + segment.len() as u64;
            let line = match segment.strip_suffix(b"\n") {
                Some(_skipped) if partial => {
                    // The window's first bytes are half a line: skip them.
                    partial = false;
                    pos = after;
                    continue;
                }
                Some(line) => line,
                // Trailing bytes without a newline: the window ends
                // mid-line, so the run stops at its last complete line.
                // When the torn-tail pass already made that cut, the
                // salvage guard below makes landing on it a no-op (the
                // same boundary, nothing left to cut); when a window
                // arrives with its torn tail still in it, this is what
                // cuts it correctly. Do not remove the branch on the
                // assumption it cannot fire - it is the fallback that
                // keeps this function total over windows.
                None => {
                    cut_at = run_end;
                    break;
                }
            };
            pos = after;
            let text = match std::str::from_utf8(line) {
                Ok(text) => text,
                Err(_) => {
                    cut_at = run_end;
                    break;
                }
            };
            let seq = match serde_json::from_str::<SeqLine>(text) {
                Ok(SeqLine { seq }) => seq,
                // A line without a seq ends the reliable log.
                Err(_) => {
                    cut_at = run_end;
                    break;
                }
            };
            if expected.is_some_and(|want| seq != want) {
                // A skipped number: no peer can ever be sent the missing
                // range, so the run (and the log) ends before it.
                cut_at = run_end;
                break;
            }
            // Replay what the state never recorded. Events this build
            // doesn't know stay in the log; the state just skips them.
            if seq >= recorded {
                if let Ok(event) = serde_json::from_str::<Event>(text) {
                    if event.session_id != *id {
                        // A line naming another chat doesn't belong in
                        // this log; the run ends before it.
                        cut_at = run_end;
                        break;
                    }
                    self.apply(&event)?;
                }
            }
            expected = Some(seq + 1);
            run_next = Some(seq + 1);
            run_end = Some(after);
        }
        Ok((run_next, cut_at))
    }

    /// Cut a chat's log back to `cut_at`, keeping what leaves: the bytes
    /// past the cut are real message content (events past the contiguous
    /// run), so they are appended to an `events.jsonl.bad-<unix-ms>`
    /// sibling beside the log - cut from the log, never deleted.
    /// Appending (not renaming) lets more than one cut in the same open
    /// share a single recovery file, and re-deriving a cut that already
    /// landed is a no-op (`cut_at >= len`), so the heal passes stay
    /// idempotent.
    fn salvage_cut(
        &self,
        session_id: &SessionId,
        file: &mut fs::File,
        cut_at: u64,
    ) -> Result<(), StoreError> {
        let len = file.metadata()?.len();
        if cut_at >= len {
            return Ok(());
        }
        file.seek(SeekFrom::Start(cut_at))?;
        let mut discarded = Vec::new();
        file.read_to_end(&mut discarded)?;
        let bad = self
            .event_log(session_id)
            .with_file_name(format!("events.jsonl.bad-{}", crate::now_ms()));
        fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(bad)?
            .write_all(&discarded)?;
        file.set_len(cut_at)?;
        Ok(())
    }

    /// Chats the state doesn't know but whose logs exist on disk: a lost
    /// or quarantined state.json. Each log replays in full from its first
    /// line, re-deriving what its events carry. A log that doesn't open
    /// with `session.created` at seq 0 can't be re-derived and is left
    /// untouched.
    fn discover_logs(&mut self) -> Result<bool, StoreError> {
        let Ok(entries) = fs::read_dir(self.dir.join("sessions")) else {
            return Ok(false);
        };
        let mut changed = false;
        for entry in entries.flatten() {
            let Some(id) = entry.file_name().to_str().and_then(SessionId::parse) else {
                continue;
            };
            if self.state.sessions.contains_key(&id) {
                continue;
            }
            if self.state.tombstones.contains_key(&id) {
                // Deleted everywhere: its log can outlive its session
                // record (a delete interrupted by the filesystem leaves
                // the dir behind), but it never brings the chat back.
                eprintln!(
                    "warning: session {id} was deleted everywhere; its orphan log stays dormant"
                );
                continue;
            }
            let Ok(mut file) = fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open(self.event_log(&id))
            else {
                eprintln!("warning: session {id} has a log the store can't read");
                continue;
            };
            let len = file.metadata()?.len();
            if len == 0 {
                continue;
            }
            file.seek(SeekFrom::Start(0))?;
            let mut raw = Vec::new();
            file.read_to_end(&mut raw)?;
            if raw.last() != Some(&b'\n') {
                match raw.iter().rposition(|b| *b == b'\n') {
                    Some(i) => {
                        file.set_len(i as u64 + 1)?;
                        raw.truncate(i + 1);
                        changed = true;
                    }
                    // One torn line: nothing to replay, nothing to cut.
                    None => {
                        file.set_len(0)?;
                        continue;
                    }
                }
            }
            // The whole file is the window, so the run must open with the
            // chat's own `session.created` - the event that re-inserts the
            // chat - or there is nothing to re-derive from.
            let opens_chat = raw
                .split(|b| *b == b'\n')
                .next()
                .and_then(|line| std::str::from_utf8(line).ok())
                .and_then(|text| serde_json::from_str::<Event>(text).ok())
                .is_some_and(|event| {
                    event.seq == 0
                        && event.session_id == id
                        && matches!(event.payload, EventPayload::SessionCreated { .. })
                });
            if !opens_chat {
                eprintln!(
                    "warning: session {id}'s log doesn't open with session.created; left as is"
                );
                continue;
            }
            let (run_next, cut_at) = self.heal_log_window(&id, &raw, 0, 0)?;
            if let Some(run_next) = run_next {
                self.state.next_seq.insert(id, run_next);
                if let Some(cut_at) = cut_at {
                    self.salvage_cut(&id, &mut file, cut_at)?;
                }
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
                            if message.role == MessageRole::User
                                && !message.text.trim().is_empty() =>
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
    /// Permission is start-time everywhere - the write lands on the next
    /// resume, never on a live process.
    #[allow(clippy::too_many_arguments)]
    pub fn update_session_settings(
        &mut self,
        id: &SessionId,
        model: Option<String>,
        effort: Option<String>,
        fast: Option<bool>,
        permission_mode: Option<String>,
    ) -> Result<Session, StoreError> {
        let session = self
            .state
            .sessions
            .get_mut(id)
            .ok_or_else(|| StoreError::NotFound(format!("session {id}")))?;
        if let Some(model) = model {
            let trimmed = model.trim().to_string();
            if !trimmed.is_empty() {
                crate::model::validate_model_id(&trimmed).map_err(StoreError::Conflict)?;
                session.model = Some(trimmed);
            } else {
                session.model = None;
            }
        }
        if effort.is_some() {
            session.effort = effort;
        }
        if let Some(fast) = fast {
            session.fast = fast;
        }
        if permission_mode.is_some() {
            session.permission_mode = permission_mode;
        }
        session.updated_at = crate::now_ms();
        let updated = session.clone();
        self.persist()?;
        Ok(updated)
    }

    /// Put a chat's model, effort and fast back exactly as they were, `None`
    /// included. A live harness that refuses a change it was handed has the
    /// change undone this way, so the store never claims a model the harness
    /// did not take.
    pub fn restore_session_settings(
        &mut self,
        id: &SessionId,
        model: Option<String>,
        effort: Option<String>,
        fast: bool,
    ) -> Result<Session, StoreError> {
        let session = self
            .state
            .sessions
            .get_mut(id)
            .ok_or_else(|| StoreError::NotFound(format!("session {id}")))?;
        session.model = model;
        session.effort = effort;
        session.fast = fast;
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

    pub fn installed_plugins(&self) -> Vec<&InstalledPlugin> {
        self.state.installed_plugins.values().collect()
    }

    /// One installed plugin by id (`name@marketplace`).
    pub fn installed_plugin(&self, id: &str) -> Result<&InstalledPlugin, StoreError> {
        self.state
            .installed_plugins
            .get(id)
            .ok_or_else(|| StoreError::NotFound(format!("plugin {id}")))
    }

    /// Record an install - a new plugin, or an update of one by id. A
    /// project install names its project: without one it would take the
    /// plain id, this machine's own install of the plugin, and replace it.
    pub fn put_installed_plugin(&mut self, plugin: InstalledPlugin) -> Result<(), StoreError> {
        if plugin.scope.is_project() && plugin.project.is_none() {
            return Err(StoreError::Conflict(format!(
                "{} is for a project but names none",
                plugin.name
            )));
        }
        self.state.installed_plugins.insert(plugin.id(), plugin);
        self.persist()
    }

    pub fn remove_installed_plugin(&mut self, id: &str) -> Result<(), StoreError> {
        if self.state.installed_plugins.remove(id).is_none() {
            return Err(StoreError::NotFound(format!("plugin {id}")));
        }
        self.persist()
    }

    pub fn set_installed_plugin_enabled(
        &mut self,
        id: &str,
        enabled: bool,
    ) -> Result<InstalledPlugin, StoreError> {
        let plugin = self
            .state
            .installed_plugins
            .get_mut(id)
            .ok_or_else(|| StoreError::NotFound(format!("plugin {id}")))?;
        plugin.enabled = enabled;
        plugin.updated_at = crate::now_ms();
        let out = plugin.clone();
        self.persist()?;
        Ok(out)
    }

    // ---- Global plugins: the account's records ----

    /// This device's own write: it replaces whatever is held.
    pub fn put_global_plugin(&mut self, record: GlobalPluginRecord) -> Result<(), StoreError> {
        self.state.global_plugins.insert(record.id(), record);
        self.persist()
    }

    /// Off, for another device's removal, without the time a person's own
    /// change gets: the record that turned it off stays the newer word
    /// until someone here installs it again or turns it back on.
    pub fn turn_off_installed_plugin_keeping_time(
        &mut self,
        id: &str,
    ) -> Result<InstalledPlugin, StoreError> {
        let plugin = self
            .state
            .installed_plugins
            .get_mut(id)
            .ok_or_else(|| StoreError::NotFound(format!("plugin {id}")))?;
        plugin.enabled = false;
        let out = plugin.clone();
        self.persist()?;
        Ok(out)
    }

    pub fn global_plugins(&self) -> Vec<&GlobalPluginRecord> {
        self.state.global_plugins.values().collect()
    }

    pub fn global_plugin(&self, id: &str) -> Option<&GlobalPluginRecord> {
        self.state.global_plugins.get(id)
    }

    /// Keep a record when it is a later write than the one held (or the
    /// first of its id). True when it was kept.
    pub fn merge_global_plugin(&mut self, record: GlobalPluginRecord) -> Result<bool, StoreError> {
        let id = record.id();
        if let Some(held) = self.state.global_plugins.get(&id) {
            if !record.newer_than(held) {
                return Ok(false);
            }
        }
        self.state.global_plugins.insert(id, record);
        self.persist()?;
        Ok(true)
    }

    pub fn builtin_enabled(&self, id: &str) -> bool {
        !self.state.disabled_builtins.contains(id)
    }

    pub fn set_builtin_enabled(&mut self, id: &str, enabled: bool) -> Result<(), StoreError> {
        if enabled {
            self.state.disabled_builtins.remove(id);
        } else {
            self.state.disabled_builtins.insert(id.to_string());
        }
        self.persist()
    }

    // ---- plugin marketplaces ----

    pub fn marketplaces(&self) -> Vec<&MarketplaceRecord> {
        self.state.marketplaces.values().collect()
    }

    pub fn marketplace(&self, name: &str) -> Result<&MarketplaceRecord, StoreError> {
        self.state
            .marketplaces
            .get(name)
            .ok_or_else(|| StoreError::NotFound(format!("marketplace {name}")))
    }

    /// Record a marketplace - newly added, or refreshed, by name.
    pub fn put_marketplace(&mut self, record: MarketplaceRecord) -> Result<(), StoreError> {
        self.state.marketplaces.insert(record.name.clone(), record);
        self.persist()
    }

    pub fn remove_marketplace(&mut self, name: &str) -> Result<(), StoreError> {
        if self.state.marketplaces.remove(name).is_none() {
            return Err(StoreError::NotFound(format!("marketplace {name}")));
        }
        self.persist()
    }

    /// Add one of OpenRemote's own marketplaces - once, ever: one the user
    /// removed stays removed. True when it was added now.
    pub fn seed_marketplace(&mut self, record: MarketplaceRecord) -> Result<bool, StoreError> {
        if self.state.seeded_marketplaces.contains(&record.name) {
            return Ok(false);
        }
        self.state.seeded_marketplaces.insert(record.name.clone());
        self.state
            .marketplaces
            .entry(record.name.clone())
            .or_insert(record);
        self.persist()?;
        Ok(true)
    }

    // ---- MCP servers ----

    pub fn mcp_servers(&self) -> Vec<&McpServerEntry> {
        self.state.mcp_servers.values().collect()
    }

    pub fn mcp_server(&self, id: &str) -> Result<&McpServerEntry, StoreError> {
        self.state
            .mcp_servers
            .get(id)
            .ok_or_else(|| StoreError::NotFound(format!("MCP server {id}")))
    }

    /// Add an MCP server on a machine. Marketplace installs dedup by
    /// `catalog@machine`; custom ones by name per machine. The state
    /// change only - machine reachability is the caller's ruling.
    pub fn add_mcp_server(
        &mut self,
        mut entry: McpServerEntry,
    ) -> Result<McpServerEntry, StoreError> {
        let dup = match &entry.catalog_id {
            Some(_) => self.state.mcp_servers.contains_key(&entry.id),
            None => self.state.mcp_servers.values().any(|p| {
                p.catalog_id.is_none() && p.machine == entry.machine && p.name == entry.name
            }),
        };
        if dup {
            return Err(StoreError::Conflict(format!(
                "'{}' is already on that machine",
                entry.name
            )));
        }
        entry.updated_at = crate::now_ms();
        let id = entry.id.clone();
        self.state.mcp_servers.insert(id, entry.clone());
        self.persist()?;
        Ok(entry)
    }

    pub fn remove_mcp_server(&mut self, id: &str) -> Result<(), StoreError> {
        if self.state.mcp_servers.remove(id).is_none() {
            return Err(StoreError::NotFound(format!("MCP server {id}")));
        }
        self.persist()
    }

    pub fn set_mcp_server_enabled(
        &mut self,
        id: &str,
        enabled: bool,
    ) -> Result<McpServerEntry, StoreError> {
        let entry = self
            .state
            .mcp_servers
            .get_mut(id)
            .ok_or_else(|| StoreError::NotFound(format!("MCP server {id}")))?;
        entry.enabled = enabled;
        entry.updated_at = crate::now_ms();
        let out = entry.clone();
        self.persist()?;
        Ok(out)
    }

    /// Record that the machine has the key this entry needs - the key
    /// itself never crosses this API.
    pub fn acknowledge_mcp_server_key(&mut self, id: &str) -> Result<McpServerEntry, StoreError> {
        let entry = self
            .state
            .mcp_servers
            .get_mut(id)
            .ok_or_else(|| StoreError::NotFound(format!("MCP server {id}")))?;
        if !entry.needs_key {
            return Err(StoreError::Conflict(format!(
                "'{}' does not need a key",
                entry.name
            )));
        }
        entry.has_key = true;
        entry.updated_at = crate::now_ms();
        let out = entry.clone();
        self.persist()?;
        Ok(out)
    }

    // ---- custom model endpoints ----

    pub fn model_endpoints(&self, harness: Option<&str>) -> Vec<&ModelEndpoint> {
        self.state
            .model_endpoints
            .values()
            .filter(|entry| harness.is_none_or(|harness| entry.harness == harness))
            .collect()
    }

    pub fn model_endpoint(&self, id: &str) -> Result<&ModelEndpoint, StoreError> {
        self.state
            .model_endpoints
            .get(id)
            .ok_or_else(|| StoreError::NotFound(format!("model endpoint {id}")))
    }

    pub fn default_model_endpoint(&self, harness: &str) -> Option<&ModelEndpoint> {
        self.state
            .model_endpoints
            .values()
            .find(|entry| entry.harness == harness && entry.enabled && entry.is_default)
    }

    /// Resolve the endpoint a chat should use: the named endpoint when
    /// given, else the harness's machine default. A named endpoint must
    /// belong to the chat's harness and be enabled.
    pub fn resolve_model_endpoint(
        &self,
        harness: &str,
        endpoint_id: Option<&str>,
    ) -> Result<Option<ModelEndpoint>, StoreError> {
        match endpoint_id {
            Some(id) => {
                let entry = self.model_endpoint(id)?.clone();
                if entry.harness != harness {
                    return Err(StoreError::Conflict(format!(
                        "endpoint '{}' belongs to {}, not {harness}",
                        entry.name, entry.harness
                    )));
                }
                if !entry.enabled {
                    return Err(StoreError::Conflict(format!(
                        "endpoint '{}' is off",
                        entry.name
                    )));
                }
                Ok(Some(entry))
            }
            None => Ok(self.default_model_endpoint(harness).cloned()),
        }
    }

    /// Create or replace a custom endpoint record. Validation lives here
    /// so every caller gets the same bar; the secret itself is stored in
    /// the owner-only state file and only ever leaves through resolved
    /// harness execution.
    pub fn put_model_endpoint(
        &mut self,
        mut entry: ModelEndpoint,
    ) -> Result<ModelEndpoint, StoreError> {
        if entry.id.trim().is_empty() {
            return Err(StoreError::Conflict(
                "a model endpoint needs an id".to_string(),
            ));
        }
        crate::model::validate_endpoint_harness(&entry.harness).map_err(StoreError::Conflict)?;
        crate::model::validate_endpoint_name(&entry.name).map_err(StoreError::Conflict)?;
        entry.base_url = crate::model::normalize_endpoint_base_url(&entry.base_url)
            .map_err(StoreError::Conflict)?;
        entry.models_path = entry
            .models_path
            .as_deref()
            .map(crate::model::normalize_endpoint_models_path)
            .transpose()
            .map_err(StoreError::Conflict)?;
        if let Some(model) = entry.default_model.as_deref() {
            crate::model::validate_model_id(model).map_err(StoreError::Conflict)?;
            entry.default_model = Some(model.trim().to_string());
        }
        entry.name = entry.name.trim().to_string();
        match entry.auth_kind {
            crate::model::EndpointAuthKind::None => {
                if entry.key.as_ref().is_some_and(|key| !key.is_empty()) {
                    return Err(StoreError::Conflict(format!(
                        "endpoint '{}' is keyless - remove its key",
                        entry.name
                    )));
                }
                entry.key = None;
            }
            crate::model::EndpointAuthKind::Bearer | crate::model::EndpointAuthKind::ApiKey => {
                if entry.key.as_ref().is_none_or(|key| key.is_empty()) {
                    return Err(StoreError::Conflict(format!(
                        "endpoint '{}' needs a key",
                        entry.name
                    )));
                }
            }
        }
        let duplicate = self.state.model_endpoints.values().any(|other| {
            other.id != entry.id
                && other.harness == entry.harness
                && other.name.eq_ignore_ascii_case(&entry.name)
        });
        if duplicate {
            return Err(StoreError::Conflict(format!(
                "a {} endpoint named '{}' already exists",
                entry.harness, entry.name
            )));
        }
        if entry.is_default {
            for other in self.state.model_endpoints.values_mut() {
                if other.harness == entry.harness && other.id != entry.id {
                    other.is_default = false;
                }
            }
        }
        entry.updated_at = crate::now_ms();
        self.state
            .model_endpoints
            .insert(entry.id.clone(), entry.clone());
        self.persist()?;
        Ok(entry)
    }

    pub fn remove_model_endpoint(&mut self, id: &str) -> Result<(), StoreError> {
        if self.state.model_endpoints.remove(id).is_none() {
            return Err(StoreError::NotFound(format!("model endpoint {id}")));
        }
        self.persist()
    }

    pub fn set_model_endpoint_enabled(
        &mut self,
        id: &str,
        enabled: bool,
    ) -> Result<ModelEndpoint, StoreError> {
        let mut entry = self.model_endpoint(id)?.clone();
        entry.enabled = enabled;
        if !enabled {
            entry.is_default = false;
        }
        self.put_model_endpoint(entry)
    }

    pub fn set_model_endpoint_default(
        &mut self,
        harness: &str,
        id: Option<&str>,
    ) -> Result<Option<ModelEndpoint>, StoreError> {
        crate::model::validate_endpoint_harness(harness).map_err(StoreError::Conflict)?;
        if let Some(id) = id {
            let mut entry = self.model_endpoint(id)?.clone();
            if entry.harness != harness {
                return Err(StoreError::Conflict(format!(
                    "endpoint '{}' belongs to {}, not {harness}",
                    entry.name, entry.harness
                )));
            }
            if !entry.enabled {
                return Err(StoreError::Conflict(format!(
                    "endpoint '{}' is off - turn it on before making it the default",
                    entry.name
                )));
            }
            entry.is_default = true;
            return self.put_model_endpoint(entry).map(Some);
        }
        let mut changed = false;
        for other in self.state.model_endpoints.values_mut() {
            if other.harness == harness && other.is_default {
                other.is_default = false;
                changed = true;
            }
        }
        if changed {
            self.persist()?;
        }
        Ok(None)
    }

    /// Point a stopped chat at another endpoint. Live chats keep the
    /// binding they started with; endpoint switches land on resume.
    pub fn set_session_endpoint(
        &mut self,
        id: &crate::ids::SessionId,
        endpoint_id: Option<&str>,
    ) -> Result<Session, StoreError> {
        let harness = self
            .state
            .sessions
            .get(id)
            .ok_or_else(|| StoreError::NotFound(format!("session {id}")))?
            .harness
            .clone();
        let resolved = self.resolve_model_endpoint(&harness, endpoint_id)?;
        let session = self
            .state
            .sessions
            .get_mut(id)
            .ok_or_else(|| StoreError::NotFound(format!("session {id}")))?;
        session.endpoint_id = resolved.map(|entry| entry.id);
        session.updated_at = crate::now_ms();
        let updated = session.clone();
        self.persist()?;
        Ok(updated)
    }

    /// Point a chat at another folder: a project move (`Some`), or out of
    /// every project into the home folder (`None`). A live harness keeps
    /// working where it is until its owner restarts it there; the supervisor's
    /// move stops and resumes it, so this write is only the fact.
    pub fn set_session_workspace(
        &mut self,
        id: &SessionId,
        workspace: Option<PathBuf>,
    ) -> Result<Session, StoreError> {
        let session = self
            .state
            .sessions
            .get_mut(id)
            .ok_or_else(|| StoreError::NotFound(format!("session {id}")))?;
        session.workspace = workspace;
        session.updated_at = crate::now_ms();
        let updated = session.clone();
        self.persist()?;
        Ok(updated)
    }

    /// Mark (`Some(seq)`) or clear (`None`) a moved chat's carry: its next
    /// plain message hands a fresh conversation the chat's messages before
    /// `seq`. `clear_grants` drops the tools the harness granted to the
    /// conversation the chat left behind.
    pub fn set_carry_transcript(
        &mut self,
        id: &SessionId,
        carry: Option<u64>,
        clear_grants: bool,
    ) -> Result<Session, StoreError> {
        let session = self
            .state
            .sessions
            .get_mut(id)
            .ok_or_else(|| StoreError::NotFound(format!("session {id}")))?;
        session.carry_transcript = carry;
        if clear_grants {
            session.approved_tools.clear();
        }
        session.updated_at = crate::now_ms();
        let updated = session.clone();
        self.persist()?;
        Ok(updated)
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

    // ---- projects ----

    pub fn projects(&self) -> Vec<&Project> {
        self.state.projects.values().collect()
    }

    pub fn project(&self, id: &ProjectId) -> Result<&Project, StoreError> {
        self.state
            .projects
            .get(id)
            .ok_or_else(|| StoreError::NotFound(format!("project {id}")))
    }

    /// Register a project. A folder already in a project is refused - one
    /// registration per folder, so a chat's project is never ambiguous.
    pub fn save_project(&mut self, mut project: Project) -> Result<Project, StoreError> {
        if project.folders.iter().any(|folder| {
            self.state
                .projects
                .values()
                .any(|p| p.folders.contains(folder))
        }) {
            return Err(StoreError::Conflict(
                "that folder is already a project".into(),
            ));
        }
        project.updated_at = crate::now_ms();
        let id = project.id;
        self.state.projects.insert(id, project.clone());
        self.persist()?;
        Ok(project)
    }

    /// Unregister: the folder on disk and every chat that ran in it are
    /// untouched - the chats stop being grouped, nothing more.
    pub fn remove_project(&mut self, id: &ProjectId) -> Result<(), StoreError> {
        if self.state.projects.remove(id).is_none() {
            return Err(StoreError::NotFound(format!("project {id}")));
        }
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
        // Before persisting, so the bounded table is what lands on disk.
        self.prune_receipts();
        self.persist()?;
        Ok(true)
    }

    /// Evict the oldest receipts past `RECEIPT_CAP` (by update time, then
    /// request id, so ties evict deterministically). Called on every
    /// record and on open, so a table that grew under an older build is
    /// bounded the first time this build sees it.
    fn prune_receipts(&mut self) -> bool {
        let len = self.state.receipts.len();
        if len <= RECEIPT_CAP {
            return false;
        }
        let mut ranked: Vec<(i64, String)> = self
            .state
            .receipts
            .iter()
            .map(|(id, r)| (r.updated_at, id.clone()))
            .collect();
        ranked.sort_unstable();
        let evicted: Vec<String> = ranked
            .into_iter()
            .take(len - RECEIPT_CAP)
            .map(|(_, id)| id)
            .collect();
        for id in evicted {
            self.state.receipts.remove(&id);
        }
        true
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

    /// The session's next event seq - one past the last applied. A cheap
    /// drain-progress signal: a caller watching a pump finish its backlog
    /// can tell "still applying" from "nothing left" without reading the
    /// whole log.
    pub fn next_seq(&self, session_id: &SessionId) -> u64 {
        self.state.next_seq.get(session_id).copied().unwrap_or(0)
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

    /// A chat's agent used OpenRemote's browser (see `Session::browser_used`).
    /// A fact this device keeps for the chat it runs, not history: the
    /// browser is used only where the chat runs.
    pub fn note_browser_used(&mut self, session_id: &SessionId) -> Result<(), StoreError> {
        let session = self
            .state
            .sessions
            .get_mut(session_id)
            .ok_or_else(|| StoreError::NotFound(format!("session {session_id}")))?;
        if !session.browser_used {
            session.browser_used = true;
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
        let mut out = Vec::new();
        let mut size = 0;
        for line in raw.lines() {
            let Ok(SeqLine { seq }) = serde_json::from_str(line) else {
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
        // Its pictures go with it. A picture that can't be removed now is
        // only disk space, never a reason to keep a deleted chat.
        if let Err(e) = self.blobs.release(*session_id) {
            eprintln!("warning: chat {session_id}'s pictures weren't all removed: {e}");
        }
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
        // The append reaches the disk before the state rewrite that
        // records it (persist fsyncs): otherwise power loss can leave
        // state's next_seq past what the log really holds, and a chat
        // whose log skips numbers wedges every peer pull on it forever.
        // One sync per batch, not per event.
        log.sync_all()?;
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
                // The queue is queue.changed's alone: an update carries the
                // session's other facts and never rewinds the queue.
                if let Some(existing) = self.state.sessions.get(&session.id) {
                    session.queue = existing.queue.clone();
                    session.queue_paused = existing.queue_paused;
                    // Who started a chat is fixed when it is created: an
                    // update from a device that predates teams carries no
                    // parent and must not take the chat out of its tree.
                    if session.parent.is_none() {
                        session.parent = existing.parent.clone();
                    }
                    // Kept by this device, never cleared: an update written
                    // elsewhere does not take the browser tools back.
                    session.browser_used |= existing.browser_used;
                }
                self.state.sessions.insert(session.id, session);
            }
            EventPayload::MessageAdded { message } => {
                if let Some(session) = self.state.sessions.get_mut(&event.session_id) {
                    session.updated_at = now;
                    // A message of pictures alone names nothing; the
                    // next one with words does.
                    if session.title.is_none()
                        && message.role == MessageRole::User
                        && !message.text.trim().is_empty()
                    {
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
            EventPayload::QueueChanged { queue, paused } => {
                if let Some(session) = self.state.sessions.get_mut(&event.session_id) {
                    session.queue = queue.clone();
                    session.queue_paused = *paused;
                    session.updated_at = now;
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
    use crate::model::{DecisionKind, DecisionOption, PluginScope};
    use crate::{EndpointAuthKind, EndpointSecret, ModelEndpoint};

    fn tempdir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("or-core-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn session() -> Session {
        Session {
            id: SessionId::new(),
            harness: "claude".into(),
            workspace: Some("/tmp/w".into()),
            status: SessionStatus::Starting,
            model: None,
            endpoint_id: None,
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
            queue: Vec::new(),
            queue_paused: false,
            mentioned_chats: Vec::new(),
            carry_transcript: None,
            browser_used: false,
            parent: None,
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
    fn mcp_servers_saved_under_the_old_plugins_name_still_load() {
        // Before the plugin system took the name, MCP servers were saved
        // as `plugins` in state.json. Such a file must still load them -
        // and the next save writes the new name.
        let dir = tempdir();
        std::fs::create_dir_all(&dir).unwrap();
        let machine = MachineId::new();
        let entry = serde_json::json!({
            "id": "custom-docs@m",
            "name": "docs",
            "detail": "search the docs",
            "command": "docs-mcp --stdio",
            "machine": machine,
            "needs_key": false,
            "created_at": 0,
            "updated_at": 0,
        });
        let state = serde_json::json!({
            "sessions": {}, "decisions": {}, "receipts": {},
            "plugins": {"custom-docs@m": entry},
        });
        std::fs::write(dir.join("state.json"), state.to_string()).unwrap();
        let mut store = Store::open(dir.clone()).unwrap();
        assert_eq!(store.mcp_servers().len(), 1);
        assert_eq!(store.mcp_server("custom-docs@m").unwrap().name, "docs");
        store
            .set_mcp_server_enabled("custom-docs@m", false)
            .unwrap();
        let saved = std::fs::read_to_string(dir.join("state.json")).unwrap();
        assert!(saved.contains("\"mcp_servers\""), "{saved}");
        assert!(!saved.contains("\"plugins\""), "{saved}");
    }

    #[test]
    fn plugins_installed_before_marketplaces_are_filed_under_local() {
        // The first plugin builds keyed an install by its bare name and
        // knew no marketplaces. Such a state loads with each one filed as
        // name@local, so a marketplace's plugin of the same name can sit
        // beside it.
        let dir = tempdir();
        std::fs::create_dir_all(&dir).unwrap();
        let plugin = serde_json::json!({
            "name": "toy",
            "runtime": "process",
            "powers": ["run_programs"],
            "source": {"kind": "folder", "path": "/somewhere/toy"},
            "scope": "machine",
            "enabled": true,
            "installed_at": 1,
            "updated_at": 1,
        });
        let state = serde_json::json!({
            "sessions": {}, "decisions": {}, "receipts": {},
            "installed_plugins": {"toy": plugin},
        });
        std::fs::write(dir.join("state.json"), state.to_string()).unwrap();
        let store = Store::open(dir.clone()).unwrap();
        let toy = store.installed_plugin("toy@local").unwrap();
        assert_eq!(toy.marketplace, "local");
        assert!(store.installed_plugin("toy").is_err());
        let saved = std::fs::read_to_string(dir.join("state.json")).unwrap();
        assert!(saved.contains("\"toy@local\""), "{saved}");
    }

    fn installed(name: &str, scope: PluginScope, project: Option<ProjectId>) -> InstalledPlugin {
        InstalledPlugin {
            name: name.into(),
            marketplace: "local".into(),
            resolved: None,
            command: None,
            version: None,
            description: None,
            runtime: crate::model::PluginRuntime::Wasm,
            powers: vec![crate::model::PluginPower::AgentTools],
            source: crate::model::PluginSource::Folder {
                path: "/somewhere".into(),
            },
            scope,
            project,
            contributes: crate::model::PluginContributions {
                tools: vec![crate::model::ToolContribution {
                    name: "shout".into(),
                    description: "upper-case a line".into(),
                    input_schema: serde_json::json!({"type": "object"}),
                    read_only: true,
                    timeout_secs: None,
                }],
                ..Default::default()
            },
            harness: None,
            enabled: true,
            installed_at: 1,
            updated_at: 1,
        }
    }

    #[test]
    fn plugins_installed_before_scopes_and_contributions_still_load() {
        // A store written by the build that had one scope and no
        // `contributes` or `project`: the same record JSON, with only what
        // that build wrote. It loads as a machine install that adds
        // nothing, under the same id - and a harness plugin's record, which
        // already named its harness, shows it as what it contributes.
        let dir = tempdir();
        let content = serde_json::json!({
            "name": "notes",
            "marketplace": "openremote",
            "runtime": "content",
            "powers": [],
            "source": {"kind": "folder", "path": "/somewhere/notes"},
            "scope": "machine",
            "enabled": false,
            "installed_at": 7,
            "updated_at": 8,
        });
        let driver = serde_json::json!({
            "name": "freebuff",
            "marketplace": "local",
            "runtime": "process",
            "powers": ["run_programs", "network"],
            "source": {"kind": "folder", "path": "/somewhere/freebuff"},
            "scope": "machine",
            "harness": {"id": "freebuff", "name": "Freebuff"},
            "enabled": true,
            "installed_at": 1,
            "updated_at": 1,
        });
        let state = serde_json::json!({
            "sessions": {}, "decisions": {}, "receipts": {},
            "installed_plugins": {"notes@openremote": content, "freebuff@local": driver},
        });
        std::fs::write(dir.join("state.json"), state.to_string()).unwrap();
        let store = Store::open(dir.clone()).unwrap();
        let notes = store.installed_plugin("notes@openremote").unwrap();
        assert_eq!(notes.scope, PluginScope::Machine);
        assert_eq!(notes.project, None);
        assert!(notes.contributes.is_empty());
        assert!(!notes.enabled);
        assert_eq!((notes.installed_at, notes.updated_at), (7, 8));
        let freebuff = store.installed_plugin("freebuff@local").unwrap();
        assert_eq!(freebuff.powers.len(), 2);
        assert_eq!(
            freebuff.contributes.harnesses,
            vec![freebuff.harness.clone().unwrap()]
        );
        assert!(freebuff.contributes.tools.is_empty());
        drop(store);
        // The next open reads what was saved, and the backfill holds still.
        let again = Store::open(dir).unwrap();
        assert_eq!(
            again
                .installed_plugin("freebuff@local")
                .unwrap()
                .contributes
                .harnesses
                .len(),
            1
        );
    }

    #[test]
    fn a_plugin_installed_for_a_project_keeps_it_and_two_projects_sit_side_by_side() {
        let dir = tempdir();
        let (one, two) = (ProjectId::new(), ProjectId::new());
        let mut store = Store::open(dir.clone()).unwrap();
        store
            .put_installed_plugin(installed("shout", PluginScope::Machine, None))
            .unwrap();
        store
            .put_installed_plugin(installed("shout", PluginScope::Project, Some(one)))
            .unwrap();
        store
            .put_installed_plugin(installed("shout", PluginScope::ProjectLocal, Some(two)))
            .unwrap();
        assert_eq!(store.installed_plugins().len(), 3);
        drop(store);
        // Saved and reopened: every record is still under its own id, with
        // its scope, its project and what it contributes.
        let mut store = Store::open(dir).unwrap();
        let machine = store.installed_plugin("shout@local").unwrap();
        assert_eq!(
            (machine.scope, machine.project),
            (PluginScope::Machine, None)
        );
        let repo = store
            .installed_plugin(&format!("shout@local~{one}"))
            .unwrap();
        assert_eq!(
            (repo.scope, repo.project),
            (PluginScope::Project, Some(one))
        );
        assert_eq!(repo.contributes.tools[0].name, "shout");
        let local = store
            .installed_plugin(&format!("shout@local~{two}"))
            .unwrap();
        assert_eq!(
            (local.scope, local.project),
            (PluginScope::ProjectLocal, Some(two))
        );
        // Turning one off leaves the others, and removing one is by its id.
        store
            .set_installed_plugin_enabled(&format!("shout@local~{one}"), false)
            .unwrap();
        assert!(store.installed_plugin("shout@local").unwrap().enabled);
        store
            .remove_installed_plugin(&format!("shout@local~{two}"))
            .unwrap();
        assert_eq!(store.installed_plugins().len(), 2);
    }

    #[test]
    fn a_project_install_without_its_project_is_refused() {
        // It would take the plain id, the one this machine's own install
        // of the plugin has, and replace that record.
        let mut store = Store::open(tempdir()).unwrap();
        store
            .put_installed_plugin(installed("shout", PluginScope::Machine, None))
            .unwrap();
        for scope in [PluginScope::Project, PluginScope::ProjectLocal] {
            let err = store
                .put_installed_plugin(installed("shout", scope, None))
                .unwrap_err();
            assert!(matches!(err, StoreError::Conflict(_)), "{err}");
        }
        assert_eq!(
            store.installed_plugin("shout@local").unwrap().scope,
            PluginScope::Machine
        );
    }

    fn global(name: &str, at: i64, by: Option<DeviceId>) -> GlobalPluginRecord {
        GlobalPluginRecord {
            name: name.into(),
            marketplace: "local".into(),
            version: None,
            description: None,
            runtime: crate::model::PluginRuntime::Wasm,
            powers: vec![crate::model::PluginPower::AgentTools],
            source: crate::model::PluginSource::Folder {
                path: "/somewhere".into(),
            },
            resolved: "abc".into(),
            archive: None,
            at,
            by,
            removed: false,
        }
    }

    #[test]
    fn this_devices_own_word_replaces_the_record_and_a_removal_turns_off_without_a_new_time() {
        let mut store = Store::open(tempdir()).unwrap();
        // A record from the future (a peer's clock) is held; this device's
        // own write still replaces it.
        store
            .merge_global_plugin(global("shout", 9_000, None))
            .unwrap();
        store.put_global_plugin(global("shout", 10, None)).unwrap();
        assert_eq!(store.global_plugin("shout@local").unwrap().at, 10);
        // Turned off for another device's removal: the time it was installed
        // or updated here is kept, so the removal still reads as newer.
        let mut here = installed("shout", PluginScope::Global, None);
        here.updated_at = 5;
        store.put_installed_plugin(here).unwrap();
        let off = store
            .turn_off_installed_plugin_keeping_time("shout@local")
            .unwrap();
        assert!(!off.enabled);
        assert_eq!(off.updated_at, 5);
    }

    #[test]
    fn the_accounts_global_records_keep_the_latest_write_and_survive_a_reopen() {
        let dir = tempdir();
        let mut store = Store::open(dir.clone()).unwrap();
        assert!(
            store
                .merge_global_plugin(global("shout", 10, None))
                .unwrap()
        );
        // An older write is not kept; a newer one is.
        assert!(!store.merge_global_plugin(global("shout", 5, None)).unwrap());
        let mut later = global("shout", 20, None);
        later.removed = true;
        assert!(store.merge_global_plugin(later).unwrap());
        // The same moment from two devices settles the same way on both.
        let (a, b) = (DeviceId::new(), DeviceId::new());
        let (low, high) = if a.to_string() < b.to_string() {
            (a, b)
        } else {
            (b, a)
        };
        assert!(
            store
                .merge_global_plugin(global("other", 30, Some(low)))
                .unwrap()
        );
        assert!(
            store
                .merge_global_plugin(global("other", 30, Some(high)))
                .unwrap()
        );
        assert!(
            !store
                .merge_global_plugin(global("other", 30, Some(low)))
                .unwrap()
        );
        drop(store);
        let store = Store::open(dir).unwrap();
        assert!(store.global_plugin("shout@local").unwrap().removed);
        assert_eq!(store.global_plugin("other@local").unwrap().by, Some(high));
        assert_eq!(store.global_plugins().len(), 2);
        // Records are the account's word, not an install: nothing here.
        assert!(store.installed_plugins().is_empty());
    }

    #[test]
    fn turning_a_plugin_off_never_changes_what_the_user_approved() {
        // powers, scope, project and contributes are the install record: the
        // store's other writes to the record leave them as they were.
        let dir = tempdir();
        let project = ProjectId::new();
        let mut store = Store::open(dir).unwrap();
        let before = installed("shout", PluginScope::Project, Some(project));
        let id = before.id();
        store.put_installed_plugin(before.clone()).unwrap();
        let after = store.set_installed_plugin_enabled(&id, false).unwrap();
        assert!(!after.enabled);
        assert_eq!(after.powers, before.powers);
        assert_eq!((after.scope, after.project), (before.scope, before.project));
        assert_eq!(after.contributes, before.contributes);
    }

    #[test]
    fn a_seeded_marketplace_the_user_removed_stays_removed() {
        let dir = tempdir();
        let mut store = Store::open(dir.clone()).unwrap();
        let record = MarketplaceRecord {
            name: "openremote".into(),
            source: crate::model::MarketplaceSource::Github {
                repo: "morewebs/OpenRemote-plugins".into(),
                reference: None,
            },
            enabled: true,
            removable: false,
            owner: None,
            description: None,
            plugin_count: 0,
            added_at: 0,
            refreshed_at: None,
            error: None,
            heads: Default::default(),
        };
        assert!(store.seed_marketplace(record.clone()).unwrap());
        assert!(!store.seed_marketplace(record.clone()).unwrap());
        store.remove_marketplace("openremote").unwrap();
        drop(store);
        let mut store = Store::open(dir).unwrap();
        assert!(!store.seed_marketplace(record).unwrap());
        assert!(store.marketplace("openremote").is_err());
    }

    #[test]
    fn a_worker_keeps_its_place_in_the_tree_through_updates_from_older_devices() {
        let mut store = Store::open(tempdir()).unwrap();
        let mut worker = session();
        worker.parent = Some(crate::model::ParentLink {
            chat: SessionId::new(),
            depth: 1,
            role: Some("worker".into()),
            brief: "fix the parser".into(),
            spawned_at: 1,
        });
        store
            .append(
                worker.id,
                EventPayload::SessionCreated {
                    session: worker.clone(),
                },
            )
            .unwrap();
        // A device that predates teams writes its copy without the field.
        let mut raw = serde_json::to_value(&worker).unwrap();
        raw.as_object_mut().unwrap().remove("parent");
        let older: Session = serde_json::from_value(raw).unwrap();
        assert!(older.parent.is_none());
        store
            .append(worker.id, EventPayload::SessionUpdated { session: older })
            .unwrap();
        assert_eq!(store.session(&worker.id).unwrap().parent, worker.parent);
    }

    #[test]
    fn a_chats_browser_use_is_kept_and_never_taken_back() {
        let dir = tempdir();
        let mut store = Store::open(dir.clone()).unwrap();
        let s = session();
        store
            .append(s.id, EventPayload::SessionCreated { session: s.clone() })
            .unwrap();
        store.note_browser_used(&s.id).unwrap();
        // An update written without it (another device, an older copy of the
        // session) leaves it on.
        store
            .append(s.id, EventPayload::SessionUpdated { session: s.clone() })
            .unwrap();
        assert!(store.session(&s.id).unwrap().browser_used);
        drop(store);
        assert!(
            Store::open(dir)
                .unwrap()
                .session(&s.id)
                .unwrap()
                .browser_used
        );
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
                        origin: None,
                        images: Vec::new(),
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
    fn a_chats_pictures_reload_by_hash_and_go_when_the_chat_does() {
        let dir = tempdir();
        let sha = "b".repeat(64);
        let s = session();
        {
            let mut store = Store::open(dir.clone()).unwrap();
            store
                .blobs()
                .put(&sha, "image/png", b"the picture", s.id)
                .unwrap();
            let mut message = user_message(1, "");
            if let EventPayload::MessageAdded { message } = &mut message {
                message.images = vec![crate::ImageRef {
                    blob: sha.clone(),
                    mime: "image/png".into(),
                }];
            }
            store
                .append_batch(
                    s.id,
                    vec![EventPayload::SessionCreated { session: s.clone() }, message],
                )
                .unwrap();
            // The log names the picture; its bytes are not in it.
            let raw = fs::read_to_string(store.event_log(&s.id)).unwrap();
            assert!(raw.contains(&sha) && !raw.contains("the picture"));
            // A message of pictures alone names nothing.
            assert_eq!(store.session(&s.id).unwrap().title, None);
        }

        let mut store = Store::open(dir).unwrap();
        let events = store.events_after(&s.id, None).unwrap();
        let images = events
            .iter()
            .find_map(|e| match &e.payload {
                EventPayload::MessageAdded { message } => Some(message.images.clone()),
                _ => None,
            })
            .unwrap();
        assert_eq!(images[0].blob, sha);
        assert_eq!(
            store.blobs().get(&sha).unwrap(),
            Some(("image/png".to_string(), b"the picture".to_vec()))
        );

        let stone = Tombstone {
            at: 1,
            by: DeviceId::new(),
        };
        store.tombstone(s.id, stone).unwrap();
        assert_eq!(store.blobs().get(&sha).unwrap(), None);
    }

    fn user_message(turn: u64, text: &str) -> EventPayload {
        EventPayload::MessageAdded {
            message: crate::model::ChatMessage {
                id: format!("m{turn}"),
                turn,
                role: MessageRole::User,
                text: text.into(),
                origin: None,
                images: Vec::new(),
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

    /// Put a next_seq into state.json directly: the on-disk shape of
    /// power loss landing state.json's fsynced rewrite while the log's
    /// append was still in flight.
    fn force_next_seq(dir: &std::path::Path, id: &SessionId, seq: u64) {
        let path = dir.join("state.json");
        let mut state: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        if state.get("next_seq").is_none() {
            state["next_seq"] = serde_json::json!({});
        }
        state["next_seq"][id.to_string()] = serde_json::json!(seq);
        fs::write(&path, serde_json::to_string(&state).unwrap()).unwrap();
    }

    #[test]
    fn a_state_ahead_of_its_log_rewinds_to_the_log_s_true_tail() {
        let dir = tempdir();
        let mut store = Store::open(dir.clone()).unwrap();
        let s = session();
        store
            .append(s.id, EventPayload::SessionCreated { session: s.clone() })
            .unwrap();
        store.append(s.id, user_message(1, "Ship it")).unwrap();
        drop(store);
        // state.json credits the log with five events; the log holds two.
        force_next_seq(&dir, &s.id, 5);
        let mut store = Store::open(dir).unwrap();
        assert_eq!(store.head(&s.id), 2, "rewound to the log's true tail");
        let next = store.append(s.id, user_message(2, "still going")).unwrap();
        assert_eq!(next.seq, 2, "no number is skipped: no gap to wedge a peer");
    }

    #[test]
    fn a_log_that_skips_numbers_is_cut_back_to_its_contiguous_run() {
        let dir = tempdir();
        let mut store = Store::open(dir.clone()).unwrap();
        let s = session();
        store
            .append(s.id, EventPayload::SessionCreated { session: s.clone() })
            .unwrap();
        store.append(s.id, user_message(1, "one")).unwrap();
        drop(store);
        // Power loss kept a later page of the log but lost seq 2's, and
        // state.json recorded all of it: the log jumps 1 -> 3.
        let orphan = |seq: u64, text: &str| {
            serde_json::to_string(&Event {
                seq,
                session_id: s.id,
                at: Some(9),
                payload: user_message(seq, text),
            })
            .unwrap()
        };
        let mut log = fs::OpenOptions::new()
            .append(true)
            .open(
                dir.join("sessions")
                    .join(s.id.to_string())
                    .join("events.jsonl"),
            )
            .unwrap();
        writeln!(log, "{}", orphan(3, "lost the middle")).unwrap();
        writeln!(log, "{}", orphan(4, "kept the later page")).unwrap();
        drop(log);
        force_next_seq(&dir, &s.id, 5);

        let store = Store::open(dir.clone()).unwrap();
        assert_eq!(
            store.head(&s.id),
            2,
            "the contiguous run's tail, not the skipped numbers"
        );
        let seqs: Vec<u64> = store
            .events_after(&s.id, None)
            .unwrap()
            .iter()
            .map(|e| e.seq)
            .collect();
        assert_eq!(
            seqs,
            vec![0, 1],
            "the gapped lines are cut: a peer pulling never hits a jump"
        );
        // The cut lines are real messages: they wait in a .bad sibling,
        // never deleted.
        let log_dir = dir.join("sessions").join(s.id.to_string());
        let salvaged = fs::read_dir(&log_dir)
            .unwrap()
            .flatten()
            .find(|e| {
                e.file_name()
                    .to_string_lossy()
                    .starts_with("events.jsonl.bad-")
            })
            .expect("the discarded lines are preserved, not deleted");
        let salvaged = fs::read_to_string(salvaged.path()).unwrap();
        assert_eq!(
            salvaged.lines().count(),
            2,
            "seq 3 and 4 wait in the sibling, verbatim"
        );
        assert!(salvaged.contains("lost the middle"));
        assert!(salvaged.contains("kept the later page"));
        // Reopening the healed store cuts nothing more and never appends
        // the same discarded bytes twice.
        drop(store);
        let store = Store::open(dir.clone()).unwrap();
        assert_eq!(store.head(&s.id), 2);
        let again: Vec<u64> = store
            .events_after(&s.id, None)
            .unwrap()
            .iter()
            .map(|e| e.seq)
            .collect();
        assert_eq!(again, vec![0, 1]);
        let salvaged_again: Vec<String> = fs::read_dir(&log_dir)
            .unwrap()
            .flatten()
            .filter(|e| {
                e.file_name()
                    .to_string_lossy()
                    .starts_with("events.jsonl.bad-")
            })
            .map(|e| fs::read_to_string(e.path()).unwrap())
            .collect();
        assert_eq!(
            salvaged_again
                .iter()
                .map(|s| s.lines().count())
                .sum::<usize>(),
            2,
            "the second open salvaged nothing new"
        );
    }

    #[test]
    fn a_tombstoned_chat_s_orphan_log_stays_dormant_at_open() {
        let dir = tempdir();
        let mut store = Store::open(dir.clone()).unwrap();
        let s = session();
        store
            .append(s.id, EventPayload::SessionCreated { session: s.clone() })
            .unwrap();
        store
            .append(s.id, user_message(1, "Delete me everywhere"))
            .unwrap();
        let log = store.event_log(&s.id);
        let log_text = fs::read_to_string(&log).unwrap();
        let id = s.id;
        // Deleted everywhere, but the delete's dir removal half-failed: the
        // log outlives the session record while the tombstone stands.
        store
            .tombstone(
                id,
                Tombstone {
                    at: 1,
                    by: DeviceId::new(),
                },
            )
            .unwrap();
        fs::create_dir_all(log.parent().unwrap()).unwrap();
        fs::write(&log, log_text).unwrap();
        // Simulate a state.json that no longer names the chat: the orphan
        // log plus a tombstone entry is what survives.
        let mut state: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(dir.join("state.json")).unwrap()).unwrap();
        state["sessions"]
            .as_object_mut()
            .unwrap()
            .remove(&id.to_string());
        state["next_seq"]
            .as_object_mut()
            .unwrap()
            .remove(&id.to_string());
        fs::write(
            dir.join("state.json"),
            serde_json::to_string(&state).unwrap(),
        )
        .unwrap();

        let store = Store::open(dir.clone()).unwrap();
        assert!(
            store.session(&id).is_err(),
            "the tombstone wins: a deleted chat never comes back"
        );
        assert!(store.tombstoned(&id));
        assert!(
            store
                .dir
                .join("sessions")
                .join(id.to_string())
                .join("events.jsonl")
                .exists(),
            "the orphan log is left as is, not deleted"
        );
    }

    #[test]
    fn a_corrupt_state_is_quarantined_and_the_logs_re_derive_the_chats() {
        let dir = tempdir();
        let mut store = Store::open(dir.clone()).unwrap();
        let s = session();
        store
            .append(s.id, EventPayload::SessionCreated { session: s.clone() })
            .unwrap();
        store
            .append(s.id, user_message(1, "Recover the title"))
            .unwrap();
        drop(store);
        // Disk-full during the rewrite, or external truncation: an
        // unparseable state.json where the daemon boots.
        fs::write(dir.join("state.json"), "{ not json").unwrap();

        let mut store = Store::open(dir.clone()).unwrap();
        let loaded = store.session(&s.id).unwrap();
        assert_eq!(loaded.title.as_deref(), Some("Recover the title"));
        assert_eq!(store.head(&s.id), 2, "the log is the history");
        // The bad file is never deleted; it waits beside the fresh one.
        let bad = fs::read_dir(&dir)
            .unwrap()
            .flatten()
            .find(|e| {
                e.file_name()
                    .to_string_lossy()
                    .starts_with("state.json.bad-")
            })
            .expect("the corrupt state is quarantined, not deleted");
        assert_eq!(fs::read_to_string(bad.path()).unwrap(), "{ not json");
        let next = store.append(s.id, user_message(2, "still works")).unwrap();
        assert_eq!(next.seq, 2);
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
    fn receipts_are_bounded_and_recent_ids_stay_idempotent() {
        let dir = tempdir();
        // A state.json straight off an older, unbounded install: more
        // receipts than the cap, which the first open must bound.
        let mut receipts = serde_json::Map::new();
        for i in 0..(RECEIPT_CAP + 2) {
            let receipt = Receipt {
                request_id: format!("old-{i}"),
                session_id: None,
                status: ReceiptStatus::Completed,
                result: None,
                error: None,
                updated_at: i as i64,
            };
            receipts.insert(format!("old-{i}"), serde_json::to_value(receipt).unwrap());
        }
        let state = serde_json::json!({
            "sessions": {},
            "decisions": {},
            "receipts": receipts,
        });
        fs::write(
            dir.join("state.json"),
            serde_json::to_string(&state).unwrap(),
        )
        .unwrap();

        let mut store = Store::open(dir).unwrap();
        assert!(
            store.receipt("old-0").is_none(),
            "past the cap, the oldest go"
        );
        assert!(store.receipt("old-1").is_none());
        assert!(store.receipt("old-2").is_some(), "a cap's worth stays");
        // One more landing evicts the next oldest, never the recent ones.
        let fresh = Receipt {
            request_id: "new".into(),
            session_id: None,
            status: ReceiptStatus::Completed,
            result: Some(serde_json::json!({"ok": true})),
            error: None,
            updated_at: RECEIPT_CAP as i64 + 100,
        };
        assert!(store.record_receipt(fresh).unwrap());
        assert!(store.receipt("old-2").is_none());
        assert!(store.receipt("old-3").is_some());
        // A recent terminal receipt still dedups: a retry never re-executes.
        let dup = Receipt {
            request_id: "new".into(),
            session_id: None,
            status: ReceiptStatus::Failed,
            result: None,
            error: Some("late".into()),
            updated_at: RECEIPT_CAP as i64 + 101,
        };
        assert!(!store.record_receipt(dup).unwrap());
        assert_eq!(
            store.receipt("new").unwrap().status,
            ReceiptStatus::Completed
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
        let mut store = Store::open(tempdir()).unwrap();
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

    #[test]
    fn a_folder_registers_once_and_removal_leaves_the_chats_alone() {
        let dir = tempdir();
        let mut store = Store::open(dir.clone()).unwrap();

        let project = Project {
            id: crate::ids::ProjectId::new(),
            folders: vec!["/tmp/one".into()],
            created_at: 0,
            updated_at: 0,
        };
        let saved = store.save_project(project).unwrap();
        assert_eq!(saved.folders, vec![std::path::PathBuf::from("/tmp/one")]);
        assert!(saved.updated_at > 0);

        // The same folder in another project is a conflict, whatever the
        // project's other folders are.
        let dup = Project {
            id: crate::ids::ProjectId::new(),
            folders: vec!["/tmp/elsewhere".into(), "/tmp/one".into()],
            created_at: 0,
            updated_at: 0,
        };
        assert!(matches!(
            store.save_project(dup),
            Err(StoreError::Conflict(_))
        ));

        // Removing the project unregisters it; the folder stays registrable
        // again, and a chat in it was never the project's to remove.
        let s = session();
        store
            .append(s.id, EventPayload::SessionCreated { session: s.clone() })
            .unwrap();
        store.remove_project(&saved.id).unwrap();
        assert!(store.project(&saved.id).is_err());
        assert_eq!(store.session(&s.id).unwrap().workspace, s.workspace);

        // Registered projects persist across a reopen.
        let again = Project {
            id: crate::ids::ProjectId::new(),
            folders: vec!["/tmp/one".into()],
            created_at: 0,
            updated_at: 0,
        };
        let id = again.id;
        store.save_project(again).unwrap();
        drop(store);
        let reopened = Store::open(dir).unwrap();
        assert_eq!(reopened.project(&id).unwrap().folders.len(), 1);
    }

    fn endpoint(id: &str, harness: &str, name: &str) -> ModelEndpoint {
        ModelEndpoint {
            id: id.into(),
            harness: harness.into(),
            name: name.into(),
            base_url: "http://localhost:4000/v1".into(),
            models_path: None,
            default_model: None,
            auth_kind: EndpointAuthKind::Bearer,
            key: Some(EndpointSecret::new("k".into())),
            enabled: true,
            is_default: false,
            created_at: 0,
            updated_at: 0,
        }
    }

    #[test]
    fn endpoint_records_validate_normalize_and_redact() {
        let mut store = Store::open(tempdir()).unwrap();
        // An unknown harness is refused, not stored.
        let mut bad = endpoint("e-bad", "pi", "Office");
        bad.key = Some(EndpointSecret::new("k".into()));
        assert!(store.put_model_endpoint(bad).is_err());
        // A bearer record without a key is refused.
        let mut nokey = endpoint("e-nokey", "claude", "Office");
        nokey.key = None;
        assert!(store.put_model_endpoint(nokey).is_err());
        // A keyless record carrying a key is refused.
        let mut withkey = endpoint("e-withkey", "claude", "Local");
        withkey.auth_kind = EndpointAuthKind::None;
        assert!(store.put_model_endpoint(withkey).is_err());
        // A base URL with credentials, a query, or no scheme is refused.
        for base in [
            "localhost:4000/v1",
            "http://user:pass@localhost:4000/v1",
            "http://localhost:4000/v1?x=1",
            "ftp://localhost:4000/v1",
        ] {
            let mut entry = endpoint("e-url", "claude", "Office");
            entry.base_url = base.into();
            assert!(store.put_model_endpoint(entry).is_err(), "{base}");
        }
        // The bar is shape, not reachability: loopback and LAN hosts are
        // allowed, and the trailing slash collapses to one canonical form.
        let mut entry = endpoint("e-1", "claude", "Office");
        entry.base_url = "HTTP://127.0.0.1:4000/v1/".into();
        let saved = store.put_model_endpoint(entry).unwrap();
        assert_eq!(saved.base_url, "http://127.0.0.1:4000/v1");
        // The view carries metadata only: no key material, `has_key` instead.
        let view = saved.view();
        assert!(view.has_key);
        let raw = serde_json::to_string(&view).unwrap();
        assert!(!raw.contains("\"key\""), "{raw}");
        // A duplicate display name on the same harness is refused,
        // whatever the case - another harness may share it.
        let dup = endpoint("e-2", "claude", "office");
        assert!(store.put_model_endpoint(dup).is_err());
        let mut other = endpoint("e-3", "codex", "office");
        other.key = Some(EndpointSecret::new("k".into()));
        assert!(store.put_model_endpoint(other).is_ok());
        // The secret persists in the owner-only state and survives a reopen.
        drop(store);
    }

    #[test]
    fn endpoint_secret_survives_a_reopen_and_views_stay_redacted() {
        let dir = tempdir();
        let mut store = Store::open(dir.clone()).unwrap();
        store
            .put_model_endpoint(endpoint("e-1", "claude", "Office"))
            .unwrap();
        drop(store);
        let reopened = Store::open(dir).unwrap();
        let entry = reopened.model_endpoint("e-1").unwrap();
        assert!(entry.key.as_ref().is_some_and(|k| !k.is_empty()));
        let raw = serde_json::to_string(&entry.view()).unwrap();
        assert!(!raw.contains("\"key\""), "{raw}");
    }

    #[test]
    fn endpoint_defaults_are_exclusive_and_disabled_ones_drop_out() {
        let mut store = Store::open(tempdir()).unwrap();
        store
            .put_model_endpoint(endpoint("e-1", "claude", "One"))
            .unwrap();
        let mut two = endpoint("e-2", "claude", "Two");
        two.is_default = true;
        store.put_model_endpoint(two).unwrap();
        assert_eq!(store.default_model_endpoint("claude").unwrap().id, "e-2");
        assert!(!store.model_endpoint("e-1").unwrap().is_default);
        // Turning the default off drops the default too - a chat created
        // with no pick then runs on the harness's own provider.
        store.set_model_endpoint_enabled("e-2", false).unwrap();
        assert!(store.default_model_endpoint("claude").is_none());
        // A disabled endpoint cannot be named the default.
        assert!(
            store
                .set_model_endpoint_default("claude", Some("e-2"))
                .is_err()
        );
        store.set_model_endpoint_enabled("e-2", true).unwrap();
        store
            .set_model_endpoint_default("claude", Some("e-2"))
            .unwrap();
        assert_eq!(store.default_model_endpoint("claude").unwrap().id, "e-2");
        // Clearing the default leaves the records, resolves none.
        store.set_model_endpoint_default("claude", None).unwrap();
        assert!(store.default_model_endpoint("claude").is_none());
        assert!(
            store
                .resolve_model_endpoint("claude", None)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn endpoint_resolution_names_a_record_or_the_default() {
        let mut store = Store::open(tempdir()).unwrap();
        let mut one = endpoint("e-1", "claude", "One");
        one.is_default = true;
        store.put_model_endpoint(one).unwrap();
        store
            .put_model_endpoint(endpoint("e-2", "claude", "Two"))
            .unwrap();
        // Named: the record. Unnamed: the default.
        assert_eq!(
            store
                .resolve_model_endpoint("claude", Some("e-2"))
                .unwrap()
                .unwrap()
                .id,
            "e-2"
        );
        assert_eq!(
            store
                .resolve_model_endpoint("claude", None)
                .unwrap()
                .unwrap()
                .id,
            "e-1"
        );
        // A record of another harness, and a disabled one, refuse with a
        // conflict - never silently the provider.
        store
            .put_model_endpoint(endpoint("e-x", "codex", "X"))
            .unwrap();
        assert!(store.resolve_model_endpoint("claude", Some("e-x")).is_err());
        store.set_model_endpoint_enabled("e-2", false).unwrap();
        assert!(store.resolve_model_endpoint("claude", Some("e-2")).is_err());
        // Unknown ids are not found, not conflicts.
        assert!(matches!(
            store.resolve_model_endpoint("claude", Some("e-missing")),
            Err(StoreError::NotFound(_))
        ));
    }

    #[test]
    fn session_endpoints_point_stopped_chats_and_model_ids_are_shape_checked() {
        let mut store = Store::open(tempdir()).unwrap();
        store
            .put_model_endpoint(endpoint("e-1", "claude", "One"))
            .unwrap();
        let mut s = session();
        store
            .append(s.id, EventPayload::SessionCreated { session: s.clone() })
            .unwrap();
        // Pointing names the record; clearing returns to the default-or-provider.
        let updated = store.set_session_endpoint(&s.id, Some("e-1")).unwrap();
        assert_eq!(updated.endpoint_id.as_deref(), Some("e-1"));
        s = updated;
        assert_eq!(
            store.session(&s.id).unwrap().endpoint_id.as_deref(),
            Some("e-1")
        );
        let cleared = store.set_session_endpoint(&s.id, None).unwrap();
        assert!(cleared.endpoint_id.is_none());
        assert!(
            store
                .set_session_endpoint(&s.id, Some("e-missing"))
                .is_err()
        );
        // A custom model id is free text in the harness's own spelling -
        // the only bar is shape.
        store
            .update_session_settings(&s.id, Some("my-custom-model-1".into()), None, None, None)
            .unwrap();
        assert_eq!(
            store.session(&s.id).unwrap().model.as_deref(),
            Some("my-custom-model-1")
        );
        assert!(
            store
                .update_session_settings(&s.id, Some("bad\nmodel".into()), None, None, None)
                .is_err()
        );
        // Empty means unpicked, never a model named "".
        store
            .update_session_settings(&s.id, Some("   ".into()), None, None, None)
            .unwrap();
        assert!(store.session(&s.id).unwrap().model.is_none());
    }

    #[test]
    fn endpoint_url_helpers_follow_the_v1_convention() {
        use crate::model::{endpoint_models_url, normalize_endpoint_base_url, validate_model_id};
        assert_eq!(
            endpoint_models_url("http://localhost:4000/v1", None).unwrap(),
            "http://localhost:4000/v1/models"
        );
        assert_eq!(
            endpoint_models_url("http://localhost:4000/v1/", None).unwrap(),
            "http://localhost:4000/v1/models"
        );
        assert_eq!(
            endpoint_models_url("http://localhost:4000/openai", Some("v1/models")).unwrap(),
            "http://localhost:4000/openai/v1/models"
        );
        assert_eq!(
            normalize_endpoint_models_path_override(" /v1/models/ "),
            "v1/models"
        );
        assert!(validate_model_id("gpt-4.1-mini").is_ok());
        assert!(validate_model_id("").is_err());
        assert!(validate_model_id("bad\nmodel").is_err());
        assert!(normalize_endpoint_base_url("http://localhost:4000/v1?x=1").is_err());
    }

    fn normalize_endpoint_models_path_override(raw: &str) -> String {
        crate::model::normalize_endpoint_models_path(raw).unwrap()
    }
}
