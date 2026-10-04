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

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering as AtomicOrdering};
use std::sync::{Arc, Mutex as StdMutex, RwLock as StdRwLock};

use openremote_core::{
    AutomationRule, Decision, DecisionId, DecisionState, Event, EventPayload, Harness, Machine,
    MachineId, MachineStatus, MachineView, Plugin, Receipt, RuleId, Session, SessionId,
    SessionStatus, Store, TriggerKind, now_ms,
};
use openremote_harness::DriverEvent;
use serde_json::Value;
use thiserror::Error;
use tokio::sync::{Mutex as AsyncMutex, broadcast, mpsc, oneshot};
use uuid::Uuid;

use crate::registry::{Backend, HarnessRegistry, SessionDriver};

/// Store conflicts surface as their own shape, not a generic store error.
fn store_error(e: openremote_core::store::StoreError) -> SupervisorError {
    match e {
        openremote_core::store::StoreError::NotFound(m) => SupervisorError::NotFound(m),
        openremote_core::store::StoreError::Conflict(m) => SupervisorError::Conflict(m),
        other => SupervisorError::Store(other),
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
fn machine_view(s: &Store, harnesses: &[Harness], machine: &Machine) -> MachineView {
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
        view.installable = crate::install::installable(&installed);
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

struct SessionEntry {
    driver: Arc<AsyncMutex<SessionDriver>>,
    /// Serializes event sequences for this session across tasks.
    order: Arc<AsyncMutex<()>>,
    stopping: Arc<AtomicBool>,
    /// Fired by the pump when the stop it was told about completes.
    done: Option<oneshot::Sender<Session>>,
}

/// The resolved CLI behind a backend - every variant holds one, and the
/// sign-in relay drives it with the same resolution the sessions do.
fn backend_resolution(backend: &crate::registry::Backend) -> &openremote_harness::Resolution {
    match backend {
        crate::registry::Backend::Claude(r) => r,
        crate::registry::Backend::Codex(r) => r,
        crate::registry::Backend::Grok(r) => r,
        crate::registry::Backend::Pi(r) => r,
        crate::registry::Backend::Opencode(r) => r,
        crate::registry::Backend::Agy(r) => r,
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
    /// The daemon's own machine - sessions run here, plugins ride from here.
    this_machine: MachineId,
}

impl Supervisor {
    /// Build the supervisor over an opened store and a probed registry.
    /// The registry is injected so tests can point harnesses at fixture
    /// agents; production passes `HarnessRegistry::probe(&HashMap::new())`.
    pub fn new(store: Arc<StdMutex<Store>>, registry: HarnessRegistry) -> Arc<Self> {
        let this_machine = {
            let mut guard = store.lock().expect("store lock");
            guard.ensure_this_machine()
        };
        let supervisor = Arc::new(Self {
            store,
            sessions: AsyncMutex::new(HashMap::new()),
            events: broadcast::channel(1024).0,
            registry: StdRwLock::new(registry),
            signins: StdMutex::new(HashMap::new()),
            this_machine,
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
        self.registry.read().expect("registry lock").harnesses()
    }

    /// Advertised models for a harness (empty = the slot stays reserved).
    pub async fn models(&self, harness: &str) -> Vec<openremote_harness::ModelDescriptor> {
        match self.backend(harness) {
            Some(backend) => backend.models().await.unwrap_or_default(),
            None => Vec::new(),
        }
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
    pub fn subscribe(&self) -> broadcast::Receiver<String> {
        self.events.subscribe()
    }

    fn with_store<R>(&self, f: impl FnOnce(&mut Store) -> R) -> R {
        let mut guard = self.store.lock().expect("store lock");
        f(&mut guard)
    }

    /// Append a sequence of events atomically (one store lock) and nudge
    /// the live listeners.
    fn emit_all(
        &self,
        id: &SessionId,
        payloads: Vec<EventPayload>,
    ) -> Result<Vec<Event>, SupervisorError> {
        let events = self.with_store(|s| {
            let mut out = Vec::with_capacity(payloads.len());
            for payload in payloads {
                out.push(s.append(*id, payload)?);
            }
            Ok::<_, openremote_core::store::StoreError>(out)
        })?;
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
        let harnesses = self.registry.read().expect("registry lock").harnesses();
        self.with_store(|s| {
            s.machines()
                .iter()
                .map(|machine| machine_view(s, &harnesses, machine))
                .collect()
        })
    }

    pub fn machine(&self, id: &MachineId) -> Result<MachineView, SupervisorError> {
        let harnesses = self.registry.read().expect("registry lock").harnesses();
        self.with_store(|s| s.machine(id).map(|m| machine_view(s, &harnesses, m)))
            .map_err(store_error)
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

    /// Install a harness on this machine: run its install command, re-probe
    /// with the same overrides, and hand back the machine's refreshed
    /// view. The receipt carries the long wait - npm runs minutes.
    pub async fn install_harness(
        &self,
        machine_id: &MachineId,
        harness_id: &str,
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

        crate::install::install(harness_id)
            .await
            .map_err(SupervisorError::Harness)?;

        self.swap_in_fresh_registry().await;

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
            backend_resolution(backend).clone()
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
    /// landed, a login finished. The guard is never held across the
    /// await: clone the overrides, probe, then swap the whole registry in.
    async fn swap_in_fresh_registry(&self) {
        let overrides = self
            .registry
            .read()
            .expect("registry lock")
            .overrides()
            .clone();
        let fresh = HarnessRegistry::probe(&overrides).await;
        *self.registry.write().expect("registry lock") = fresh;
    }

    // ---- plugins ----

    pub fn plugins(&self) -> Vec<Plugin> {
        self.with_store(|s| s.plugins().into_iter().cloned().collect())
    }

    /// Install a plugin from the marketplace (by catalog id) or written by
    /// hand, on a machine that has checked in. The store dedups; the
    /// machine's reachability is ruled here. A custom plugin is
    /// `(name, detail, command, needs_key)`.
    pub fn install_plugin(
        &self,
        machine_id: &MachineId,
        catalog_id: Option<&str>,
        custom: Option<(&str, &str, &str, bool)>,
    ) -> Result<Plugin, SupervisorError> {
        let machine = self
            .with_store(|s| s.machine(machine_id).cloned())
            .map_err(store_error)?;
        if machine.status != MachineStatus::Online {
            return Err(SupervisorError::Conflict(format!(
                "{} has not checked in yet",
                machine.name
            )));
        }
        let plugin = match (catalog_id, custom) {
            (Some(catalog_id), _) => {
                let entry = crate::plugins::catalog_entry(catalog_id).ok_or_else(|| {
                    SupervisorError::NotFound(format!("no marketplace entry '{catalog_id}'"))
                })?;
                Plugin {
                    id: format!("{catalog_id}@{}", machine.id),
                    catalog_id: Some(entry.id),
                    name: entry.name,
                    detail: entry.detail,
                    command: entry.command,
                    machine: machine.id,
                    needs_key: entry.needs_key,
                    has_key: false,
                    enabled: true,
                    created_at: now_ms(),
                    updated_at: now_ms(),
                }
            }
            (None, Some((name, detail, command, needs_key))) => Plugin {
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
            },
            (None, None) => {
                return Err(SupervisorError::Conflict(
                    "a plugin comes from the marketplace or by hand - one of the two".into(),
                ));
            }
        };
        self.with_store(|s| s.install_plugin(plugin))
            .map_err(store_error)
    }

    pub fn remove_plugin(&self, id: &str) -> Result<(), SupervisorError> {
        self.with_store(|s| s.remove_plugin(id))
            .map_err(store_error)
    }

    pub fn set_plugin_enabled(&self, id: &str, enabled: bool) -> Result<Plugin, SupervisorError> {
        self.with_store(|s| s.set_plugin_enabled(id, enabled))
            .map_err(store_error)
    }

    pub fn acknowledge_plugin_key(&self, id: &str) -> Result<Plugin, SupervisorError> {
        self.with_store(|s| s.acknowledge_plugin_key(id))
            .map_err(store_error)
    }

    /// The MCP servers a new session on this machine should start: every
    /// enabled plugin whose key (if it needs one) the machine has. A plugin
    /// still waiting on its key acknowledgment doesn't ride - starting it
    /// would just fail.
    fn mcp_servers(&self) -> Vec<openremote_harness::McpServer> {
        let machine = self.this_machine;
        self.with_store(|s| {
            s.plugins()
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
                rule.trigger.time = Some(validate_time(rule.trigger.time.as_deref())?);
                rule.trigger.key = None;
            }
            TriggerKind::Webhook => {
                rule.trigger.time = None;
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
        self.with_store(|s| s.remove_rule(id)).map_err(store_error)
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
    /// the request must carry the rule's own key.
    pub async fn fire_webhook(
        self: &Arc<Self>,
        id: &RuleId,
        key: Option<&str>,
    ) -> Result<Session, SupervisorError> {
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
        if key.unwrap_or_default() != expected {
            return Err(SupervisorError::Conflict(
                "the webhook key does not match".into(),
            ));
        }
        self.fire_rule(&rule).await
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
                rule.workspace.clone(),
                rule.model.clone(),
                None,
                false,
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
            let mut fired: HashMap<RuleId, String> = HashMap::new();
            loop {
                tick.tick().await;
                let (now_minute, now_time) = current_local_minute();
                let due: Vec<AutomationRule> = supervisor
                    .with_store(|s| {
                        s.rules()
                            .into_iter()
                            .filter(|r| {
                                r.enabled
                                    && r.trigger.kind == TriggerKind::Schedule
                                    && r.trigger.time.as_deref() == Some(now_time.as_str())
                            })
                            .cloned()
                            .collect::<Vec<AutomationRule>>()
                    })
                    .into_iter()
                    .filter(|rule| {
                        fired
                            .get(&rule.id)
                            .map(|m| m != &now_minute)
                            .unwrap_or(true)
                    })
                    .collect();
                for rule in due {
                    fired.insert(rule.id, now_minute.clone());
                    match supervisor.fire_rule(&rule).await {
                        Ok(session) => eprintln!(
                            "automation '{}': fired on the clock, chat {}",
                            rule.name, session.id
                        ),
                        Err(err) => eprintln!("automation '{}': fire failed: {err}", rule.name),
                    }
                }
            }
        });
    }

    // ---- session lifecycle ----

    /// Create a session and spawn its harness driver. The session exists
    /// even when the spawn fails - the failure is the session's first fact.
    pub async fn create_session(
        self: &Arc<Self>,
        harness: &str,
        workspace: std::path::PathBuf,
        model: Option<String>,
        permission_mode: Option<String>,
        fast: bool,
    ) -> Result<Session, SupervisorError> {
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
        let (model, effort) = match model {
            Some(picked) => (Some(picked), None),
            None if !self.is_fixture(harness) => backend
                .default_model()
                .map(|(m, e)| (Some(m), e))
                .unwrap_or((None, None)),
            None => (None, None),
        };
        let session = Session {
            id: SessionId::new(),
            harness: harness.to_string(),
            workspace,
            status: SessionStatus::Starting,
            model,
            effort,
            permission_mode,
            harness_session_ref: None,
            created_at: now_ms(),
            updated_at: now_ms(),
            last_error: None,
            next_turn: 0,
            fast,
            approved_tools: Vec::new(),
        };
        let id = session.id;
        self.emit_all(&id, vec![EventPayload::SessionCreated { session }])?;

        let current = self.session(&id)?;
        let opts = openremote_harness::SpawnOptions {
            cwd: current.workspace.clone(),
            model: current.model.clone(),
            permission_mode: current.permission_mode.clone(),
            resume: None,
            include_deltas: true,
            fast: current.fast,
            mcp_servers: self.mcp_servers(),
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
        let entry = self.entry(id).await?;
        let _order = entry.order.lock().await;
        let turn = self.with_store(|s| s.bump_turn(id))?;
        self.emit_all(
            id,
            vec![
                EventPayload::SessionStatusChanged {
                    status: SessionStatus::Working,
                    reason: None,
                },
                EventPayload::TurnStarted { turn },
                EventPayload::MessageAdded {
                    message: openremote_core::ChatMessage {
                        id: Uuid::new_v4().to_string(),
                        turn,
                        role: openremote_core::MessageRole::User,
                        text: text.to_string(),
                    },
                },
            ],
        )?;
        drop(_order);
        entry.driver.lock().await.send_prompt(text).await?;
        Ok(())
    }

    /// Change model, effort, or fast on a live chat, in the harness's own
    /// words. A harness that only accepts the change at process start gets
    /// it on the next resume - the chat must be stopped first, and the
    /// error says so. Codex and Pi apply it on the running thread now.
    pub async fn update_settings(
        &self,
        id: &SessionId,
        settings: openremote_harness::SessionSettings,
    ) -> Result<Session, SupervisorError> {
        if settings.model.is_none() && settings.effort.is_none() && settings.fast.is_none() {
            return self.session(id);
        }
        let session = self.session(id)?;
        if let Ok(entry) = self.entry(id).await {
            entry
                .driver
                .lock()
                .await
                .apply_settings(&settings)
                .await
                .map_err(SupervisorError::from)?;
        } else if session.status.is_alive() {
            return Err(SupervisorError::Conflict(
                "the chat is starting - try again in a moment".into(),
            ));
        }
        let updated = self
            .with_store(|s| {
                s.update_session_settings(
                    id,
                    settings.model.clone(),
                    settings.effort.clone(),
                    settings.fast,
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

    /// Interrupt the running turn (the harness cancels; its boundary event
    /// follows).
    pub async fn interrupt(&self, id: &SessionId) -> Result<(), SupervisorError> {
        let entry = self.entry(id).await?;
        entry.driver.lock().await.interrupt().await?;
        Ok(())
    }

    /// Stop the session: kill the driver; the pump records the terminal
    /// status and retires pending decisions. Returns the session as it
    /// settled (stopped, or failed if the process died first).
    pub async fn stop(&self, id: &SessionId) -> Result<Session, SupervisorError> {
        let entry = self.entry(id).await?;
        entry.stopping.store(true, AtomicOrdering::SeqCst);
        let (tx, rx) = oneshot::channel();
        {
            let mut sessions = self.sessions.lock().await;
            if let Some(e) = sessions.get_mut(id) {
                e.done = Some(tx);
            }
        }
        entry.driver.lock().await.shutdown().await?;
        match tokio::time::timeout(std::time::Duration::from_secs(3), rx).await {
            Ok(Ok(session)) => Ok(session),
            _ => {
                // The pump never answered: either it already died (the
                // status is terminal) or the driver has no live process at
                // all (print-mode harnesses between prompts) - settle an
                // still-alive session here, the single writer rule bent
                // only because nothing else is writing.
                if self
                    .session(id)
                    .map(|s| s.status.is_alive())
                    .unwrap_or(false)
                {
                    self.emit_all(
                        id,
                        vec![EventPayload::SessionStatusChanged {
                            status: SessionStatus::Stopped,
                            reason: None,
                        }],
                    )?;
                    self.with_store(|s| s.retire_pending(id))?;
                    self.sessions.lock().await.remove(id);
                }
                self.session(id)
            }
        }
    }

    /// Resume a stopped/failed session on its harness's own conversation
    /// (claude `--resume=<id>`, codex `thread/resume`, pi `--session`, …).
    pub async fn resume(self: &Arc<Self>, id: &SessionId) -> Result<Session, SupervisorError> {
        let session = self.session(id)?;
        if session.status.is_alive() {
            return Err(SupervisorError::Conflict(format!(
                "session {id} is already running"
            )));
        }
        let harness_ref = session.harness_session_ref.clone().ok_or_else(|| {
            SupervisorError::Conflict(
                "the harness never reported a conversation to resume".to_string(),
            )
        })?;
        let Some(backend) = self.backend(&session.harness) else {
            return Err(SupervisorError::Harness(format!(
                "harness '{}' is not available",
                session.harness
            )));
        };
        let opts = openremote_harness::SpawnOptions {
            cwd: session.workspace.clone(),
            model: session.model.clone(),
            permission_mode: session.permission_mode.clone(),
            resume: Some(harness_ref),
            include_deltas: true,
            fast: session.fast,
            mcp_servers: self.mcp_servers(),
        };
        let (driver, rx) = backend.spawn(opts).await?;
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
        self.emit_all(
            id,
            vec![EventPayload::SessionStatusChanged {
                status: SessionStatus::Starting,
                reason: None,
            }],
        )?;
        self.spawn_pump(*id, rx);
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

    // ---- the pump: driver events → store events → broadcast ----

    fn spawn_pump(self: &Arc<Self>, id: SessionId, mut rx: mpsc::Receiver<DriverEvent>) {
        let supervisor = Arc::clone(self);
        tokio::spawn(async move {
            while let Some(event) = rx.recv().await {
                let order = {
                    let sessions = supervisor.sessions.lock().await;
                    match sessions.get(&id) {
                        Some(entry) => Arc::clone(&entry.order),
                        None => break, // stopped and reaped; nothing left to say
                    }
                };
                let _guard = order.lock().await;
                if let Err(err) = supervisor.pump_one(&id, event).await {
                    eprintln!("session {id}: pump error: {err}");
                }
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
                self.emit_all(id, vec![EventPayload::ThinkingTokens { tokens }])?;
            }
            DriverEvent::ToolStarted { name, input, .. } => {
                let turn = self.current_turn(id);
                self.emit_all(id, vec![EventPayload::ToolStarted { turn, name, input }])?;
            }
            DriverEvent::ToolResult {
                name,
                text,
                is_error,
                ..
            } => {
                let turn = self.current_turn(id);
                self.emit_all(
                    id,
                    vec![EventPayload::ToolResult {
                        turn,
                        name,
                        output: Value::String(text),
                        is_error,
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
                self.emit_all(id, payloads)?;
            }
            DriverEvent::ContextUsed { used, window } => {
                self.emit_all(id, vec![EventPayload::ContextUsed { used, window }])?;
            }
            DriverEvent::TurnCost { cost_usd } => {
                self.emit_all(id, vec![EventPayload::UsageCost { cost_usd }])?;
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
                if stopping {
                    self.emit_all(
                        id,
                        vec![EventPayload::SessionStatusChanged {
                            status: SessionStatus::Stopped,
                            reason: None,
                        }],
                    )?;
                } else {
                    self.emit_all(
                        id,
                        vec![EventPayload::SessionStatusChanged {
                            status: SessionStatus::Failed,
                            reason: Some("harness process exited".to_string()),
                        }],
                    )?;
                }
                let retired = self.with_store(|s| s.retire_pending(id))?;
                if retired > 0 {
                    eprintln!(
                        "session {id}: retired {retired} pending decision(s) with the process"
                    );
                }
                let done = {
                    let mut sessions = self.sessions.lock().await;
                    sessions.remove(id).and_then(|e| e.done)
                };
                if let Some(tx) = done {
                    if let Ok(session) = self.session(id) {
                        let _ = tx.send(session);
                    }
                }
            }
        }
        Ok(())
    }

    fn current_turn(&self, id: &SessionId) -> u64 {
        self.with_store(|s| {
            s.session(id)
                .map(|s| s.next_turn.saturating_sub(1))
                .unwrap_or(0)
        })
    }
}
