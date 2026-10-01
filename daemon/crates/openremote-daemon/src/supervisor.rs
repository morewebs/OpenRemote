//! The supervisor: one live process per session, events into the store,
//! broadcasts to every listener. All session steering funnels through here
//! so receipts, decisions, and lifecycles stay single-tracked.
//!
//! Ordering contract: the store is the truth; the broadcast channel is a
//! live nudge (SSE replays from the store on connect, so a dropped nudge
//! heals). Per session, an `order` lock serializes event sequences between
//! the steering paths (prompt / answer / stop) and the pump task; process
//! death (StdoutClosed) is applied by the pump alone — the single writer
//! for terminal statuses — so a stop can never race a failure into a lie.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering as AtomicOrdering};
use std::sync::{Arc, Mutex as StdMutex};

use openremote_claude::{Driver, DriverEvent, DriverOptions, Resolution};
use openremote_core::{
    Decision, DecisionId, DecisionKind, DecisionOption, DecisionState, Event, EventPayload,
    Harness, Receipt, Session, SessionId, SessionStatus, Store, TurnOutcome, now_ms,
};
use serde_json::Value;
use thiserror::Error;
use tokio::sync::{Mutex as AsyncMutex, broadcast, mpsc, oneshot};
use uuid::Uuid;

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
    Driver(#[from] openremote_claude::DriverError),
}

struct SessionEntry {
    driver: Arc<AsyncMutex<Driver>>,
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
    harnesses: Vec<Harness>,
    resolution: Resolution,
}

impl Supervisor {
    /// Build the supervisor over an opened store and a resolved harness.
    /// The resolution is injected (not re-read from the environment) so
    /// tests can point sessions at the fixture agent.
    pub fn new(store: Arc<StdMutex<Store>>, resolution: Resolution) -> Arc<Self> {
        let harness = Harness {
            id: "claude".into(),
            name: "Claude Code".into(),
            path: match &resolution {
                Resolution::Executable(p) => Some(p.display().to_string()),
                Resolution::NodeScript { node, script } => {
                    Some(format!("{} {}", node.display(), script.display()))
                }
                Resolution::Unavailable => None,
            },
            version: None,
            available: resolution.is_available(),
        };
        Arc::new(Self {
            store,
            sessions: AsyncMutex::new(HashMap::new()),
            events: broadcast::channel(1024).0,
            harnesses: vec![harness],
            resolution,
        })
    }

    pub fn harnesses(&self) -> &[Harness] {
        &self.harnesses
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

    // ---- session lifecycle ----

    /// Create a session and spawn its harness process. The session exists
    /// even when the spawn fails — the failure is the session's first fact.
    pub async fn create_session(
        self: &Arc<Self>,
        harness: &str,
        workspace: std::path::PathBuf,
        model: Option<String>,
        permission_mode: Option<String>,
    ) -> Result<Session, SupervisorError> {
        if !self
            .harnesses
            .iter()
            .any(|h| h.id == harness && h.available)
        {
            return Err(SupervisorError::Harness(format!(
                "harness '{harness}' is not available"
            )));
        }
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
        };
        let id = session.id;
        self.emit_all(&id, vec![EventPayload::SessionCreated { session }])?;

        let opts = DriverOptions {
            cwd: self.session(&id)?.workspace.clone(),
            model: self.session(&id)?.model.clone(),
            permission_mode: self.session(&id)?.permission_mode.clone(),
            resume: None,
            include_deltas: false,
        };
        match Driver::spawn(&self.resolution, opts) {
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

    /// Interrupt the running turn (the harness cancels; a result frame follows).
    pub async fn interrupt(&self, id: &SessionId) -> Result<(), SupervisorError> {
        let entry = self.entry(id).await?;
        entry.driver.lock().await.interrupt().await?;
        Ok(())
    }

    /// Stop the session: kill the process; the pump records the terminal
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
            _ => self.session(id), // pump gone (already dead); status is terminal already
        }
    }

    /// Resume a stopped/failed session on its harness thread (`--resume=<ref>`).
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
        let resolution = self.resolution.clone();
        if !resolution.is_available() {
            return Err(SupervisorError::Harness("claude CLI not found".into()));
        }
        let opts = DriverOptions {
            cwd: session.workspace.clone(),
            model: session.model.clone(),
            permission_mode: session.permission_mode.clone(),
            resume: Some(harness_ref),
            include_deltas: false,
        };
        let (driver, rx) = Driver::spawn(&resolution, opts)?;
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
    /// correlation id, record it, and let the turn continue.
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

        let allowed = choice == "allow";
        let original_input = decision
            .harness_request
            .get("input")
            .cloned()
            .unwrap_or(Value::Null);
        entry
            .driver
            .lock()
            .await
            .answer_approval(
                &harness_ref,
                allowed,
                &original_input,
                "Denied from OpenRemote",
            )
            .await?;
        {
            let _order = entry.order.lock().await;
            self.emit_all(
                &session_id,
                vec![
                    EventPayload::DecisionResponded {
                        decision_id: *decision_id,
                        choice: choice.to_string(),
                    },
                    EventPayload::SessionStatusChanged {
                        status: SessionStatus::Working,
                        reason: None,
                    },
                ],
            )?;
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
            DriverEvent::ApprovalRequested {
                request_id,
                request,
            } => {
                let decision = self.build_decision(id, &request_id, request);
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
                terminal_reason,
                is_error,
            } => {
                let coarse = if subtype == "success" {
                    TurnOutcome::Completed
                } else if terminal_reason
                    .as_deref()
                    .is_some_and(|r| r.starts_with("aborted"))
                {
                    TurnOutcome::Interrupted
                } else {
                    let _ = is_error;
                    TurnOutcome::Failed
                };
                let turn = self.current_turn(id);
                self.emit_all(
                    id,
                    vec![
                        EventPayload::TurnCompleted {
                            turn,
                            outcome: subtype,
                            coarse,
                        },
                        EventPayload::SessionStatusChanged {
                            status: SessionStatus::Idle,
                            reason: None,
                        },
                    ],
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

    /// Build a Decision from a `can_use_tool` request: approvals and
    /// questions are one concept; the options are the harness's own words.
    fn build_decision(&self, id: &SessionId, request_id: &str, request: Value) -> Decision {
        let tool_name = request
            .get("tool_name")
            .and_then(|t| t.as_str())
            .unwrap_or("tool");
        let (kind, options) = if tool_name == "AskUserQuestion" {
            let mut options = Vec::new();
            if let Some(questions) = request
                .get("input")
                .and_then(|i| i.get("questions"))
                .and_then(|q| q.as_array())
            {
                if let Some(first) = questions.first() {
                    if let Some(opts) = first.get("options").and_then(|o| o.as_array()) {
                        for opt in opts {
                            if let Some(label) = opt.get("label").and_then(|l| l.as_str()) {
                                options.push(DecisionOption {
                                    id: label.to_string(),
                                    label: label.to_string(),
                                });
                            }
                        }
                    }
                }
            }
            (DecisionKind::Question, options)
        } else {
            (
                DecisionKind::Approval,
                vec![
                    DecisionOption {
                        id: "allow".into(),
                        label: "Allow".into(),
                    },
                    DecisionOption {
                        id: "deny".into(),
                        label: "Deny".into(),
                    },
                ],
            )
        };
        Decision {
            id: DecisionId::new(),
            session_id: *id,
            turn: self.current_turn(id),
            kind,
            state: DecisionState::Pending,
            harness_request: request,
            harness_ref: Some(request_id.to_string()),
            options,
            created_at: now_ms(),
            answer: None,
        }
    }
}
