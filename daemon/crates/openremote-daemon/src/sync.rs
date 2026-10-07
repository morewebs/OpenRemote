//! Keeping a copy of every synced chat on every signed-in device, so a chat
//! is readable anywhere and laptops that are never online together still
//! meet through an always-on machine. moreweb stores none of it.
//!
//! One device runs a chat; its log only grows there, and every other copy is
//! a prefix of it, so syncing is copying ranges:
//! - when two devices find each other they compare summaries (each chat's
//!   head and runner, and deletions) and pull what they lack, from any peer
//!   that has it;
//! - the runner pushes new events at once to devices that have the chat open
//!   (`sync.watch`), and tells the others where the head is (`sync.head`) so
//!   they pull;
//! - deleting a synced chat leaves a tombstone that every device honours.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use openremote_cloud::rpc::Request;
use openremote_cloud::{Mesh, PeerContext};
use openremote_core::store::{StoreError, Tombstone};
use openremote_core::{DeviceId, SessionId};
use serde_json::{Value, json};

use crate::app::App;

/// Bytes per pulled range.
const RANGE_BYTES: usize = 256 * 1024;
/// Head notices for one chat go out at most this often.
const HEAD_EVERY: Duration = Duration::from_millis(1000);

#[derive(Default)]
pub struct SyncState {
    /// Who has each chat this device runs open: they get its events live.
    watchers: Mutex<HashMap<SessionId, HashSet<DeviceId>>>,
    /// Chats with a pull in flight: one at a time per chat.
    pulling: Mutex<HashSet<SessionId>>,
    /// When each chat's last head notice went out, and whether a trailing
    /// one is already scheduled.
    heads: Mutex<HashMap<SessionId, (Instant, bool)>>,
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// What this device holds: synced chats only (private ones never leave).
pub fn summary(app: &App) -> Value {
    let store = lock(&app.store);
    let (sessions, tombstones) = store.summaries();
    json!({
        "v": 1,
        "sessions": sessions,
        "tombstones": tombstones.iter().map(|(id, t)| json!({"id": id, "at": t.at, "by": t.by})).collect::<Vec<_>>(),
    })
}

/// A range of a synced chat's log, verbatim.
pub fn range(app: &App, id: SessionId, from: u64) -> Result<Value, (u16, &'static str)> {
    let store = lock(&app.store);
    if store.tombstoned(&id) {
        return Err((410, "this chat was deleted"));
    }
    match store.session(&id) {
        Ok(s) if s.executor.is_some() => {}
        // A private chat is never handed out, and nobody learns it exists.
        _ => return Err((404, "no such chat")),
    }
    let (lines, head) = store
        .raw_range(&id, from, RANGE_BYTES)
        .map_err(|_| (404, "no such chat"))?;
    let events: Vec<Value> = lines
        .iter()
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect();
    Ok(json!({"events": events, "head": head}))
}

/// Takes events from a peer into this device's copy, and tells local
/// listeners (the console's open chat) about the new ones.
fn ingest(app: &App, id: SessionId, events: Vec<Value>) -> Result<u64, StoreError> {
    let (fresh, head) = {
        let mut store = lock(&app.store);
        let before = store.head(&id);
        let done = store.append_replicated(id, events.clone())?;
        let fresh: Vec<String> = events
            .iter()
            .filter(|e| {
                e["seq"]
                    .as_u64()
                    .is_some_and(|s| s >= before && s < done.head)
            })
            .map(Value::to_string)
            .collect();
        (fresh, done.head)
    };
    for raw in fresh {
        app.supervisor.announce(raw);
    }
    Ok(head)
}

fn local_head(app: &App, id: &SessionId) -> u64 {
    lock(&app.store).head(id)
}

async fn get(mesh: &Mesh, peer: DeviceId, path: String) -> Option<Value> {
    let response = mesh
        .request(
            peer,
            Request {
                method: "GET".into(),
                path,
                content_type: None,
                body: Vec::new(),
            },
            Duration::from_secs(60),
        )
        .await
        .ok()?;
    (response.status == 200)
        .then(|| serde_json::from_slice(&response.body).ok())
        .flatten()
}

/// Copies what `peer` has of chat `id` beyond this device's head. One pull
/// runs per chat at a time; a second one waits for it and then catches up
/// itself, so a caller that needs the copy (a chat just created on a
/// machine) never returns before it exists.
pub async fn pull(app: &Arc<App>, peer: DeviceId, id: SessionId) {
    let deadline = Instant::now() + Duration::from_secs(60);
    while !lock(&app.sync.pulling).insert(id) {
        if Instant::now() > deadline {
            return;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    let Some(mesh) = app.cloud.mesh() else {
        lock(&app.sync.pulling).remove(&id);
        return;
    };
    loop {
        let from = local_head(app, &id);
        let Some(range) = get(
            &mesh,
            peer,
            format!("/sync/sessions/{id}/events?from={from}"),
        )
        .await
        else {
            break;
        };
        let events: Vec<Value> = range["events"].as_array().cloned().unwrap_or_default();
        let theirs = range["head"].as_u64().unwrap_or(0);
        if events.is_empty() {
            break;
        }
        match ingest(app, id, events) {
            Ok(head) if head < theirs && head > from => continue,
            _ => break,
        }
    }
    lock(&app.sync.pulling).remove(&id);
}

/// Deletes a synced chat here because it was deleted somewhere.
pub async fn apply_tombstone(app: &Arc<App>, id: SessionId, stone: Tombstone) {
    let running_here = {
        let store = lock(&app.store);
        store
            .session(&id)
            .is_ok_and(|s| s.executor.is_some() && store.executes_here(s) && s.status.is_alive())
    };
    if running_here {
        let _ = app.supervisor.stop(&id).await;
    }
    let _ = lock(&app.store).tombstone(id, stone);
    lock(&app.sync.watchers).remove(&id);
}

/// Two devices found each other: trade deletions, then pull what's missing.
pub async fn sync_with(app: &Arc<App>, peer: DeviceId) {
    let Some(mesh) = app.cloud.mesh() else {
        return;
    };
    let Some(theirs) = get(&mesh, peer, "/sync/summary".into()).await else {
        return;
    };
    for stone in theirs["tombstones"].as_array().into_iter().flatten() {
        let (Some(id), Some(by)) = (
            stone["id"].as_str().and_then(SessionId::parse),
            stone["by"].as_str().and_then(DeviceId::parse),
        ) else {
            continue;
        };
        if !lock(&app.store).tombstoned(&id) {
            let at = stone["at"].as_i64().unwrap_or(0);
            apply_tombstone(app, id, Tombstone { at, by }).await;
        }
    }
    let me = mesh.me();
    let mut wanted: Vec<(i64, SessionId)> = Vec::new();
    for s in theirs["sessions"].as_array().into_iter().flatten() {
        let (Some(id), Some(executor), Some(head)) = (
            s["id"].as_str().and_then(SessionId::parse),
            s["executor"].as_str().and_then(DeviceId::parse),
            s["head"].as_u64(),
        ) else {
            continue;
        };
        // The runner's own log is the source; it never takes a copy.
        if executor == me || lock(&app.store).tombstoned(&id) {
            continue;
        }
        if head > local_head(app, &id) {
            wanted.push((s["updated_at"].as_i64().unwrap_or(0), id));
        }
    }
    // Newest chats first, so the list fills in the order it is read.
    wanted.sort_by_key(|(updated, _)| std::cmp::Reverse(*updated));
    for (_, id) in wanted {
        pull(app, peer, id).await;
    }
}

/// One-way messages from peers.
pub fn on_note(app: &Arc<App>, peer: PeerContext, note: Value) {
    let Some(id) = note["session"].as_str().and_then(SessionId::parse) else {
        return;
    };
    match note["type"].as_str() {
        Some("sync.watch") => {
            let runs_here = {
                let store = lock(&app.store);
                store
                    .session(&id)
                    .is_ok_and(|s| s.executor.is_some() && store.executes_here(s))
            };
            if !runs_here {
                return;
            }
            let mut watchers = lock(&app.sync.watchers);
            let set = watchers.entry(id).or_default();
            if note["on"].as_bool().unwrap_or(false) {
                set.insert(peer.device_id);
            } else {
                set.remove(&peer.device_id);
            }
        }
        Some("sync.events") => {
            let events = note["events"].as_array().cloned().unwrap_or_default();
            if let Err(StoreError::Gap { .. }) = ingest(app, id, events) {
                let app = Arc::clone(app);
                tokio::spawn(async move { pull(&app, peer.device_id, id).await });
            }
        }
        Some("sync.head") => {
            let behind = note["head"]
                .as_u64()
                .is_some_and(|h| h > local_head(app, &id));
            let deleted = lock(&app.store).tombstoned(&id);
            if behind && !deleted {
                let app = Arc::clone(app);
                tokio::spawn(async move { pull(&app, peer.device_id, id).await });
            }
        }
        Some("sync.tombstone") => {
            let at = note["at"].as_i64().unwrap_or(0);
            let by = note["by"]
                .as_str()
                .and_then(DeviceId::parse)
                .unwrap_or(peer.device_id);
            let app = Arc::clone(app);
            tokio::spawn(async move { apply_tombstone(&app, id, Tombstone { at, by }).await });
        }
        _ => {}
    }
}

/// While the console has a copy open, its runner sends the events live.
pub struct Watch {
    app: Arc<App>,
    executor: DeviceId,
    session: SessionId,
}

impl Drop for Watch {
    fn drop(&mut self) {
        let (app, executor, session) = (Arc::clone(&self.app), self.executor, self.session);
        tokio::spawn(async move {
            if let Some(mesh) = app.cloud.mesh() {
                let off = json!({"type": "sync.watch", "session": session, "on": false});
                let _ = mesh.note(executor, &off).await;
            }
        });
    }
}

/// Starts watching a copy (catching up first). None for chats run here.
pub async fn watch(app: &Arc<App>, session: SessionId) -> Option<Watch> {
    let executor = {
        let store = lock(&app.store);
        let s = store.session(&session).ok()?;
        if store.executes_here(s) {
            return None;
        }
        s.executor?
    };
    let mesh = app.cloud.mesh()?;
    let on = json!({"type": "sync.watch", "session": session, "on": true});
    let _ = mesh.note(executor, &on).await;
    pull(app, executor, session).await;
    Some(Watch {
        app: Arc::clone(app),
        executor,
        session,
    })
}

/// Sends a deletion to every device online now; the rest learn it from the
/// next summary.
pub async fn announce_tombstone(app: &Arc<App>, id: SessionId, stone: Tombstone) {
    let Some(mesh) = app.cloud.mesh() else {
        return;
    };
    let note = json!({"type": "sync.tombstone", "session": id, "at": stone.at, "by": stone.by});
    for peer in mesh.online() {
        let _ = mesh.note(peer, &note).await;
    }
}

/// The runner's side: every event of a synced chat it runs goes live to the
/// chat's watchers, and its new head to everyone else, at most once a
/// second (a trailing notice makes sure the last one goes out).
pub fn start_pusher(app: &Arc<App>) {
    let mut events = app.supervisor.subscribe();
    let weak = Arc::downgrade(app);
    tokio::spawn(async move {
        loop {
            let raw = match events.recv().await {
                Ok(raw) => raw,
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(_) => return,
            };
            let Some(app) = weak.upgrade() else {
                return;
            };
            let Ok(event) = serde_json::from_str::<Value>(&raw) else {
                continue;
            };
            let Some(id) = event["session_id"].as_str().and_then(SessionId::parse) else {
                continue;
            };
            let runs_here = {
                let store = lock(&app.store);
                store
                    .session(&id)
                    .is_ok_and(|s| s.executor.is_some() && store.executes_here(s))
            };
            let Some(mesh) = app.cloud.mesh().filter(|_| runs_here) else {
                continue;
            };
            let watchers: Vec<DeviceId> = lock(&app.sync.watchers)
                .get(&id)
                .map(|w| w.iter().copied().collect())
                .unwrap_or_default();
            let note = json!({"type": "sync.events", "session": id, "events": [event]});
            for peer in &watchers {
                if mesh.note(*peer, &note).await.is_err() {
                    // Too big for one message, or the link dropped: the
                    // head notice below makes the watcher pull instead.
                    let head =
                        json!({"type": "sync.head", "session": id, "head": local_head(&app, &id)});
                    let _ = mesh.note(*peer, &head).await;
                }
            }
            send_head(&app, &mesh, id, watchers).await;
        }
    });
}

async fn send_head(app: &Arc<App>, mesh: &Arc<Mesh>, id: SessionId, skip: Vec<DeviceId>) {
    let due = {
        let mut heads = lock(&app.sync.heads);
        let entry = heads
            .entry(id)
            .or_insert((Instant::now() - HEAD_EVERY, false));
        if entry.0.elapsed() >= HEAD_EVERY {
            entry.0 = Instant::now();
            true
        } else if !entry.1 {
            entry.1 = true;
            let (app, mesh, skip) = (Arc::clone(app), Arc::clone(mesh), skip.clone());
            tokio::spawn(async move {
                tokio::time::sleep(HEAD_EVERY).await;
                if let Some(e) = lock(&app.sync.heads).get_mut(&id) {
                    e.1 = false;
                    e.0 = Instant::now();
                }
                notify_head(&app, &mesh, id, &skip).await;
            });
            false
        } else {
            false
        }
    };
    if due {
        notify_head(app, mesh, id, &skip).await;
    }
}

async fn notify_head(app: &Arc<App>, mesh: &Arc<Mesh>, id: SessionId, skip: &[DeviceId]) {
    let note = json!({"type": "sync.head", "session": id, "head": local_head(app, &id)});
    for peer in mesh.online() {
        if !skip.contains(&peer) {
            let _ = mesh.note(peer, &note).await;
        }
    }
}
