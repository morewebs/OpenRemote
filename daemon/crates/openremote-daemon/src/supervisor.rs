//! The supervisor: one live driver per session, events into the store,
//! broadcasts to every listener. All session steering funnels through here
//! so receipts, decisions, and lifecycles stay single-tracked. It knows
//! nothing about any specific harness — the registry dispatches.
//!
//! Ordering contract: the store is the truth; the broadcast channel is a
//! live nudge (SSE replays from the store on connect, so a dropped nudge
//! heals). Per session, an `order` lock serializes event sequences between
//! the steering paths (prompt / answer / stop) and the pump task; process
//! death (StdoutClosed) is applied by the pump alone — the single writer
//! for terminal statuses — so a stop can never race a failure into a lie.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering as AtomicOrdering};
use std::sync::{Arc, Mutex as StdMutex, RwLock as StdRwLock};

use openremote_core::{
    Decision, DecisionId, DecisionState, Event, EventPayload, Harness, Machine, MachineId,
    MachineStatus, MachineView, Plugin, Receipt, Session, SessionId, SessionStatus, Store, now_ms,
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

pub struct Supervisor {
    store: Arc<StdMutex<Store>>,
    sessions: AsyncMutex<HashMap<SessionId, SessionEntry>>,
    events: broadcast::Sender<String>,
    /// Swappable: an install re-probes the machine and trades the set in.
    registry: StdRwLock<HarnessRegistry>,
    /// The daemon's own machine — sessions run here, plugins ride from here.
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
            this_machine,
        });
        supervisor.spawn_presence_ticker(this_machine);
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

    /// A clone of the harness's backend — never a borrowed guard, so
    /// nothing holds the registry across an await.
    fn backend(&self, id: &str) -> Option<Backend> {
        self.registry
            .read()
            .expect("registry lock")
            .backend(id)
            .cloned()
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
    /// runs there, and — for this machine — what could be installed.
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

    /// Add a machine: it lands `waiting` — it comes online when its agent
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
    /// view. The receipt carries the long wait — npm runs minutes.
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

        // Fresh facts. The guard is never held across the await: clone the
        // overrides, probe, then swap the whole registry in.
        let overrides = self
            .registry
            .read()
            .expect("registry lock")
            .overrides()
            .clone();
        let fresh = HarnessRegistry::probe(&overrides).await;
        *self.registry.write().expect("registry lock") = fresh;

        self.machine(machine_id)
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
                    "a plugin comes from the marketplace or by hand — one of the two".into(),
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
    /// still waiting on its key acknowledgment doesn't ride — starting it
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

    // ---- session lifecycle ----

    /// Create a session and spawn its harness driver. The session exists
    /// even when the spawn fails — the failure is the session's first fact.
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
        let session = Session {
            id: SessionId::new(),
            harness: harness.to_string(),
            workspace,
            status: SessionStatus::Starting,
            model,
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
            include_deltas: false,
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
                // all (print-mode harnesses between prompts) — settle an
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
            include_deltas: false,
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
        // first — recording the consequence before the answer itself. The
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
            // rest of the chat — record it and let the fresh session ride
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
                self.with_store(|s| s.note_session(id, Some(session_ref), model, permission_mode))?;
                // A slow-booting harness may deliver init after a prompt
                // already moved the session to working — init only settles
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
