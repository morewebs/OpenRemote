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
use crate::supervisor::lock_recovering;

/// Bytes per pulled range.
const RANGE_BYTES: usize = 256 * 1024;
/// Head notices for one chat go out at most this often.
const HEAD_EVERY: Duration = Duration::from_millis(1000);
/// How long one range request (and every other peer request this module makes
/// through `get`) is allowed to take. Named because `PULL_RETRY_DELAY` is
/// bounded by it, and the pull's slot wait is the same figure.
const GET_TIMEOUT: Duration = Duration::from_secs(60);
/// A pull that lost its range waits this long before its one retry: long enough
/// for a reconnect to start, and far enough below `GET_TIMEOUT` (60 s) that the
/// retried range is a fresh request inside the peer's live link rather than a
/// second wait added to a caller that is already at its limit. Raising this
/// past a fraction of `GET_TIMEOUT` turns a pull that recovers into a pull that
/// times out, which is why the test beside it pins the relation.
const PULL_RETRY_DELAY: Duration = Duration::from_secs(5);
/// How often this device compares notes with the peers that are up. This is the
/// beat a link event does not give: a tombstone this device refused (the stop
/// and the force step both failed) and a copy left behind by a cut-off pull are
/// offered again here, without waiting for a restart.
const RETRY_EVERY: Duration = Duration::from_secs(300);

/// How often the browser watcher compares its fingerprint of the synced rows,
/// and the floor between two snapshots it sends. One second notices a change
/// without a write per keystroke; two seconds between sends makes a burst of
/// edits one note, and the merge on the far side makes a repeat harmless.
const BROWSER_TICK: Duration = Duration::from_secs(1);
const BROWSER_EVERY: Duration = Duration::from_secs(2);

#[derive(Default)]
pub struct SyncState {
    /// Who has each chat this device runs open: they get its events live.
    watchers: Mutex<HashMap<SessionId, HashSet<DeviceId>>>,
    /// Chats with a pull in flight: one at a time per chat. Shared with
    /// the guard that releases a chat's slot, so a pull dropped mid-flight
    /// (a cancelled HTTP handler) still frees it.
    pulling: Arc<Mutex<HashSet<SessionId>>>,
    /// When each chat's last head notice went out, and whether a trailing
    /// one is already scheduled.
    heads: Mutex<HashMap<SessionId, (Instant, bool)>>,
    /// What arrived in the browser notes so far: the last sequence merged from
    /// each peer, so a repeated snapshot is skipped before its rows are walked.
    /// The merge itself is idempotent by stamp; this only saves the walk.
    browser: Mutex<crate::browser::sync::Received>,
}

/// A chat's slot in the pulling set, held for exactly as long as its pull
/// runs. Drop gives it back on every exit - completion, error, or
/// cancellation: the create_remote handler awaits its pull inline, and a
/// dropped connection drops that future mid-pull, which used to strand the
/// flag forever (every later pull for the chat then spun its wait out and
/// gave up, and the chat never synced again until restart).
struct PullGuard {
    set: Arc<Mutex<HashSet<SessionId>>>,
    id: SessionId,
}

impl Drop for PullGuard {
    fn drop(&mut self) {
        lock_recovering(&self.set).remove(&self.id);
    }
}

impl SyncState {
    /// Takes a chat's pull slot if no pull is running; None while one is.
    fn try_claim_pulling(&self, id: SessionId) -> Option<PullGuard> {
        lock_recovering(&self.pulling)
            .insert(id)
            .then(|| PullGuard {
                set: Arc::clone(&self.pulling),
                id,
            })
    }

    /// Takes a chat's pull slot, waiting out any pull in flight: the
    /// newcomer then catches up itself, so a caller that needs the copy (a
    /// chat just created on a machine) never returns before it exists.
    /// None: the slot stayed busy past `wait`, and the caller gives up
    /// rather than pile on.
    async fn claim_pulling(&self, id: SessionId, wait: Duration) -> Option<PullGuard> {
        let deadline = Instant::now() + wait;
        loop {
            if let Some(guard) = self.try_claim_pulling(id) {
                return Some(guard);
            }
            if Instant::now() > deadline {
                return None;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    }

    /// A chat is gone: every per-chat entry goes with it. Only the watcher
    /// entry was cleared before; the head-notice entry stayed for the
    /// daemon's lifetime, so the map grew with every chat ever deleted.
    fn forget(&self, id: SessionId) {
        lock_recovering(&self.watchers).remove(&id);
        lock_recovering(&self.heads).remove(&id);
    }
}

/// What this device holds: synced chats only (private ones never leave).
pub fn summary(app: &App) -> Value {
    let store = lock_recovering(&app.store);
    let (sessions, tombstones) = store.summaries();
    json!({
        "v": 1,
        "sessions": sessions,
        "tombstones": tombstones.iter().map(|(id, t)| json!({"id": id, "at": t.at, "by": t.by})).collect::<Vec<_>>(),
    })
}

/// A range of a synced chat's log, verbatim.
pub fn range(app: &App, id: SessionId, from: u64) -> Result<Value, (u16, &'static str)> {
    let store = lock_recovering(&app.store);
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
fn ingest(
    app: &App,
    id: SessionId,
    runner: DeviceId,
    events: Vec<Value>,
) -> Result<u64, StoreError> {
    let (fresh, head) = {
        let mut store = lock_recovering(&app.store);
        let before = store.head(&id);
        let done = store.append_replicated(id, runner, events.clone())?;
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
    lock_recovering(&app.store).head(id)
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
            GET_TIMEOUT,
        )
        .await
        .ok()?;
    (response.status == 200)
        .then(|| serde_json::from_slice(&response.body).ok())
        .flatten()
}

/// Whether a pull that just lost its range may wait and try once more. One
/// retry, then the beat: a pull that re-armed this would hold the chat's slot
/// and keep re-asking a peer that is gone, which is the shape the sleep and the
/// log line were meant to be a single occurrence of. A pure rule so the guard
/// is pinned without a network.
pub fn may_retry_pull(already_retried: bool) -> bool {
    !already_retried
}

/// Copies what `peer` has of chat `id` beyond this device's head. One pull
/// runs per chat at a time; a second one waits for it and then catches up
/// itself, so a caller that needs the copy (a chat just created on a
/// machine) never returns before it exists. The slot is a guard: if this
/// pull is dropped mid-flight, Drop releases it instead of stranding the
/// chat behind a stuck flag.
pub async fn pull(app: &Arc<App>, peer: DeviceId, id: SessionId, runner: DeviceId) {
    let Some(_slot) = app.sync.claim_pulling(id, Duration::from_secs(60)).await else {
        return;
    };
    let Some(mesh) = app.cloud.mesh() else {
        return;
    };
    let mut retried = false;
    loop {
        let from = local_head(app, &id);
        let range = get(
            &mesh,
            peer,
            format!("/sync/sessions/{id}/events?from={from}"),
        )
        .await;
        let Some(range) = range else {
            // A range that never arrived is the network (or the peer) going
            // away between two chunks, not the end of the chat: say so once and
            // try once more before leaving the copy behind for the retry beat
            // to pick up.
            if !may_retry_pull(retried) {
                break;
            }
            retried = true;
            eprintln!(
                "openremote-daemon: pulling chat {id} from {peer} was cut off after seq {from}; trying once more"
            );
            tokio::time::sleep(PULL_RETRY_DELAY).await;
            continue;
        };
        let events: Vec<Value> = range["events"].as_array().cloned().unwrap_or_default();
        let theirs = range["head"].as_u64().unwrap_or(0);
        if events.is_empty() {
            break;
        }
        match ingest(app, id, runner, events) {
            Ok(head) if head < theirs && head > from => continue,
            _ => break,
        }
    }
    // The slot goes back here, on cancellation, or on any early return
    // above: the guard's Drop is the single release path.
}

/// Deletes a synced chat here because it was deleted somewhere. A
/// private chat (no executor: never synced, invisible to peers on every
/// other path) is refused whole - no stop, no tombstone, nothing: a
/// note naming one is stale or foreign, and honouring it would destroy
/// local data on the word of a peer that could not legitimately know
/// the chat exists. This was the one peer-note path with no ownership
/// check; it now holds the same line the rest of sync does.
pub async fn apply_tombstone(app: &Arc<App>, id: SessionId, stone: Tombstone) {
    let (private_here, running_here) = {
        let store = lock_recovering(&app.store);
        match store.session(&id) {
            Ok(s) => (
                s.executor.is_none(),
                s.executor.is_some() && store.executes_here(s) && s.status.is_alive(),
            ),
            // A chat this device never held: nothing runs here, nothing
            // to refuse - the tombstone memory is the whole job.
            Err(_) => (false, false),
        }
    };
    if private_here {
        return;
    }
    if running_here {
        if let Err(reason) = stop_chat(app, &id).await {
            // Two attempts at the kill path and the child is still there. The
            // row stays: a child whose row has been deleted is the one state
            // that leaves a harness running with nothing left that could ever
            // stop it, spending on a chat nobody can see. Keeping the row also
            // keeps the delete retriable - the peer offers its tombstone on
            // every meeting and the retry beat asks every few minutes - and a
            // child that survives both attempts is a machine-level failure the
            // log shouts about rather than a quiet leak.
            eprintln!(
                "openremote-daemon: could not stop {id} on two attempts before deleting it: {reason}"
            );
            return;
        }
    }
    let _ = lock_recovering(&app.store).tombstone(id, stone);
    app.sync.forget(id);
    // What its tabs note told it goes with it too.
    app.supervisor.forget_prompt_note(&id);
    // A held live-settings change belongs to the chat; it goes with it.
    crate::supervisor::forget_live_settings(&id);
}

/// Whether a stop attempt reported the child gone. Only a stop that succeeded
/// may be followed by the tombstone; a chat that is not here at all also
/// reports an error, and keeping its row until the next attempt is the
/// harmless half of that (there is no child left to spend).
pub fn tombstone_after_stop(stop: &Result<(), String>) -> bool {
    stop.is_ok()
}

/// Asks the supervisor to kill a chat's child, twice. `Supervisor::stop` is
/// the kill path this crate can reach: it shuts the driver down and settles
/// the entry, and it is the only per-chat kill the supervisor offers from
/// outside itself. So the second call is the force step - the same mechanism,
/// tried again after the child has had a moment to die on the first - rather
/// than a new one. A chat that is not here reports that too, and there is
/// nothing to kill.
async fn stop_chat(app: &App, id: &SessionId) -> Result<(), String> {
    // Only this chat: what becomes of its workers was the user's call when
    // they deleted it.
    let first = app
        .supervisor
        .stop_only(id)
        .await
        .map(|_| ())
        .map_err(|e| e.to_string());
    if tombstone_after_stop(&first) {
        return Ok(());
    }
    tokio::time::sleep(PULL_RETRY_DELAY).await;
    app.supervisor
        .stop_only(id)
        .await
        .map(|_| ())
        .map_err(|e| e.to_string())
}

/// The beat a link event does not give. Every few minutes this device compares
/// notes with each peer that is up, so a tombstone refused because the child
/// would not die, and a copy left behind by a cut-off pull, are offered again
/// on an account where nothing restarts. Runs until the process ends with the
/// rest of the mesh work; peers that went offline are not in the list, so the
/// loop only spends on live links.
pub async fn retry_loop(app: Arc<App>) {
    loop {
        tokio::time::sleep(RETRY_EVERY).await;
        let Some(mesh) = app.cloud.mesh() else {
            continue;
        };
        for peer in mesh.peers() {
            let app = Arc::clone(&app);
            tokio::spawn(async move { sync_with(&app, peer.id).await });
        }
    }
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
        if !lock_recovering(&app.store).tombstoned(&id) {
            let at = stone["at"].as_i64().unwrap_or(0);
            apply_tombstone(app, id, Tombstone { at, by }).await;
        }
    }
    let me = mesh.me();
    let mut wanted: Vec<(i64, SessionId, DeviceId)> = Vec::new();
    for s in theirs["sessions"].as_array().into_iter().flatten() {
        let (Some(id), Some(executor), Some(head)) = (
            s["id"].as_str().and_then(SessionId::parse),
            s["executor"].as_str().and_then(DeviceId::parse),
            s["head"].as_u64(),
        ) else {
            continue;
        };
        // The runner's own log is the source; it never takes a copy.
        if executor == me || lock_recovering(&app.store).tombstoned(&id) {
            continue;
        }
        if head > local_head(app, &id) {
            wanted.push((s["updated_at"].as_i64().unwrap_or(0), id, executor));
        }
    }
    // Newest chats first, so the list fills in the order it is read.
    wanted.sort_by_key(|(updated, _, _)| std::cmp::Reverse(*updated));
    for (_, id, runner) in wanted {
        pull(app, peer, id, runner).await;
    }
    // The browser's settings and bookmarks ride the same link, one snapshot
    // each way: the peer merges only rows newer than its own, so a peer that
    // was away adopts what changed here and a peer that already holds it
    // changes nothing.
    send_browser(app, &mesh, peer).await;
}

/// One-way messages from peers.
pub fn on_note(app: &Arc<App>, peer: PeerContext, note: Value) {
    // The browser's settings and bookmarks travel as their own note, naming no
    // session: it is read before the session parse below, which is where every
    // chat note proves what it is about. Any device of the account may send
    // this one and every device takes it - docs/cloud-trust-model.md has the
    // row, beside the tombstone a device can also send - and the merge is per
    // key by stamp, so a repeat or a stale snapshot changes nothing.
    if crate::browser::sync::is_browser_note(&note) {
        let from = peer.device_id.to_string();
        let mut store = lock_recovering(&app.browser.store);
        let mut received = lock_recovering(&app.sync.browser);
        // A refusal leaves the store as it was: the ceilings in
        // browser/sync.rs are checked before any row is written. The peer is
        // told nothing, because a note is one-way and a reply would be a second
        // protocol, but this device's own log says why the note was ignored,
        // since a refusal that leaves no trace anywhere cannot be diagnosed.
        if let Err(refused) = received.merge(&mut store, &from, &note) {
            eprintln!(
                "warning: a browser note from {from} was refused: {}",
                refused.reason()
            );
        }
        return;
    }
    let Some(id) = note["session"].as_str().and_then(SessionId::parse) else {
        return;
    };
    match note["type"].as_str() {
        Some("sync.watch") => {
            let runs_here = {
                let store = lock_recovering(&app.store);
                store
                    .session(&id)
                    .is_ok_and(|s| s.executor.is_some() && store.executes_here(s))
            };
            if !runs_here {
                return;
            }
            let mut watchers = lock_recovering(&app.sync.watchers);
            let set = watchers.entry(id).or_default();
            if note["on"].as_bool().unwrap_or(false) {
                set.insert(peer.device_id);
            } else {
                set.remove(&peer.device_id);
            }
        }
        Some("sync.events") => {
            let events = note["events"].as_array().cloned().unwrap_or_default();
            // Live events only ever come from the chat's runner.
            let runner = peer.device_id;
            if let Err(StoreError::Gap { .. }) = ingest(app, id, runner, events) {
                let app = Arc::clone(app);
                tokio::spawn(async move { pull(&app, runner, id, runner).await });
            }
        }
        Some("sync.head") => {
            let behind = note["head"]
                .as_u64()
                .is_some_and(|h| h > local_head(app, &id));
            let deleted = lock_recovering(&app.store).tombstoned(&id);
            // Head notices too: only the runner sends them.
            if behind && !deleted {
                let app = Arc::clone(app);
                let runner = peer.device_id;
                tokio::spawn(async move { pull(&app, runner, id, runner).await });
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
        let store = lock_recovering(&app.store);
        let s = store.session(&session).ok()?;
        if store.executes_here(s) {
            return None;
        }
        s.executor?
    };
    let mesh = app.cloud.mesh()?;
    let on = json!({"type": "sync.watch", "session": session, "on": true});
    let _ = mesh.note(executor, &on).await;
    pull(app, executor, session, executor).await;
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

/// This device's browser snapshot, ready to send: the settings and the
/// bookmark tree with the stamp that made each row, deletions included. A
/// snapshot rather than a delta, so a peer that missed a note - a link that
/// dropped, a device that was off - catches up from the next one, and a
/// repeat is a no-op because the merge takes only rows newer than its own.
fn browser_note(app: &App) -> Value {
    let mut store = lock_recovering(&app.browser.store);
    let seq = crate::browser::sync::next_seq(&mut store);
    crate::browser::sync::note(&store, seq)
}

/// A cheap summary of the synced rows, so the watcher can tell one tick from
/// another. Only the two collections a note carries count: the sequence
/// counter sits in its own, so sending a note does not look like a change and
/// cannot make the watcher send again.
fn browser_fingerprint(app: &App) -> u64 {
    let store = lock_recovering(&app.browser.store);
    let mut hash: u64 = 0;
    for col in ["setting", "bookmark"] {
        for (key, at, _) in store.rows(col) {
            hash = hash.wrapping_mul(1099511628211).wrapping_add(at.ms as u64);
            hash = hash
                .wrapping_mul(1099511628211)
                .wrapping_add(u64::from(at.n));
            hash = hash
                .wrapping_mul(1099511628211)
                .wrapping_add(at.dev.len() as u64);
            for byte in key.bytes() {
                hash = hash
                    .wrapping_mul(1099511628211)
                    .wrapping_add(u64::from(byte));
            }
        }
    }
    hash
}

/// Sends the browser snapshot to one peer. The peer merges it per key, so this
/// is safe to repeat, and callers do not track whether one arrived.
async fn send_browser(app: &Arc<App>, mesh: &Mesh, peer: DeviceId) {
    let note = browser_note(app);
    let _ = mesh.note(peer, &note).await;
}

/// The chats a lag must reconcile: every entry in the watcher map. An
/// entry can hold an empty set by now (its last watcher left), but the
/// chat's copies still missed their head notices, so the entry still
/// counts.
fn lagged_chats(watchers: &HashMap<SessionId, HashSet<DeviceId>>) -> Vec<SessionId> {
    watchers.keys().copied().collect()
}

/// Who the rate-limited head notice may skip: the watchers that already
/// got the event live. A watcher whose send failed joins the audience
/// instead - its direct retry may have died with the same link.
fn head_skips(watchers: &[DeviceId], failed: &[DeviceId]) -> Vec<DeviceId> {
    watchers
        .iter()
        .copied()
        .filter(|peer| !failed.contains(peer))
        .collect()
}

/// The runner's side: every event of a synced chat it runs goes live to the
/// chat's watchers, and its new head to everyone else, at most once a
/// second (a trailing notice makes sure the last one goes out). It also
/// carries the browser snapshot, whose store has no event stream of its own.
pub fn start_pusher(app: &Arc<App>) {
    // The browser's settings and bookmarks are written from the console's
    // routes and from the browser's own calls, neither of which is a channel
    // this module can subscribe to. A fingerprint of the synced rows compared
    // once a second notices a change; the send is debounced, so a burst of
    // edits is one note. Same shape as the chat announcements: the peers
    // online now hear it, the rest learn it from the next summary, and a
    // device a version behind ignores the type.
    let watcher = Arc::downgrade(app);
    tokio::spawn(async move {
        let mut sent: Option<u64> = None;
        let mut at = Instant::now();
        loop {
            tokio::time::sleep(BROWSER_TICK).await;
            let Some(app) = watcher.upgrade() else {
                return;
            };
            let now = browser_fingerprint(&app);
            if sent == Some(now) || at.elapsed() < BROWSER_EVERY {
                continue;
            }
            let Some(mesh) = app.cloud.mesh() else {
                continue;
            };
            let peers = mesh.online();
            if peers.is_empty() {
                continue;
            }
            let note = browser_note(&app);
            for peer in peers {
                let _ = mesh.note(peer, &note).await;
            }
            sent = Some(now);
            at = Instant::now();
        }
    });
    let mut events = app.supervisor.subscribe();
    let weak = Arc::downgrade(app);
    tokio::spawn(async move {
        loop {
            let raw = match events.recv().await {
                Ok(raw) => raw,
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                    // The loop stalled (a wedged watcher link holds every
                    // send hostage through its link retries) and the
                    // channel overwrote events. Which chats they named is
                    // unknowable, but the loss only bites chats with
                    // watchers: everyone else is on the pull path, and
                    // watchers are exactly whom head notices skip. Every
                    // watched chat gets one now - a watcher pulls only if
                    // behind, so an up-to-date one does nothing with it.
                    let Some(app) = weak.upgrade() else {
                        return;
                    };
                    if let Some(mesh) = app.cloud.mesh() {
                        let behind = lagged_chats(&lock_recovering(&app.sync.watchers));
                        for id in behind {
                            send_head(&app, &mesh, id, Vec::new()).await;
                        }
                    }
                    continue;
                }
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
                let store = lock_recovering(&app.store);
                store
                    .session(&id)
                    .is_ok_and(|s| s.executor.is_some() && store.executes_here(s))
            };
            let Some(mesh) = app.cloud.mesh().filter(|_| runs_here) else {
                continue;
            };
            let watchers: Vec<DeviceId> = lock_recovering(&app.sync.watchers)
                .get(&id)
                .map(|w| w.iter().copied().collect())
                .unwrap_or_default();
            let note = json!({"type": "sync.events", "session": id, "events": [event]});
            let mut failed: Vec<DeviceId> = Vec::new();
            for peer in &watchers {
                if mesh.note(*peer, &note).await.is_err() {
                    // Too big for one message, or the link dropped. Two
                    // nudges follow: a head note right back through this
                    // (possibly dying) link, and a seat in the audience of
                    // the rate-limited notice below, which still reaches
                    // the watcher when the direct retry dies with the
                    // same link. A watcher that is behind pulls; one that
                    // isn't does nothing.
                    let head =
                        json!({"type": "sync.head", "session": id, "head": local_head(&app, &id)});
                    let _ = mesh.note(*peer, &head).await;
                    failed.push(*peer);
                }
            }
            send_head(&app, &mesh, id, head_skips(&watchers, &failed)).await;
        }
    });
}

async fn send_head(app: &Arc<App>, mesh: &Arc<Mesh>, id: SessionId, skip: Vec<DeviceId>) {
    let due = {
        let mut heads = lock_recovering(&app.sync.heads);
        let entry = heads
            .entry(id)
            .or_insert((Instant::now() - HEAD_EVERY, false));
        if entry.0.elapsed() >= HEAD_EVERY {
            entry.0 = Instant::now();
            true
        } else if !entry.1 {
            entry.1 = true;
            let (app, mesh) = (Arc::clone(app), Arc::clone(mesh));
            tokio::spawn(async move {
                tokio::time::sleep(HEAD_EVERY).await;
                if let Some(e) = lock_recovering(&app.sync.heads).get_mut(&id) {
                    e.1 = false;
                    e.0 = Instant::now();
                }
                // The trailing notice carries no skip list: by the time it
                // fires, the skipped watchers may be exactly who needs it.
                // A lag recovery that arrived inside the rate window rides
                // this pending trailing out, so it must reach watchers too.
                notify_head(&app, &mesh, id, &[]).await;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_failed_stop_never_deletes_the_row() {
        // The pair D1 exists for: a deleted row with a live child is a harness
        // spending with nothing left that could stop it. Only a stop that
        // reported success may be followed by the tombstone.
        assert!(tombstone_after_stop(&Ok(())));
        assert!(!tombstone_after_stop(&Err(
            "the harness did not exit".to_string()
        )));
    }

    #[test]
    fn the_delete_path_asks_for_the_kill_and_keeps_the_result() {
        // Wiring pin, source text. Nothing in this crate can make `stop` fail on
        // demand, so an e2e that drives a tombstone through a failing stop does
        // not exist yet; what can be pinned is the call site, because a
        // regression that put `let _ = app.supervisor.stop(&id)` back would
        // leave every other test in the tree green: the rule helper is still
        // tested and the store is still correct, and only the wiring lies.
        let src = include_str!("sync.rs");
        let body = src
            .split("pub async fn apply_tombstone")
            .nth(1)
            .expect("apply_tombstone is in this file")
            .split("pub fn tombstone_after_stop")
            .next()
            .expect("the function has a body");
        assert!(
            body.contains("stop_chat(app, &id).await"),
            "the delete path must ask for the kill through stop_chat"
        );
        assert!(
            !body.contains("let _ = app.supervisor.stop"),
            "and must not discard the result again"
        );
        assert!(
            body.contains("return;"),
            "a child that survived both attempts must keep the row"
        );
    }

    #[test]
    fn the_retry_beat_is_no_slower_than_the_refresher() {
        // The beat exists because a link event is not guaranteed on a pair that
        // simply stays up. Five minutes keeps it in step with the mesh's
        // device-list refresher; a longer one would leave a refused delete
        // waiting for a restart that may never come.
        assert!(RETRY_EVERY <= Duration::from_secs(300));
    }

    #[test]
    fn a_lost_range_gets_one_more_try_and_then_the_beat() {
        // The guard the sleep sits behind: one retry, never a loop. Without
        // this pin, turning `retried` into a loop or a reset would look like a
        // resilience improvement and would hold the chat's slot forever.
        assert!(may_retry_pull(false), "the first loss may retry");
        assert!(!may_retry_pull(true), "the second loss waits for the beat");
    }

    #[test]
    fn the_pull_retry_delay_stays_inside_the_request_timeout() {
        // A raise past the range timeout would make "retry once" mean "add a
        // second wait to a caller already at its limit", which is a silent
        // timeout rather than a recovery. The relation is the reason the two
        // constants sit next to each other.
        assert!(
            PULL_RETRY_DELAY * 4 <= GET_TIMEOUT,
            "the retry sleep must stay well under GET_TIMEOUT"
        );
    }

    #[test]
    fn a_pull_slot_guard_releases_on_any_drop() {
        let state = SyncState::default();
        let id = SessionId::new();
        let guard = state.try_claim_pulling(id).expect("the slot is free");
        assert!(lock_recovering(&state.pulling).contains(&id));
        // A pull dropped mid-flight is exactly this drop: cancellation
        // runs Drop the same way a normal return does.
        drop(guard);
        assert!(lock_recovering(&state.pulling).is_empty());
    }

    #[tokio::test]
    async fn a_second_pull_waits_out_a_running_one_and_takes_over_after() {
        let state = SyncState::default();
        let id = SessionId::new();
        let first = state.try_claim_pulling(id).expect("the slot is free");
        // While the first pull runs, a second cannot have the slot and
        // gives up after its wait rather than pile on.
        assert!(
            state
                .claim_pulling(id, Duration::from_millis(60))
                .await
                .is_none()
        );
        drop(first);
        // The first finished: the slot is free again, taken at once.
        let second = state
            .claim_pulling(id, Duration::from_millis(0))
            .await
            .expect("the freed slot is claimed immediately");
        drop(second);
        assert!(lock_recovering(&state.pulling).is_empty());
    }

    #[tokio::test]
    async fn a_cancelled_pull_frees_its_slot() {
        // The scenario the guard exists for: create_remote awaits its
        // pull inline, and a dropped connection drops that future at its
        // await point, mid-flight. The slot must come back with it, or
        // every later pull for the chat spins its wait out and the chat
        // never syncs again until restart.
        let state = Arc::new(SyncState::default());
        let id = SessionId::new();
        let held = Arc::clone(&state);
        let pull = tokio::spawn(async move {
            let _slot = held.claim_pulling(id, Duration::from_secs(60)).await;
            std::future::pending::<()>().await;
        });
        let deadline = Instant::now() + Duration::from_secs(5);
        while !lock_recovering(&state.pulling).contains(&id) {
            assert!(Instant::now() < deadline, "the pull takes its slot");
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        pull.abort();
        let _ = pull.await;
        assert!(
            lock_recovering(&state.pulling).is_empty(),
            "the cancelled pull releases its slot"
        );
    }

    #[test]
    fn forgetting_a_chat_evicts_its_watcher_and_head_notice_entries() {
        let state = SyncState::default();
        let id = SessionId::new();
        lock_recovering(&state.watchers).insert(id, HashSet::from([DeviceId::new()]));
        lock_recovering(&state.heads).insert(id, (Instant::now(), false));
        state.forget(id);
        assert!(lock_recovering(&state.watchers).is_empty());
        assert!(lock_recovering(&state.heads).is_empty());
    }

    #[test]
    fn a_lag_reconciles_every_watched_chat_even_a_newly_unwatched_one() {
        let mut watchers = HashMap::new();
        let watched = SessionId::new();
        let just_left = SessionId::new();
        watchers.insert(watched, HashSet::from([DeviceId::new()]));
        // The last watcher left: the entry stays until the chat is
        // deleted, and its copies still missed their head notices.
        watchers.insert(just_left, HashSet::new());
        let chats = lagged_chats(&watchers);
        assert_eq!(chats.len(), 2, "both entries are reconciled");
        assert!(chats.contains(&watched) && chats.contains(&just_left));
    }

    #[test]
    fn a_failed_watcher_send_joins_the_head_notice_audience() {
        let a = DeviceId::new();
        let b = DeviceId::new();
        // a's live send landed, b's failed: only a may be skipped, so b's
        // copy still gets the rate-limited notice below.
        assert_eq!(head_skips(&[a, b], &[b]), vec![a]);
        // No failures: the usual skip, nobody gets notified twice.
        assert_eq!(head_skips(&[a, b], &[]), vec![a, b]);
    }

    /// A real app over a throwaway store, no harnesses: enough to drive
    /// `apply_tombstone` against store-built chats. The engines tick in
    /// the background and hold the directory open, so the directory
    /// outlives the run as junk in temp (no cleanup to race).
    fn test_app(dir: &std::path::Path) -> Arc<App> {
        let store = Arc::new(Mutex::new(
            openremote_core::Store::open(dir.to_path_buf()).expect("store opens"),
        ));
        let supervisor = crate::supervisor::Supervisor::new(
            store.clone(),
            crate::registry::HarnessRegistry::none(),
        );
        crate::app::App::assemble(
            crate::app::AppOptions {
                data_dir: dir.to_path_buf(),
                token: "test-token".into(),
                cloud: openremote_cloud::CloudConfig {
                    api_base: "http://127.0.0.1:9/v1/openremote".into(),
                    auth_issuer: "http://127.0.0.1:9/v1/auth".into(),
                    client_id: openremote_cloud::config::CLIENT_ID.into(),
                    device_name: "test-device".into(),
                    platform: "test".into(),
                    redirect_uri: None,
                },
            },
            store,
            supervisor,
        )
    }

    /// A `sync.tombstone` note naming a private chat is refused whole
    /// - no stop, no tombstone, no data loss - while a synced chat's
    /// tombstone still applies. Every other peer path keeps private
    /// chats invisible; this was the one that could destroy one's data
    /// on the word of a peer that could not legitimately know the chat
    /// exists.
    #[tokio::test]
    async fn a_tombstone_naming_a_private_chat_is_refused_and_its_data_survives() {
        let dir = std::env::temp_dir().join(format!("or-tombstone-{}", uuid::Uuid::new_v4()));
        let app = test_app(&dir);
        let session = |executor: Option<DeviceId>| openremote_core::Session {
            id: SessionId::new(),
            harness: "claude".into(),
            workspace: None,
            status: openremote_core::SessionStatus::Starting,
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
            executor,
            title: None,
            queue: Vec::new(),
            queue_paused: false,
            mentioned_chats: Vec::new(),
            carry_transcript: None,
            parent: None,
        };
        let private = session(None);
        let mut synced = session(Some(DeviceId::new()));
        synced.id = SessionId::new();
        let user_turn = openremote_core::EventPayload::MessageAdded {
            message: openremote_core::ChatMessage {
                id: "m1".into(),
                turn: 1,
                role: openremote_core::MessageRole::User,
                text: "private words".into(),
                origin: None,
                images: Vec::new(),
            },
        };
        {
            let mut store = lock_recovering(&app.store);
            for s in [&private, &synced] {
                store
                    .append(
                        s.id,
                        openremote_core::EventPayload::SessionCreated { session: s.clone() },
                    )
                    .unwrap();
            }
            store.append(private.id, user_turn).unwrap();
        }

        // The note arrives for both; only the synced one goes.
        apply_tombstone(
            &app,
            private.id,
            Tombstone {
                at: 1,
                by: DeviceId::new(),
            },
        )
        .await;
        apply_tombstone(
            &app,
            synced.id,
            Tombstone {
                at: 1,
                by: DeviceId::new(),
            },
        )
        .await;

        {
            let store = lock_recovering(&app.store);
            assert!(
                store.session(&private.id).is_ok(),
                "the private chat survives the note"
            );
            assert!(
                !store.tombstoned(&private.id),
                "no tombstone was recorded against it"
            );
            assert_eq!(store.head(&private.id), 2, "its events all survive");
            let (lines, _) = store.raw_range(&private.id, 0, 64 * 1024).unwrap();
            assert_eq!(
                lines.len(),
                2,
                "the log holds both events, verbatim - the data is intact"
            );
            assert!(
                store.session(&synced.id).is_err(),
                "the control: a synced chat's tombstone still applies"
            );
            assert!(store.tombstoned(&synced.id));
        }
    }
}
