//! The supervisor: one live driver per session, events into the store,
//! broadcasts to every listener. All session steering funnels through here
//! so receipts, decisions, and lifecycles stay single-tracked. It knows
//! nothing about any specific harness - the registry dispatches.
//!
//! Ordering contract: the store is the truth; the broadcast channel is a
//! live nudge (SSE replays from the store on connect, so a dropped nudge
//! heals). Per session, an `order` lock serializes event sequences between
//! the steering paths (prompt / answer / stop) and the pump task; process
//! death (StdoutClosed) is applied by the pump alone - the single writer
//! for terminal statuses - so a stop can never race a failure into a lie.

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering as AtomicOrdering};
use std::sync::{Arc, Mutex as StdMutex, RwLock as StdRwLock};
use std::time::{Duration, Instant};

use openremote_core::{
    AutomationRule, Decision, DecisionId, DecisionState, Event, EventPayload, Harness, ImageRef,
    Machine, MachineId, MachineStatus, MachineView, McpServerEntry, Origin, ParentLink, Project,
    ProjectId, QueuedMessage, Receipt, RuleId, Session, SessionId, SessionStatus, Store,
    TriggerKind, TurnOutcome, now_ms,
};
use openremote_harness::{DriverEvent, Resolution};
use serde_json::Value;
use thiserror::Error;
use tokio::sync::{Mutex as AsyncMutex, broadcast, mpsc, oneshot};
use uuid::Uuid;

use crate::images::NewImage;
use crate::registry::{Backend, HarnessRegistry, SessionDriver};

mod carry;
mod composer;
pub use composer::{AgentEntry, CommandEntry, CommandsView, McpServerName};
mod team;
pub use team::{Offer, SpawnRequest, TeamSettings, state_word};

/// Store conflicts surface as their own shape, not a generic store error.
fn store_error(e: openremote_core::store::StoreError) -> SupervisorError {
    match e {
        openremote_core::store::StoreError::NotFound(m) => SupervisorError::NotFound(m),
        openremote_core::store::StoreError::Conflict(m) => SupervisorError::Conflict(m),
        other => SupervisorError::Store(other),
    }
}

/// A live harness applies a change on its running process, but not while a
/// turn is open: the driver is reading its own screen then, and a write would
/// land between two frames. The daemon's status is the fact the driver cannot
/// see, so the refusal is decided here, before any write.
fn refuse_live_mid_turn(settings_live: bool, status: SessionStatus) -> Option<&'static str> {
    (settings_live
        && matches!(
            status,
            SessionStatus::Starting | SessionStatus::Working | SessionStatus::Waiting
        ))
    .then_some("wait for this turn to finish, then change the model")
}

/// What a live harness's chat said before its latest settings change, kept
/// until the driver answers. The live wire has no acknowledgement, so the
/// change is stored when it is written; a `settings_refused` puts these
/// values back. Only the latest change is held: a driver answers its
/// commands in order, and the values before the latest change are the ones a
/// refusal of it restores.
#[derive(Clone, Debug, PartialEq)]
struct PriorSettings {
    model: Option<String>,
    effort: Option<String>,
    fast: bool,
}

fn prior_live_settings() -> &'static StdMutex<std::collections::HashMap<SessionId, PriorSettings>> {
    static PRIOR: std::sync::OnceLock<
        StdMutex<std::collections::HashMap<SessionId, PriorSettings>>,
    > = std::sync::OnceLock::new();
    PRIOR.get_or_init(Default::default)
}

/// Drop a chat's held settings: its driver is gone (stopped, exited) or the
/// chat itself is deleted, so no refusal can come for them any more.
pub fn forget_live_settings(id: &SessionId) {
    lock_recovering(prior_live_settings()).remove(id);
}

/// The chats being moved right now. A second move of the same chat is refused
/// rather than queued, so two moves never stop and restart one harness at once.
fn moving_chats() -> &'static StdMutex<std::collections::HashSet<SessionId>> {
    static MOVING: std::sync::OnceLock<StdMutex<std::collections::HashSet<SessionId>>> =
        std::sync::OnceLock::new();
    MOVING.get_or_init(Default::default)
}

/// A chat that is being moved stays put until the move ends: a resume or a
/// steer would start its harness in the folder it is leaving, and the move's
/// own restart would then fail on top of it.
fn refuse_moving(id: &SessionId) -> Result<(), SupervisorError> {
    if lock_recovering(moving_chats()).contains(id) {
        return Err(SupervisorError::Conflict(
            "this chat is being moved to another project - try again in a moment".into(),
        ));
    }
    Ok(())
}

/// A chat's place in [`moving_chats`], given back however its move ends.
struct Moving(SessionId);

impl Drop for Moving {
    fn drop(&mut self) {
        lock_recovering(moving_chats()).remove(&self.0);
    }
}

/// Moving needs a quiet chat: a turn in flight would be cut off mid-work.
/// `live` is whether the harness is up. A chat "starting" with its harness up
/// is only waiting for its first message (claude, grok and agy name their
/// conversation then) and moves like an idle one; starting with no harness yet
/// is a spawn still under way in the folder it had, and that is refused.
fn refuse_busy(session: &Session, live: bool) -> Result<(), SupervisorError> {
    match session.status {
        SessionStatus::Working | SessionStatus::Waiting => Err(SupervisorError::Conflict(
            "a turn is running in this chat - wait for it to finish, or stop it, then move the chat"
                .into(),
        )),
        SessionStatus::Starting if !live => Err(SupervisorError::Conflict(
            "this chat is starting - try again in a moment".into(),
        )),
        _ => Ok(()),
    }
}

/// A folder as the UI names a project: its last part.
fn folder_name(folder: &std::path::Path) -> String {
    folder
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| folder.display().to_string())
}

/// The line the transcript keeps where the chat moved.
fn move_note(before: &Session, target: Option<&std::path::Path>) -> String {
    match (target, &before.workspace) {
        (Some(folder), _) => format!(
            "Moved to {}. From the next message the agent works in {}.",
            folder_name(folder),
            folder.display()
        ),
        (None, Some(old)) => format!(
            "Moved out of {}. From the next message the agent works in the home folder.",
            folder_name(old)
        ),
        (None, None) => {
            "Moved out of its project. From the next message the agent works in the home folder."
                .to_string()
        }
    }
}

/// `HH:MM` in the machine's own wall clock, plus the day-minute key the
/// engine arms per rule (one fire per day at its time).
fn current_local_minute() -> (String, String) {
    let now = chrono::Local::now();
    (
        now.format("%Y-%m-%d %H:%M").to_string(),
        now.format("%H:%M").to_string(),
    )
}

/// Whether a schedule is due at `now_ms` and `now_time`. A daily rule
/// matches its own HH:MM; an interval rule is due once its cadence has
/// passed since the run it last recorded, or since it was created. The
/// clock's arming map still stops a second fire inside one tick.
fn schedule_due(rule: &AutomationRule, now_ms: i64, now_time: &str) -> bool {
    if !rule.enabled || rule.trigger.kind != TriggerKind::Schedule {
        return false;
    }
    match (rule.trigger.time.as_deref(), rule.trigger.every_minutes) {
        (Some(time), _) => time == now_time,
        (None, Some(minutes)) => {
            let since = rule.last_run.unwrap_or(rule.created_at);
            now_ms.saturating_sub(since) >= i64::from(minutes) * 60_000
        }
        (None, None) => false,
    }
}

/// Drops arming entries from days other than `today` (the `YYYY-MM-DD`
/// prefix of the day-minute key). An entry from an earlier day can never
/// match again - the engine arms per day - so it is garbage the moment
/// the day changes. Nothing to do while the day holds.
fn prune_fired(fired: &mut HashMap<RuleId, String>, today: &str) {
    fired.retain(|_, minute| minute.split(' ').next().unwrap_or_default() == today);
}

/// `HH:MM` or a clear error - the schedule's own format.
fn validate_time(time: Option<&str>) -> Result<String, SupervisorError> {
    let Some(time) = time else {
        return Err(SupervisorError::Conflict(
            "a schedule needs a time - HH:MM".into(),
        ));
    };
    if chrono::NaiveTime::parse_from_str(time, "%H:%M").is_ok() {
        Ok(time.to_string())
    } else {
        Err(SupervisorError::Conflict(
            "a schedule's time is HH:MM, local".into(),
        ))
    }
}

/// One machine's console view: this machine carries its real inventory,
/// sessions, presence, and install rows; a waiting one carries the install
/// command for its OS (the enrollment token rides beside it).
fn machine_view(
    s: &Store,
    harnesses: &[Harness],
    node_ready: bool,
    machine: &Machine,
) -> MachineView {
    let mut view = MachineView {
        machine: machine.clone(),
        harnesses: Vec::new(),
        sessions: Vec::new(),
        presence: Vec::new(),
        installable: Vec::new(),
        install_command: None,
    };
    if machine.this_machine {
        view.harnesses = harnesses.to_vec();
        view.sessions = s.sessions().into_iter().cloned().collect();
        view.presence = s.presence(&machine.id);
        let installed: Vec<String> = harnesses
            .iter()
            .filter(|h| h.available)
            .map(|h| h.id.clone())
            .collect();
        view.installable = crate::install::installable(&installed, node_ready);
    } else {
        view.install_command = Some(match machine.platform.as_str() {
            "windows" => "irm openremote.space/install.ps1 | iex".to_string(),
            _ => "curl -fsSL openremote.space/install | sh".to_string(),
        });
    }
    view
}

#[derive(Debug, Error)]
pub enum SupervisorError {
    #[error("{0}")]
    NotFound(String),
    #[error("{0}")]
    Conflict(String),
    #[error("harness unavailable: {0}")]
    Harness(String),
    #[error("store: {0}")]
    Store(#[from] openremote_core::store::StoreError),
    #[error("driver: {0}")]
    Driver(#[from] openremote_harness::DriverError),
}

/// What a webhook delivery did. Webhook senders are outside the
/// console's request_id contract, so a refused delivery answers 200
/// with `fired: false`, not an error status - an error status is what
/// sends a retrying sender around again, and the retry is exactly what
/// the refusal exists to stop.
pub enum WebhookFire {
    /// The delivery fired: the chat it opened.
    Fired(Box<Session>),
    /// The delivery was refused - a replay inside the dedup window, or
    /// the rule inside its cooldown. The string is the reason the
    /// answer carries back to the sender.
    Refused(&'static str),
}

/// A webhook delivery replays as a duplicate inside this window. Sender
/// retries come back in seconds (the daemon still busy on the first
/// delivery, the sender's patience already spent) and the body is the
/// only idempotency key an open route's caller brings. Past the window a
/// replay fires again, exactly as a machine that never saw the first
/// delivery would answer it - same reasoning as the receipt cap, whose
/// retries also come back within minutes, not years.
const WEBHOOK_DEDUP_WINDOW: Duration = Duration::from_secs(5 * 60);

/// A rule fires at most this often, whatever the bodies. A storm of
/// deliveries - a sender's retry loop, a captured URL - cannot multiply
/// chats faster than one per interval, and a single legitimate delivery
/// never sees it: nothing fired the rule in the second before it. The
/// interval runs from a fire's end, so a fire that itself takes a while
/// has not burned its own cooldown while it ran.
const WEBHOOK_COOLDOWN: Duration = Duration::from_secs(1);

/// A webhook rule's delivery memory: what fired recently. Refused
/// deliveries record nothing, so the maps grow only as fast as real
/// fires - at most one delivery per rule per cooldown - and prune with
/// the window on the next delivery anywhere.
#[derive(Default)]
struct WebhookMemory {
    /// (rule, digest of the raw body) -> when it fired. The digest, not
    /// the body: a hook's payload is the sender's to define and can be
    /// arbitrarily large, and the map holds only 32 bytes of it.
    deliveries: HashMap<(RuleId, [u8; 32]), Instant>,
    /// rule -> when it last fired (or started firing - the reservation
    /// that closes the in-flight window a concurrent replay would race
    /// through).
    cooled: HashMap<RuleId, Instant>,
}

impl WebhookMemory {
    /// Entries older than their window are garbage: the window is the
    /// retention, and a replay past it is a new delivery.
    fn prune(&mut self) {
        self.deliveries
            .retain(|_, fired| fired.elapsed() < WEBHOOK_DEDUP_WINDOW);
        self.cooled
            .retain(|_, fired| fired.elapsed() < WEBHOOK_COOLDOWN);
    }

    /// A deleted rule's memory goes with it - the deliveries it deduped
    /// and its cooldown slot belong to nothing.
    fn forget_rule(&mut self, id: &RuleId) {
        self.cooled.remove(id);
        self.deliveries.retain(|(rule, _), _| rule != id);
    }
}

struct SessionEntry {
    driver: Arc<AsyncMutex<SessionDriver>>,
    /// Serializes event sequences for this session across tasks.
    order: Arc<AsyncMutex<()>>,
    stopping: Arc<AtomicBool>,
    /// Fired by the pump when the stop it was told about completes.
    done: Option<oneshot::Sender<Session>>,
}

// ---- stop's drain window ----
//
// After the kill, the pump may still hold a backlog of the turn's tail -
// up to 256 buffered events, each an append_batch (a log write plus a
// state persist) under the order lock. A flat clock here is a race: too
// short and the fallback removes the entry while the tail is still
// applying, discarding tool results, the turn boundary, and the
// retire that rides it; too long and every stop of a print-mode harness
// between prompts (no process, nothing to drain) waits it out. So the
// wait ends on evidence, not on a guess: the pump answers when it
// drains (it applies StdoutClosed and settles), the transcript's seq
// advances while it is still applying, and only a transcript that has
// stopped advancing for this long is declared quiet.

/// One progress check per slice while waiting for the pump.
const STOP_DRAIN_SLICE: Duration = Duration::from_millis(250);
/// No seq advance for this long means nothing is draining - settle here.
const STOP_DRAIN_QUIET: Duration = Duration::from_millis(750);
/// The generous total bound: a draining pump gets at most this long.
const STOP_DRAIN_BOUND: Duration = Duration::from_secs(10);
/// The fallback's wait for the order lock: a pump wedged mid-event holds
/// it, and the settle must not wedge behind it.
const STOP_SETTLE_LOCK: Duration = Duration::from_secs(2);

// ---- the per-session stderr ring ----
//
// A harness that dies with its explanation on stderr ("No conversation
// found") used to leave that line only in the daemon's own log; the
// session recorded the generic fact. The ring keeps the harness's last
// words where the death can quote them.

/// Lines kept per session - enough tail to name the failure, little
/// enough that a chatty harness costs nothing.
const STDERR_RING_LINES: usize = 8;
/// Lines spliced into the Failed reason - the last few, not the whole
/// ring; a reason is a sentence, not a log.
const STDERR_REASON_LINES: usize = 4;
/// One line's bound on capture - a single runaway stderr line must not
/// ride into the session record whole.
const STDERR_LINE_BOUND: usize = 300;

/// The resolved CLI behind a backend - every variant holds one, and the
/// sign-in relay drives it with the same resolution the sessions do. A
/// plugin's harness has no CLI the daemon resolved; it signs in through its
/// own setup, so the relay has nothing to drive.
fn backend_resolution(
    backend: &crate::registry::Backend,
) -> Option<&openremote_harness::Resolution> {
    match backend {
        crate::registry::Backend::Claude(r) => Some(r),
        crate::registry::Backend::Codex(r) => Some(r),
        crate::registry::Backend::Grok(r) => Some(r),
        crate::registry::Backend::Pi(r) => Some(r),
        crate::registry::Backend::Opencode(r) => Some(r),
        crate::registry::Backend::Agy(r) => Some(r),
        crate::registry::Backend::Plugin(_) => None,
    }
}

pub struct Supervisor {
    store: Arc<StdMutex<Store>>,
    sessions: AsyncMutex<HashMap<SessionId, SessionEntry>>,
    events: broadcast::Sender<String>,
    /// Swappable: an install re-probes the machine and trades the set in.
    registry: StdRwLock<HarnessRegistry>,
    /// Live sign-in relays, one per harness: the console polls their view
    /// and feeds the human's answers through.
    signins: StdMutex<HashMap<String, Arc<crate::signin::SignInRun>>>,
    /// The clock's arming memory: the minute each schedule rule last fired
    /// in, so a rule fires once per day. Shared with rule removal - a
    /// deleted rule's entry must go with it - and pruned to the current
    /// day each tick, or the map ratchets with every rule the account
    /// ever had.
    fired: StdMutex<HashMap<RuleId, String>>,
    /// Webhook delivery memory (see `WebhookMemory`): the open route's
    /// own idempotency - hook senders bring no request_id, so the body
    /// they post is all the identity a delivery has.
    webhooks: StdMutex<WebhookMemory>,
    /// Each live session's recent harness stderr:
    /// the bounded ring a session's death quotes into its Failed reason.
    /// Entries ride with the session map entry - created at spawn, gone
    /// at removal - so the map never outlives the session it explains.
    stderr_ring: StdMutex<HashMap<SessionId, VecDeque<String>>>,
    /// A message steered into a harness that cannot take input mid-turn
    /// (one process per prompt): the turn was interrupted for it, and the
    /// turn's end sends it before anything queued.
    steer_next: StdMutex<HashMap<SessionId, Steered>>,
    /// The daemon's own machine - sessions run here, plugins ride from here.
    this_machine: MachineId,
    /// Each harness's command list per workspace, as the composer's /
    /// menu shows it (composer.rs).
    command_cache: StdMutex<HashMap<(String, std::path::PathBuf), composer::CachedCommands>>,
    /// The command names a running chat last listed, per harness and
    /// workspace. They stand in when the harness has no way to list its
    /// commands before a chat runs (grok names them only on a chat's init).
    listed_commands: StdMutex<HashMap<(String, std::path::PathBuf), Vec<String>>>,
    /// The read-a-chat MCP server (OpenRemote Controls' first tool): this
    /// binary in its `mcp-chat` mode, told the API's port. Unset, chats
    /// run without it (the e2e suites without a daemon binary).
    controls: std::sync::OnceLock<composer::Controls>,
    /// A short note for the agent added to each message as the harness
    /// receives it - never to what is stored or shown (the browser's open
    /// tabs). Set once by whoever assembles the app.
    prompt_note: std::sync::OnceLock<Arc<dyn PromptNotes>>,
    /// The plugin runtime's ear for a plugin folder replaced here.
    plugin_changed: std::sync::OnceLock<PluginChanged>,
    /// Agent teams: the user's limits and the reports in flight.
    team: team::Team,
    /// The chats' pictures: what the user attached, what tools answered
    /// with. A chat's go when the chat does (the store releases them).
    blobs: Arc<openremote_core::blobs::Blobs>,
}

/// Where the note riding a chat's delivered messages comes from (the
/// browser's open tabs), and what it hears back: a note counts as told only
/// once the message carrying it reached the harness.
pub trait PromptNotes: Send + Sync {
    /// The note for a chat's next message, if there is anything to tell it.
    fn note(&self, id: &SessionId) -> Option<String>;
    /// The message carrying the latest note was delivered, or was not.
    fn settled(&self, id: &SessionId, delivered: bool);
    /// The chat's conversation starts over (a fresh start, /clear) or the
    /// chat is gone: it was told nothing.
    fn forget(&self, id: &SessionId);
}

type PluginChanged =
    Box<dyn Fn(String) -> futures_util::future::BoxFuture<'static, ()> + Send + Sync>;

/// A message held for the next turn: its words, who sent it when another
/// agent did, and its pictures, already kept.
type Steered = (String, Option<Origin>, Vec<ImageRef>);

/// One poisoning policy for every mutex that routes through this helper
/// (the store, the sync maps, the schedule's arming, the webhooks'
/// delivery memory): a poisoned lock means a panic escaped while it was
/// held, not that the data inside is unusable, so the guard is recovered
/// instead of lost. The sign-in relay's and the registry's locks keep
/// their own patterns, deliberately. Every store acquisition goes
/// through here: before this was shared, a single panic under the lock
/// would brick every store-backed route until restart while sync kept
/// mutating state the API could no longer read (sync has always
/// recovered this way; this is that policy, shared). A recovered lock
/// may hold half-finished state, which can surface as one bad answer,
/// the better side of the trade against a dead API.
pub(crate) fn lock_recovering<T>(m: &StdMutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// What a harness reads ahead of a message another agent sent: who it is
/// from, and that it is not the user - a lead or a worker never takes an
/// agent's words for the user's.
fn origin_line(origin: &Origin) -> String {
    let title = origin.title.replace('"', "'").replace(['\n', '\r'], " ");
    format!("[Message from agent \"{title}\", not from the user]")
}

impl Supervisor {
    /// Build the supervisor over an opened store and a probed registry.
    /// The registry is injected so tests can point harnesses at fixture
    /// agents; production passes `HarnessRegistry::probe(&HashMap::new())`.
    pub fn new(store: Arc<StdMutex<Store>>, registry: HarnessRegistry) -> Arc<Self> {
        let (this_machine, data_dir, blobs) = {
            let mut guard = lock_recovering(&store);
            (
                guard.ensure_this_machine(),
                guard.dir().to_path_buf(),
                guard.blobs(),
            )
        };
        let supervisor = Arc::new(Self {
            store,
            sessions: AsyncMutex::new(HashMap::new()),
            events: broadcast::channel(1024).0,
            registry: StdRwLock::new(registry),
            signins: StdMutex::new(HashMap::new()),
            fired: StdMutex::new(HashMap::new()),
            webhooks: StdMutex::new(WebhookMemory::default()),
            stderr_ring: StdMutex::new(HashMap::new()),
            steer_next: StdMutex::new(HashMap::new()),
            this_machine,
            command_cache: StdMutex::new(HashMap::new()),
            listed_commands: StdMutex::new(HashMap::new()),
            controls: std::sync::OnceLock::new(),
            prompt_note: std::sync::OnceLock::new(),
            plugin_changed: std::sync::OnceLock::new(),
            team: team::Team::open(&data_dir),
            blobs,
        });
        supervisor.spawn_presence_ticker(this_machine);
        supervisor.spawn_schedule_engine();
        supervisor
    }

    /// The agent's own presence: mark this machine's half-hour slice while
    /// the daemon runs (the uptime band's truth).
    fn spawn_presence_ticker(self: &Arc<Self>, machine: MachineId) {
        let supervisor = Arc::clone(self);
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(std::time::Duration::from_secs(60));
            tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            loop {
                tick.tick().await;
                let _ = supervisor.with_store(|s| s.mark_present(&machine));
            }
        });
    }

    pub fn harnesses(&self) -> Vec<Harness> {
        let mut list = self.registry.read().expect("registry lock").harnesses();
        self.with_store(|s| {
            for harness in &mut list {
                // The store remembers the on/off switch of the six built-in
                // harness plugins. A plugin-provided harness follows its own
                // plugin's record, which the registry probe already applied.
                if harness.plugin.is_none() {
                    harness.enabled = s.builtin_enabled(&harness.id);
                }
            }
        });
        list
    }

    /// The models a harness advertises, in its own words, with the
    /// probe's failure kept apart from the answer: `Ok(None)` = the
    /// harness advertises nothing through us (a real answer; the slot
    /// stays reserved), `Ok(Some(list))` = the probe answered (an empty
    /// list is the harness advertising no models), `Err(Driver)` = the
    /// probe failed - transient, a later ask may find the CLI signed in
    /// and booted. The other errors name the harness itself: unknown, or
    /// known but not installed.
    pub async fn models(
        &self,
        harness: &str,
    ) -> Result<Option<Vec<openremote_harness::ModelDescriptor>>, SupervisorError> {
        let backend = {
            let registry = self.registry.read().expect("registry lock");
            if !registry.harnesses().iter().any(|h| h.id == harness) {
                return Err(SupervisorError::Harness(format!(
                    "harness '{harness}' is not known"
                )));
            }
            registry.backend(harness).cloned()
        };
        // A harness that resolved nowhere (not installed) never got a
        // probe to fail: its answer is its own error, not a catalog.
        let Some(backend) = backend else {
            return Err(SupervisorError::Harness(format!(
                "'{harness}' is not installed on this machine"
            )));
        };
        backend.models().await.map_err(SupervisorError::Driver)
    }

    /// A clone of the harness's backend - never a borrowed guard, so
    /// nothing holds the registry across an await.
    fn backend(&self, id: &str) -> Option<Backend> {
        self.registry
            .read()
            .expect("registry lock")
            .backend(id)
            .cloned()
    }

    /// Whether the harness slot is fixture-backed (e2e injection) - its
    /// config reads stay with the machine's real harnesses.
    fn is_fixture(&self, id: &str) -> bool {
        self.registry.read().expect("registry lock").is_fixture(id)
    }

    /// Subscribe to the live event stream (serialized event JSON).
    /// Syncs a private chat to the user's other devices: it keeps running
    /// here, now as a synced chat, and every device copies it whole.
    pub fn sync_session(
        &self,
        id: &SessionId,
        me: openremote_core::DeviceId,
    ) -> Result<Session, SupervisorError> {
        let mut session = self.session(id)?;
        match session.executor {
            Some(runner) if runner == me => return Ok(session),
            Some(_) => {
                return Err(SupervisorError::Conflict(
                    "this chat runs on another device".into(),
                ));
            }
            None => {}
        }
        session.executor = Some(me);
        self.emit_all(id, vec![EventPayload::SessionUpdated { session }])?;
        self.session(id)
    }

    /// Tells live listeners about an event that arrived from another
    /// device (a copy of a chat running there).
    pub fn announce(&self, raw: String) {
        let _ = self.events.send(raw);
    }

    pub fn subscribe(&self) -> broadcast::Receiver<String> {
        self.events.subscribe()
    }

    fn with_store<R>(&self, f: impl FnOnce(&mut Store) -> R) -> R {
        let mut guard = lock_recovering(&self.store);
        f(&mut guard)
    }

    /// A copy of a chat that runs on another device is only ever read here:
    /// acting on it (above all, resuming it) would start a second runner.
    fn ensure_here(&self, id: &SessionId) -> Result<(), SupervisorError> {
        let here = self
            .with_store(|s| s.session(id).map(|session| s.executes_here(session)))
            .map_err(store_error)?;
        if here {
            Ok(())
        } else {
            Err(SupervisorError::Conflict(
                "this chat runs on another device".into(),
            ))
        }
    }

    /// Append a sequence of events atomically (one store lock) and nudge
    /// the live listeners.
    fn emit_all(
        &self,
        id: &SessionId,
        payloads: Vec<EventPayload>,
    ) -> Result<Vec<Event>, SupervisorError> {
        let events = self.with_store(|s| s.append_batch(*id, payloads))?;
        for event in &events {
            let _ = self
                .events
                .send(serde_json::to_string(event).unwrap_or_default());
        }
        Ok(events)
    }

    pub fn sessions(&self) -> Vec<Session> {
        self.with_store(|s| s.sessions().into_iter().cloned().collect())
    }

    pub fn session(&self, id: &SessionId) -> Result<Session, SupervisorError> {
        self.with_store(|s| s.session(id).cloned())
            .map_err(|e| SupervisorError::NotFound(e.to_string()))
    }

    pub fn decisions(&self, id: &SessionId) -> Vec<Decision> {
        self.with_store(|s| s.decisions(id).into_iter().cloned().collect())
    }

    pub fn decision(&self, id: &DecisionId) -> Result<Decision, SupervisorError> {
        self.with_store(|s| s.decision(id).cloned())
            .map_err(|e| SupervisorError::NotFound(e.to_string()))
    }

    pub fn receipt(&self, request_id: &str) -> Option<Receipt> {
        self.with_store(|s| s.receipt(request_id).cloned())
    }

    /// Record a receipt; `false` means the request was seen before and the
    /// stored outcome stands (the caller returns it, never re-executes).
    pub fn record_receipt(&self, receipt: Receipt) -> bool {
        self.with_store(|s| s.record_receipt(receipt).unwrap_or(false))
    }

    // ---- machines ----

    /// Every machine as the console sees it: what's really on it, what
    /// runs there, and - for this machine - what could be installed.
    pub fn machines(&self) -> Vec<MachineView> {
        let (harnesses, node_ready) = self.inventory();
        self.with_store(|s| {
            s.machines()
                .iter()
                .map(|machine| machine_view(s, &harnesses, node_ready, machine))
                .collect()
        })
    }

    pub fn machine(&self, id: &MachineId) -> Result<MachineView, SupervisorError> {
        let (harnesses, node_ready) = self.inventory();
        self.with_store(|s| {
            s.machine(id)
                .map(|m| machine_view(s, &harnesses, node_ready, m))
        })
        .map_err(store_error)
    }

    /// The probed harnesses and whether pi's runtime is here - one read
    /// of the registry, never held across the store lock.
    fn inventory(&self) -> (Vec<Harness>, bool) {
        let registry = self.registry.read().expect("registry lock");
        (registry.harnesses(), registry.node_ready())
    }

    /// Add a machine: it lands `waiting` - it comes online when its agent
    /// checks in (the next machines pass), never from this call.
    pub fn create_machine(&self, name: &str, platform: &str) -> Result<Machine, SupervisorError> {
        self.with_store(|s| s.create_machine(name, platform))
            .map_err(store_error)
    }

    pub fn remove_machine(&self, id: &MachineId) -> Result<(), SupervisorError> {
        self.with_store(|s| s.remove_machine(id))
            .map_err(store_error)
    }

    /// Install a harness on this machine: run its owner's installer,
    /// re-probe with the same overrides, and hand back the machine's
    /// refreshed view. The receipt carries the long wait - installers run
    /// minutes. `with_runtime` is the human's go-ahead to set up pi's
    /// Node.js first.
    pub async fn install_harness(
        &self,
        machine_id: &MachineId,
        harness_id: &str,
        with_runtime: bool,
    ) -> Result<MachineView, SupervisorError> {
        let machine = self
            .with_store(|s| s.machine(machine_id).cloned())
            .map_err(store_error)?;
        if !machine.this_machine {
            return Err(SupervisorError::Conflict(format!(
                "{} has not checked in yet",
                machine.name
            )));
        }
        let entry = self
            .registry
            .read()
            .expect("registry lock")
            .harnesses()
            .into_iter()
            .find(|h| h.id == harness_id);
        let Some(entry) = entry else {
            return Err(SupervisorError::Harness(format!(
                "harness '{harness_id}' is not known"
            )));
        };
        if entry.available {
            return Err(SupervisorError::Conflict(format!(
                "'{harness_id}' is already installed on {}",
                machine.name
            )));
        }

        let options = crate::install::InstallOptions {
            with_runtime,
            node_ready: self.registry.read().expect("registry lock").node_ready(),
        };
        let output = crate::install::install(harness_id, options)
            .await
            .map_err(|failed| SupervisorError::Harness(format!("install failed: {failed}")))?;

        self.swap_in_fresh_registry().await;

        // An installer that exits cleanly but leaves nothing where this
        // harness is looked for is not an install - say so with its own
        // last words.
        let landed = self
            .registry
            .read()
            .expect("registry lock")
            .harnesses()
            .into_iter()
            .any(|h| h.id == harness_id && h.available);
        if !landed {
            return Err(SupervisorError::Harness(format!(
                "the installer finished, but {} wasn't found where OpenRemote looks for it:\n{output}",
                entry.name
            )));
        }

        self.machine(machine_id)
    }

    /// Run the harness's own login command, relayed. The console streams
    /// the CLI's own words and feeds the human's answers through; when
    /// the CLI exits, the registry is re-probed so the sign-in fact comes
    /// from the harness's own status words. The returned view is the
    /// first beat of the relay.
    pub async fn start_sign_in(
        self: &Arc<Self>,
        harness_id: &str,
    ) -> Result<crate::signin::SignInView, SupervisorError> {
        if !crate::signin::login_supported(harness_id) {
            return Err(SupervisorError::Harness(format!(
                "'{harness_id}' signs in through its own setup - no login command to relay"
            )));
        }
        let resolution = {
            let registry = self.registry.read().expect("registry lock");
            let entry = registry
                .harnesses()
                .into_iter()
                .find(|h| h.id == harness_id)
                .ok_or_else(|| {
                    SupervisorError::Harness(format!("harness '{harness_id}' is not known"))
                })?;
            if !entry.available {
                return Err(SupervisorError::Harness(format!(
                    "'{harness_id}' is not installed on this machine"
                )));
            }
            if registry.is_fixture(harness_id) {
                return Err(SupervisorError::Harness(format!(
                    "'{harness_id}' has no login command to relay"
                )));
            }
            let backend = registry.backend(harness_id).ok_or_else(|| {
                SupervisorError::Harness(format!("'{harness_id}' is not available to the daemon"))
            })?;
            backend_resolution(backend).cloned().ok_or_else(|| {
                SupervisorError::Harness(format!(
                    "'{harness_id}' signs in through its own setup, in a terminal"
                ))
            })?
        };
        // A settled relay makes room for the fresh one: the old run's view
        // stays answerable (the console may still be showing its last
        // beat), but the new run replaces it in the map.
        if let Some(existing) = self.signins.lock().expect("signin lock").get(harness_id) {
            if existing.view().running {
                return Err(SupervisorError::Conflict(format!(
                    "a sign-in for '{harness_id}' is already in progress"
                )));
            }
        }

        let (run, mut child) = crate::signin::spawn_login(harness_id, &resolution)
            .await
            .map_err(SupervisorError::Harness)?;
        self.signins
            .lock()
            .expect("signin lock")
            .insert(harness_id.to_string(), std::sync::Arc::clone(&run));

        // The reaper: the CLI's exit settles the relay, then the registry
        // re-probes so the sign-in fact is the harness's own status words.
        let supervisor = Arc::clone(self);
        let reaper_run = std::sync::Arc::clone(&run);
        tokio::spawn(async move {
            // The login runs until the CLI exits, the human stops it, or
            // the timeout trips. The stop signal stores its permit, so a
            // stop that lands before the reaper reaches its wait still
            // takes effect.
            let stop = reaper_run.stopped();
            tokio::select! {
                status = child.wait() => {
                    let settled = match status {
                        Ok(status) => crate::signin::settle(&reaper_run, &status),
                        Err(err) => crate::signin::fail(&reaper_run, &err.to_string()),
                    };
                    if settled.ok {
                        supervisor.swap_in_fresh_registry().await;
                    }
                }
                _ = stop => {
                    let _ = child.kill().await;
                    crate::signin::fail(&reaper_run, "stopped");
                }
                _ = tokio::time::sleep(crate::signin::timeout()) => {
                    let _ = child.kill().await;
                    crate::signin::fail(&reaper_run, "login timed out after ten minutes");
                }
            }
        });
        Ok(run.view())
    }

    /// The current beat of a harness's sign-in relay - `None` when none
    /// was ever started for it.
    pub fn sign_in_view(&self, harness_id: &str) -> Option<crate::signin::SignInView> {
        self.signins
            .lock()
            .expect("signin lock")
            .get(harness_id)
            .map(|run| run.view())
    }

    /// Feed one line to the harness's own login prompt (claude's pasted
    /// code). The words the CLI prints back arrive through the view.
    pub fn feed_sign_in(&self, harness_id: &str, line: &str) -> Result<(), SupervisorError> {
        let run = self
            .signins
            .lock()
            .expect("signin lock")
            .get(harness_id)
            .cloned()
            .ok_or_else(|| {
                SupervisorError::Conflict(format!("no sign-in is running for '{harness_id}'"))
            })?;
        run.feed(line).map_err(SupervisorError::Harness)
    }

    /// Stop a running sign-in relay - a human who abandoned the browser
    /// flow should not wait out the timeout to try again. Stopping a
    /// relay that already settled is a no-op, not an error.
    pub fn stop_sign_in(&self, harness_id: &str) {
        if let Some(run) = self.signins.lock().expect("signin lock").get(harness_id) {
            run.stop();
        }
    }

    /// Fresh facts after the machine changed underneath us - an install
    /// landed, a login finished, a plugin was installed, toggled or removed.
    /// The guard is never held across the await: the overrides and the
    /// installed plugins' harness specs are cloned first, then the whole
    /// registry is swapped in.
    async fn swap_in_fresh_registry(&self) {
        let plugins = self.plugin_harnesses();
        let overrides = self
            .registry
            .read()
            .expect("registry lock")
            .overrides()
            .clone();
        let fresh = HarnessRegistry::probe(&overrides, &plugins).await;
        *self.registry.write().expect("registry lock") = fresh;
    }

    /// The harness specs the installed plugins declare, read from their
    /// folders. Called at probe time and after any plugin change.
    fn plugin_harnesses(&self) -> Vec<crate::plugins::HarnessSpec> {
        self.with_store(|store| {
            let data_dir = store.dir().to_path_buf();
            crate::plugins::installed_harnesses(&data_dir, &store.installed_plugins())
        })
    }

    /// Re-probe after a plugin changed what it provides. Called by the
    /// plugin routes; chats already running keep their driver process.
    pub async fn reprobe_harnesses(&self) {
        self.swap_in_fresh_registry().await;
    }

    /// The installed plugin's spec behind a harness id, when a plugin
    /// provides it. A catalog refresh runs the driver itself, so it needs
    /// the folder and the executable, which only the spec carries.
    pub fn plugin_spec(&self, id: &str) -> Option<crate::plugins::HarnessSpec> {
        match self.backend(id) {
            Some(Backend::Plugin(plugin)) => Some(plugin.spec),
            _ => None,
        }
    }

    /// Whether this harness's driver can change a setting on a running chat.
    /// Read from the probed row, which the plugin's manifest decided, so the
    /// session's own stored word never decides it.
    fn settings_are_live(&self, harness: &str) -> bool {
        self.registry
            .read()
            .expect("registry lock")
            .harnesses()
            .into_iter()
            .any(|row| row.id == harness && row.settings_live)
    }

    /// The live chats running a plugin's harnesses, by session id. A plugin
    /// update or removal replaces the folder its driver runs from, and
    /// Windows will not rename or delete a folder holding a running image,
    /// so the caller refuses with these instead of a bare access error.
    pub async fn plugin_chats(&self, plugin_id: &str) -> Vec<String> {
        let harnesses: Vec<String> = self
            .registry
            .read()
            .expect("registry lock")
            .harnesses()
            .into_iter()
            .filter(|harness| harness.plugin.as_deref() == Some(plugin_id))
            .map(|harness| harness.id)
            .collect();
        if harnesses.is_empty() {
            return Vec::new();
        }
        let ids: Vec<SessionId> = self.sessions.lock().await.keys().cloned().collect();
        let mut chats = Vec::new();
        for id in ids {
            let Ok(session) = self.session(&id) else {
                continue;
            };
            if !harnesses.iter().any(|harness| harness == &session.harness) {
                continue;
            }
            if matches!(
                session.status,
                openremote_core::SessionStatus::Starting
                    | openremote_core::SessionStatus::Working
                    | openremote_core::SessionStatus::Waiting
                    | openremote_core::SessionStatus::Idle
            ) {
                chats.push(session.id.to_string());
            }
        }
        chats
    }

    // ---- MCP servers ----

    pub fn mcp_server_entries(&self) -> Vec<McpServerEntry> {
        self.with_store(|s| s.mcp_servers().into_iter().cloned().collect())
    }

    /// Add an MCP server written by hand, on a machine that has checked
    /// in. The store dedups; the machine's reachability is ruled here.
    pub fn add_mcp_server(
        &self,
        machine_id: &MachineId,
        name: &str,
        detail: &str,
        command: &str,
        needs_key: bool,
    ) -> Result<McpServerEntry, SupervisorError> {
        let machine = self
            .with_store(|s| s.machine(machine_id).cloned())
            .map_err(store_error)?;
        if machine.status != MachineStatus::Online {
            return Err(SupervisorError::Conflict(format!(
                "{} has not checked in yet",
                machine.name
            )));
        }
        let server = McpServerEntry {
            id: format!("custom-{}@{}", uuid::Uuid::new_v4(), machine.id),
            catalog_id: None,
            name: name.to_string(),
            detail: detail.to_string(),
            command: command.to_string(),
            machine: machine.id,
            needs_key,
            has_key: false,
            enabled: true,
            created_at: now_ms(),
            updated_at: now_ms(),
        };
        self.with_store(|s| s.add_mcp_server(server))
            .map_err(store_error)
    }

    /// Where `harness` keeps the state it would otherwise write into a
    /// chat's workspace: a folder of its own inside the data folder, which
    /// is this user's alone. The driver creates what it needs inside.
    fn harness_state_dir(&self, harness: &str) -> std::path::PathBuf {
        self.with_store(|s| s.dir().join("harness-state").join(harness))
    }

    /// The folders of the plugins a chat in `workspace` gets that claude
    /// can load - each has Claude's own plugin.json (the install writes one
    /// where Claude parts arrived without it). A chat in a project gets
    /// that project's plugins too, and no other project's.
    fn plugin_dirs(&self, workspace: Option<&std::path::Path>) -> Vec<std::path::PathBuf> {
        self.with_store(|s| {
            let data_dir = s.dir().to_path_buf();
            crate::plugin_runtime::scope::active(s, &data_dir, workspace)
                .into_iter()
                .map(|active| active.dir)
                .filter(|dir| dir.join(".claude-plugin").join("plugin.json").is_file())
                .collect()
        })
    }

    /// How the supervisor tells the plugin runtime that a plugin's folder
    /// changed under it (a command plugin made again): its program stops,
    /// and the next need starts the new one.
    pub fn set_plugin_changed(
        &self,
        changed: impl Fn(String) -> futures_util::future::BoxFuture<'static, ()> + Send + Sync + 'static,
    ) {
        let _ = self.plugin_changed.set(Box::new(changed));
    }

    /// Before a claude chat starts: each enabled plugin installed from a
    /// `command` source runs its approved command again, and a changed
    /// folder replaces the copy. A failure leaves the last copy; the
    /// answer is the notes the chat shows about it.
    async fn rerun_plugin_commands(&self, harness: &str) -> Vec<String> {
        if harness != "claude" {
            return Vec::new();
        }
        let (data_dir, plugins) = self.with_store(|s| {
            let plugins: Vec<openremote_core::InstalledPlugin> = s
                .installed_plugins()
                .into_iter()
                .filter(|p| p.enabled && p.command.is_some())
                .cloned()
                .collect();
            (s.dir().to_path_buf(), plugins)
        });
        let mut notes = Vec::new();
        for plugin in plugins {
            // Its program stops before the folder is replaced.
            let id = plugin.id();
            let stop = async {
                if let Some(changed) = self.plugin_changed.get() {
                    changed(id).await;
                }
            };
            match crate::plugins::rerun_command(&data_dir, &plugin, stop).await {
                Ok(Some(hash)) => {
                    let mut updated = plugin.clone();
                    updated.resolved = Some(hash);
                    updated.updated_at = now_ms();
                    let _ = self.with_store(|s| s.put_installed_plugin(updated));
                    // The folder was replaced: a program still running the
                    // old copy stops, and the next need starts this one.
                    if let Some(changed) = self.plugin_changed.get() {
                        changed(plugin.id()).await;
                    }
                }
                Ok(None) => {}
                Err(e) => notes.push(format!(
                    "The plugin {}'s command failed ({e}); this chat has its last copy.",
                    plugin.id()
                )),
            }
        }
        notes
    }

    pub fn remove_mcp_server(&self, id: &str) -> Result<(), SupervisorError> {
        self.with_store(|s| s.remove_mcp_server(id))
            .map_err(store_error)
    }

    pub fn set_mcp_server_enabled(
        &self,
        id: &str,
        enabled: bool,
    ) -> Result<McpServerEntry, SupervisorError> {
        self.with_store(|s| s.set_mcp_server_enabled(id, enabled))
            .map_err(store_error)
    }

    pub fn acknowledge_mcp_server_key(&self, id: &str) -> Result<McpServerEntry, SupervisorError> {
        self.with_store(|s| s.acknowledge_mcp_server_key(id))
            .map_err(store_error)
    }

    /// The MCP servers a new session on this machine should start: every
    /// enabled plugin whose key (if it needs one) the machine has. A plugin
    /// still waiting on its key acknowledgment doesn't ride - starting it
    /// would just fail.
    fn mcp_servers(&self) -> Vec<openremote_harness::McpServer> {
        let machine = self.this_machine;
        self.with_store(|s| {
            s.mcp_servers()
                .iter()
                .filter(|p| p.machine == machine && p.state() == "running")
                .filter_map(|p| {
                    let mut words = p.command.split_whitespace();
                    let command = words.next()?.to_string();
                    Some(openremote_harness::McpServer {
                        id: p.server_id(),
                        command,
                        args: words.map(String::from).collect(),
                    })
                })
                .collect()
        })
    }

    // ---- automations ----

    pub fn rules(&self) -> Vec<AutomationRule> {
        self.with_store(|s| s.rules().into_iter().cloned().collect())
    }

    /// Save a rule (create or edit - the id decides). The workspace must
    /// be real (the same bar POST /sessions holds); a webhook gets its
    /// own key generated on create, kept on edit.
    pub fn save_rule(
        &self,
        mut rule: AutomationRule,
        existing: Option<RuleId>,
    ) -> Result<AutomationRule, SupervisorError> {
        if let Some(existing) = existing {
            let _ = self
                .with_store(|s| s.rule(&existing).cloned())
                .map_err(store_error)?;
            rule.id = existing;
        } else {
            rule.id = RuleId::new();
            rule.created_at = now_ms();
        }
        if rule.name.trim().is_empty() || rule.task.trim().is_empty() {
            return Err(SupervisorError::Conflict(
                "a rule needs a name and a task".into(),
            ));
        }
        if !rule.workspace.is_absolute() || !rule.workspace.is_dir() {
            return Err(SupervisorError::Conflict(
                "the workspace must be an absolute path to an existing directory".into(),
            ));
        }
        match rule.trigger.kind {
            TriggerKind::Schedule => {
                match (rule.trigger.time.as_deref(), rule.trigger.every_minutes) {
                    (Some(time), None) => {
                        rule.trigger.time = Some(validate_time(Some(time))?);
                    }
                    (None, Some(minutes)) => {
                        if !(5..=1440).contains(&minutes) {
                            return Err(SupervisorError::Conflict(
                                "a schedule's interval is 5 to 1440 minutes".into(),
                            ));
                        }
                    }
                    (Some(_), Some(_)) => {
                        return Err(SupervisorError::Conflict(
                            "a schedule picks a time or an interval, not both".into(),
                        ));
                    }
                    (None, None) => {
                        return Err(SupervisorError::Conflict(
                            "a schedule needs a time (HH:MM) or an interval".into(),
                        ));
                    }
                }
                rule.trigger.key = None;
            }
            TriggerKind::Webhook => {
                rule.trigger.time = None;
                rule.trigger.every_minutes = None;
                // A webhook keeps the key it was born with - the hook
                // URLs already in the wild must keep working.
                rule.trigger.key = Some(
                    rule.trigger
                        .key
                        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string()),
                );
            }
        }
        rule.updated_at = now_ms();
        self.with_store(|s| s.save_rule(rule)).map_err(store_error)
    }

    pub fn remove_rule(&self, id: &RuleId) -> Result<(), SupervisorError> {
        self.with_store(|s| s.remove_rule(id))
            .map_err(store_error)?;
        // The rule is gone: its arming entry is garbage the clock would
        // otherwise carry for the daemon's lifetime - every rule the
        // account ever deleted, one map entry each.
        lock_recovering(&self.fired).remove(id);
        lock_recovering(&self.webhooks).forget_rule(id);
        Ok(())
    }

    // ---- projects ----

    pub fn projects(&self) -> Vec<Project> {
        self.with_store(|s| s.projects().into_iter().cloned().collect())
    }

    /// Register a project: its folders must be real (the same bar a chat's
    /// own create holds) and registered nowhere else. The folder's basename
    /// is the project's name - never stored, so the disk stays the only
    /// place it lives.
    pub fn save_project(
        &self,
        folders: Vec<std::path::PathBuf>,
    ) -> Result<Project, SupervisorError> {
        if folders.is_empty() {
            return Err(SupervisorError::Conflict("a project needs a folder".into()));
        }
        for folder in &folders {
            if !folder.is_absolute() || !folder.is_dir() {
                return Err(SupervisorError::Conflict(
                    "the folder must be an absolute path to an existing directory".into(),
                ));
            }
        }
        let project = Project {
            id: ProjectId::new(),
            folders,
            created_at: now_ms(),
            updated_at: now_ms(),
        };
        self.with_store(|s| s.save_project(project))
            .map_err(store_error)
    }

    pub fn remove_project(&self, id: &ProjectId) -> Result<(), SupervisorError> {
        self.with_store(|s| s.remove_project(id))
            .map_err(store_error)
    }

    pub fn set_rule_enabled(
        &self,
        id: &RuleId,
        enabled: bool,
    ) -> Result<AutomationRule, SupervisorError> {
        let mut rule = self
            .with_store(|s| s.rule(id).cloned())
            .map_err(store_error)?;
        rule.enabled = enabled;
        self.with_store(|s| s.save_rule(rule)).map_err(store_error)
    }

    /// Run a rule now - the same path the clock and the webhook take: a
    /// chat opens on the rule's machine with the rule's task.
    pub async fn run_rule(self: &Arc<Self>, id: &RuleId) -> Result<Session, SupervisorError> {
        let rule = self
            .with_store(|s| s.rule(id).cloned())
            .map_err(store_error)?;
        if !rule.enabled {
            return Err(SupervisorError::Conflict("the rule is not enabled".into()));
        }
        self.fire_rule(&rule).await
    }

    /// A webhook arrived: the rule must be a webhook rule, enabled, and
    /// the request must carry the rule's own key. The delivery is then
    /// deduped on (rule id, digest of the raw body) and rate-limited per
    /// rule - the open route has no request_id contract to lean on, and
    /// an answered error is exactly what sends a sender retrying (each
    /// retry fired a fresh chat before this). A refused delivery
    /// answers through, not as an error: errors are for retries.
    pub async fn fire_webhook(
        self: &Arc<Self>,
        id: &RuleId,
        key: Option<&str>,
        raw_body: &[u8],
    ) -> Result<WebhookFire, SupervisorError> {
        let rule = self
            .with_store(|s| s.rule(id).cloned())
            .map_err(store_error)?;
        if rule.trigger.kind != TriggerKind::Webhook {
            return Err(SupervisorError::Conflict("that rule has no webhook".into()));
        }
        if !rule.enabled {
            return Err(SupervisorError::Conflict("the rule is not enabled".into()));
        }
        let expected = rule.trigger.key.clone().unwrap_or_default();
        if expected.is_empty() || !openremote_core::same_secret(key.unwrap_or_default(), &expected)
        {
            return Err(SupervisorError::Conflict(
                "the webhook key does not match".into(),
            ));
        }
        // The open route's idempotency key: the body the sender brought,
        // hashed. Reservations and refusals happen under the one lock, so
        // two deliveries racing cannot both pass.
        let digest = openremote_cloud::sha256(raw_body);
        let delivery = (rule.id, digest);
        {
            let mut webhooks = lock_recovering(&self.webhooks);
            webhooks.prune();
            if webhooks
                .deliveries
                .get(&delivery)
                .is_some_and(|fired| fired.elapsed() < WEBHOOK_DEDUP_WINDOW)
            {
                return Ok(WebhookFire::Refused(
                    "this delivery already fired - a retry changes nothing",
                ));
            }
            if webhooks
                .cooled
                .get(&rule.id)
                .is_some_and(|fired| fired.elapsed() < WEBHOOK_COOLDOWN)
            {
                return Ok(WebhookFire::Refused(
                    "the rule is cooling down - it fired just now",
                ));
            }
            // Reserve before firing, so a replay that lands while the
            // fire is still in flight is a refusal, not a race through
            // an empty slot.
            webhooks.deliveries.insert(delivery, Instant::now());
            webhooks.cooled.insert(rule.id, Instant::now());
        }
        // The reservation stands whatever the fire does: a failure past
        // the session's creation is the chat's first fact (the rule ran,
        // badly), and a retry storm must not multiply chats just because
        // the first fire failed. The window is the sender's own re-arming
        // - a genuine later delivery is a new one by then.
        self.fire_rule(&rule)
            .await
            .map(|s| WebhookFire::Fired(Box::new(s)))
    }

    /// The one firing path: create the chat, deliver the task, record the
    /// run. A spawn that fails lands as the chat's first fact - the rule
    /// still ran.
    async fn fire_rule(
        self: &Arc<Self>,
        rule: &AutomationRule,
    ) -> Result<Session, SupervisorError> {
        // Rules run where the agent is online: today that is this machine
        // alone. A rule saved against a waiting machine refuses honestly
        // instead of quietly running here.
        if rule.machine != self.this_machine {
            let name = self
                .with_store(|s| s.machine(&rule.machine).map(|m| m.name.clone()))
                .ok()
                .unwrap_or_else(|| "that machine".to_string());
            return Err(SupervisorError::Conflict(format!(
                "'{name}' hasn't checked in - the rule waits until its agent is online"
            )));
        }
        let session = self
            .create_session(
                &rule.harness,
                Some(rule.workspace.clone()),
                rule.model.clone(),
                rule.effort.clone(),
                rule.permission_mode.clone(),
                false,
                None,
                None,
            )
            .await?;
        let chat = session.id;
        self.prompt(&chat, rule.task.trim()).await?;
        self.with_store(|s| s.record_rule_run(&rule.id, chat))
            .map_err(store_error)?;
        Ok(session)
    }

    /// The clock: every enabled schedule rule fires once per day at its
    /// own HH:MM. In-memory per-minute arming - a daemon restart re-arms;
    /// the worst case is a second chat in the same minute.
    fn spawn_schedule_engine(self: &Arc<Self>) {
        let supervisor = Arc::clone(self);
        tokio::spawn(async move {
            let tick_ms = std::env::var("OPENREMOTE_SCHEDULE_TICK_MS")
                .ok()
                .and_then(|v| v.parse::<u64>().ok())
                .unwrap_or(30_000);
            let mut tick = tokio::time::interval(std::time::Duration::from_millis(tick_ms));
            tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            loop {
                tick.tick().await;
                let (now_minute, now_time) = current_local_minute();
                let now = now_ms();
                let due: Vec<AutomationRule> = supervisor
                    .with_store(|s| {
                        s.rules()
                            .into_iter()
                            .filter(|r| schedule_due(r, now, &now_time))
                            .cloned()
                            .collect::<Vec<AutomationRule>>()
                    })
                    .into_iter()
                    .filter(|rule| {
                        // Already armed for this minute: done for the day.
                        lock_recovering(&supervisor.fired)
                            .get(&rule.id)
                            .is_none_or(|m| m != &now_minute)
                    })
                    .collect();
                for rule in due {
                    lock_recovering(&supervisor.fired).insert(rule.id, now_minute.clone());
                    match supervisor.fire_rule(&rule).await {
                        Ok(session) => eprintln!(
                            "automation '{}': fired on the clock, chat {}",
                            rule.name, session.id
                        ),
                        Err(err) => eprintln!("automation '{}': fire failed: {err}", rule.name),
                    }
                }
                // A day turned: an arming entry from an earlier day can
                // never match again (the engine arms per day), so it is
                // garbage the moment the day changes.
                let today = now_minute.split(' ').next().unwrap_or_default();
                prune_fired(&mut lock_recovering(&supervisor.fired), today);
            }
        });
    }

    // ---- session lifecycle ----

    /// Where a project-less chat runs: the home folder, the place the OS
    /// already means by "nowhere in particular".
    fn home_dir() -> std::path::PathBuf {
        std::env::var_os("USERPROFILE")
            .or_else(|| std::env::var_os("HOME"))
            .map(std::path::PathBuf::from)
            .unwrap_or_else(std::env::temp_dir)
    }

    /// Create a session and spawn its harness driver. The session exists
    /// even when the spawn fails - the failure is the session's first fact.
    /// A chat without a workspace (no project) runs in the home folder.
    #[allow(clippy::too_many_arguments)]
    /// The resolved custom endpoint for one spawn: metadata plus the secret
    /// for the harness child only. None means the harness's own provider.
    fn endpoint_binding(
        &self,
        harness: &str,
        endpoint_id: Option<&str>,
    ) -> Result<Option<openremote_harness::EndpointBinding>, SupervisorError> {
        let resolved = self
            .with_store(|s| s.resolve_model_endpoint(harness, endpoint_id))
            .map_err(store_error)?;
        Ok(resolved.map(|entry| openremote_harness::EndpointBinding {
            endpoint_id: entry.id.clone(),
            harness: entry.harness.clone(),
            endpoint_name: entry.name.clone(),
            base_url: entry.base_url.clone(),
            auth_kind: entry.auth_kind,
            secret: entry.key.clone(),
        }))
    }

    // Nine parameters, one per fact a caller decides when it starts a chat. A struct
    // would be the tidier signature and also a change to every caller and test; the
    // lint is a style rule, so it is allowed here with the reason rather than silenced.
    #[allow(clippy::too_many_arguments)]
    pub async fn create_session(
        self: &Arc<Self>,
        harness: &str,
        workspace: Option<std::path::PathBuf>,
        model: Option<String>,
        effort: Option<String>,
        permission_mode: Option<String>,
        fast: bool,
        executor: Option<openremote_core::DeviceId>,
        endpoint_id: Option<String>,
    ) -> Result<Session, SupervisorError> {
        self.create_session_in(
            harness,
            workspace,
            model,
            effort,
            permission_mode,
            fast,
            executor,
            endpoint_id,
            None,
            None,
        )
        .await
    }

    /// `create_session`, for a chat that starts out somewhere: in a team (it
    /// is born with its parent, so no copy of it ever shows it on its own)
    /// and under a name its starter gave it.
    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn create_session_in(
        self: &Arc<Self>,
        harness: &str,
        workspace: Option<std::path::PathBuf>,
        model: Option<String>,
        effort: Option<String>,
        permission_mode: Option<String>,
        fast: bool,
        executor: Option<openremote_core::DeviceId>,
        endpoint_id: Option<String>,
        parent: Option<ParentLink>,
        title: Option<String>,
    ) -> Result<Session, SupervisorError> {
        // A custom model id is free text in the harness's own spelling -
        // the only bar is shape, never the catalog. An empty pick means
        // unpicked, never a model named "".
        let model = match model.map(|m| m.trim().to_string()) {
            Some(m) if m.is_empty() => None,
            Some(m) => {
                openremote_core::model::validate_model_id(&m).map_err(SupervisorError::Conflict)?;
                Some(m)
            }
            None => None,
        };
        let Some(backend) = self.backend(harness) else {
            return Err(SupervisorError::Harness(format!(
                "harness '{harness}' is not available"
            )));
        };
        // A picked model is the user's; unpicked, the harness's own config
        // says what a fresh chat runs (grok's config.toml, agy's settings,
        // claude's priority chain) - its words, replaced by the wire's
        // first-turn truth. A fixture-backed backend is not the harness:
        // the machine's real config never leaks into an injected one.
        // An effort default rides along the same way - the user's word
        // when given, the harness's own pairing otherwise.
        let (model, effort) = match (model, effort) {
            (Some(picked), user_effort) => (Some(picked), user_effort),
            (None, user_effort) if !self.is_fixture(harness) => {
                // The harness's own default pairing - a real backend's
                // default_model() names its model, a fixture's absence
                // leaves both unpicked.
                match backend.default_model() {
                    Some((dm, de)) => (Some(dm), user_effort.or(de)),
                    None => (None, user_effort),
                }
            }
            (None, user_effort) => (None, user_effort),
        };
        let binding = self.endpoint_binding(harness, endpoint_id.as_deref())?;
        let session = Session {
            id: SessionId::new(),
            harness: harness.to_string(),
            workspace,
            status: SessionStatus::Starting,
            model,
            endpoint_id: binding.as_ref().map(|b| b.endpoint_id.clone()),
            effort,
            permission_mode,
            harness_session_ref: None,
            created_at: now_ms(),
            updated_at: now_ms(),
            last_error: None,
            next_turn: 0,
            fast,
            approved_tools: Vec::new(),
            executor,
            title,
            queue: Vec::new(),
            queue_paused: false,
            mentioned_chats: Vec::new(),
            carry_transcript: None,
            parent,
        };
        let id = session.id;
        self.emit_all(&id, vec![EventPayload::SessionCreated { session }])?;
        let notes = self.rerun_plugin_commands(harness).await;
        if !notes.is_empty() {
            self.emit_all(
                &id,
                notes
                    .into_iter()
                    .map(|text| EventPayload::NoteAdded { text })
                    .collect(),
            )?;
        }

        let current = self.session(&id)?;
        let opts = openremote_harness::SpawnOptions {
            cwd: current.workspace.clone().unwrap_or_else(Self::home_dir),
            model: current.model.clone(),
            endpoint: binding,
            effort: current.effort.clone(),
            permission_mode: current.permission_mode.clone(),
            resume: None,
            include_deltas: true,
            fast: current.fast,
            mcp_servers: self.mcp_servers_for(&id, &current.harness),
            plugin_dirs: self.plugin_dirs(current.workspace.as_deref()),
            allowed_tools: self.allowed_tools(&current.harness),
            state_dir: Some(self.harness_state_dir(&current.harness)),
        };
        match backend.spawn(opts).await {
            Ok((driver, rx)) => {
                let mut sessions = self.sessions.lock().await;
                sessions.insert(
                    id,
                    SessionEntry {
                        driver: Arc::new(AsyncMutex::new(driver)),
                        order: Arc::new(AsyncMutex::new(())),
                        stopping: Arc::new(AtomicBool::new(false)),
                        done: None,
                    },
                );
                drop(sessions);
                self.spawn_pump(id, rx);
            }
            Err(e) => {
                let reason = e.to_string();
                // Lazy eviction: a spawn that failed
                // while the resolved files are gone means the harness was
                // uninstalled since the probe - stop offering it until a
                // re-probe instead of failing every create with a raw OS
                // error while the console keeps listing it.
                self.evict_harness_if_gone(harness);
                self.emit_all(
                    &id,
                    vec![EventPayload::SessionStatusChanged {
                        status: SessionStatus::Failed,
                        reason: Some(reason),
                    }],
                )?;
            }
        }
        self.session(&id)
    }

    /// Send a prompt: turn + user message events, then the envelope.
    pub async fn prompt(&self, id: &SessionId, text: &str) -> Result<(), SupervisorError> {
        self.prompt_as(id, text, None, &[]).await
    }

    /// A message with pictures (the composer's attachments): kept for the
    /// chat first, so its event can name them, then sent like any prompt.
    pub async fn prompt_with(
        &self,
        id: &SessionId,
        text: &str,
        images: Vec<NewImage>,
    ) -> Result<(), SupervisorError> {
        let kept = self.keep_images(id, &images)?;
        self.prompt_as(id, text, None, &kept).await
    }

    /// A message another agent sent (a lead's brief to its worker): the chat
    /// shows whose it is, and the harness reads it behind a line saying it
    /// did not come from the user.
    pub async fn prompt_from(
        &self,
        id: &SessionId,
        text: &str,
        origin: &Origin,
    ) -> Result<(), SupervisorError> {
        self.prompt_as(id, text, Some(origin), &[]).await
    }

    /// A prompt whose pictures are already kept.
    async fn prompt_as(
        &self,
        id: &SessionId,
        text: &str,
        origin: Option<&Origin>,
        images: &[ImageRef],
    ) -> Result<(), SupervisorError> {
        self.ensure_here(id)?;
        let entry = self.entry(id).await?;
        // Built before anything is written: a picture that can't be read
        // leaves no turn behind.
        let pictures = self.harness_images(&entry, images).await?;
        let order = entry.order.lock().await;
        self.open_prompt_locked(id, &entry, text, origin, images)
            .await?;
        drop(order);
        self.send_prompt_text(id, &entry, text, origin, pictures)
            .await
    }

    /// Refuse a message's pictures when the chat's harness reads none, in
    /// a sentence naming the app. The routes ask this before they write
    /// anything for the message (its receipt, the chats it mentions).
    pub fn refuse_pictures(
        &self,
        id: &SessionId,
        images: &[NewImage],
    ) -> Result<(), SupervisorError> {
        if images.is_empty() {
            return Ok(());
        }
        let harness = self.session(id)?.harness;
        let reads_pictures = self
            .registry
            .read()
            .expect("registry lock")
            .harnesses()
            .into_iter()
            .find(|h| h.id == harness)
            .is_some_and(|h| h.honors.attachments);
        if !reads_pictures {
            return Err(SupervisorError::Conflict(format!(
                "{} doesn't read pictures - send the message without them",
                self.harness_name(&harness)
            )));
        }
        Ok(())
    }

    /// Keep a message's pictures for its chat, refusing them first where
    /// the harness reads none - before anything is kept or written.
    fn keep_images(
        &self,
        id: &SessionId,
        images: &[NewImage],
    ) -> Result<Vec<ImageRef>, SupervisorError> {
        if images.is_empty() {
            return Ok(Vec::new());
        }
        self.ensure_here(id)?;
        self.refuse_pictures(id, images)?;
        crate::images::keep(&self.blobs, *id, images).map_err(|e| SupervisorError::Store(e.into()))
    }

    /// A message's kept pictures in the form its harness's wire takes.
    async fn harness_images(
        &self,
        entry: &SessionEntry,
        images: &[ImageRef],
    ) -> Result<Vec<openremote_harness::Image>, SupervisorError> {
        if images.is_empty() {
            return Ok(Vec::new());
        }
        let as_files = entry.driver.lock().await.reads_image_files();
        images
            .iter()
            .map(|image| crate::images::for_harness(&self.blobs, image, as_files))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| SupervisorError::Store(e.into()))
    }

    /// The chats' pictures.
    pub fn blobs(&self) -> Arc<openremote_core::blobs::Blobs> {
        Arc::clone(&self.blobs)
    }

    /// A prompt's turn and message events, written under the session's
    /// order lock the caller holds - so taking a message off the queue
    /// and opening its turn is one step no other prompt can split.
    async fn open_prompt_locked(
        &self,
        id: &SessionId,
        entry: &SessionEntry,
        text: &str,
        origin: Option<&Origin>,
        images: &[ImageRef],
    ) -> Result<(), SupervisorError> {
        // The Working gate: a driver that runs one
        // process per prompt cannot take a second while a turn is open -
        // `send_prompt` would spawn a second process, replace the first
        // Child un-killed, and interleave two turns' frames under the
        // newest turn number. The driver advertises the semantics itself
        // (`per_prompt_process`), so the check follows the flag, never a
        // harness id. The answer is Conflict, not an error about the chat:
        // the turn is in flight and the console lands the message where
        // the user reads it. Inside the order lock so two racing prompts
        // see the first one's Working before either bumps the turn; the
        // driver lock is released before the gate answers (send_prompt
        // below re-locks it - the mutex is not reentrant).
        // Stdin-carrying harnesses (claude/codex/pi/opencode) answer false
        // and keep their behavior - the harness queues or interleaves on
        // its own wire, as it always has.
        let (per_prompt_process, steers_mid_turn) = {
            let driver = entry.driver.lock().await;
            (driver.per_prompt_process(), driver.steers_mid_turn())
        };
        let turn_open = matches!(
            self.session(id)?.status,
            SessionStatus::Working | SessionStatus::Waiting
        );
        if per_prompt_process && turn_open {
            return Err(SupervisorError::Conflict(
                "a turn is still running on this chat - this harness takes \
                 one prompt at a time; wait for the turn to finish or stop \
                 the chat"
                    .into(),
            ));
        }
        // A message that steers a working turn belongs to that turn: the
        // harness folds it into the run in progress and answers once, so a
        // new turn here would never complete - and every frame after it
        // (the tool results of the turn still running) would be filed
        // under the wrong turn.
        let steering = steers_mid_turn && turn_open;
        let turn = if steering {
            self.current_turn(id)
        } else {
            self.with_store(|s| s.bump_turn(id))?
        };
        let message = EventPayload::MessageAdded {
            message: openremote_core::ChatMessage {
                id: Uuid::new_v4().to_string(),
                turn,
                role: openremote_core::MessageRole::User,
                text: text.to_string(),
                origin: origin.cloned(),
                images: images.to_vec(),
            },
        };
        let payloads = if steering {
            vec![message]
        } else {
            vec![
                EventPayload::SessionStatusChanged {
                    status: SessionStatus::Working,
                    reason: None,
                },
                EventPayload::TurnStarted { turn },
                message,
            ]
        };
        self.emit_all(id, payloads)?;
        Ok(())
    }

    /// A harness's name as the harness spells it, for words the user reads.
    fn harness_name(&self, id: &str) -> String {
        self.registry
            .read()
            .expect("registry lock")
            .harnesses()
            .into_iter()
            .find(|h| h.id == id)
            .map(|h| h.name)
            .unwrap_or_else(|| id.to_string())
    }

    /// The note added to a chat's delivered messages, from here on.
    pub fn set_prompt_notes(&self, notes: Arc<dyn PromptNotes>) {
        let _ = self.prompt_note.set(notes);
    }

    /// A chat's conversation starts over, or the chat is gone.
    pub(crate) fn forget_prompt_note(&self, id: &SessionId) {
        if let Some(notes) = self.prompt_note.get() {
            notes.forget(id);
        }
    }

    /// Whether the message that carried a note (`noted`) reached the harness.
    fn note_delivered(&self, id: &SessionId, noted: bool, delivered: bool) {
        if !noted {
            return;
        }
        if let Some(notes) = self.prompt_note.get() {
            notes.settled(id, delivered);
        }
    }

    /// A message as its harness receives it: the words, then the open-tabs
    /// note where this harness has OpenRemote's tools, and whether a note
    /// rode along. The note is never stored or shown, and a message that
    /// starts with a / command goes without it, so it never becomes that
    /// command's arguments; `/clear` starts the conversation over.
    fn with_prompt_note(
        &self,
        id: &SessionId,
        rides_mcp: bool,
        text: &str,
        origin: Option<&Origin>,
    ) -> (String, bool) {
        // Another agent's message is read behind a line that says so: an
        // agent must never take it for the user's word.
        let mut words = match origin {
            Some(origin) => format!("{}\n{text}", origin_line(origin)),
            None => text.to_string(),
        };
        let mut noted = false;
        let command = text.trim_start();
        if !rides_mcp || self.controls.get().is_none() {
            return (words, noted);
        }
        if command.starts_with('/') {
            if command.split_whitespace().next() == Some("/clear") {
                self.forget_prompt_note(id);
            }
        } else if let Some(note) = self.prompt_note.get().and_then(|notes| notes.note(id)) {
            if !words.is_empty() {
                words.push_str("\n\n");
            }
            words.push_str(&note);
            noted = true;
        }
        (words, noted)
    }

    /// Write a prompt whose turn is already open to the harness. A failed
    /// write settles that turn (and the session, when the process died).
    async fn send_prompt_text(
        &self,
        id: &SessionId,
        entry: &SessionEntry,
        text: &str,
        origin: Option<&Origin>,
        images: Vec<openremote_harness::Image>,
    ) -> Result<(), SupervisorError> {
        let mut driver = entry.driver.lock().await;
        // The first plain message after a move the harness's conversation
        // couldn't follow carries the chat so far (carry.rs); the
        // transcript keeps the message as the user wrote it.
        let carried = self.carried_prompt(id, text, driver.per_prompt_process());
        let said = carried.as_deref().unwrap_or(text);
        let (text, noted) = self.with_prompt_note(id, driver.rides_mcp(), said, origin);
        let prompt = openremote_harness::Prompt { text, images };
        let sent = driver.send_prompt(&prompt).await;
        drop(driver);
        self.note_delivered(id, noted, sent.is_ok());
        if carried.is_some() && sent.is_ok() {
            self.settle_carry(id);
        }
        if let Err(e) = sent {
            // The turn's existence is already durable: a write that
            // failed must not leave it open forever. The boundary rides
            // through open_turn_boundary (a no-op when the turn is
            // already closed) and the status only lands while the
            // session is still alive, because the process death behind
            // the failed write also fires the pump's StdoutClosed -
            // whichever of the two settles first is the record, and
            // the loser appends nothing on top of it.
            let _order = entry.order.lock().await;
            let mut payloads = self.open_turn_boundary(id);
            if self
                .session(id)
                .map(|s| s.status.is_alive())
                .unwrap_or(false)
            {
                payloads.push(EventPayload::SessionStatusChanged {
                    status: SessionStatus::Failed,
                    reason: Some(e.to_string()),
                });
            }
            self.emit_all(id, payloads)?;
            // A harness whose spawn failed while its files are gone
            // also ends its slot (lazy eviction's print-mode path).
            let harness = self.session(id).map(|s| s.harness).ok();
            if let Some(harness) = harness {
                self.evict_harness_if_gone(&harness);
            }
            return Err(e.into());
        }
        Ok(())
    }

    /// Change model, effort, or fast on a live chat, in the harness's own
    /// words. Codex, Pi, and OpenCode v2 apply a model on the running
    /// thread. A harness that only accepts the change at process start
    /// keeps the same conversation: the word is stored, and an idle
    /// process is restarted onto it. A turn that is still working has to
    /// finish first.
    pub async fn update_settings(
        self: &Arc<Self>,
        id: &SessionId,
        settings: openremote_harness::SessionSettings,
    ) -> Result<Session, SupervisorError> {
        if settings.model.is_none()
            && settings.effort.is_none()
            && settings.fast.is_none()
            && settings.permission_mode.is_none()
        {
            return self.session(id);
        }
        // Permission is start-time on every harness - no driver can take
        // the word on a live process. Refuse before the driver call so the
        // stopped chat's change lands as a store write only (resume passes
        // it), exactly like the model on a start-time harness.
        if settings.permission_mode.is_some() {
            let entry_alive = self.entry(id).await.is_ok();
            let status_alive = self
                .session(id)
                .map(|s| s.status.is_alive())
                .unwrap_or(false);
            if entry_alive || status_alive {
                return Err(SupervisorError::Conflict(
                    "permission is set when the chat starts - stop the chat to change it".into(),
                ));
            }
        }
        self.ensure_here(id)?;
        let session = self.session(id)?;
        // A live driver applies the change to the process it already runs,
        // which it can only do between turns. Decided from the probed row
        // and the session's status before any driver write.
        if let Some(message) =
            refuse_live_mid_turn(self.settings_are_live(&session.harness), session.status)
        {
            return Err(SupervisorError::Conflict(message.into()));
        }
        // A start-time harness has no live wire. The stored word still
        // changes; an idle process is restarted onto it below so the
        // user does not have to stop and resume by hand.
        let restart = if let Ok(entry) = self.entry(id).await {
            // Strip the start-time field before the driver sees it: the
            // live wire carries only what a harness can apply now.
            let mut live = settings.clone();
            live.permission_mode = None;
            if self.settings_are_live(&session.harness) {
                return self
                    .apply_live_settings(id, &session, &entry, &settings, &live)
                    .await;
            }
            match entry.driver.lock().await.apply_settings(&live).await {
                Ok(()) => false,
                Err(openremote_harness::DriverError::Harness(message))
                    if message.contains("when the chat starts") =>
                {
                    if matches!(
                        session.status,
                        openremote_core::SessionStatus::Starting
                            | openremote_core::SessionStatus::Working
                            | openremote_core::SessionStatus::Waiting
                    ) {
                        return Err(SupervisorError::Conflict(
                            "wait for this turn to finish, then change the model".into(),
                        ));
                    }
                    true
                }
                Err(error) => return Err(error.into()),
            }
        } else if session.status.is_alive() {
            return Err(SupervisorError::Conflict(
                "the chat is starting - try again in a moment".into(),
            ));
        } else {
            false
        };
        let updated = self.store_settings(id, &settings)?;
        if restart {
            // Same conversation, new process. Resume passes the model
            // just stored. A stop that already settled is still resumable.
            self.stop(id).await?;
            return self.resume(id).await;
        }
        Ok(updated)
    }

    /// Validate and store a settings change, then tell the console.
    fn store_settings(
        &self,
        id: &SessionId,
        settings: &openremote_harness::SessionSettings,
    ) -> Result<Session, SupervisorError> {
        let updated = self
            .with_store(|s| {
                // A custom model id is free text in the harness's own
                // spelling - shape-checked here so a stopped chat's stored
                // change is already valid when resume passes it.
                if let Some(model) = settings.model.as_deref() {
                    let trimmed = model.trim();
                    if !trimmed.is_empty() {
                        openremote_core::model::validate_model_id(trimmed)
                            .map_err(openremote_core::store::StoreError::Conflict)?;
                    }
                }
                s.update_session_settings(
                    id,
                    settings.model.clone(),
                    settings.effort.clone(),
                    settings.fast,
                    settings.permission_mode.clone(),
                )
            })
            .map_err(store_error)?;
        self.emit_all(
            id,
            vec![EventPayload::SessionUpdated {
                session: updated.clone(),
            }],
        )?;
        Ok(updated)
    }

    /// A live harness's change, in the one order a refusal cannot undercut:
    /// the change is validated and stored first, then the values before it
    /// are held, and only then is the driver written to. The live wire has
    /// no acknowledgement; a `settings_refused` (handled on the pump) puts
    /// the held values back, and it can only arrive after the write, by
    /// which time the store already holds the change it undoes. A write that
    /// fails reached no driver, so it is undone here and nothing is held.
    async fn apply_live_settings(
        &self,
        id: &SessionId,
        session: &Session,
        entry: &SessionEntry,
        settings: &openremote_harness::SessionSettings,
        live: &openremote_harness::SessionSettings,
    ) -> Result<Session, SupervisorError> {
        let prior = PriorSettings {
            model: session.model.clone(),
            effort: session.effort.clone(),
            fast: session.fast,
        };
        // A change that fails validation or the store write holds nothing.
        let updated = self.store_settings(id, settings)?;
        lock_recovering(prior_live_settings()).insert(*id, prior.clone());
        let written = entry.driver.lock().await.apply_settings(live).await;
        if let Err(error) = written {
            forget_live_settings(id);
            if let Ok(restored) = self.with_store(|s| {
                s.restore_session_settings(id, prior.model, prior.effort, prior.fast)
            }) {
                let _ = self.emit_all(id, vec![EventPayload::SessionUpdated { session: restored }]);
            }
            return Err(error.into());
        }
        Ok(updated)
    }

    /// Point a stopped chat at another custom endpoint. A live chat keeps
    /// the binding it started with - the switch lands on resume.
    pub async fn set_session_endpoint(
        &self,
        id: &SessionId,
        endpoint_id: Option<&str>,
    ) -> Result<Session, SupervisorError> {
        let cleaned = endpoint_id.map(str::trim).filter(|s| !s.is_empty());
        if self.entry(id).await.is_ok()
            || self
                .session(id)
                .map(|s| s.status.is_alive())
                .unwrap_or(false)
        {
            return Err(SupervisorError::Conflict(
                "stop the chat to change its endpoint - a live chat keeps the binding it started with"
                    .into(),
            ));
        }
        self.ensure_here(id)?;
        let updated = self
            .with_store(|s| s.set_session_endpoint(id, cleaned))
            .map_err(store_error)?;
        self.emit_all(
            id,
            vec![EventPayload::SessionUpdated {
                session: updated.clone(),
            }],
        )?;
        Ok(updated)
    }

    /// Interrupt the running turn (the harness cancels; its boundary event
    /// follows).
    pub async fn interrupt(&self, id: &SessionId) -> Result<(), SupervisorError> {
        self.ensure_here(id)?;
        let entry = self.entry(id).await?;
        // An interrupt usually means "wait, not that": the queue holds
        // until the user resumes it, instead of firing the next message.
        {
            let _order = entry.order.lock().await;
            self.hold_queue(id)?;
        }
        entry.driver.lock().await.interrupt().await?;
        Ok(())
    }

    /// Stop the session: kill the driver, then wait for the pump to drain
    /// what the harness already said - the transcript's tail must land
    /// before the entry goes. Returns the session as it settled (stopped,
    /// or failed if the process died first).
    pub async fn stop_only(&self, id: &SessionId) -> Result<Session, SupervisorError> {
        self.ensure_here(id)?;
        let entry = self.entry(id).await?;
        {
            let _order = entry.order.lock().await;
            self.hold_queue(id)?;
        }
        entry.stopping.store(true, AtomicOrdering::SeqCst);
        let (tx, rx) = oneshot::channel();
        {
            let mut sessions = self.sessions.lock().await;
            if let Some(e) = sessions.get_mut(id) {
                e.done = Some(tx);
            }
        }
        entry.driver.lock().await.shutdown().await?;
        // The pump answering at any point below is the true quiescence:
        // it drained to StdoutClosed and settled the session itself
        // (stopped, or failed - a process that died before the kill).
        // The fallback after the drain wait is for the driver with no
        // live process at all (print-mode harnesses between prompts) or a
        // drain wedged past all bounds - settle what remains and remove
        // the entry unconditionally (a stale entry keeps a dead session
        // answerable).
        let drained = self.drain_pump_quiescence(id, rx).await;
        if let Some(session) = drained {
            return Ok(session);
        }
        let settled_here = self.settle_stopped(id, &entry).await;
        if !settled_here {
            // The pump settled the entry itself while the fallback
            // waited on the order lock - its events are the record.
            return self.session(id);
        }
        self.sessions.lock().await.remove(id);
        lock_recovering(&self.stderr_ring).remove(id);
        forget_live_settings(id);
        self.session(id)
    }

    /// Watch the pump finish its backlog: the transcript's seq is the
    /// drain's own progress signal, so wait until it stops moving (the
    /// pump is quiet) or the generous bound passes - never a flat clock
    /// that either races a slow drain or stalls a quiet one. Returns the
    /// pump's settled session when it answered during the wait.
    async fn drain_pump_quiescence(
        &self,
        id: &SessionId,
        mut rx: oneshot::Receiver<Session>,
    ) -> Option<Session> {
        let bound = tokio::time::Instant::now() + STOP_DRAIN_BOUND;
        let mut last_seq = self.with_store(|s| s.next_seq(id));
        let mut quiet = tokio::time::Instant::now() + STOP_DRAIN_QUIET;
        loop {
            // The pump answering at any point is the true quiescence: it
            // applied StdoutClosed (or found the entry gone). Either way
            // the store holds the settled truth and there is nothing left
            // to wait for.
            match rx.try_recv() {
                Ok(session) => return Some(session),
                Err(tokio::sync::oneshot::error::TryRecvError::Closed) => {
                    // The pump dropped its end without settling (it was
                    // removed some other way); the store is the truth.
                    return self.session(id).ok();
                }
                Err(tokio::sync::oneshot::error::TryRecvError::Empty) => {}
            }
            if tokio::time::Instant::now() >= bound {
                return None;
            }
            tokio::time::sleep(STOP_DRAIN_SLICE).await;
            let seq = self.with_store(|s| s.next_seq(id));
            if seq != last_seq {
                last_seq = seq;
                quiet = tokio::time::Instant::now() + STOP_DRAIN_QUIET;
            } else if tokio::time::Instant::now() >= quiet {
                return None;
            }
        }
    }

    /// The stop fallback's settle: append a boundary
    /// for the turn the transcript left open, settle the status, retire
    /// the decisions - the same events the pump's own StdoutClosed path
    /// writes, the single-writer rule bent only because the pump just
    /// proved it will not write. Returns whether the fallback settled
    /// (false = the pump settled the entry itself after all, and its
    /// events are the record).
    async fn settle_stopped(&self, id: &SessionId, entry: &SessionEntry) -> bool {
        // The order lock bounds the wait: a wedged pump could hold it.
        // On timeout the entry is still removed by the caller - a
        // stalled drain must not keep a dead session answerable - but
        // without events from us; the pump may still be mid-backlog and
        // its own StdoutClosed settle wins when it lands.
        let Ok(_order) = tokio::time::timeout(STOP_SETTLE_LOCK, entry.order.lock()).await else {
            return true;
        };
        if self.sessions.lock().await.get(id).is_none() {
            // The pump settled after all (it raced the drain's quiet
            // call) - its events are the record, ours would duplicate.
            return false;
        }
        let mut payloads: Vec<EventPayload> = Vec::new();
        if let Ok(session) = self.session(id) {
            if session.status.is_alive() {
                payloads.push(EventPayload::SessionStatusChanged {
                    status: SessionStatus::Stopped,
                    reason: None,
                });
            }
        }
        payloads.extend(self.open_turn_boundary(id));
        let _ = self.emit_all(id, payloads);
        let _ = self.with_store(|s| s.retire_pending(id));
        true
    }

    /// A synthetic boundary for the turn the transcript left open, if
    /// any. The current turn is `next_turn - 1` (bump_turn hands out the
    /// pre-increment number, so turn 0 is real); whether it is open is
    /// decided by the last turn-scoped event in the log - a completed
    /// boundary for it closes it, its own start leaves it open, and
    /// everything after the last turn event (messages, notes, status
    /// changes, daemon errors) belongs to that turn either way. Reading
    /// only the last event's kind would misjudge both directions: a
    /// closed turn whose error note landed last would close twice, and
    /// an answered decision's working status after the start would read
    /// as closed while the turn runs on.
    fn open_turn_boundary(&self, id: &SessionId) -> Vec<EventPayload> {
        let boundary = self.with_store(|s| {
            let Ok(session) = s.session(id) else {
                return None;
            };
            let current = session.next_turn.saturating_sub(1);
            if session.next_turn == 0 {
                return None; // no turn was ever started
            }
            let events = s.events_after(id, None).ok()?;
            // Scan back over everything to the last word on the current
            // turn: messages, notes, status changes, and daemon errors
            // all belong to a turn and never decide its state. The first
            // turn-scoped event found (walking back) is that word.
            let decided = events.iter().rev().find(|event| {
                matches!(
                    &event.payload,
                    EventPayload::TurnStarted { turn, .. }
                    | EventPayload::TurnCompleted { turn, .. }
                        if *turn == current
                )
            });
            let open = match decided {
                // Its own start, with no boundary after, leaves it open.
                Some(event) => matches!(&event.payload, EventPayload::TurnStarted { .. }),
                // No word on the current turn at all (a heal, a torn
                // write): no boundary - never a guess from silence.
                None => return None,
            };
            open.then(|| EventPayload::TurnCompleted {
                turn: current,
                outcome: "failed".to_string(),
                coarse: TurnOutcome::Failed,
            })
        });
        boundary.into_iter().collect()
    }

    /// Lazy eviction's gate: only a spawn whose io
    /// kind is NotFound means the resolved files are gone - every other
    /// failure (a locked file, a permissions blip, a protocol timeout)
    /// keeps the harness offered. The look is cheap (metadata, never a
    /// run) and the backend is only dropped when the look agrees.
    fn evict_harness_if_gone(&self, harness: &str) {
        let resolution = {
            let registry = self.registry.read().expect("registry lock");
            let Some(backend) = registry.backend(harness) else {
                return;
            };
            match backend_resolution(backend) {
                Some(resolution) => resolution.clone(),
                // A plugin harness's driver lives inside its plugin folder;
                // removing or updating the plugin already re-probes, so a
                // failed spawn here is not a file that went missing under a
                // registry that still points at it.
                None => return,
            }
        };
        let gone = match &resolution {
            // An unavailable slot is already evicted; nothing to do.
            Resolution::Unavailable => return,
            Resolution::Executable(path) => !path.is_file(),
            Resolution::NodeScript { node, script } => !node.is_file() || !script.is_file(),
        };
        if gone {
            self.registry
                .write()
                .expect("registry lock")
                .mark_unavailable(harness);
            eprintln!(
                "harness '{harness}' failed to spawn and its files are gone - marked unavailable until a re-probe"
            );
        }
    }

    /// The daemon is exiting: sweep every live session
    /// through the same bounded drain stop uses, so harness children do
    /// not outlive the daemon (opencode serve children have null stdin
    /// and never see EOF). A few seconds per session, the whole sweep
    /// bounded too - the exit must not hang on a wedged drain, and the
    /// store's own reconcile heals whatever the bound cuts off.
    pub async fn shutdown_all(self: &Arc<Self>) {
        let ids: Vec<SessionId> = self.sessions.lock().await.keys().copied().collect();
        for id in ids {
            let entry = {
                let sessions = self.sessions.lock().await;
                sessions.get(&id).map(|e| Arc::clone(&e.driver))
            };
            let Some(driver) = entry else { continue };
            let _ = driver.lock().await.shutdown().await;
            // Drain with the same quiescence evidence stop uses: the
            // transcript's tail lands before the process goes, and a
            // quiet session (a print-mode harness between prompts, no
            // process, nothing draining) is recognized by its seq
            // stopping - not by burning the whole bound.
            let bound = tokio::time::Instant::now() + STOP_DRAIN_BOUND;
            let mut last_seq = self.with_store(|s| s.next_seq(&id));
            let mut quiet = tokio::time::Instant::now() + STOP_DRAIN_QUIET;
            loop {
                let settled = self
                    .session(&id)
                    .map(|s| !s.status.is_alive())
                    .unwrap_or(true);
                if settled || tokio::time::Instant::now() >= bound {
                    break;
                }
                tokio::time::sleep(STOP_DRAIN_SLICE).await;
                let seq = self.with_store(|s| s.next_seq(&id));
                if seq != last_seq {
                    last_seq = seq;
                    quiet = tokio::time::Instant::now() + STOP_DRAIN_QUIET;
                } else if tokio::time::Instant::now() >= quiet {
                    break;
                }
            }
        }
    }

    /// Resume a stopped/failed session on its harness's own conversation
    /// (claude `--resume=<id>`, codex `thread/resume`, pi `--session`, …).
    pub async fn resume(self: &Arc<Self>, id: &SessionId) -> Result<Session, SupervisorError> {
        self.ensure_here(id)?;
        refuse_moving(id)?;
        self.resume_here(id).await
    }

    /// The resume itself, for the move that holds the chat's move slot and
    /// restarts it in the new folder.
    async fn resume_here(self: &Arc<Self>, id: &SessionId) -> Result<Session, SupervisorError> {
        let session = self.session(id)?;
        if session.status.is_alive() {
            return Err(SupervisorError::Conflict(format!(
                "session {id} is already running"
            )));
        }
        // A chat that moved where its harness's conversation couldn't follow
        // starts a fresh one there; its next message carries the chat so far
        // (carry.rs).
        let harness_ref = match session.carry_transcript {
            Some(_) => {
                // A fresh conversation has been told nothing.
                self.forget_prompt_note(id);
                None
            }
            None => Some(session.harness_session_ref.clone().ok_or_else(|| {
                SupervisorError::Conflict(
                    "the harness never reported a conversation to resume".to_string(),
                )
            })?),
        };
        let Some(backend) = self.backend(&session.harness) else {
            return Err(SupervisorError::Harness(format!(
                "harness '{}' is not available",
                session.harness
            )));
        };
        let notes = self.rerun_plugin_commands(&session.harness).await;
        let binding = self.endpoint_binding(&session.harness, session.endpoint_id.as_deref())?;
        let opts = openremote_harness::SpawnOptions {
            cwd: session.workspace.clone().unwrap_or_else(Self::home_dir),
            model: session.model.clone(),
            endpoint: binding,
            effort: session.effort.clone(),
            permission_mode: session.permission_mode.clone(),
            resume: harness_ref,
            include_deltas: true,
            fast: session.fast,
            mcp_servers: self.mcp_servers_for(id, &session.harness),
            plugin_dirs: self.plugin_dirs(session.workspace.as_deref()),
            allowed_tools: self.allowed_tools(&session.harness),
            state_dir: Some(self.harness_state_dir(&session.harness)),
        };
        let (driver, rx) = match backend.spawn(opts).await {
            Ok(pair) => pair,
            Err(e) => {
                // Lazy eviction: a resume that fails
                // while the resolved files are gone is the same uninstall
                // a create would see.
                self.evict_harness_if_gone(&session.harness);
                return Err(e.into());
            }
        };
        {
            let mut sessions = self.sessions.lock().await;
            sessions.insert(
                *id,
                SessionEntry {
                    driver: Arc::new(AsyncMutex::new(driver)),
                    order: Arc::new(AsyncMutex::new(())),
                    stopping: Arc::new(AtomicBool::new(false)),
                    done: None,
                },
            );
        }
        let mut events = vec![EventPayload::SessionStatusChanged {
            status: SessionStatus::Starting,
            reason: None,
        }];
        events.extend(
            notes
                .into_iter()
                .map(|text| EventPayload::NoteAdded { text }),
        );
        self.emit_all(id, events)?;
        self.spawn_pump(*id, rx);
        self.session(id)
    }

    /// Move a chat into `project`, one of this machine's projects, or out of
    /// every project with `None` - it then works in the home folder.
    ///
    /// A chat with a turn in flight is refused: the turn would be cut off
    /// mid-work. A chat whose harness is up but idle stops and starts again in
    /// the new folder, ready for its next message; a stopped one starts there
    /// when it is resumed. The chat's own transcript is the daemon's and does
    /// not move with it; the harness's conversation is the harness's to find
    /// again, and a move never rewrites its files.
    pub async fn move_chat(
        self: &Arc<Self>,
        id: &SessionId,
        project: Option<&ProjectId>,
    ) -> Result<Session, SupervisorError> {
        self.ensure_here(id)?;
        let target = match project {
            None => None,
            Some(project) => {
                let folders = self
                    .with_store(|s| s.project(project).map(|p| p.folders.clone()))
                    .map_err(|_| {
                        SupervisorError::NotFound(
                            "that project isn't registered on this machine".into(),
                        )
                    })?;
                let session = self.session(id)?;
                if session
                    .workspace
                    .as_ref()
                    .is_some_and(|folder| folders.contains(folder))
                {
                    return Ok(session);
                }
                let folder = folders.first().cloned().ok_or_else(|| {
                    SupervisorError::Conflict("that project has no folder".into())
                })?;
                if !folder.is_dir() {
                    return Err(SupervisorError::Conflict(format!(
                        "{} isn't a folder on this machine any more",
                        folder.display()
                    )));
                }
                Some(folder)
            }
        };
        if target.is_none() && self.session(id)?.workspace.is_none() {
            return self.session(id);
        }
        // A turn that opens while the project is looked up still wins: the
        // rule runs again under the move slot, just before the stop.
        refuse_busy(&self.session(id)?, self.entry(id).await.is_ok())?;
        let _moving = {
            let mut moving = lock_recovering(moving_chats());
            if !moving.insert(*id) {
                return Err(SupervisorError::Conflict(
                    "this chat is already moving".into(),
                ));
            }
            Moving(*id)
        };
        let live = self.entry(id).await.is_ok();
        refuse_busy(&self.session(id)?, live)?;
        if live {
            match self.stop(id).await {
                // Stopped by someone else meanwhile: just as quiet.
                Ok(_) | Err(SupervisorError::NotFound(_)) => {}
                Err(err) => return Err(err),
            }
        }
        let before = self.session(id)?;
        // The harness's own conversation follows where it can (carry.rs);
        // where it can't, the chat's next message carries the chat so far.
        let follows = self.follows(&before, target.as_deref()).await;
        let mut note = move_note(&before, target.as_deref());
        if !follows {
            if let Some(left) = self.left_behind_note(&before) {
                note.push_str(&left);
            }
        }
        let session = self
            .with_store(|s| {
                let carry_from = s.next_seq(id);
                s.set_session_workspace(id, target.clone())?;
                if follows {
                    s.set_carry_transcript(id, None, false)
                } else {
                    // The harness granted its tools to the conversation that
                    // stays behind.
                    s.set_carry_transcript(id, Some(carry_from), true)
                }
            })
            .map_err(store_error)?;
        self.emit_all(
            id,
            vec![
                EventPayload::NoteAdded { text: note },
                EventPayload::SessionUpdated {
                    session: session.clone(),
                },
            ],
        )?;
        if live {
            if let Err(err) = self.resume_here(id).await {
                // The move stands: the chat says why it could not start in the
                // new folder, and Resume tries again.
                let _ = self.emit_all(
                    id,
                    vec![EventPayload::SessionStatusChanged {
                        status: SessionStatus::Failed,
                        reason: Some(format!("couldn't start in the new folder: {err}")),
                    }],
                );
            }
        }
        self.session(id)
    }

    /// Answer a pending decision: route it to the harness with its own
    /// correlation id and choice word, record it, and let the turn continue
    /// (or settle, when the choice interrupts it).
    pub async fn answer_decision(
        &self,
        decision_id: &DecisionId,
        choice: &str,
    ) -> Result<Decision, SupervisorError> {
        let decision = self.decision(decision_id)?;
        self.ensure_here(&decision.session_id)?;
        if decision.state != DecisionState::Pending {
            return Err(SupervisorError::Conflict(
                "the decision is no longer pending".into(),
            ));
        }
        let harness_ref = decision.harness_ref.clone().ok_or_else(|| {
            SupervisorError::Conflict("the decision has no harness reference".into())
        })?;
        let session_id = decision.session_id;
        let entry = self.entry(&session_id).await?;

        // The order lock is held ACROSS the answer write: the harness's
        // consequence events (tool results, turn completion) arrive on the
        // pump while we emit, and without this the pump can grab the lock
        // first - recording the consequence before the answer itself. The
        // write never needs the pump, so holding across it is deadlock-free.
        let _order = entry.order.lock().await;
        let outcome = entry
            .driver
            .lock()
            .await
            .answer(
                &harness_ref,
                choice,
                &decision.harness_request,
                decision.tool_name.as_deref(),
            )
            .await?;
        {
            let mut payloads = vec![EventPayload::DecisionResponded {
                decision_id: *decision_id,
                choice: choice.to_string(),
            }];
            // The harness's own session-scope word granted the tool for the
            // rest of the chat - record it and let the fresh session ride
            // out so the console renders "«tool» allowed for this chat".
            if let Some(tool) = &outcome.session_grant {
                self.with_store(|s| s.note_approved_tool(&session_id, tool))?;
                if let Ok(session) = self.session(&session_id) {
                    payloads.push(EventPayload::SessionUpdated { session });
                }
            }
            // Choices that continue the turn move the session back to
            // working; interrupting choices let the boundary event settle it.
            if !outcome.interrupts_turn {
                payloads.push(EventPayload::SessionStatusChanged {
                    status: SessionStatus::Working,
                    reason: None,
                });
            }
            self.emit_all(&session_id, payloads)?;
        }
        self.decision(decision_id)
    }

    async fn entry(&self, id: &SessionId) -> Result<SessionEntry, SupervisorError> {
        let sessions = self.sessions.lock().await;
        let entry = sessions
            .get(id)
            .ok_or_else(|| SupervisorError::NotFound(format!("session {id} is not running")))?;
        Ok(SessionEntry {
            driver: Arc::clone(&entry.driver),
            order: Arc::clone(&entry.order),
            stopping: Arc::clone(&entry.stopping),
            done: None,
        })
    }

    // ---- the queue: messages that wait for the running turn to end ----

    /// The event that holds a waiting queue, when there is one to hold.
    fn hold_payload(&self, id: &SessionId) -> Option<EventPayload> {
        let session = self.session(id).ok()?;
        (!session.queue.is_empty() && !session.queue_paused).then_some(EventPayload::QueueChanged {
            queue: session.queue,
            paused: true,
        })
    }

    fn hold_queue(&self, id: &SessionId) -> Result<(), SupervisorError> {
        match self.hold_payload(id) {
            Some(payload) => self.emit_all(id, vec![payload]).map(|_| ()),
            None => Ok(()),
        }
    }

    /// Write the queue as it now stands. An empty queue is never held.
    fn emit_queue(
        &self,
        id: &SessionId,
        queue: Vec<QueuedMessage>,
        paused: bool,
    ) -> Result<(), SupervisorError> {
        let paused = paused && !queue.is_empty();
        self.emit_all(id, vec![EventPayload::QueueChanged { queue, paused }])
            .map(|_| ())
    }

    /// The session's order lock while a driver is live. A stopped chat
    /// has no pump to race, so its queue changes need none.
    async fn order_of(&self, id: &SessionId) -> Option<Arc<AsyncMutex<()>>> {
        self.sessions
            .lock()
            .await
            .get(id)
            .map(|e| Arc::clone(&e.order))
    }

    /// Queue a message behind the running turn. A chat with no turn
    /// running takes it at once.
    pub async fn queue_add(&self, id: &SessionId, text: &str) -> Result<Session, SupervisorError> {
        self.queue_add_with(id, text, Vec::new()).await
    }

    /// Queue a message with pictures: they are kept at once, and the queue
    /// holds where they are.
    pub async fn queue_add_with(
        &self,
        id: &SessionId,
        text: &str,
        images: Vec<NewImage>,
    ) -> Result<Session, SupervisorError> {
        self.ensure_here(id)?;
        self.queue_add_as(id, text, None, images).await
    }

    /// Queue a message another agent sent: it waits behind the running
    /// turn like any other, and the chat shows whose it is. A worker's
    /// report replaces its own earlier one that is still waiting - the
    /// lead wants where the worker stands now, once.
    pub async fn queue_add_from(
        &self,
        id: &SessionId,
        text: &str,
        origin: Origin,
    ) -> Result<Session, SupervisorError> {
        self.ensure_here(id)?;
        self.queue_add_as(id, text, Some(origin), Vec::new()).await
    }

    async fn queue_add_as(
        &self,
        id: &SessionId,
        text: &str,
        origin: Option<Origin>,
        images: Vec<NewImage>,
    ) -> Result<Session, SupervisorError> {
        let kept = self.keep_images(id, &images)?;
        {
            let order = self.order_of(id).await;
            let _guard = match &order {
                Some(o) => Some(o.lock().await),
                None => None,
            };
            let session = self.session(id)?;
            let mut queue = session.queue;
            if let Some(report) = origin.as_ref().filter(|o| o.report) {
                queue.retain(|m| {
                    !m.origin
                        .as_ref()
                        .is_some_and(|o| o.report && o.chat == report.chat)
                });
            }
            queue.push(QueuedMessage {
                id: Uuid::new_v4().to_string(),
                text: text.to_string(),
                at: now_ms(),
                origin,
                images: kept,
            });
            self.emit_queue(id, queue, session.queue_paused)?;
        }
        self.drain_queue(id).await?;
        self.session(id)
    }

    /// Take one message out of the queue. Already gone is fine: two
    /// devices removing the same message agree.
    pub async fn queue_remove(
        &self,
        id: &SessionId,
        item: &str,
    ) -> Result<Session, SupervisorError> {
        self.ensure_here(id)?;
        {
            let order = self.order_of(id).await;
            let _guard = match &order {
                Some(o) => Some(o.lock().await),
                None => None,
            };
            let session = self.session(id)?;
            if session.queue.iter().any(|m| m.id == item) {
                let queue = session.queue.into_iter().filter(|m| m.id != item).collect();
                self.emit_queue(id, queue, session.queue_paused)?;
            }
        }
        self.session(id)
    }

    /// Steer: put a message into the running turn now. Claude and codex
    /// fold it into the turn; pi and opencode take it on their own wire.
    /// A harness that runs one process per prompt cannot take input
    /// mid-turn, so the turn is interrupted and the message runs next.
    /// With nothing running it simply sends - starting a stopped chat
    /// first.
    pub async fn steer(
        self: &Arc<Self>,
        id: &SessionId,
        text: &str,
    ) -> Result<(), SupervisorError> {
        self.steer_as(id, text, None, &[]).await
    }

    /// Steer a message with pictures, kept first.
    pub async fn steer_with(
        self: &Arc<Self>,
        id: &SessionId,
        text: &str,
        images: Vec<NewImage>,
    ) -> Result<(), SupervisorError> {
        let kept = self.keep_images(id, &images)?;
        self.steer_as(id, text, None, &kept).await
    }

    /// Steer a message another agent sent into the running turn.
    pub async fn steer_from(
        self: &Arc<Self>,
        id: &SessionId,
        text: &str,
        origin: &Origin,
    ) -> Result<(), SupervisorError> {
        self.steer_as(id, text, Some(origin), &[]).await
    }

    /// Steer a message whose pictures are already kept.
    async fn steer_as(
        self: &Arc<Self>,
        id: &SessionId,
        text: &str,
        origin: Option<&Origin>,
        images: &[ImageRef],
    ) -> Result<(), SupervisorError> {
        self.ensure_here(id)?;
        refuse_moving(id)?;
        if !self.session(id)?.status.is_alive() {
            self.resume(id).await?;
        }
        let entry = self.entry(id).await?;
        let per_prompt = entry.driver.lock().await.per_prompt_process();
        let turn_open = matches!(
            self.session(id)?.status,
            SessionStatus::Working | SessionStatus::Waiting
        );
        if per_prompt && turn_open {
            lock_recovering(&self.steer_next)
                .insert(*id, (text.to_string(), origin.cloned(), images.to_vec()));
            entry.driver.lock().await.interrupt().await?;
            return Ok(());
        }
        self.prompt_as(id, text, origin, images).await
    }

    /// Steer one queued message: it leaves the queue and goes in now. If
    /// it could not go in, it goes back to the front of the queue.
    pub async fn queue_steer(
        self: &Arc<Self>,
        id: &SessionId,
        item: &str,
    ) -> Result<(), SupervisorError> {
        self.ensure_here(id)?;
        let message = {
            let order = self.order_of(id).await;
            let _guard = match &order {
                Some(o) => Some(o.lock().await),
                None => None,
            };
            let session = self.session(id)?;
            let Some(message) = session.queue.iter().find(|m| m.id == item).cloned() else {
                return Err(SupervisorError::NotFound(format!("queued message {item}")));
            };
            let queue = session.queue.into_iter().filter(|m| m.id != item).collect();
            self.emit_queue(id, queue, session.queue_paused)?;
            message
        };
        if let Err(err) = self
            .steer_as(id, &message.text, message.origin.as_ref(), &message.images)
            .await
        {
            let order = self.order_of(id).await;
            let _guard = match &order {
                Some(o) => Some(o.lock().await),
                None => None,
            };
            if let Ok(session) = self.session(id) {
                let mut queue = session.queue;
                queue.insert(0, message);
                let _ = self.emit_queue(id, queue, session.queue_paused);
            }
            return Err(err);
        }
        Ok(())
    }

    /// Release a held queue and send its next message. A stopped or
    /// failed chat starts again first.
    pub async fn queue_resume(
        self: &Arc<Self>,
        id: &SessionId,
    ) -> Result<Session, SupervisorError> {
        self.ensure_here(id)?;
        refuse_moving(id)?;
        if !self.session(id)?.status.is_alive() {
            self.resume(id).await?;
        }
        {
            let order = self.order_of(id).await;
            let _guard = match &order {
                Some(o) => Some(o.lock().await),
                None => None,
            };
            let session = self.session(id)?;
            self.emit_queue(id, session.queue, false)?;
        }
        self.drain_queue(id).await?;
        self.session(id)
    }

    /// Send the next queued message when nothing is running and the queue
    /// isn't held. Taking it off the queue and opening its turn happen
    /// under one lock, so two turn ends never fire the same slot twice.
    pub async fn drain_queue(&self, id: &SessionId) -> Result<(), SupervisorError> {
        let Ok(entry) = self.entry(id).await else {
            return Ok(());
        };
        let order = entry.order.lock().await;
        let session = self.session(id)?;
        if session.queue_paused
            || session.queue.is_empty()
            || !matches!(
                session.status,
                SessionStatus::Idle | SessionStatus::Starting
            )
        {
            return Ok(());
        }
        let mut queue = session.queue;
        let next = queue.remove(0);
        // Built before the queue changes: a picture that can't be read
        // leaves the message where it was.
        let pictures = self.harness_images(&entry, &next.images).await?;
        self.emit_queue(id, queue, false)?;
        self.open_prompt_locked(id, &entry, &next.text, next.origin.as_ref(), &next.images)
            .await?;
        drop(order);
        self.send_prompt_text(id, &entry, &next.text, next.origin.as_ref(), pictures)
            .await
    }

    /// A turn ended: a steered message goes first, then the queue.
    async fn after_turn(&self, id: &SessionId) {
        let steered = lock_recovering(&self.steer_next).remove(id);
        let sent = match steered {
            Some((text, origin, images)) => {
                self.prompt_as(id, &text, origin.as_ref(), &images).await
            }
            None => self.drain_queue(id).await,
        };
        if let Err(err) = sent {
            eprintln!("session {id}: queue: {err}");
        }
    }

    // ---- the pump: driver events → store events → broadcast ----

    fn spawn_pump(self: &Arc<Self>, id: SessionId, mut rx: mpsc::Receiver<DriverEvent>) {
        let supervisor = Arc::clone(self);
        tokio::spawn(async move {
            let mut saw_close = false;
            while let Some(event) = rx.recv().await {
                let (order, stopping) = {
                    let sessions = supervisor.sessions.lock().await;
                    match sessions.get(&id) {
                        Some(entry) => (
                            Arc::clone(&entry.order),
                            entry.stopping.load(AtomicOrdering::SeqCst),
                        ),
                        None => break, // stopped and reaped; nothing left to say
                    }
                };
                if matches!(event, DriverEvent::StdoutClosed) {
                    saw_close = true;
                }
                let turn_ended = matches!(event, DriverEvent::TurnCompleted { .. });
                // What the lead of a worker hears of this event is worked
                // out before the event is applied.
                let team_end = supervisor.team_ending(&id, &event, stopping);
                let guard = order.lock().await;
                if let Err(err) = supervisor.pump_one(&id, event).await {
                    eprintln!("session {id}: pump error: {err}");
                }
                drop(guard);
                // What waits for the turn's end goes next - a steered
                // message first, then the queue. Its own task: sending
                // takes the order lock the pump just released, and the
                // pump must keep reading the harness meanwhile.
                if turn_ended {
                    let supervisor = Arc::clone(&supervisor);
                    tokio::spawn(async move { supervisor.after_turn(&id).await });
                }
                // A worker's report to its lead, likewise on its own task.
                if let Some(end) = team_end {
                    let supervisor = Arc::clone(&supervisor);
                    tokio::spawn(async move { supervisor.team_turn_ended(&id, end).await });
                }
            }
            // The channel ended: the driver dropped
            // its senders. With StdoutClosed applied, the pump_one above
            // settled and removed the entry; without it (a driver that
            // ended its channel without reporting - an opencode watcher
            // that just returned), the entry is a zombie holding a dead
            // session answerable. Remove it and settle what is left.
            if !saw_close {
                supervisor.settle_process_gone(&id, false).await;
            }
        });
    }

    async fn pump_one(
        self: &Arc<Self>,
        id: &SessionId,
        event: DriverEvent,
    ) -> Result<(), SupervisorError> {
        match event {
            DriverEvent::Initialized {
                session_ref,
                model,
                permission_mode,
            } => {
                let changed = self.with_store(|s| {
                    s.note_session(id, Some(session_ref), model, permission_mode)
                })?;
                // A slow-booting harness may deliver init after a prompt
                // already moved the session to working - init only settles
                // a session that is still starting.
                if matches!(
                    self.session(id).map(|s| s.status),
                    Ok(SessionStatus::Starting)
                ) {
                    self.emit_all(
                        id,
                        vec![EventPayload::SessionStatusChanged {
                            status: SessionStatus::Idle,
                            reason: None,
                        }],
                    )?;
                }
                // A model the console didn't have (the first init's echo of
                // a default it now runs) rides as an update - the poll
                // otherwise delays it.
                if changed {
                    if let Ok(session) = self.session(id) {
                        self.emit_all(id, vec![EventPayload::SessionUpdated { session }])?;
                    }
                }
            }
            DriverEvent::CommandsListed { names } => {
                self.note_command_names(id, &names);
            }
            DriverEvent::ModelReported { model } => {
                // A mid-conversation model fact (opencode's step-start):
                // fold it into the session and tell the console. A repeat
                // of what the session already says is not an event.
                if self.with_store(|s| s.note_session(id, None, Some(model), None))? {
                    if let Ok(session) = self.session(id) {
                        self.emit_all(id, vec![EventPayload::SessionUpdated { session }])?;
                    }
                }
            }
            DriverEvent::SettingsRefused { message } => {
                // The driver would not take a live change, and said why in one
                // sentence. It lands in the transcript as a note so the user
                // reads the reason where they asked for the change. The change
                // was stored when it was written, so it is undone here: the
                // chip and a later resume keep what the harness actually runs.
                let prior = lock_recovering(prior_live_settings()).remove(id);
                if let Some(prior) = prior {
                    let restored = self
                        .with_store(|s| {
                            s.restore_session_settings(id, prior.model, prior.effort, prior.fast)
                        })
                        .map_err(store_error)?;
                    self.emit_all(id, vec![EventPayload::SessionUpdated { session: restored }])?;
                }
                self.emit_all(id, vec![EventPayload::DaemonError { message }])?;
            }
            DriverEvent::AssistantText { text } => {
                let turn = self.current_turn(id);
                self.emit_all(
                    id,
                    vec![EventPayload::MessageAdded {
                        message: openremote_core::ChatMessage {
                            id: Uuid::new_v4().to_string(),
                            turn,
                            role: openremote_core::MessageRole::Assistant,
                            text,
                            origin: None,
                            images: Vec::new(),
                        },
                    }],
                )?;
            }
            DriverEvent::TextDelta { text } => {
                let turn = self.current_turn(id);
                self.emit_all(id, vec![EventPayload::MessageDelta { turn, text }])?;
            }
            DriverEvent::ReasoningText { text } => {
                let turn = self.current_turn(id);
                self.emit_all(id, vec![EventPayload::ReasoningAdded { turn, text }])?;
            }
            DriverEvent::ReasoningDelta { text } => {
                let turn = self.current_turn(id);
                self.emit_all(id, vec![EventPayload::ReasoningDelta { turn, text }])?;
            }
            DriverEvent::ThinkingTokens { tokens } => {
                // The turn this number belongs to, from the same fold the
                // turn-scoped siblings above read: it lets a reader replace a
                // re-reported turn's tally instead of adding to it. The first
                // turn is 0, so the wire field is `Option<u64>`: `Some(0)` is
                // that first turn and `None` is an older daemon that did not
                // say, and a reader must tell the two apart.
                let turn = Some(self.current_turn(id));
                self.emit_all(id, vec![EventPayload::ThinkingTokens { tokens, turn }])?;
            }
            DriverEvent::ToolStarted { name, input, .. } => {
                let turn = self.current_turn(id);
                self.emit_all(id, vec![EventPayload::ToolStarted { turn, name, input }])?;
            }
            DriverEvent::ToolResult {
                name,
                text,
                is_error,
                images,
                ..
            } => {
                let turn = self.current_turn(id);
                // A screenshot's bytes go to the blob store; the event
                // only names it.
                let images = crate::images::keep_tool_images(&self.blobs, *id, &images);
                self.emit_all(
                    id,
                    vec![EventPayload::ToolResult {
                        turn,
                        name,
                        output: Value::String(text),
                        is_error,
                        images,
                    }],
                )?;
            }
            DriverEvent::ApprovalRequested { approval } => {
                // The driver built the spec with the harness's own words.
                let decision = Decision {
                    id: DecisionId::new(),
                    session_id: *id,
                    turn: self.current_turn(id),
                    kind: approval.spec.kind,
                    state: DecisionState::Pending,
                    harness_request: approval.request,
                    harness_ref: Some(approval.harness_ref),
                    tool_name: approval.spec.tool_name,
                    options: approval.spec.options,
                    created_at: now_ms(),
                    answer: None,
                };
                self.emit_all(
                    id,
                    vec![
                        EventPayload::DecisionRequested { decision },
                        EventPayload::SessionStatusChanged {
                            status: SessionStatus::Waiting,
                            reason: None,
                        },
                    ],
                )?;
            }
            DriverEvent::TurnCompleted {
                subtype,
                coarse,
                is_error: _,
                error_message,
            } => {
                let turn = self.current_turn(id);
                let failed = matches!(coarse, TurnOutcome::Failed);
                let steering = lock_recovering(&self.steer_next).contains_key(id);
                let mut payloads = vec![
                    EventPayload::TurnCompleted {
                        turn,
                        outcome: subtype,
                        coarse,
                    },
                    EventPayload::SessionStatusChanged {
                        status: SessionStatus::Idle,
                        reason: None,
                    },
                ];
                // A failed turn explains itself: the harness's own message
                // lands in the transcript as a note.
                if let Some(message) = error_message {
                    payloads.push(EventPayload::DaemonError { message });
                }
                // A failed turn holds the queue, like an interrupt does -
                // unless the turn ended because a message was steered in.
                if failed && !steering {
                    payloads.extend(self.hold_payload(id));
                }
                self.emit_all(id, payloads)?;
            }
            DriverEvent::ContextUsed { used, window } => {
                self.emit_all(id, vec![EventPayload::ContextUsed { used, window }])?;
            }
            DriverEvent::TurnCost { cost_usd } => {
                // Same turn identity as the thinking tokens, so money and
                // tokens are replaced per turn together; `Some(0)` is the
                // first turn, not "not said".
                let turn = Some(self.current_turn(id));
                self.emit_all(id, vec![EventPayload::UsageCost { cost_usd, turn }])?;
            }
            DriverEvent::Compacted => {
                // The harness compacted its own conversation - the
                // transcript says so where it happened.
                self.emit_all(
                    id,
                    vec![EventPayload::NoteAdded {
                        text: "This session was compacted before this message.".to_string(),
                    }],
                )?;
            }
            DriverEvent::Stderr { line } => {
                // The bounded ring: the harness's last
                // words must outlive the eprintln - the session's death
                // quotes them into its Failed reason.
                let mut bounded = line.clone();
                if bounded.chars().count() > STDERR_LINE_BOUND {
                    // One runaway line must not ride into the session
                    // record whole; keep the head, which is where a
                    // harness puts the reason.
                    bounded = bounded.chars().take(STDERR_LINE_BOUND).collect();
                    bounded.push_str(" ...");
                }
                {
                    let mut rings = lock_recovering(&self.stderr_ring);
                    let ring = rings.entry(*id).or_default();
                    ring.push_back(bounded);
                    while ring.len() > STDERR_RING_LINES {
                        ring.pop_front();
                    }
                }
                eprintln!("session {id} harness stderr: {line}");
            }
            DriverEvent::StdoutClosed => {
                let stopping = {
                    let sessions = self.sessions.lock().await;
                    sessions
                        .get(id)
                        .map(|e| e.stopping.load(AtomicOrdering::SeqCst))
                        .unwrap_or(true)
                };
                self.settle_terminal_locked(id, stopping).await?;
            }
        }
        Ok(())
    }

    /// The StdoutClosed settle, the pump's single-writer path.
    /// `stopping` is read while the entry still exists; the boundary is
    /// appended before the status so the transcript closes its open
    /// turn first, and no dangling open turn survives a death. The
    /// status itself only lands while the session is still alive: the
    /// failed-write arm of `prompt` can settle the session first, and a
    /// terminal status is a fact, not an opinion to state twice. The
    /// stderr ring's tail splices into the Failed
    /// reason and goes with the entry.
    async fn settle_terminal_locked(
        &self,
        id: &SessionId,
        stopping: bool,
    ) -> Result<(), SupervisorError> {
        let mut payloads: Vec<EventPayload> = self.open_turn_boundary(id);
        payloads.extend(self.hold_payload(id));
        let alive = self
            .session(id)
            .map(|s| s.status.is_alive())
            .unwrap_or(false);
        if alive {
            payloads.push(if stopping {
                EventPayload::SessionStatusChanged {
                    status: SessionStatus::Stopped,
                    reason: None,
                }
            } else {
                EventPayload::SessionStatusChanged {
                    status: SessionStatus::Failed,
                    reason: Some(self.death_reason(id)),
                }
            });
        }
        self.emit_all(id, payloads)?;
        let retired = self.with_store(|s| s.retire_pending(id))?;
        if retired > 0 {
            eprintln!("session {id}: retired {retired} pending decision(s) with the process");
        }
        let done = {
            let mut sessions = self.sessions.lock().await;
            sessions.remove(id).and_then(|e| e.done)
        };
        lock_recovering(&self.stderr_ring).remove(id);
        forget_live_settings(id);
        if let Some(tx) = done {
            if let Ok(session) = self.session(id) {
                let _ = tx.send(session);
            }
        }
        Ok(())
    }

    /// A driver that ended its channel without reporting StdoutClosed:
    /// the pump's loop above found the channel
    /// closed on entry - the entry is a zombie against a dead driver.
    /// Same settle as StdoutClosed, minus the order lock the loop's
    /// caller may already hold: this path runs outside pump_one.
    async fn settle_process_gone(&self, id: &SessionId, stopping: bool) {
        let entry = {
            let sessions = self.sessions.lock().await;
            sessions.get(id).map(|e| Arc::clone(&e.order))
        };
        let Some(order) = entry else {
            return; // already settled and removed
        };
        let _guard = order.lock().await;
        if let Err(err) = self.settle_terminal_locked(id, stopping).await {
            eprintln!("session {id}: zombie settle failed: {err}");
        }
    }

    /// The reason a dying session records: the harness's own last stderr
    /// words when it has them, the generic fact
    /// otherwise. The ring's last few lines, joined - a reason is a
    /// sentence, not a log.
    fn death_reason(&self, id: &SessionId) -> String {
        let tail: Vec<String> = {
            let rings = lock_recovering(&self.stderr_ring);
            rings
                .get(id)
                .map(|ring| {
                    ring.iter()
                        .rev()
                        .take(STDERR_REASON_LINES)
                        .cloned()
                        .rev()
                        .collect()
                })
                .unwrap_or_default()
        };
        if tail.is_empty() {
            "harness process exited".to_string()
        } else {
            format!(
                "harness process exited; its last words: {}",
                tail.join(" | ")
            )
        }
    }

    fn current_turn(&self, id: &SessionId) -> u64 {
        self.with_store(|s| {
            s.session(id)
                .map(|s| s.next_turn.saturating_sub(1))
                .unwrap_or(0)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A supervisor over a throwaway store with no harnesses: enough for
    /// the maps and rule removal - nothing here spawns a session.
    /// Records what the supervisor asks of the tabs note.
    #[derive(Default)]
    struct Notes(StdMutex<Vec<String>>);

    impl PromptNotes for Notes {
        fn note(&self, _: &SessionId) -> Option<String> {
            self.0.lock().unwrap().push("note".into());
            Some("<tabs>".into())
        }
        fn settled(&self, _: &SessionId, delivered: bool) {
            self.0.lock().unwrap().push(format!("settled {delivered}"));
        }
        fn forget(&self, _: &SessionId) {
            self.0.lock().unwrap().push("forget".into());
        }
    }

    use std::path::PathBuf;

    #[tokio::test]
    async fn a_note_counts_only_once_delivered_and_a_cleared_conversation_forgets_it() {
        let supervisor = supervisor();
        supervisor.set_controls(PathBuf::from("openremote-daemon"), 1);
        let notes = Arc::new(Notes::default());
        supervisor.set_prompt_notes(notes.clone());
        let id = SessionId::new();
        let calls = || std::mem::take(&mut *notes.0.lock().unwrap());

        let (text, noted) = supervisor.with_prompt_note(&id, true, "hi", None);
        assert_eq!((text.as_str(), noted), ("hi\n\n<tabs>", true));
        supervisor.note_delivered(&id, noted, false);
        assert_eq!(calls(), ["note", "settled false"]);
        supervisor.note_delivered(&id, true, true);
        assert_eq!(calls(), ["settled true"]);

        // /clear starts the conversation over: what it was told is gone, and
        // the command itself carries no note.
        let (text, noted) = supervisor.with_prompt_note(&id, true, "/clear", None);
        assert_eq!((text.as_str(), noted), ("/clear", false));
        assert_eq!(calls(), ["forget"]);
        // An App without OpenRemote's tools is never asked.
        let (_, noted) = supervisor.with_prompt_note(&id, false, "hi", None);
        supervisor.note_delivered(&id, noted, true);
        assert!(calls().is_empty());
    }

    fn supervisor() -> Arc<Supervisor> {
        let dir = std::env::temp_dir().join(format!("or-supervisor-{}", Uuid::new_v4()));
        let store = Arc::new(StdMutex::new(Store::open(dir).expect("store opens")));
        let supervisor = Supervisor::new(store, HarnessRegistry::none());
        // The engines tick in the background and would hold the store's
        // directory open; the test asserts only the in-memory maps, so
        // the directory outlives the run as junk in temp. No cleanup to
        // race.
        supervisor
    }

    /// The move's quiet-chat rule, including the case a route test cannot
    /// catch: a spawn still under way. A chat whose harness is up and only
    /// waiting for its first message moves; one with no harness yet does not.
    #[test]
    fn a_chat_spawning_is_refused_and_one_waiting_for_its_first_message_is_not() {
        let session = |status: SessionStatus| Session {
            id: SessionId::new(),
            harness: "claude".into(),
            workspace: None,
            status,
            model: None,
            endpoint_id: None,
            effort: None,
            permission_mode: None,
            harness_session_ref: None,
            created_at: 0,
            updated_at: 0,
            last_error: None,
            next_turn: 0,
            fast: false,
            approved_tools: Vec::new(),
            executor: None,
            title: None,
            queue: Vec::new(),
            queue_paused: false,
            mentioned_chats: Vec::new(),
            carry_transcript: None,
            parent: None,
        };
        // A spawn still under way is refused, and the word says why.
        let err = refuse_busy(&session(SessionStatus::Starting), false).unwrap_err();
        assert!(err.to_string().contains("this chat is starting"), "{err}");
        // The same status with the harness up is only waiting for its first
        // message (claude, grok and agy name their conversation then).
        assert!(refuse_busy(&session(SessionStatus::Starting), true).is_ok());
        // A turn in flight is refused, whatever the process is doing.
        for status in [SessionStatus::Working, SessionStatus::Waiting] {
            let err = refuse_busy(&session(status), true).unwrap_err();
            assert!(
                err.to_string().contains("a turn is running in this chat"),
                "{err}"
            );
        }
        // Everything else moves.
        for status in [
            SessionStatus::Idle,
            SessionStatus::Stopped,
            SessionStatus::Failed,
        ] {
            assert!(refuse_busy(&session(status), true).is_ok(), "{status:?}");
        }
    }

    /// A schedule rule on this machine, saved through the real path.
    async fn schedule_rule(supervisor: &Arc<Supervisor>) -> RuleId {
        let ws = std::env::temp_dir().join(format!("or-rule-ws-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&ws).expect("workspace dir");
        let machine = supervisor.this_machine;
        let rule = AutomationRule {
            id: RuleId::new(),
            name: "a rule".into(),
            trigger: openremote_core::Trigger {
                kind: TriggerKind::Schedule,
                time: Some("09:00".into()),
                every_minutes: None,
                key: None,
            },
            harness: "claude".into(),
            model: None,
            effort: None,
            permission_mode: None,
            workspace: ws,
            machine,
            task: "a task".into(),
            enabled: true,
            last_chat: None,
            last_run: None,
            created_at: 0,
            updated_at: 0,
        };
        supervisor.save_rule(rule, None).expect("rule saves").id
    }

    fn interval_rule(every: u32, last_run: Option<i64>, created_at: i64) -> AutomationRule {
        AutomationRule {
            id: RuleId::new(),
            name: "an interval".into(),
            trigger: openremote_core::Trigger {
                kind: TriggerKind::Schedule,
                time: None,
                every_minutes: Some(every),
                key: None,
            },
            harness: "claude".into(),
            model: None,
            effort: None,
            permission_mode: None,
            workspace: std::env::temp_dir(),
            machine: MachineId::new(),
            task: "a task".into(),
            enabled: true,
            last_chat: None,
            last_run,
            created_at,
            updated_at: 0,
        }
    }

    #[test]
    fn an_interval_schedule_is_due_exactly_one_cadence_after_its_last_run() {
        let now = 1_000_000_000i64;
        let minute = 60_000i64;
        assert!(schedule_due(
            &interval_rule(30, Some(now - 30 * minute), 0),
            now,
            "00:00"
        ));
        assert!(!schedule_due(
            &interval_rule(30, Some(now - 29 * minute - 59_999), 0),
            now,
            "00:00"
        ));
        // No recorded run: the cadence counts from creation, not from zero.
        assert!(schedule_due(
            &interval_rule(30, None, now - 30 * minute),
            now,
            "00:00"
        ));
        assert!(!schedule_due(
            &interval_rule(30, None, now - 29 * minute),
            now,
            "00:00"
        ));
        // The daily path is unchanged, and a disabled rule is never due.
        let mut daily = interval_rule(30, None, 0);
        daily.trigger.every_minutes = None;
        daily.trigger.time = Some("09:00".into());
        assert!(schedule_due(&daily, now, "09:00"));
        assert!(!schedule_due(&daily, now, "09:01"));
        daily.enabled = false;
        assert!(!schedule_due(&daily, now, "09:00"));
    }

    /// A deleted rule leaves nothing behind in the clock's arming map:
    /// removal drops the rule's entry, or the map ratchets with every
    /// rule the account ever deleted.
    #[tokio::test]
    async fn removing_a_rule_evicts_its_fired_arming_entry() {
        let supervisor = supervisor();
        let id = schedule_rule(&supervisor).await;
        lock_recovering(&supervisor.fired).insert(id, "2001-01-01 09:00".into());
        supervisor.remove_rule(&id).expect("rule removes");
        assert!(
            lock_recovering(&supervisor.fired).get(&id).is_none(),
            "the removed rule's arming entry is evicted"
        );
    }

    /// The day prune: an entry from an earlier day is garbage the moment
    /// the day changes - the engine arms per day, so it can never match
    /// again - while the current day's arming survives to keep its
    /// once-per-day guarantee.
    #[test]
    fn the_day_prune_drops_only_other_days_arming_entries() {
        let mut fired = HashMap::new();
        let old = RuleId::new();
        let today = RuleId::new();
        fired.insert(old, "2001-01-01 09:00".into());
        fired.insert(today, "2001-01-02 09:00".into());
        prune_fired(&mut fired, "2001-01-02");
        assert!(fired.contains_key(&today), "today's arming survives");
        assert!(!fired.contains_key(&old), "the old day's arming is gone");
    }

    /// The poisoning policy is real, not aspirational: a panic while the
    /// lock is held poisons the mutex, and the shared acquisition must
    /// recover the guard so the next request is served, not bricked.
    #[test]
    fn a_poisoned_store_lock_is_recovered_not_fatal() {
        let dir = std::env::temp_dir().join(format!("or-poisoned-lock-{}", Uuid::new_v4()));
        let store = Arc::new(StdMutex::new(
            Store::open(dir.clone()).expect("store opens"),
        ));
        let poisoned = Arc::clone(&store);
        // A real panic under the lock is what poisons a std mutex: the
        // spawned thread's unwind drops the guard on its way out.
        let panicked = std::thread::spawn(move || {
            let _guard = poisoned.lock().unwrap();
            panic!("boom under the store lock");
        })
        .join();
        assert!(panicked.is_err(), "the setup thread must have panicked");
        assert!(store.is_poisoned(), "the lock must be poisoned by now");
        // The shared policy: the next acquisition recovers the guard and
        // serves instead of taking the daemon's whole API down with it.
        let guard = lock_recovering(&store);
        let sessions = guard.sessions();
        assert!(
            sessions.is_empty(),
            "the recovered guard must read the store"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    // ---- supervisor lifecycle hardening ----
    //
    // The quiet-session stop path is covered end to end by e2e.rs's
    // `stop_retires_pending_decisions_and_the_late_answer_conflicts`; the
    // unit tests here drive the internals the e2e harness cannot stage
    // deterministically - a wedged drain, a channel that ends without
    // StdoutClosed, the ring's bounds, and the dead-write window.

    /// A supervisor with a session created through the real path and a
    /// map entry holding a stand-in driver whose process is already
    /// gone. The registry stays empty - these tests drive the
    /// supervisor's own settle paths, not any real harness's.
    async fn lifecycle_supervisor() -> (Arc<Supervisor>, SessionId) {
        let supervisor = supervisor();
        let session = Session {
            id: SessionId::new(),
            harness: "claude".into(),
            workspace: None,
            status: SessionStatus::Starting,
            model: None,
            endpoint_id: None,
            effort: None,
            permission_mode: None,
            harness_session_ref: None,
            created_at: now_ms(),
            updated_at: now_ms(),
            last_error: None,
            next_turn: 0,
            fast: false,
            approved_tools: Vec::new(),
            executor: None,
            title: None,
            queue: Vec::new(),
            queue_paused: false,
            mentioned_chats: Vec::new(),
            carry_transcript: None,
            parent: None,
        };
        let id = session.id;
        supervisor
            .emit_all(&id, vec![EventPayload::SessionCreated { session }])
            .expect("session created");
        supervisor.sessions.lock().await.insert(
            id,
            SessionEntry {
                driver: Arc::new(AsyncMutex::new(dead_driver())),
                order: Arc::new(AsyncMutex::new(())),
                stopping: Arc::new(AtomicBool::new(false)),
                done: None,
            },
        );
        (supervisor, id)
    }

    #[tokio::test]
    async fn a_resume_or_steer_mid_move_is_refused_and_the_slot_freed_lets_it_through() {
        let supervisor = supervisor();
        let session = Session {
            id: SessionId::new(),
            harness: "claude".into(),
            workspace: None,
            status: SessionStatus::Stopped,
            model: None,
            endpoint_id: None,
            effort: None,
            permission_mode: None,
            harness_session_ref: Some("c-1".into()),
            created_at: now_ms(),
            updated_at: now_ms(),
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
            parent: None,
        };
        let id = session.id;
        supervisor
            .emit_all(&id, vec![EventPayload::SessionCreated { session }])
            .expect("session created");
        let slot = {
            lock_recovering(moving_chats()).insert(id);
            Moving(id)
        };
        let moving = |result: Result<(), SupervisorError>, what: &str| match result {
            Err(SupervisorError::Conflict(words)) => {
                assert!(words.contains("being moved"), "{what}: {words}")
            }
            other => panic!("{what} mid-move must be refused: {other:?}"),
        };
        moving(supervisor.resume(&id).await.map(|_| ()), "resume");
        moving(supervisor.steer(&id, "hi").await.map(|_| ()), "steer");
        moving(
            supervisor.queue_resume(&id).await.map(|_| ()),
            "queue resume",
        );
        // Once the move is over, the same resume gets past the move check
        // (and on to the harness, which this test has none of).
        drop(slot);
        if let Err(SupervisorError::Conflict(words)) = supervisor.resume(&id).await {
            assert!(!words.contains("being moved"), "{words}")
        }
    }

    /// A claude driver over a process that is gone: spawned through the
    /// real path against this test binary run with argv that makes it
    /// exit immediately - a dead harness process whose stdin write
    /// fails, the shape the dead-write paths exercise. The binary runs
    /// its own exe with `--version`-style argv; the test binary with an
    /// unknown arg exits with the harness-error path quickly. Using the
    /// real spawn keeps the channel, readers, and stdin all real.
    fn dead_driver() -> crate::registry::SessionDriver {
        let resolution = Resolution::Executable(std::env::current_exe().expect("test exe"));
        let opts = openremote_harness::SpawnOptions {
            cwd: std::env::temp_dir(),
            // A nonexistent harness flag is irrelevant - the argv only
            // needs the claude stream-json shape so the spawn takes it.
            ..Default::default()
        };
        let (driver, rx) = openremote_claude::Driver::spawn(&resolution, opts)
            .map(|(d, r)| (crate::registry::SessionDriver::Claude(d), r))
            .expect("spawn the dead-driver stand-in");
        // Keep the receiver alive but unread; the driver's process exits
        // on its own (the test binary answers the claude argv's stdin EOF
        // by exiting), leaving the dead-driver state.
        std::mem::forget(rx);
        driver
    }

    /// A stopped session leaves no map entry behind, and a later prompt
    /// on it answers NotFound by name - the supervisor's side of the
    /// no-zombie contract. The stand-in driver has
    /// no live process, so stop's fallback settle is the path taken -
    /// the same path a print-mode harness between prompts takes.
    #[tokio::test]
    async fn a_settled_stop_leaves_no_answerable_entry_behind() {
        let (supervisor, id) = lifecycle_supervisor().await;
        let stopped = supervisor.stop(&id).await.expect("stop settles");
        assert_eq!(stopped.status, SessionStatus::Stopped, "settled stopped");
        assert!(
            supervisor.sessions.lock().await.get(&id).is_none(),
            "the entry is removed - no zombie keeps the dead session answerable"
        );
        let err = supervisor.prompt(&id, "late").await.expect_err("refused");
        assert!(
            err.to_string().contains("not running"),
            "the error names the contract: {err}"
        );
    }

    /// The fallback settle must not duplicate what the pump already
    /// wrote: when the entry is gone by the time the fallback looks
    /// (the pump settled it during the drain window), the fallback
    /// stands down - one boundary, one status event, no duplicates.
    #[tokio::test]
    async fn the_fallback_settle_stands_down_when_the_pump_settled_first() {
        let (supervisor, id) = lifecycle_supervisor().await;
        // Open a turn so a settle would append a boundary if it ran.
        supervisor
            .emit_all(
                &id,
                vec![EventPayload::SessionStatusChanged {
                    status: SessionStatus::Working,
                    reason: None,
                }],
            )
            .expect("working");
        // The pump's own settle path runs first and removes the entry.
        supervisor
            .settle_terminal_locked(&id, true)
            .await
            .expect("pump settles");
        // The fallback arrives with the stale entry handle stop holds:
        // the sessions map no longer has the id, so it must stand down.
        let stale = SessionEntry {
            driver: Arc::new(AsyncMutex::new(dead_driver())),
            order: Arc::new(AsyncMutex::new(())),
            stopping: Arc::new(AtomicBool::new(true)),
            done: None,
        };
        let before = supervisor
            .with_store(|s| s.events_after(&id, None))
            .expect("events")
            .len();
        let settled = supervisor.settle_stopped(&id, &stale).await;
        assert!(!settled, "the fallback stands down: the entry is gone");
        let after = supervisor
            .with_store(|s| s.events_after(&id, None))
            .expect("events");
        assert_eq!(after.len(), before, "no duplicate events were appended");
    }

    /// A zombie driver - the channel ends without StdoutClosed (harness
    /// audit F10) - must not leave the entry: the pump's channel-end
    /// path settles the session Failed, closes the dangling turn, and
    /// removes the entry, so nothing stays answerable against a dead
    /// driver.
    #[tokio::test]
    async fn a_channel_that_ends_without_stdoutclosed_still_removes_the_entry() {
        let (supervisor, id) = lifecycle_supervisor().await;
        // Leave a turn open so the settle must close it - the dangling
        // turn is half the finding. bump_turn is the real flow's own
        // numbering (prompt calls it before the events).
        let turn = supervisor.with_store(|s| s.bump_turn(&id)).expect("turn");
        supervisor
            .emit_all(
                &id,
                vec![
                    EventPayload::SessionStatusChanged {
                        status: SessionStatus::Working,
                        reason: None,
                    },
                    EventPayload::TurnStarted { turn },
                ],
            )
            .expect("open turn");
        // The pump task's own channel-end path, staged directly.
        supervisor.settle_process_gone(&id, false).await;
        let session = supervisor.session(&id).expect("session lives in the store");
        assert_eq!(
            session.status,
            SessionStatus::Failed,
            "a driver that vanished without a word failed its session"
        );
        assert!(
            supervisor.sessions.lock().await.get(&id).is_none(),
            "the zombie entry is gone"
        );
        assert!(
            lock_recovering(&supervisor.stderr_ring).get(&id).is_none(),
            "the stderr ring went with the entry"
        );
        let events = supervisor
            .with_store(|s| s.events_after(&id, None))
            .expect("events");
        let kinds: Vec<&str> = events.iter().map(|e| e.kind()).collect();
        assert!(
            kinds.contains(&"turn.completed"),
            "the dangling turn got a synthetic boundary; kinds: {kinds:?}"
        );
        // A second call is a no-op: already settled and removed.
        supervisor.settle_process_gone(&id, false).await;
        let session = supervisor.session(&id).expect("session still in the store");
        assert_eq!(session.status, SessionStatus::Failed, "unchanged");
    }

    /// The stderr ring keeps a bounded tail and the death reason splices
    /// its last lines: a harness that dies
    /// explaining itself on stderr leaves its own words in the Failed
    /// reason, and a chatty harness cannot grow the ring without bound.
    #[tokio::test]
    async fn a_dying_harnesss_last_stderr_words_reach_the_failed_reason() {
        let (supervisor, id) = lifecycle_supervisor().await;
        for i in 0..(STDERR_RING_LINES + 4) {
            supervisor
                .pump_one(
                    &id,
                    DriverEvent::Stderr {
                        line: format!("chatter {i}"),
                    },
                )
                .await
                .expect("stderr pump");
        }
        // The ring kept only the tail (the guard never crosses the await
        // points below).
        let kept = {
            let ring = lock_recovering(&supervisor.stderr_ring);
            ring.get(&id).expect("ring exists").clone()
        };
        assert_eq!(kept.len(), STDERR_RING_LINES, "the ring is bounded");
        assert_eq!(
            kept.front().map(String::as_str),
            Some("chatter 4"),
            "the oldest beyond the bound was dropped"
        );
        assert_eq!(
            kept.back().map(String::as_str),
            Some(&*format!("chatter {}", STDERR_RING_LINES + 3)),
            "the newest is the ring's tail"
        );
        // The death reason quotes the ring's last few lines, in order.
        // The ring holds chatter 4..=11 (12 lines, ring of 8), so the
        // last 4 are 8..=11.
        let reason = supervisor.death_reason(&id);
        assert!(
            reason.contains("chatter 8 | chatter 9 | chatter 10 | chatter 11"),
            "the reason splices the last {STDERR_REASON_LINES} lines in order; got: {reason}"
        );
        // And the settle puts them into the Failed reason on the record.
        supervisor
            .emit_all(
                &id,
                vec![EventPayload::SessionStatusChanged {
                    status: SessionStatus::Working,
                    reason: None,
                }],
            )
            .expect("alive");
        supervisor
            .settle_terminal_locked(&id, false)
            .await
            .expect("settle");
        let session = supervisor.session(&id).expect("session");
        assert_eq!(session.status, SessionStatus::Failed);
        assert!(
            session
                .last_error
                .as_deref()
                .is_some_and(|r| r.contains("chatter 11")),
            "the harness's own last words are the failure reason; got: {:?}",
            session.last_error
        );
        // An empty ring keeps the honest generic fact.
        let (supervisor2, id2) = lifecycle_supervisor().await;
        assert_eq!(
            supervisor2.death_reason(&id2),
            "harness process exited",
            "no stderr, no invented words"
        );
    }

    /// One runaway stderr line must not ride into the session record
    /// whole: the ring bounds the line itself, not just the count.
    #[tokio::test]
    async fn a_runaway_stderr_line_is_truncated_into_the_ring() {
        let (supervisor, id) = lifecycle_supervisor().await;
        let runaway: String = "x".repeat(STDERR_LINE_BOUND + 500);
        supervisor
            .pump_one(
                &id,
                DriverEvent::Stderr {
                    line: runaway.clone(),
                },
            )
            .await
            .expect("stderr pump");
        let kept = {
            let ring = lock_recovering(&supervisor.stderr_ring);
            ring.get(&id).expect("ring exists").clone()
        };
        let line = kept.back().expect("the line was kept").clone();
        assert!(
            line.chars().count() <= STDERR_LINE_BOUND + 4,
            "the runaway line is bounded; len: {}",
            line.chars().count()
        );
        assert!(line.ends_with(" ..."), "the truncation says so");
        let reason = supervisor.death_reason(&id);
        assert!(
            reason.chars().count() < STDERR_LINE_BOUND + 100,
            "the reason carries the truncated line only; got {} chars",
            reason.chars().count()
        );
    }

    /// A slow-draining pump: stop must wait for the
    /// transcript to stop advancing - not a flat clock - so the tail
    /// lands. A fake pump applies events slower than the old 3s fallback
    /// could tolerate... no: slower than one quiet window, exactly the
    /// pace a chatty stream on a slow disk produces. Every event must be
    /// in the log, the open turn closed, and the pending decision
    /// retired when stop returns.
    #[tokio::test]
    async fn stop_drains_a_slow_backlog_before_settling() {
        let (supervisor, id) = lifecycle_supervisor().await;
        let turn = supervisor.with_store(|s| s.bump_turn(&id)).expect("turn");
        supervisor
            .emit_all(
                &id,
                vec![
                    EventPayload::SessionStatusChanged {
                        status: SessionStatus::Working,
                        reason: None,
                    },
                    EventPayload::TurnStarted { turn },
                ],
            )
            .expect("open turn");
        // A pending decision the stop must retire: decisions left
        // answerable against a dead process are the other half of the
        // finding.
        supervisor
            .emit_all(
                &id,
                vec![EventPayload::DecisionRequested {
                    decision: Decision {
                        id: DecisionId::new(),
                        session_id: id,
                        turn,
                        kind: openremote_core::DecisionKind::Approval,
                        state: DecisionState::Pending,
                        harness_request: Value::Null,
                        harness_ref: Some("cr-1".into()),
                        tool_name: Some("Bash".into()),
                        options: Vec::new(),
                        created_at: now_ms(),
                        answer: None,
                    },
                }],
            )
            .expect("pending decision");
        // The fake pump: six tail events, one per 300ms - each slower
        // than a quiet slice but faster than the quiet window, so the
        // drain's seq evidence keeps it waiting exactly as a real
        // mid-drain backlog would.
        let pumper = Arc::clone(&supervisor);
        let pump_id = id;
        let pump = tokio::spawn(async move {
            for i in 0..6 {
                tokio::time::sleep(Duration::from_millis(300)).await;
                if let Err(err) = pumper.emit_all(
                    &pump_id,
                    vec![EventPayload::MessageAdded {
                        message: openremote_core::ChatMessage {
                            id: Uuid::new_v4().to_string(),
                            turn,
                            role: openremote_core::MessageRole::Assistant,
                            text: format!("tail {i}"),
                            origin: None,
                            images: Vec::new(),
                        },
                    }],
                ) {
                    eprintln!("fake pump: {err}");
                }
            }
        });
        let stopped = supervisor.stop(&id).await.expect("stop settles");
        assert_eq!(stopped.status, SessionStatus::Stopped);
        // The pump finished before the settle - every tail event landed.
        pump.await.expect("fake pump done");
        let events = supervisor
            .with_store(|s| s.events_after(&id, None))
            .expect("events");
        let texts: Vec<&str> = events
            .iter()
            .filter_map(|e| match &e.payload {
                EventPayload::MessageAdded { message } => Some(message.text.as_str()),
                _ => None,
            })
            .collect();
        for i in 0..6 {
            assert!(
                texts.contains(&format!("tail {i}").as_str()),
                "tail {i} landed; texts: {texts:?}"
            );
        }
        let kinds: Vec<&str> = events.iter().map(|e| e.kind()).collect();
        let boundary = kinds
            .iter()
            .position(|k| *k == "turn.completed")
            .expect("the open turn was closed");
        let last_tail = texts.len();
        assert!(
            kinds[..boundary].contains(&"message.added"),
            "the boundary follows the drained tail; kinds: {kinds:?}, {last_tail} messages"
        );
        // The decision retired with the process.
        let decisions = supervisor.decisions(&id);
        assert_eq!(
            decisions[0].state,
            DecisionState::Retired,
            "retired, not answerable"
        );
        assert!(
            supervisor.sessions.lock().await.get(&id).is_none(),
            "the entry is removed after the drain"
        );
    }

    /// A closed turn followed by its trailing events (a failed turn's
    /// error note, a status change) must not be closed twice: the
    /// boundary decision scans to the last turn-scoped event, not the
    /// log's last line. This is the grok `die` shape end to end - a
    /// failed turn with its note, then a stop that finds nothing to
    /// close.
    #[tokio::test]
    async fn a_closed_turn_with_trailing_events_is_not_closed_twice() {
        let (supervisor, id) = lifecycle_supervisor().await;
        let turn = supervisor.with_store(|s| s.bump_turn(&id)).expect("turn");
        supervisor
            .emit_all(
                &id,
                vec![
                    EventPayload::SessionStatusChanged {
                        status: SessionStatus::Working,
                        reason: None,
                    },
                    EventPayload::TurnStarted { turn },
                    EventPayload::TurnCompleted {
                        turn,
                        outcome: "failed".into(),
                        coarse: TurnOutcome::Failed,
                    },
                    // The failed turn's own note lands after the boundary
                    // - trailing, never deciding.
                    EventPayload::DaemonError {
                        message: "grok exited before the turn finished".into(),
                    },
                ],
            )
            .expect("turn closed with a trailing note");
        assert!(
            supervisor.open_turn_boundary(&id).is_empty(),
            "the closed turn is not closed twice"
        );
        // The next turn's events then open a fresh decision point.
        let next = supervisor
            .with_store(|s| s.bump_turn(&id))
            .expect("next turn");
        supervisor
            .emit_all(&id, vec![EventPayload::TurnStarted { turn: next }])
            .expect("next turn started");
        assert_eq!(
            supervisor.open_turn_boundary(&id).len(),
            1,
            "the new open turn is closable"
        );
    }

    /// A prompt whose write fails after the turn's events are durable:
    /// the turn must not dangle - a synthetic
    /// failed boundary lands for the just-bumped turn, and the session
    /// settles Failed with the write's own reason.
    #[tokio::test]
    async fn a_failed_prompt_write_closes_the_justbumped_turn() {
        let (supervisor, id) = lifecycle_supervisor().await;
        // The session must be alive for prompt to proceed past its checks.
        supervisor
            .emit_all(
                &id,
                vec![EventPayload::SessionStatusChanged {
                    status: SessionStatus::Idle,
                    reason: None,
                }],
            )
            .expect("idle");
        // The stand-in driver's process exits on its own soon after the
        // spawn (the test binary under claude argv answers stdin EOF by
        // exiting); give it a moment to be gone, then send into the
        // dead-driver window the audit describes.
        tokio::time::sleep(Duration::from_millis(300)).await;
        let result = supervisor.prompt(&id, "retry into a dead harness").await;
        // Whatever the write did, the turn cannot dangle.
        let events = supervisor
            .with_store(|s| s.events_after(&id, None))
            .expect("events");
        let kinds: Vec<&str> = events.iter().map(|e| e.kind()).collect();
        assert!(
            kinds.contains(&"turn.started"),
            "the turn was recorded; kinds: {kinds:?}"
        );
        assert!(
            kinds.contains(&"turn.completed"),
            "the failed write still closed the turn; kinds: {kinds:?}"
        );
        let session = supervisor.session(&id).expect("session");
        assert_eq!(
            session.status,
            SessionStatus::Failed,
            "the session settles Failed with the write's own reason; kinds: {kinds:?}"
        );
        let reason = session.last_error.unwrap_or_default();
        assert!(
            !reason.is_empty(),
            "the failure names the write error, not nothing"
        );
        // A write that raced the process's last moment still left the
        // closed turn and the settled session - the contract holds
        // either way, so the error arm only asserts the caller sees
        // something; the Ok arm needs nothing more.
        let _ = result;
    }

    /// The failed-write arm and the pump's StdoutClosed settle race for
    /// the same session when the process death behind the write also
    /// delivers its close event. Whichever settles first is the record;
    /// the loser must append nothing on top - one boundary for the turn,
    /// one terminal status for the session, whichever order they run in.
    /// The arm routes its boundary through open_turn_boundary (a no-op
    /// on a closed turn) and its status only while the session reads
    /// alive; the pump's settle skips its status the same way.
    #[tokio::test]
    async fn a_failed_prompt_write_racing_the_pumps_close_settles_once() {
        // Pump first: the StdoutClosed settle closes the turn and fails
        // the session before the prompt arm takes the order lock back.
        let (supervisor, id) = lifecycle_supervisor().await;
        supervisor
            .emit_all(
                &id,
                vec![EventPayload::SessionStatusChanged {
                    status: SessionStatus::Working,
                    reason: None,
                }],
            )
            .expect("working");
        let turn = supervisor.with_store(|s| s.bump_turn(&id)).expect("turn");
        supervisor
            .emit_all(&id, vec![EventPayload::TurnStarted { turn }])
            .expect("turn started");
        supervisor
            .settle_terminal_locked(&id, false)
            .await
            .expect("pump settles first");
        // The prompt arm arrives late with its write already failed; its
        // own route reads the store, not the entry, and must find
        // nothing left to append.
        let mut payloads = supervisor.open_turn_boundary(&id);
        if supervisor
            .session(&id)
            .map(|s| s.status.is_alive())
            .unwrap_or(false)
        {
            payloads.push(EventPayload::SessionStatusChanged {
                status: SessionStatus::Failed,
                reason: Some("write failed".into()),
            });
        }
        assert!(
            payloads.is_empty(),
            "the loser appends nothing: the turn is closed and the status is terminal; got {payloads:?}"
        );

        // Arm first: the prompt arm settles, then the pump's close
        // event finds the session already terminal and appends no
        // second status.
        let (supervisor, id) = lifecycle_supervisor().await;
        supervisor
            .emit_all(
                &id,
                vec![EventPayload::SessionStatusChanged {
                    status: SessionStatus::Working,
                    reason: None,
                }],
            )
            .expect("working");
        let turn = supervisor.with_store(|s| s.bump_turn(&id)).expect("turn");
        supervisor
            .emit_all(&id, vec![EventPayload::TurnStarted { turn }])
            .expect("turn started");
        // The arm's own route: boundary via the scan, status while alive.
        let mut payloads = supervisor.open_turn_boundary(&id);
        assert_eq!(payloads.len(), 1, "the open turn is closable");
        if supervisor
            .session(&id)
            .map(|s| s.status.is_alive())
            .unwrap_or(false)
        {
            payloads.push(EventPayload::SessionStatusChanged {
                status: SessionStatus::Failed,
                reason: Some("write failed".into()),
            });
        }
        supervisor
            .emit_all(&id, payloads)
            .expect("arm settles first");
        // Now the pump's settle arrives.
        supervisor
            .settle_terminal_locked(&id, false)
            .await
            .expect("pump settles second");
        let events = supervisor
            .with_store(|s| s.events_after(&id, None))
            .expect("events");
        let kinds: Vec<&str> = events.iter().map(|e| e.kind()).collect();
        let boundaries = kinds.iter().filter(|k| **k == "turn.completed").count();
        // The arm's Failed status is the last word: the pump's late
        // settle found the session terminal and appended nothing after
        // it. (The Working status the test emitted while opening the
        // turn is a live status, not a settle - it does not count.)
        let settles = kinds
            .iter()
            .filter(|k| **k == "session.status_changed")
            .count();
        assert_eq!(
            boundaries, 1,
            "one boundary whichever settled first; kinds: {kinds:?}"
        );
        assert_eq!(
            settles, 2,
            "the setup's Working plus exactly one terminal settle; kinds: {kinds:?}"
        );
        assert_eq!(
            kinds.last(),
            Some(&"session.status_changed"),
            "the arm's settle is the final event; the pump appended nothing after it"
        );
    }

    /// A live harness is refused mid-turn before the driver is written to,
    /// because the driver reads its own screen between turns. A start-time
    /// harness keeps its own path: the driver answers, and an idle process
    /// is restarted (a working one gets the same sentence).
    #[test]
    fn a_live_model_change_is_refused_while_a_turn_works() {
        for status in [
            SessionStatus::Starting,
            SessionStatus::Working,
            SessionStatus::Waiting,
        ] {
            let err = refuse_live_mid_turn(true, status).expect("a live harness is refused");
            assert!(err.contains("wait for this turn to finish"), "{err}");
            // A start-time harness is not refused here; its driver decides.
            assert!(refuse_live_mid_turn(false, status).is_none(), "{status:?}");
        }
        for status in [
            SessionStatus::Idle,
            SessionStatus::Stopped,
            SessionStatus::Failed,
        ] {
            assert!(refuse_live_mid_turn(true, status).is_none(), "{status:?}");
        }
    }

    /// Wiring pin, source text. The race a refusal could win is an order of
    /// statements: the change must be stored and its prior held before the
    /// driver is written to, or a fast refusal restores first and the late
    /// store write puts the refused model back. No test can schedule the
    /// pump between two awaits on demand, so the order itself is pinned.
    #[test]
    fn a_live_change_is_stored_and_held_before_the_driver_write() {
        let src = include_str!("supervisor.rs");
        let body = src
            .split("async fn apply_live_settings(")
            .nth(1)
            .expect("apply_live_settings is in this file")
            .split("/// Point a stopped chat")
            .next()
            .expect("the function has a body");
        let stored = body
            .find("self.store_settings(id, settings)?")
            .expect("stored");
        let held = body.find("prior_live_settings()).insert(").expect("held");
        let written = body.find(".apply_settings(live)").expect("written");
        assert!(stored < held && held < written, "store, hold, then write");
        // The delete path and both ends of a driver forget the held values.
        assert!(include_str!("sync.rs").contains("forget_live_settings(&id)"));
        assert!(src.matches("forget_live_settings(id);").count() >= 3);
    }
}
