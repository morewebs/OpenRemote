//! The mesh: this device's links to the account's other devices, over the
//! relay. Peers and their public keys come from the registry; the first key
//! seen for each device is pinned, and a device whose key later changes is
//! never linked again. Links come up on demand and drop whenever a frame may
//! have been lost (the peer went offline, the relay reconnected, a message
//! failed to decrypt); the next request simply links again.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use futures_util::future::BoxFuture;
use futures_util::{SinkExt, StreamExt};
use openremote_core::DeviceId;
use serde::Serialize;
use serde_json::{Value, json};
use tokio::sync::{Notify, mpsc, oneshot};
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;

use crate::link::{self, HS1, HS2, Link, MSG, NUDGE};
use crate::rpc::{self, Request, Response};
use crate::{Cloud, Phase};

/// Who is asking, as far as the mesh can vouch: a device of this account.
#[derive(Clone, Debug)]
pub struct PeerContext {
    pub device_id: DeviceId,
    pub name: String,
    pub kind: String,
}

/// What the daemon does with traffic from the user's other devices.
pub trait CloudHost: Send + Sync + 'static {
    /// Serve one request from a peer (the daemon gates what peers may do).
    fn dispatch(&self, peer: PeerContext, request: Request) -> BoxFuture<'static, Response>;
    /// A one-way message from a peer.
    fn note(&self, peer: PeerContext, note: Value);
    /// A link to `peer` just came up.
    fn linked(&self, peer: DeviceId);
    /// `peer` is online (it just came online, or was when this device
    /// connected).
    fn online(&self, peer: DeviceId);
    /// This device was removed from the account.
    fn revoked(&self);
}

#[derive(Debug, thiserror::Error)]
pub enum MeshError {
    #[error("that device isn't one of yours")]
    Unknown,
    #[error("{0} is offline")]
    Offline(String),
    #[error("{0} doesn't answer")]
    Unreachable(String),
    #[error("{0}'s key changed - remove it and add it again")]
    KeyChanged(String),
    #[error("{0} took too long to answer")]
    Timeout(String),
    #[error("the link to {0} dropped")]
    Reset(String),
    #[error("the request is too large")]
    TooLarge,
    #[error("not connected to OpenRemote Cloud")]
    NotConnected,
}

#[derive(Clone, Debug, Serialize)]
pub struct Peer {
    pub id: DeviceId,
    pub name: String,
    pub platform: String,
    pub kind: String,
    pub online: bool,
    pub app_version: Option<String>,
    pub last_seen_at: Option<f64>,
    pub created_via: Option<String>,
    pub auto_update: bool,
    pub auto_update_override: Option<bool>,
    /// `key_changed` when the registry's key no longer matches the pin.
    pub problem: Option<&'static str>,
    #[serde(skip)]
    pubkey: Vec<u8>,
}

struct Call {
    head: Option<Response>,
    body: Vec<u8>,
    done: Option<oneshot::Sender<Result<Response, MeshError>>>,
}

/// One inbound stream: the request so far, and when its slot in the
/// per-peer window is reclaimed if the END never comes.
struct Inbound {
    request: Request,
    deadline: Instant,
    /// The END arrived and the work is running: the slot is held until the
    /// response is sealed, so the window counts work in flight rather than
    /// half-written requests.
    dispatched: bool,
    /// Which entry this is. The TTL sweep can free a stream whose work is
    /// still running, and the peer may then take that id again, so the
    /// finishing task must know its own entry from the next request's.
    generation: u64,
}

#[derive(Default)]
struct State {
    peers: HashMap<DeviceId, Peer>,
    links: HashMap<DeviceId, Link>,
    waiters: HashMap<DeviceId, Vec<oneshot::Sender<()>>>,
    calls: HashMap<(DeviceId, u32), Call>,
    inbound: HashMap<(DeviceId, u32), Inbound>,
    /// A link attempt is in flight to this peer until the deadline: what
    /// keeps a nudging device from spawning a task per nudge frame.
    attempts: HashMap<DeviceId, Instant>,
    next_stream: u32,
    next_generation: u64,
    auto_update: bool,
    online: std::collections::HashSet<DeviceId>,
    /// Online, by the relay's word, before the device list named them. The
    /// host was told they are online when this device could not yet reach
    /// them (no key to link with), so its compare failed; it is told again
    /// when the list catches up. Without this, a device that signs in while
    /// the list is still loading meets its peers only on the next periodic
    /// compare, minutes later.
    unlisted_online: std::collections::HashSet<DeviceId>,
}

pub struct Mesh {
    me: DeviceId,
    account: String,
    private: Vec<u8>,
    kind: Mutex<String>,
    out: mpsc::UnboundedSender<(DeviceId, Vec<u8>)>,
    state: Mutex<State>,
    host: Arc<dyn CloudHost>,
    pins_file: PathBuf,
    refresh: Notify,
    /// Check the relay link now: reconnect at once if it's down, and probe
    /// it if it looks up (it may have died while the OS froze the app).
    wake: Notify,
}

const MAX_INBOUND_PER_PEER: usize = 32;
const LINK_ATTEMPT: Duration = Duration::from_secs(4);
/// `ensure_link` tries this many times, `LINK_ATTEMPT` apart.
const LINK_ATTEMPTS: u32 = 3;
/// How long a link attempt counts as in flight for the nudge coalescing: the
/// whole retry budget, because every retry is still one attempt from the
/// nudging device's point of view.
const ATTEMPT_WINDOW: Duration = Duration::from_secs(12);
/// The largest frame the relay may hand us. Every frame the protocol builds
/// fits in one Noise message (65535 bytes plus the framing), so this is a
/// bound on memory per socket, not on anything legitimate. It also covers the
/// relay's own text frames, and its device list rides one of those: a large
/// account's list must not be closed as too big, so the ceiling is well above
/// the binary frames it guards.
const MAX_FRAME: usize = 512 * 1024;
/// How long an inbound stream may wait for its END before its slot is
/// reclaimed. It must outlast the peer's own request timeout (a harness
/// install may answer slowly) so a live request never loses its slot; the
/// bound exists for the peer that freezes and never ends anything, which
/// nothing else would ever clean up.
const INBOUND_TTL: Duration = Duration::from_secs(960);
/// Relay sessions in a row that never even established before the phase
/// admits the relay is down: one failed handshake is a deploy-window
/// blip; three in a row, seconds apart under backoff, is an outage the
/// user should see instead of a phase that sits at "connecting" with no
/// error.
const HANDSHAKE_FAILURES_TO_ERROR: u32 = 3;
/// Device-list fetches in a row that fail before they surface. At the
/// refresher's five-minute cadence this is ten minutes of a frozen device
/// list, which is an outage, not a blip.
const REFRESH_FAILURES_TO_ERROR: u32 = 2;
/// The two sentences this module puts next to a phase when a connection is
/// the problem. They are the only notices it may replace with one of its own
/// or clear again; anything else on screen (a takeover, a removal, a prompt
/// to sign in) was written by something that knows more and is left alone.
const UNREACHABLE: &str = "can't reach OpenRemote Cloud";
const DROPPED: &str = "the connection to OpenRemote Cloud dropped";

/// Whether the notice on screen is one of this module's own generic
/// connection sentences, and so may be replaced or cleared by it.
fn generic_notice(error: Option<&str>) -> bool {
    matches!(error, None | Some(UNREACHABLE) | Some(DROPPED))
}

/// What the relay loop writes into the phase once a session has ended, or
/// `None` when there is nothing truer to say than what is already there.
///
/// A phase that still says "online" is demoted on the first retry whatever
/// ended the session: the sentence described a link that is gone. The phase is
/// demoted whatever the slot holds, but the sentence is only replaced when it
/// is this loop's own generic one, so a specific reason that is still true is
/// not made vaguer. A run of handshakes that never even established is an
/// outage (a proxy in front of the relay 503ing through a maintenance window, a
/// deploy) and reaches the console as one - raised once per run, and never on
/// top of a notice that already explains why the phase moved.
fn retry_notice(
    was_online: bool,
    error: Option<&str>,
    established: bool,
    failures: u32,
) -> Option<(Phase, Option<&'static str>)> {
    if was_online {
        // A retry always demotes the phase, because "online" beside no link is
        // a lie the console would keep showing. The sentence is only replaced
        // when the slot holds this loop's own generic one: a specific reason
        // that is still true must not be overwritten with a vaguer sentence.
        return Some((Phase::Offline, generic_notice(error).then_some(DROPPED)));
    }
    if !established && failures == HANDSHAKE_FAILURES_TO_ERROR && generic_notice(error) {
        return Some((Phase::Offline, Some(UNREACHABLE)));
    }
    None
}

/// The refresher's error bookkeeping: how long the device-list fetches have
/// been failing, and the phase it raised its own notice at. Kept apart from
/// the loop so the transitions can be read and tested without a live
/// registry.
#[derive(Default)]
struct Surfacer {
    failures: u32,
    at: Option<Phase>,
}

impl Surfacer {
    /// One failed fetch: the notice to raise, when this is the moment. A run
    /// of failures raises it once, and only into a slot that says nothing
    /// more specific: the generic sentence must never overwrite the one that
    /// explains why (a takeover, a removal, a prompt to sign in), which is
    /// what a plain re-set of the same string did every five minutes.
    fn failed(
        &mut self,
        phase: Phase,
        error: Option<&str>,
    ) -> Option<(Phase, Option<&'static str>)> {
        // A notice that is no longer what is on screen is no longer a reason
        // to stay quiet either. The relay loop writes "online" when the link
        // comes back and clears the sentence with it, so the outage this loop
        // reported can vanish from the view while the registry is still down.
        // Without this the refresher went deaf at the first outage until a
        // fetch succeeded, and the console read "online" beside a frozen
        // device list for the rest of it.
        if self.at != Some(phase) || error != Some(UNREACHABLE) {
            self.at = None;
        }
        self.failures = self.failures.saturating_add(1);
        if self.failures < REFRESH_FAILURES_TO_ERROR || self.at.is_some() || !generic_notice(error)
        {
            return None;
        }
        self.at = Some(phase);
        Some((phase, Some(UNREACHABLE)))
    }

    /// One good fetch: the phase whose notice to clear, when the notice still
    /// on screen is exactly the one this loop raised.
    fn recovered(&mut self, phase: Phase, error: Option<&str>) -> Option<Phase> {
        self.failures = 0;
        let at = self.at.take()?;
        (phase == at && error == Some(UNREACHABLE)).then_some(at)
    }
}

/// A device-list entry's noise key, decoded: `None` unless it is exactly
/// the 32 bytes an X25519 public key is, in clean base64url. Anything
/// else must never be pinned or stored as key material; an entry without
/// a usable key is skipped and fetched again.
fn validated_pubkey(raw: &str) -> Option<Vec<u8>> {
    use base64::Engine;
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(raw)
        .ok()?;
    (bytes.len() == 32).then_some(bytes)
}

impl Mesh {
    pub fn me(&self) -> DeviceId {
        self.me
    }

    fn state(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Fetches the device list again (after this device changed it).
    pub fn refresh_now(&self) {
        self.refresh.notify_one();
    }

    pub fn wake(&self) {
        self.wake.notify_one();
    }

    /// This device's own kind, as the registry last said.
    pub fn kind(&self) -> String {
        self.kind.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    pub fn auto_update(&self) -> bool {
        self.state().auto_update
    }

    /// The account's devices, this one included, as last fetched.
    pub fn peers(&self) -> Vec<Peer> {
        let st = self.state();
        let mut peers: Vec<Peer> = st
            .peers
            .values()
            .map(|p| Peer {
                online: p.id == self.me || st.online.contains(&p.id),
                ..p.clone()
            })
            .collect();
        peers.sort_by(|a, b| a.name.cmp(&b.name));
        peers
    }

    pub fn peer(&self, id: DeviceId) -> Option<Peer> {
        self.peers().into_iter().find(|p| p.id == id)
    }

    fn hello(&self) -> Vec<u8> {
        json!({"v": 1, "kind": self.kind(), "version": env!("CARGO_PKG_VERSION")})
            .to_string()
            .into_bytes()
    }

    fn context(&self, st: &State, peer: DeviceId) -> PeerContext {
        let info = st.peers.get(&peer);
        PeerContext {
            device_id: peer,
            name: info.map(|p| p.name.clone()).unwrap_or_default(),
            kind: info.map(|p| p.kind.clone()).unwrap_or_default(),
        }
    }

    fn pins(&self) -> HashMap<String, String> {
        std::fs::read(&self.pins_file)
            .ok()
            .and_then(|raw| serde_json::from_slice(&raw).ok())
            .unwrap_or_default()
    }

    /// Takes the registry's device list. New keys are pinned; a changed
    /// key marks the device and its key is not used. An entry whose
    /// `noise_pubkey` is absent or malformed is skipped, never pinned:
    /// pinning it would flag the device `key_changed` forever once the
    /// real key shows up. Returns whether every entry was usable; the
    /// refresher treats a skipped entry like a failed fetch.
    pub fn update_devices(&self, body: &Value) -> bool {
        let mut pins = self.pins();
        let mut pinned_new = false;
        let mut peers = HashMap::new();
        let mut unpinnable: Vec<DeviceId> = Vec::new();
        for d in body["devices"].as_array().into_iter().flatten() {
            let Some(id) = d["id"].as_str().and_then(DeviceId::parse) else {
                continue;
            };
            let Some(key) = d["noise_pubkey"].as_str() else {
                unpinnable.push(id);
                continue;
            };
            let Some(pubkey) = validated_pubkey(key) else {
                unpinnable.push(id);
                continue;
            };
            let problem = match pins.get(&id.to_string()) {
                // A pin that isn't valid key material was never a real
                // key: an older daemon pinned an empty one when the
                // registry served this entry without a key, and it
                // poisoned the device. Re-pin the real key instead of
                // flagging the device forever.
                Some(pin) if validated_pubkey(pin).is_none() => {
                    pins.insert(id.to_string(), key.to_string());
                    pinned_new = true;
                    None
                }
                Some(pin) if *pin != key => Some("key_changed"),
                Some(_) => None,
                None => {
                    pins.insert(id.to_string(), key.to_string());
                    pinned_new = true;
                    None
                }
            };
            if id == self.me {
                *self.kind.lock().unwrap_or_else(|e| e.into_inner()) =
                    d["kind"].as_str().unwrap_or("desktop").to_string();
            }
            peers.insert(
                id,
                Peer {
                    id,
                    name: d["name"].as_str().unwrap_or("device").to_string(),
                    platform: d["platform"].as_str().unwrap_or_default().to_string(),
                    kind: d["kind"].as_str().unwrap_or("desktop").to_string(),
                    online: d["online"].as_bool().unwrap_or(false),
                    app_version: d["app_version"].as_str().map(str::to_string),
                    last_seen_at: d["last_seen_at"].as_f64(),
                    created_via: d["created_via"].as_str().map(str::to_string),
                    auto_update: d["auto_update"].as_bool().unwrap_or(true),
                    auto_update_override: d["auto_update_override"].as_bool(),
                    problem,
                    pubkey,
                },
            );
        }
        if pinned_new {
            if let Ok(json) = serde_json::to_vec_pretty(&pins) {
                let _ = openremote_core::fsx::write_private(&self.pins_file, &json);
            }
        }
        let mut st = self.state();
        // A skipped entry keeps its last good self: one glitchy fetch must
        // not drop a working peer, and its link, from the list.
        for id in &unpinnable {
            if let Some(peer) = st.peers.remove(id) {
                peers.insert(*id, peer);
            }
        }
        // Devices that left the account lose their links.
        let gone: Vec<DeviceId> = st
            .links
            .keys()
            .filter(|id| !peers.contains_key(id))
            .copied()
            .collect();
        for id in gone {
            fail_peer(&mut st, id, "removed");
        }
        st.peers = peers;
        st.auto_update = body["prefs"]["auto_update"].as_bool().unwrap_or(true);
        // Peers that came online before this list named them, and that this
        // device can reach now: the host hears they are online again, so the
        // compare that failed for want of a key runs now.
        let reachable: Vec<DeviceId> = st
            .unlisted_online
            .iter()
            .copied()
            .filter(|id| {
                st.online.contains(id) && st.peers.get(id).is_some_and(|p| p.problem.is_none())
            })
            .collect();
        for id in &reachable {
            st.unlisted_online.remove(id);
        }
        drop(st);
        for id in reachable {
            self.host.online(id);
        }
        unpinnable.is_empty()
    }

    fn set_online(&self, ids: &[DeviceId]) {
        let mut st = self.state();
        st.online = ids.iter().copied().collect();
        let unlisted: Vec<DeviceId> = ids
            .iter()
            .copied()
            .filter(|id| *id != self.me && !st.peers.contains_key(id))
            .collect();
        st.unlisted_online = unlisted.into_iter().collect();
    }

    fn presence(&self, id: DeviceId, online: bool) {
        let mut st = self.state();
        if online {
            st.online.insert(id);
            if id != self.me && !st.peers.contains_key(&id) {
                st.unlisted_online.insert(id);
            }
        } else {
            st.online.remove(&id);
            st.unlisted_online.remove(&id);
            fail_peer(&mut st, id, "offline");
        }
    }

    /// Every link is suspect after the relay reconnects: a frame in flight
    /// may be gone, and Noise transport needs every one, in order.
    fn drop_links(&self) {
        let mut st = self.state();
        let ids: Vec<DeviceId> = st.links.keys().copied().collect();
        for id in ids {
            fail_peer(&mut st, id, "relay reconnected");
        }
    }

    fn send_sealed(&self, st: &mut State, peer: DeviceId, plaintext: &[u8]) -> bool {
        let Some(link) = st.links.get_mut(&peer) else {
            return false;
        };
        match link.seal(plaintext) {
            Ok(sealed) => self.out.send((peer, sealed)).is_ok(),
            Err(_) => false,
        }
    }

    async fn ensure_link(&self, peer: DeviceId) -> Result<(), MeshError> {
        // Record the attempt before the first frame goes out: a nudge that
        // arrives while this runs must not start a second one, or a device of
        // the account could spawn as many attempts as it sends frames.
        let window = Instant::now() + ATTEMPT_WINDOW;
        self.state().attempts.insert(peer, window);
        let result = self.attempt_links(peer).await;
        {
            let mut st = self.state();
            // Only clear our own window: a later attempt may hold it by now.
            if st.attempts.get(&peer) == Some(&window) {
                st.attempts.remove(&peer);
            }
        }
        result
    }

    async fn attempt_links(&self, peer: DeviceId) -> Result<(), MeshError> {
        for attempt in 0..LINK_ATTEMPTS {
            let rx = {
                let mut st = self.state();
                if st.links.get(&peer).is_some_and(Link::is_up) {
                    return Ok(());
                }
                let info = st.peers.get(&peer).cloned().ok_or(MeshError::Unknown)?;
                if info.problem.is_some() {
                    return Err(MeshError::KeyChanged(info.name));
                }
                if !st.online.contains(&peer) {
                    return Err(MeshError::Offline(info.name));
                }
                let (tx, rx) = oneshot::channel();
                st.waiters.entry(peer).or_default().push(tx);
                if link::initiates(self.me, peer) {
                    let restart = attempt > 0 || !st.links.contains_key(&peer);
                    if restart {
                        let prologue = link::prologue(&self.account, self.me, peer);
                        if let Ok((link, hs1)) =
                            link::initiate(&self.private, &info.pubkey, &prologue, &self.hello())
                        {
                            st.links.insert(peer, link);
                            let _ = self.out.send((peer, hs1));
                        }
                    }
                } else {
                    let _ = self.out.send((peer, vec![NUDGE]));
                }
                rx
            };
            if matches!(tokio::time::timeout(LINK_ATTEMPT, rx).await, Ok(Ok(()))) {
                return Ok(());
            }
            // The attempt timed out and its receiver is gone, so the sender it
            // left behind is dead weight. Only a handshake that completes
            // empties `waiters`, and a peer that never completes one would
            // otherwise grow this vector for the life of the process: one
            // entry per call for a device that is online and never answers.
            let mut st = self.state();
            if let Some(waiters) = st.waiters.get_mut(&peer) {
                waiters.retain(|tx| !tx.is_closed());
            }
            if st.waiters.get(&peer).is_some_and(Vec::is_empty) {
                st.waiters.remove(&peer);
            }
        }
        let name = self
            .state()
            .peers
            .get(&peer)
            .map(|p| p.name.clone())
            .unwrap_or_default();
        Err(MeshError::Unreachable(name))
    }

    /// One request to a peer's own API, answered in full.
    pub async fn request(
        &self,
        peer: DeviceId,
        request: Request,
        timeout: Duration,
    ) -> Result<Response, MeshError> {
        if request.body.len() > rpc::MAX_REQUEST {
            return Err(MeshError::TooLarge);
        }
        self.ensure_link(peer).await?;
        let (tx, rx) = oneshot::channel();
        let stream = {
            let mut st = self.state();
            st.next_stream = st.next_stream.wrapping_add(1).max(1);
            let stream = st.next_stream;
            st.calls.insert(
                (peer, stream),
                Call {
                    head: None,
                    body: Vec::new(),
                    done: Some(tx),
                },
            );
            let header = serde_json::to_vec(&json!({"op": "http", "method": request.method, "path": request.path, "content_type": request.content_type}))
                .unwrap_or_default();
            for frame in rpc::frames(rpc::OPEN, stream, &header, &request.body) {
                if !self.send_sealed(&mut st, peer, &frame) {
                    st.calls.remove(&(peer, stream));
                    return Err(MeshError::Reset(String::new()));
                }
            }
            stream
        };
        let name = || self.peer(peer).map(|p| p.name).unwrap_or_default();
        match tokio::time::timeout(timeout, rx).await {
            Ok(Ok(result)) => result.map_err(|e| match e {
                MeshError::Reset(_) => MeshError::Reset(name()),
                other => other,
            }),
            Ok(Err(_)) => Err(MeshError::Reset(name())),
            Err(_) => {
                let mut st = self.state();
                st.calls.remove(&(peer, stream));
                let reset = rpc::frame(rpc::RESET, stream, b"timeout");
                self.send_sealed(&mut st, peer, &reset);
                Err(MeshError::Timeout(name()))
            }
        }
    }

    /// A one-way message to a peer; dropped if it can't be delivered.
    pub async fn note(&self, peer: DeviceId, note: &Value) -> Result<(), MeshError> {
        self.ensure_link(peer).await?;
        let frame = rpc::frame(rpc::NOTE, 0, note.to_string().as_bytes());
        if frame.len() > link::MAX_PLAINTEXT {
            return Err(MeshError::TooLarge);
        }
        let mut st = self.state();
        if self.send_sealed(&mut st, peer, &frame) {
            Ok(())
        } else {
            Err(MeshError::Reset(String::new()))
        }
    }

    /// Ids of the devices online right now, this one excluded.
    pub fn online(&self) -> Vec<DeviceId> {
        self.state()
            .online
            .iter()
            .copied()
            .filter(|id| *id != self.me)
            .collect()
    }

    fn on_frame(self: &Arc<Self>, from: DeviceId, payload: &[u8]) {
        let Some((&kind, body)) = payload.split_first() else {
            return;
        };
        match kind {
            HS1 => {
                let mut st = self.state();
                let Some(info) = st.peers.get(&from).filter(|p| p.problem.is_none()).cloned()
                else {
                    // A device the list doesn't know yet: fetch it again.
                    self.refresh.notify_one();
                    return;
                };
                let prologue = link::prologue(&self.account, self.me, from);
                let Ok((up, _hello, hs2)) =
                    link::respond(&self.private, &info.pubkey, &prologue, body, &self.hello())
                else {
                    return;
                };
                // A new handshake means the peer started over: the old
                // link's streams are gone.
                fail_peer(&mut st, from, "renewed");
                st.links.insert(from, up);
                let _ = self.out.send((from, hs2));
                for waiter in st.waiters.remove(&from).unwrap_or_default() {
                    let _ = waiter.send(());
                }
                drop(st);
                self.host.linked(from);
            }
            HS2 => {
                let mut st = self.state();
                // Only an initiated link takes an answer; a stray HS2 must
                // not tear down a link that is up.
                if !matches!(st.links.get(&from), Some(Link::Initiating(_))) {
                    return;
                }
                let Some(pending) = st.links.remove(&from) else {
                    return;
                };
                match link::complete(pending, body) {
                    Ok((up, _hello)) => {
                        st.links.insert(from, up);
                        for waiter in st.waiters.remove(&from).unwrap_or_default() {
                            let _ = waiter.send(());
                        }
                        drop(st);
                        self.host.linked(from);
                    }
                    Err(_) => fail_peer(&mut st, from, "handshake failed"),
                }
            }
            MSG => {
                let mut st = self.state();
                let opened = st.links.get_mut(&from).map(|l| l.open(body));
                match opened {
                    Some(Ok(plaintext)) => {
                        drop(st);
                        self.on_rpc(from, &plaintext);
                    }
                    Some(Err(_)) => fail_peer(&mut st, from, "decrypt failed"),
                    None => {}
                }
            }
            NUDGE if link::initiates(self.me, from) => {
                let now = Instant::now();
                {
                    let mut st = self.state();
                    // A nudge is a peer asking for a link it does not have. If
                    // this one is up there is nothing to do, and spawning a
                    // task per frame to discover that is the churn this
                    // coalescing exists to stop.
                    if st.links.get(&from).is_some_and(Link::is_up) {
                        return;
                    }
                    // A device the list doesn't know yet knocked: fetch the
                    // list again, as a handshake from one does. An attempt
                    // now would fail at once for want of its key, and the
                    // list's arrival is what links it.
                    if !st.peers.contains_key(&from) {
                        self.refresh.notify_one();
                        return;
                    }
                    st.attempts.retain(|_, until| now < *until);
                    if st.attempts.contains_key(&from) {
                        // An attempt to this peer is already in flight, and its
                        // own retries cover this nudge. Coalescing here is what
                        // keeps a device of the account from costing one task
                        // per frame it sends.
                        return;
                    }
                    st.attempts.insert(from, now + ATTEMPT_WINDOW);
                }
                let mesh = Arc::clone(self);
                tokio::spawn(async move {
                    let _ = mesh.ensure_link(from).await;
                });
            }
            _ => {}
        }
    }

    fn on_rpc(self: &Arc<Self>, from: DeviceId, plaintext: &[u8]) {
        let Some((kind, stream, body)) = rpc::parse(plaintext) else {
            return;
        };
        let mut st = self.state();
        match kind {
            rpc::OPEN => {
                let now = Instant::now();
                // Reclaim expired slots first: a peer that OPENed streams
                // and then stalled without ending them must not wedge its
                // whole window to "busy".
                reclaim_expired(&mut st.inbound, now);
                let open = st.inbound.keys().filter(|(p, _)| *p == from).count();
                let request = serde_json::from_slice::<Value>(body).ok().map(|h| Request {
                    method: h["method"].as_str().unwrap_or("GET").to_string(),
                    path: h["path"].as_str().unwrap_or("/").to_string(),
                    content_type: h["content_type"].as_str().map(str::to_string),
                    body: Vec::new(),
                });
                // An id that is already here is refused, not replaced. The
                // window counts entries, so a peer could otherwise OPEN a
                // stream, END it (the work starts, the entry stays), take the
                // same id again, END again, and hold as many dispatches as it
                // likes while the count never moves. A finished stream frees
                // its id; a stream still waiting or still running does not.
                let taken = st.inbound.contains_key(&(from, stream));
                match request {
                    Some(r) if !taken && open < MAX_INBOUND_PER_PEER => {
                        let generation = st.next_generation;
                        st.next_generation += 1;
                        st.inbound.insert(
                            (from, stream),
                            Inbound {
                                request: r,
                                deadline: now + INBOUND_TTL,
                                dispatched: false,
                                generation,
                            },
                        );
                    }
                    _ => {
                        let reset = rpc::frame(rpc::RESET, stream, b"busy");
                        self.send_sealed(&mut st, from, &reset);
                    }
                }
            }
            rpc::DATA => {
                let too_big = match st.inbound.get_mut(&(from, stream)) {
                    // A stream whose work has started takes no more body: its
                    // request is already where the work reads it.
                    Some(r) if !r.dispatched => {
                        r.request.body.extend_from_slice(body);
                        r.request.body.len() > rpc::MAX_REQUEST
                    }
                    _ => false,
                };
                if too_big {
                    st.inbound.remove(&(from, stream));
                    let reset = rpc::frame(rpc::RESET, stream, b"too large");
                    self.send_sealed(&mut st, from, &reset);
                }
            }
            rpc::END => {
                let Some(inbound) = st.inbound.get_mut(&(from, stream)) else {
                    return;
                };
                if inbound.dispatched {
                    return;
                }
                // The slot stays taken until the response is sealed. The
                // window is here to bound the work this daemon does for one
                // peer, and work that has been asked for but not answered is
                // work in flight: freeing the slot at END would cap
                // half-written requests instead, and a peer could make this
                // daemon run as many chats, folder listings and installers at
                // once as it can send ENDs.
                inbound.dispatched = true;
                let request = inbound.request.clone();
                let generation = inbound.generation;
                let context = self.context(&st, from);
                drop(st);
                let mesh = Arc::clone(self);
                tokio::spawn(async move {
                    let response = mesh.host.dispatch(context, request).await;
                    let header = serde_json::to_vec(
                        &json!({"status": response.status, "content_type": response.content_type}),
                    )
                    .unwrap_or_default();
                    let mut st = mesh.state();
                    for frame in rpc::frames(rpc::HEAD, stream, &header, &response.body) {
                        if !mesh.send_sealed(&mut st, from, &frame) {
                            break;
                        }
                    }
                    // The answer is out (or its link is gone): give the slot
                    // back. The TTL sweep is the only other way it comes back,
                    // for a dispatch that never returns at all, and if it took
                    // this entry the peer may already hold the id again: only
                    // the entry this task was started for is its to remove.
                    if st
                        .inbound
                        .get(&(from, stream))
                        .is_some_and(|e| e.generation == generation)
                    {
                        st.inbound.remove(&(from, stream));
                    }
                });
            }
            rpc::HEAD => {
                if let Some(call) = st.calls.get_mut(&(from, stream)) {
                    let h: Value = serde_json::from_slice(body).unwrap_or(Value::Null);
                    call.head = Some(Response {
                        status: h["status"].as_u64().unwrap_or(502) as u16,
                        content_type: h["content_type"].as_str().map(str::to_string),
                        body: Vec::new(),
                    });
                }
            }
            rpc::RDATA => {
                let too_big = match st.calls.get_mut(&(from, stream)) {
                    Some(call) => {
                        call.body.extend_from_slice(body);
                        call.body.len() > rpc::MAX_RESPONSE
                    }
                    None => false,
                };
                if too_big {
                    if let Some(mut call) = st.calls.remove(&(from, stream)) {
                        if let Some(done) = call.done.take() {
                            let _ = done.send(Err(MeshError::TooLarge));
                        }
                    }
                }
            }
            rpc::REND => {
                if let Some(mut call) = st.calls.remove(&(from, stream)) {
                    let mut response = call.head.take().unwrap_or(Response {
                        status: 502,
                        content_type: None,
                        body: Vec::new(),
                    });
                    response.body = std::mem::take(&mut call.body);
                    if let Some(done) = call.done.take() {
                        let _ = done.send(Ok(response));
                    }
                }
            }
            rpc::RESET => {
                // A reset abandons a stream that has not started; once the
                // work is running the slot stays until it answers, or a peer
                // could END then RESET to reopen its window on top of running
                // work.
                if st
                    .inbound
                    .get(&(from, stream))
                    .is_some_and(|i| !i.dispatched)
                {
                    st.inbound.remove(&(from, stream));
                }
                if let Some(mut call) = st.calls.remove(&(from, stream)) {
                    if let Some(done) = call.done.take() {
                        let _ = done.send(Err(MeshError::Reset(String::new())));
                    }
                }
            }
            rpc::NOTE => {
                let context = self.context(&st, from);
                drop(st);
                if let Ok(note) = serde_json::from_slice(body) {
                    self.host.note(context, note);
                }
            }
            _ => {}
        }
    }
}

/// Reclaims inbound streams whose END never came: a peer frozen mid
/// request must not hold its slots in the per-peer window forever. The
/// OPEN handler checks before admitting new streams, and the refresher
/// sweeps with this periodically so a silent peer's slots come back even
/// when it sends nothing more. Returns how many slots were reclaimed.
fn reclaim_expired(inbound: &mut HashMap<(DeviceId, u32), Inbound>, now: Instant) -> usize {
    let before = inbound.len();
    inbound.retain(|_, entry| now < entry.deadline);
    before - inbound.len()
}

/// Drops a peer's link and fails everything in flight with it.
fn fail_peer(st: &mut State, peer: DeviceId, _why: &str) {
    st.links.remove(&peer);
    // A waiter is waiting for a link that will not come up: drop it rather
    // than leave the sender behind. The attempt window goes too, so the next
    // nudge starts a fresh attempt instead of being coalesced into this one.
    st.waiters.remove(&peer);
    st.attempts.remove(&peer);
    let keys: Vec<(DeviceId, u32)> = st
        .calls
        .keys()
        .filter(|(p, _)| *p == peer)
        .copied()
        .collect();
    for key in keys {
        if let Some(mut call) = st.calls.remove(&key) {
            if let Some(done) = call.done.take() {
                let _ = done.send(Err(MeshError::Reset(String::new())));
            }
        }
    }
    st.inbound.retain(|(p, _), _| *p != peer);
}

/// How a relay session ended.
enum Ended {
    /// Reconnect after a pause.
    Retry,
    /// Stop: the credential is no good, or this device was removed.
    Stop,
    /// Stop because this daemon closed the session itself: sign-out, leave or
    /// shutdown. The caller owns the phase it wants shown, so a phase this
    /// loop would otherwise correct must be left alone.
    Closed,
}

/// Starts the mesh for a joined device: the relay connection (kept up with
/// backoff) and the device-list refresher. The handles come back named so
/// `Cloud` can tell the run's liveness (the relay loop) from its housekeeping
/// (the refresher, which never ends on its own).
pub fn start(
    cloud: &Arc<Cloud>,
    host: Arc<dyn CloudHost>,
) -> Option<(
    Arc<Mesh>,
    tokio::task::JoinHandle<()>,
    tokio::task::JoinHandle<()>,
)> {
    // Nothing here may return `None` in silence. `Cloud::open` already refuses
    // a state on disk that cannot work, and this is the same check for files
    // that changed under a running daemon (a crash mid-write, a virus scanner
    // quarantine): the phase moves to "sign in again" with the reason, instead
    // of leaving the console at "connecting" for good with nothing to read.
    let Some(identity) = cloud.identity() else {
        // Whichever shape the file is in, the phase has a sentence for it: a
        // missing identity lets a sign-in join the account as a new device,
        // while a damaged one has to be reset first. Saying the wrong one would
        // promise a remedy that does not exist.
        let sentence = if matches!(cloud.identity_state(), crate::identity::Stored::Unreadable) {
            crate::IDENTITY_BROKEN
        } else {
            crate::IDENTITY_MISSING
        };
        cloud.set_phase(Phase::Relink, Some(sentence.into()));
        return None;
    };
    let Some(private) = identity.private_key() else {
        cloud.set_phase(Phase::Relink, Some(crate::KEY_BROKEN.into()));
        return None;
    };
    let Some(account) = cloud.view()["account"]["id"].as_str().map(str::to_string) else {
        cloud.set_phase(Phase::Relink, Some(crate::SIGNIN_BROKEN.into()));
        return None;
    };
    let kind = cloud.view()["device"]["kind"]
        .as_str()
        .unwrap_or("desktop")
        .to_string();
    let (out, out_rx) = mpsc::unbounded_channel();
    let mesh = Arc::new(Mesh {
        me: identity.device_id,
        account,
        private,
        kind: Mutex::new(kind),
        out,
        state: Mutex::new(State {
            auto_update: true,
            ..State::default()
        }),
        host,
        pins_file: cloud.data_dir().join("cloud").join("peers.json"),
        refresh: Notify::new(),
        wake: Notify::new(),
    });
    let relay = tokio::spawn(run_relay(Arc::clone(cloud), Arc::clone(&mesh), out_rx));
    let refresher = tokio::spawn(refresh_devices(Arc::clone(cloud), Arc::clone(&mesh)));
    Some((mesh, relay, refresher))
}

/// Keeps the device list current: at start, when the relay says it
/// changed, when an unknown device knocks, and every few minutes. A
/// fetch that keeps failing is surfaced instead of swallowed: after
/// REFRESH_FAILURES_TO_ERROR consecutive failures (a registry that keeps
/// refusing the credential, or entries too broken to pin) the console
/// shows an error next to the phase instead of a silently frozen device
/// list, and the next good fetch clears it. The notice is raised once per run
/// of failures and never over one that says something more specific. The loop
/// also rides the inbound sweep: a stream whose END never arrives gives its
/// slot back here.
async fn refresh_devices(cloud: Arc<Cloud>, mesh: Arc<Mesh>) {
    let mut surfacer = Surfacer::default();
    loop {
        if let Some(credential) = cloud.device_credential() {
            let ok = match cloud.registry().devices(&credential).await {
                Ok(body) => mesh.update_devices(&body),
                Err(crate::registry::RegistryError::Revoked) => {
                    cloud.removed(&*mesh.host);
                    return;
                }
                Err(_) => false,
            };
            let (phase, error) = cloud.phase_and_error();
            let write = if ok {
                surfacer
                    .recovered(phase, error.as_deref())
                    .map(|at| (at, None))
            } else {
                surfacer
                    .failed(phase, error.as_deref())
                    .map(|(at, notice)| (at, notice.map(str::to_string)))
            };
            if let Some((at, notice)) = write {
                cloud.set_phase(at, notice);
            }
        }
        {
            let mut st = mesh.state();
            reclaim_expired(&mut st.inbound, Instant::now());
        }
        tokio::select! {
            _ = mesh.refresh.notified() => {}
            _ = tokio::time::sleep(Duration::from_secs(300)) => {}
        }
    }
}

/// The relay loop's wrapper: whatever ended the loop (a credential the relay
/// refuses, this device removed from the account, it connecting from
/// elsewhere, a relay url that can't be used, the credential gone outright),
/// the run it belonged to is released here. Without this the loop's dead
/// handles stayed in `Cloud`'s Running slot and every later start was
/// turned away, so one Relink or removal ended Cloud until a restart.
async fn run_relay(
    cloud: Arc<Cloud>,
    mesh: Arc<Mesh>,
    out_rx: mpsc::UnboundedReceiver<(DeviceId, Vec<u8>)>,
) {
    relay_loop(&cloud, &mesh, out_rx).await;
    cloud.reap(&mesh);
}

async fn relay_loop(
    cloud: &Arc<Cloud>,
    mesh: &Arc<Mesh>,
    mut out_rx: mpsc::UnboundedReceiver<(DeviceId, Vec<u8>)>,
) {
    let mut backoff = Duration::from_secs(1);
    let mut handshake_failures = 0u32;
    loop {
        let Some(credential) = cloud.device_credential() else {
            return;
        };
        let (established, ended) = relay_session(cloud, mesh, &credential, &mut out_rx).await;
        mesh.drop_links();
        let (phase, error) = cloud.phase_and_error();
        let was_online = phase == Phase::Online;
        if was_online {
            // The link was up: this is a fresh drop, not a run of failures.
            backoff = Duration::from_secs(1);
        }
        if established {
            handshake_failures = 0;
        } else {
            handshake_failures = handshake_failures.saturating_add(1);
        }
        if let Some((at, notice)) = retry_notice(
            was_online,
            error.as_deref(),
            established,
            handshake_failures,
        ) {
            // This runs before the Stop exit below on purpose: a credential
            // that dies mid-session must not leave "online" behind, because
            // nothing is connected any more and the next start is a sign-in
            // this loop will not make. A session this daemon closed itself is
            // the exception, and it says so.
            if !matches!(ended, Ended::Closed) {
                cloud.set_phase(at, notice.map(str::to_string));
            }
        }
        if matches!(ended, Ended::Stop | Ended::Closed) {
            return;
        }
        // Frames queued during the dead session rode links that were just
        // dropped and can't be delivered in order; anything queued during
        // the pause below belongs to a fresh link and must ride the next
        // session. Draining after the pause ate exactly that: the first
        // handshake of every reconnect.
        while out_rx.try_recv().is_ok() {}
        // Jitter, so a relay restart isn't met by every device at once.
        let jitter = Duration::from_millis(u64::from(uuid::Uuid::new_v4().as_bytes()[0]) * 4);
        tokio::select! {
            _ = tokio::time::sleep(backoff + jitter) => {
                backoff = (backoff * 2).min(Duration::from_secs(60));
            }
            _ = mesh.wake.notified() => backoff = Duration::from_secs(1),
        }
    }
}

/// One relay session: the WebSocket handshake and the connected loop.
/// Returns whether the session was ever established (the handshake
/// completed, even if the socket then died at once), so the loop can tell
/// a drop apart from a handshake that keeps failing, and how it ended.
async fn relay_session(
    cloud: &Arc<Cloud>,
    mesh: &Arc<Mesh>,
    credential: &str,
    out_rx: &mut mpsc::UnboundedReceiver<(DeviceId, Vec<u8>)>,
) -> (bool, Ended) {
    let url = cloud.config().relay_url();
    let Ok(uri) = url.parse::<http::Uri>() else {
        return (false, Ended::Stop);
    };
    let Ok(mut request) = url.as_str().into_client_request() else {
        return (false, Ended::Stop);
    };
    let auth = format!("Bearer {credential}");
    if let Ok(value) = auth.parse() {
        request.headers_mut().insert("authorization", value);
    }
    if let Ok(value) = env!("CARGO_PKG_VERSION").parse() {
        request.headers_mut().insert("x-openremote-version", value);
    }
    let Ok(stream) = cloud.http().connect(&uri).await else {
        cloud.set_phase(Phase::Offline, Some(UNREACHABLE.into()));
        return (false, Ended::Retry);
    };
    // The relay carries sealed frames, and the protocol keeps every one inside
    // one Noise message, so a 128 KiB ceiling costs nothing legitimate and
    // stops a relay (or anything in its place) from making this process
    // allocate a 64 MiB message first.
    let ws_config = tokio_tungstenite::tungstenite::protocol::WebSocketConfig::default()
        .max_message_size(Some(MAX_FRAME));
    let ws =
        match tokio_tungstenite::client_async_with_config(request, stream, Some(ws_config)).await {
            Ok((ws, _)) => ws,
            Err(tokio_tungstenite::tungstenite::Error::Http(response)) => {
                return match response.status().as_u16() {
                    401 => {
                        cloud.set_phase(Phase::Relink, Some(crate::SIGN_IN_AGAIN.into()));
                        (false, Ended::Stop)
                    }
                    410 => {
                        cloud.removed(&*mesh.host);
                        (false, Ended::Stop)
                    }
                    _ => (false, Ended::Retry),
                };
            }
            Err(_) => return (false, Ended::Retry),
        };
    let (mut sink, mut stream) = ws.split();
    let mut ping = tokio::time::interval(Duration::from_secs(25));
    ping.tick().await;
    // Set by a wake: the relay must answer the probe ping by then.
    let mut probe: Option<tokio::time::Instant> = None;
    loop {
        let quiet = probe.map_or(Duration::from_secs(60), |by| {
            by.saturating_duration_since(tokio::time::Instant::now())
        });
        tokio::select! {
            incoming = tokio::time::timeout(quiet, stream.next()) => {
                let message = match incoming {
                    Ok(Some(Ok(m))) => m,
                    _ => return (true, Ended::Retry),
                };
                probe = None;
                match message {
                    Message::Binary(data) if data.len() > 16 => {
                        let mut id = [0u8; 16];
                        id.copy_from_slice(&data[..16]);
                        mesh.on_frame(DeviceId::from_bytes(id), &data[16..]);
                    }
                    Message::Text(text) => {
                        let control: Value = serde_json::from_str(text.as_str()).unwrap_or(Value::Null);
                        match control["type"].as_str() {
                            Some("hello") => {
                                let online: Vec<DeviceId> = control["online"]
                                    .as_array()
                                    .into_iter()
                                    .flatten()
                                    .filter_map(|v| v.as_str().and_then(DeviceId::parse))
                                    .collect();
                                mesh.set_online(&online);
                                cloud.set_phase(Phase::Online, None);
                                mesh.refresh.notify_one();
                                for peer in online {
                                    mesh.host.online(peer);
                                }
                            }
                            Some("presence") => {
                                if let Some(id) = control["device_id"].as_str().and_then(DeviceId::parse) {
                                    let online = control["online"].as_bool().unwrap_or(false);
                                    mesh.presence(id, online);
                                    if online {
                                        mesh.host.online(id);
                                    }
                                }
                            }
                            Some("devices_changed") => mesh.refresh.notify_one(),
                            Some("undeliverable") => {
                                if let Some(id) = control["peer"].as_str().and_then(DeviceId::parse) {
                                    mesh.presence(id, false);
                                }
                            }
                            _ => {}
                        }
                    }
                    Message::Close(frame) => {
                        let code = frame.map(|f| u16::from(f.code));
                        return match code {
                            Some(4003) => {
                                cloud.removed(&*mesh.host);
                                (true, Ended::Stop)
                            }
                            Some(4001) => {
                                cloud.set_phase(Phase::Offline, Some("this device connected from somewhere else".into()));
                                (true, Ended::Stop)
                            }
                            _ => (true, Ended::Retry),
                        };
                    }
                    _ => {}
                }
            }
            outgoing = out_rx.recv() => {
                let Some((to, payload)) = outgoing else {
                    // Nobody can send any more: this daemon is closing the run
                    // (sign-out, leave, shutdown), so the phase is the caller's
                    // to write and this loop must not report a drop it caused.
                    return (true, Ended::Closed);
                };
                let mut frame = to.to_bytes().to_vec();
                frame.extend_from_slice(&payload);
                if sink.send(Message::Binary(frame.into())).await.is_err() {
                    return (true, Ended::Retry);
                }
            }
            _ = ping.tick() => {
                if sink.send(Message::Ping(Default::default())).await.is_err() {
                    return (true, Ended::Retry);
                }
            }
            _ = mesh.wake.notified() => {
                if sink.send(Message::Ping(Default::default())).await.is_err() {
                    return (true, Ended::Retry);
                }
                probe.get_or_insert(tokio::time::Instant::now() + Duration::from_secs(10));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A host that records nothing: the tests here only exercise the
    /// pure bookkeeping, never a live frame.
    struct Nop;
    impl CloudHost for Nop {
        fn dispatch(&self, _: PeerContext, _: Request) -> BoxFuture<'static, Response> {
            Box::pin(std::future::ready(Response {
                status: 200,
                content_type: None,
                body: Vec::new(),
            }))
        }
        fn note(&self, _: PeerContext, _: Value) {}
        fn linked(&self, _: DeviceId) {}
        fn online(&self, _: DeviceId) {}
        fn revoked(&self) {}
    }

    /// A mesh over a throwaway pins file, in the temp dir the way this
    /// workspace's other unit tests do it.
    fn test_mesh() -> (Mesh, std::path::PathBuf) {
        let (mesh, _out, dir) = mesh_over(Arc::new(Nop));
        (mesh, dir)
    }

    /// The same, over a host the test chooses, with the relay side of the
    /// `out` channel kept.
    fn mesh_over(
        host: Arc<dyn CloudHost>,
    ) -> (
        Mesh,
        mpsc::UnboundedReceiver<(DeviceId, Vec<u8>)>,
        std::path::PathBuf,
    ) {
        let dir = std::env::temp_dir().join(format!("or-mesh-{}", uuid::Uuid::new_v4()));
        openremote_core::fsx::create_private_dir(&dir).unwrap();
        let (out, rx) = mpsc::unbounded_channel();
        let mesh = Mesh {
            me: DeviceId::new(),
            account: "test".into(),
            private: Vec::new(),
            kind: Mutex::new("desktop".into()),
            out,
            state: Mutex::new(State::default()),
            host,
            pins_file: dir.join("peers.json"),
            refresh: Notify::new(),
            wake: Notify::new(),
        };
        (mesh, rx, dir)
    }

    /// A listed, online peer with a real key: the state the link code finds
    /// before it tries to talk to a device.
    fn listed(id: DeviceId) -> Peer {
        Peer {
            id,
            name: "peer".into(),
            platform: "linux".into(),
            kind: "machine".into(),
            online: true,
            app_version: None,
            last_seen_at: None,
            created_via: None,
            auto_update: true,
            auto_update_override: None,
            problem: None,
            pubkey: decode(&key()),
        }
    }

    /// A device of the account, listed and online, and never answered by
    /// anything: the shape of a peer that makes this daemon wait.
    fn listed_and_online(mesh: &Mesh) -> DeviceId {
        let id = DeviceId::new();
        let mut st = mesh.state();
        st.peers.insert(id, listed(id));
        st.online.insert(id);
        id
    }

    /// A host whose work parks until the test lets it answer, so a test can
    /// hold a peer's window open and count what got in.
    struct Parked {
        started: Arc<std::sync::atomic::AtomicUsize>,
        release: Arc<tokio::sync::Notify>,
    }

    impl CloudHost for Parked {
        fn dispatch(&self, _: PeerContext, _: Request) -> BoxFuture<'static, Response> {
            let started = Arc::clone(&self.started);
            let release = Arc::clone(&self.release);
            Box::pin(async move {
                started.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                release.notified().await;
                Response {
                    status: 200,
                    content_type: None,
                    body: Vec::new(),
                }
            })
        }
        fn note(&self, _: PeerContext, _: Value) {}
        fn linked(&self, _: DeviceId) {}
        fn online(&self, _: DeviceId) {}
        fn revoked(&self) {}
    }

    /// A fresh X25519 public key, base64url, exactly as the registry
    /// serves them.
    fn key() -> String {
        use base64::Engine;
        let keys = snow::Builder::new(crate::identity::NOISE_PARAMS.parse().unwrap())
            .generate_keypair()
            .unwrap();
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(keys.public)
    }

    fn decode(raw: &str) -> Vec<u8> {
        use base64::Engine;
        base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(raw)
            .unwrap()
    }

    /// A link that completed its handshake, this device to `peer`: the state a
    /// nudge finds when the other side's reconnect went unnoticed here.
    fn up_link(mesh: &Mesh, peer: DeviceId) -> Link {
        let keys = || {
            snow::Builder::new(crate::identity::NOISE_PARAMS.parse().unwrap())
                .generate_keypair()
                .unwrap()
        };
        let (mine, theirs) = (keys(), keys());
        let prologue = link::prologue(&mesh.account, mesh.me, peer);
        let (link, hs1) = link::initiate(&mine.private, &theirs.public, &prologue, b"").unwrap();
        let (_, _, hs2) =
            link::respond(&theirs.private, &mine.public, &prologue, &hs1[1..], b"").unwrap();
        link::complete(link, &hs2[1..]).unwrap().0
    }

    fn pins_at(mesh: &Mesh) -> HashMap<String, String> {
        std::fs::read(&mesh.pins_file)
            .ok()
            .and_then(|raw| serde_json::from_slice(&raw).ok())
            .unwrap_or_default()
    }

    /// `None`: an unpinnable key is never key material.
    #[test]
    fn keys_without_32_clean_base64_bytes_are_rejected() {
        use base64::Engine;
        let b64 = |bytes: &[u8]| base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes);
        assert!(validated_pubkey(&key()).is_some(), "a real key passes");
        assert!(validated_pubkey("").is_none(), "empty never pins");
        assert!(validated_pubkey("no!").is_none(), "bad base64 never pins");
        assert!(validated_pubkey(&b64(&[0u8; 31])).is_none(), "too short");
        assert!(validated_pubkey(&b64(&[0u8; 33])).is_none(), "too long");
        assert_eq!(
            validated_pubkey(&key()).map(|k| k.len()),
            Some(32),
            "the decoded key is exactly the pin"
        );
    }

    /// An entry without a usable key is skipped and re-fetched, never
    /// pinned: when the real key arrives it pins cleanly with no
    /// `key_changed`, and a glitchy re-fetch never drops a good peer.
    #[test]
    fn an_unpinnable_entry_is_skipped_not_pinned() {
        let (mesh, dir) = test_mesh();
        let good = (DeviceId::new(), key());
        let silent = DeviceId::new(); // no noise_pubkey at all
        let blank = (DeviceId::new(), String::new());
        let malformed = (DeviceId::new(), "not base64!".to_string());
        let short = (DeviceId::new(), {
            use base64::Engine;
            base64::engine::general_purpose::URL_SAFE_NO_PAD.encode([0u8; 31])
        });

        let entry = |id: Option<DeviceId>, raw: Option<&str>| {
            let mut d = json!({"name": "d", "kind": "desktop"});
            if let Some(id) = id {
                d["id"] = id.to_string().into();
            }
            if let Some(raw) = raw {
                d["noise_pubkey"] = raw.into();
            }
            d
        };
        let body = json!({"devices": [
            entry(Some(good.0), Some(&good.1)),
            entry(Some(silent), None),
            entry(Some(blank.0), Some(&blank.1)),
            entry(Some(malformed.0), Some(&malformed.1)),
            entry(Some(short.0), Some(&short.1)),
        ]});
        assert!(
            !mesh.update_devices(&body),
            "an unpinnable entry reads as a failed fetch"
        );

        let peers = mesh.peers();
        assert_eq!(peers.len(), 1, "only the pinnable device is listed");
        assert_eq!(peers[0].id, good.0);
        assert_eq!(peers[0].problem, None, "the good device has no problem");
        assert_eq!(peers[0].pubkey, decode(&good.1), "its key decodes");
        let pins = pins_at(&mesh);
        assert_eq!(pins.len(), 1, "nothing unpinnable was stored");
        assert_eq!(pins[&good.0.to_string()], good.1, "the good key is pinned");

        // The registry recovers and serves the real key: no key_changed,
        // because nothing was ever pinned for these devices.
        let recovered = json!({"devices": [
            entry(Some(good.0), Some(&good.1)),
            entry(Some(silent), Some(&key())),
            entry(Some(blank.0), Some(&key())),
            entry(Some(malformed.0), Some(&key())),
            entry(Some(short.0), Some(&key())),
        ]});
        assert!(
            mesh.update_devices(&recovered),
            "the recovered list is clean"
        );
        for id in [&silent, &blank.0, &malformed.0, &short.0] {
            let peer = mesh.peer(*id).expect("recovered into the list");
            assert_eq!(peer.problem, None, "the real key pins without a flag");
        }
        assert_eq!(pins_at(&mesh).len(), 5, "each device ended up pinned once");

        // One glitchy fetch after a good one: the entry keeps its last
        // good self (and its link) instead of being dropped from the list.
        let glitch = json!({"devices": [entry(Some(good.0), None)]});
        assert!(!mesh.update_devices(&glitch));
        let kept = mesh
            .peer(good.0)
            .expect("a skipped entry keeps its last good self");
        assert_eq!(kept.problem, None);
        assert_eq!(kept.pubkey, decode(&good.1), "the last good key survives");
        // A device the list left out entirely is still removed, as before.
        assert!(mesh.peer(silent).is_none(), "absent still means removed");

        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// A pin that isn't valid key material was never a real key, so the
    /// real key later is a re-pin, not a change. A genuine key change is
    /// still flagged.
    #[test]
    fn an_empty_legacy_pin_is_healed_and_a_real_change_is_still_flagged() {
        let (mesh, dir) = test_mesh();
        let id = DeviceId::new();
        assert!(
            !mesh.pins_file.exists(),
            "the throwaway pins file starts clean"
        );

        // An older daemon pinned the empty key a glitchy fetch served.
        openremote_core::fsx::write_private(
            &mesh.pins_file,
            &serde_json::to_vec(&json!({id.to_string(): ""})).unwrap(),
        )
        .unwrap();

        let real = key();
        let body = json!({"devices": [
            {"id": id.to_string(), "noise_pubkey": real, "name": "d", "kind": "desktop"},
        ]});
        assert!(mesh.update_devices(&body), "the list is otherwise clean");
        let peer = mesh.peer(id).unwrap();
        assert_eq!(peer.problem, None, "the empty pin is healed, not flagged");
        assert_eq!(peer.pubkey, decode(&real));
        assert_eq!(
            pins_at(&mesh)[&id.to_string()],
            real,
            "the pin is the real key"
        );

        // The registry's key genuinely changing is still refused.
        let changed = json!({"devices": [
            {"id": id.to_string(), "noise_pubkey": key(), "name": "d", "kind": "desktop"},
        ]});
        assert!(mesh.update_devices(&changed));
        assert_eq!(
            mesh.peer(id).unwrap().problem,
            Some("key_changed"),
            "a real change is still flagged"
        );

        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// Two failed device-list fetches in a row are an outage, not a blip: the
    /// console gets a notice next to the phase. It is raised once per run of
    /// failures (rewriting it every five minutes would only churn the view)
    /// and the next good fetch takes it back down - but only when the notice
    /// on screen is still the one this loop raised.
    #[test]
    fn a_failing_device_list_is_surfaced_once_per_run_and_cleared_once() {
        let mut surfacer = Surfacer::default();
        assert_eq!(
            surfacer.failed(Phase::Online, None),
            None,
            "one failed fetch is a blip"
        );
        assert_eq!(
            surfacer.failed(Phase::Online, None),
            Some((Phase::Online, Some(UNREACHABLE))),
            "two in a row is an outage the user should see"
        );
        assert_eq!(
            surfacer.failed(Phase::Online, Some(UNREACHABLE)),
            None,
            "the notice is raised once, not rewritten on every later failure"
        );
        assert_eq!(
            surfacer.recovered(Phase::Online, Some(UNREACHABLE)),
            Some(Phase::Online),
            "the next good fetch clears it"
        );
        assert_eq!(
            surfacer.failed(Phase::Online, None),
            None,
            "the run of failures restarted with that success"
        );
    }

    /// A notice that came from somewhere that knows more - a takeover, a
    /// removal, a prompt to sign in - is never replaced by the refresher's
    /// generic one, however long the outage runs.
    #[test]
    fn a_failing_device_list_never_overwrites_a_more_specific_notice() {
        let takeover = Some("this device connected from somewhere else");
        let mut surfacer = Surfacer::default();
        for _ in 0..5 {
            assert_eq!(
                surfacer.failed(Phase::Offline, takeover),
                None,
                "a specific notice is left alone"
            );
        }
    }

    /// Clearing is the mirror image: a good fetch may only take down the
    /// notice this loop raised, never one another writer put there.
    #[test]
    fn the_refresher_clears_only_the_notice_it_raised() {
        // The loop raised its own notice at Offline; meanwhile something that
        // knew more replaced it with the reason.
        let mut replaced = Surfacer {
            failures: REFRESH_FAILURES_TO_ERROR,
            at: Some(Phase::Offline),
        };
        assert_eq!(
            replaced.recovered(
                Phase::Offline,
                Some("This device was removed from your account.")
            ),
            None,
            "another writer's notice survives a good fetch"
        );
        // Its own notice still comes down.
        let mut own = Surfacer {
            failures: REFRESH_FAILURES_TO_ERROR,
            at: Some(Phase::Offline),
        };
        assert_eq!(
            own.recovered(Phase::Offline, Some(UNREACHABLE)),
            Some(Phase::Offline),
            "the notice this loop raised is its to clear"
        );
        assert_eq!(
            own.failed(Phase::Offline, None),
            None,
            "and the next outage starts counting from scratch"
        );
    }

    /// A handshake that keeps failing must reach the console as an outage:
    /// one failed attempt is a deploy-window blip, a run of them is a relay
    /// nobody can get to, and the phase must not sit at "connecting" with no
    /// notice. It is raised once per run, and a session that did come up is
    /// never reported as one.
    #[test]
    fn a_run_of_failed_handshakes_surfaces_an_outage_once() {
        assert_eq!(
            retry_notice(false, None, false, 1),
            None,
            "one failed handshake is a blip"
        );
        assert_eq!(
            retry_notice(false, None, false, HANDSHAKE_FAILURES_TO_ERROR - 1),
            None,
            "two in a row are still a blip"
        );
        assert_eq!(
            retry_notice(false, None, false, HANDSHAKE_FAILURES_TO_ERROR),
            Some((Phase::Offline, Some(UNREACHABLE))),
            "three in a row is an outage the user should see"
        );
        assert_eq!(
            retry_notice(false, None, false, HANDSHAKE_FAILURES_TO_ERROR + 4),
            None,
            "the notice is raised once, not rewritten on every later retry"
        );
        assert_eq!(
            retry_notice(false, None, true, HANDSHAKE_FAILURES_TO_ERROR),
            None,
            "a session that came up is not an outage"
        );
    }

    /// An "online" that is no longer true is demoted on the first retry,
    /// whether or not the session had established and however it ended.
    #[test]
    fn online_is_demoted_on_the_first_retry() {
        assert_eq!(
            retry_notice(true, None, true, 0),
            Some((Phase::Offline, Some(DROPPED))),
            "a link that was up and dropped is not left saying online"
        );
        assert_eq!(
            retry_notice(true, None, false, 1),
            Some((Phase::Offline, Some(DROPPED))),
            "and neither is one whose session was already gone"
        );
        assert_eq!(
            retry_notice(
                true,
                Some("this device connected from somewhere else"),
                true,
                0
            ),
            Some((Phase::Offline, None)),
            "the phase is demoted, but a specific reason is not replaced by a vaguer one"
        );
    }

    /// The generic outage sentence never lands on top of the one that
    /// explains why the phase moved.
    #[test]
    fn an_outage_never_overwrites_a_more_specific_notice() {
        let takeover = Some("this device connected from somewhere else");
        assert_eq!(
            retry_notice(false, takeover, false, HANDSHAKE_FAILURES_TO_ERROR),
            None,
            "the takeover sentence is left alone"
        );
    }

    /// A notice the relay cleared is raised again. The relay writes "online"
    /// with no sentence when the link comes back, so the phase and the reason
    /// this loop put on screen can both go away without any fetch succeeding.
    /// Nothing else clears the failure count, so the notice must come back on
    /// the next failed fetch: the alternative was a console reading "online"
    /// beside a frozen device list for the whole outage.
    #[test]
    fn a_notice_another_writer_cleared_is_raised_again_on_the_next_failed_fetch() {
        let mut surfacer = Surfacer::default();
        assert_eq!(
            surfacer.failed(Phase::Offline, None),
            None,
            "one failed fetch is a blip"
        );
        assert_eq!(
            surfacer.failed(Phase::Offline, None),
            Some((Phase::Offline, Some(UNREACHABLE))),
            "the outage is raised"
        );
        assert_eq!(
            surfacer.failed(Phase::Offline, Some(UNREACHABLE)),
            None,
            "and is not rewritten while it is still on screen"
        );

        // The relay reconnects: the phase is now Online with no sentence, so
        // the notice this loop raised is gone from the view.
        assert_eq!(
            surfacer.failed(Phase::Online, None),
            Some((Phase::Online, Some(UNREACHABLE))),
            "a cleared notice is raised again"
        );
        assert_eq!(
            surfacer.failed(Phase::Online, Some(DROPPED)),
            Some((Phase::Online, Some(UNREACHABLE))),
            "a generic sentence from the other loop is not a reason to stay quiet either"
        );
        assert_eq!(
            surfacer.failed(Phase::Online, Some(UNREACHABLE)),
            None,
            "and once it is back on screen the run settles again"
        );
    }

    /// An inbound stream whose END never comes gives its slot back at its
    /// deadline; a live one keeps it.
    #[test]
    fn expired_inbound_streams_reclaim_their_slots() {
        let a = DeviceId::new();
        let b = DeviceId::new();
        let live = || Request {
            method: "GET".into(),
            path: "/".into(),
            content_type: None,
            body: Vec::new(),
        };
        let mut inbound = HashMap::new();
        let now = Instant::now();
        inbound.insert(
            (a, 1),
            Inbound {
                request: live(),
                dispatched: false,
                generation: 0,
                deadline: now - Duration::from_secs(1),
            },
        );
        inbound.insert(
            (a, 2),
            Inbound {
                request: live(),
                dispatched: false,
                generation: 0,
                deadline: now, // exactly at the deadline: gone
            },
        );
        inbound.insert(
            (a, 3),
            Inbound {
                request: live(),
                dispatched: false,
                generation: 0,
                deadline: now + Duration::from_secs(60),
            },
        );
        inbound.insert(
            (b, 1),
            Inbound {
                request: live(),
                dispatched: false,
                generation: 0,
                deadline: now + Duration::from_secs(60),
            },
        );
        assert_eq!(reclaim_expired(&mut inbound, now), 2, "the stale pair goes");
        assert_eq!(inbound.len(), 2, "the live ones stay");
        assert!(inbound.contains_key(&(a, 3)));
        assert!(inbound.contains_key(&(b, 1)));
        assert_eq!(
            reclaim_expired(&mut inbound, now + Duration::from_secs(61)),
            2,
            "the next sweep takes what has aged out since"
        );
        assert!(inbound.is_empty());
    }

    /// The window is measured after the sweep, so a peer whose slots are all
    /// abandoned streams can talk again the moment it comes back, without
    /// waiting for the periodic sweep or a reconnect. A live window still
    /// refuses: the cap itself is untouched.
    #[test]
    fn a_stalled_peers_abandoned_slots_are_reclaimed_before_the_next_open() {
        let (mesh, dir) = test_mesh();
        let mesh = Arc::new(mesh);
        let peer = DeviceId::new();
        let header = br#"{"method":"GET","path":"/devices"}"#;
        let entry = |deadline: Instant| Inbound {
            request: Request {
                method: "GET".into(),
                path: "/devices".into(),
                content_type: None,
                body: Vec::new(),
            },
            deadline,
            dispatched: false,
            generation: 0,
        };
        {
            let mut st = mesh.state();
            for stream in 0..MAX_INBOUND_PER_PEER as u32 {
                st.inbound.insert(
                    (peer, stream),
                    entry(Instant::now() - Duration::from_secs(1)),
                );
            }
        }
        mesh.on_rpc(peer, &rpc::frame(rpc::OPEN, 99, header));
        {
            let st = mesh.state();
            assert_eq!(
                st.inbound.len(),
                1,
                "the abandoned slots went, and the new stream took one"
            );
            assert!(
                st.inbound.contains_key(&(peer, 99)),
                "the peer is not refused \"busy\" for streams it already gave up on"
            );
        }

        {
            let mut st = mesh.state();
            st.inbound.clear();
            for stream in 100..100 + MAX_INBOUND_PER_PEER as u32 {
                st.inbound
                    .insert((peer, stream), entry(Instant::now() + INBOUND_TTL));
            }
        }
        mesh.on_rpc(peer, &rpc::frame(rpc::OPEN, 200, header));
        let st = mesh.state();
        assert_eq!(
            st.inbound.len(),
            MAX_INBOUND_PER_PEER,
            "a window of live streams is still full"
        );
        assert!(
            !st.inbound.contains_key(&(peer, 200)),
            "and the stream over the cap is still refused"
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// An attempt that times out must take its dead sender with it. Only a
    /// completed handshake ever empties `waiters`, so a peer that is listed,
    /// online and never answers used to leave one sender behind per call, for
    /// the life of the process. This test pays the real price of the timeout
    /// path: three attempts, four seconds apart.
    #[tokio::test]
    async fn a_timed_out_link_attempt_leaves_no_sender_behind() {
        let (mesh, _dir) = test_mesh();
        let mesh = Arc::new(mesh);
        let peer = listed_and_online(&mesh);
        assert!(
            matches!(mesh.ensure_link(peer).await, Err(MeshError::Unreachable(_))),
            "nothing answered, so there is no link"
        );
        let st = mesh.state();
        assert_eq!(
            st.waiters.get(&peer).map_or(0, Vec::len),
            0,
            "the timed-out attempts took their senders with them"
        );
        assert!(
            !st.attempts.contains_key(&peer),
            "and the window closed, so the next nudge starts a fresh attempt"
        );
    }

    /// A nudging device may send as fast as it likes. Without coalescing every
    /// frame spawns an attempt task, and every task parks a sender in
    /// `waiters` for its own four seconds: a listed peer that never answers
    /// grows this process with every frame it sends.
    #[tokio::test]
    async fn a_flood_of_nudges_leaves_one_attempt_in_flight() {
        let (mesh, _dir) = test_mesh();
        let mesh = Arc::new(mesh);
        // The higher-numbered device nudges, so ours has to be the lower one
        // for the frame to be acted on at all.
        let mut peer = DeviceId::new();
        while peer.to_bytes() <= mesh.me.to_bytes() {
            peer = DeviceId::new();
        }
        {
            let mut st = mesh.state();
            st.peers.insert(peer, listed(peer));
            st.online.insert(peer);
        }
        for _ in 0..200 {
            mesh.on_frame(peer, &[NUDGE]);
        }
        // Long enough for every spawned task to have run up to its first
        // await, which is where a sender sits in `waiters`.
        tokio::time::sleep(Duration::from_millis(250)).await;
        let pending = mesh.state().waiters.get(&peer).map_or(0, Vec::len);
        assert!(
            pending <= 1,
            "one attempt, not one per nudge: {pending} senders are waiting"
        );
    }

    /// The window is a cap on the work this daemon does for one peer, not on
    /// half-written requests. A peer that OPENs and ENDs a stream at once gets
    /// its request running, and the slot stays held until the answer is
    /// sealed: 32 at a time however fast it ENDs them, and the 33rd waits.
    #[tokio::test]
    async fn an_ended_stream_keeps_its_slot_until_its_answer_is_sealed() {
        let started = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let release = Arc::new(tokio::sync::Notify::new());
        let host = Arc::new(Parked {
            started: Arc::clone(&started),
            release: Arc::clone(&release),
        });
        let (mesh, _out, _dir) = mesh_over(host);
        let mesh = Arc::new(mesh);
        let peer = DeviceId::new();
        let header = br#"{"method":"GET","path":"/devices"}"#;
        for stream in 0..=MAX_INBOUND_PER_PEER as u32 {
            mesh.on_rpc(peer, &rpc::frame(rpc::OPEN, stream, header));
            mesh.on_rpc(peer, &rpc::frame(rpc::END, stream, b""));
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
        assert_eq!(
            mesh.state().inbound.len(),
            MAX_INBOUND_PER_PEER,
            "the streams that started keep their slots while they run"
        );
        assert_eq!(
            started.load(std::sync::atomic::Ordering::SeqCst),
            MAX_INBOUND_PER_PEER,
            "and only 32 requests are running here, not 33"
        );
        release.notify_waiters();
        tokio::time::sleep(Duration::from_millis(250)).await;
        assert_eq!(
            mesh.state().inbound.len(),
            0,
            "the slots come back when the answers are sealed"
        );
    }

    /// One stream id, taken again and again the moment its END has started the
    /// work: the window counts entries, so without a refusal for an id that is
    /// already here the count never moves while the dispatches pile up. A peer
    /// gets one dispatch per id, and a new request has to wait for the old one
    /// to answer.
    #[tokio::test]
    async fn one_stream_id_buys_one_dispatch_not_a_hundred() {
        let started = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let release = Arc::new(tokio::sync::Notify::new());
        let host = Arc::new(Parked {
            started: Arc::clone(&started),
            release: Arc::clone(&release),
        });
        let (mesh, _out, _dir) = mesh_over(host);
        let mesh = Arc::new(mesh);
        let peer = DeviceId::new();
        let header = br#"{"method":"GET","path":"/devices"}"#;
        for _ in 0..100 {
            mesh.on_rpc(peer, &rpc::frame(rpc::OPEN, 7, header));
            mesh.on_rpc(peer, &rpc::frame(rpc::DATA, 7, b"x"));
            mesh.on_rpc(peer, &rpc::frame(rpc::END, 7, b""));
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
        assert_eq!(
            started.load(std::sync::atomic::Ordering::SeqCst),
            1,
            "one stream id bought one dispatch, not one per END"
        );
        assert_eq!(
            mesh.state().inbound.len(),
            1,
            "and that one stream is still the only slot it holds"
        );
        release.notify_waiters();
        tokio::time::sleep(Duration::from_millis(250)).await;
        assert_eq!(
            mesh.state().inbound.len(),
            0,
            "the id comes back when its answer is sealed"
        );
    }

    /// A dispatch that outlives the TTL sweep must not take the next request's
    /// slot with it when it finally answers: the peer may hold the id again by
    /// then, and the slot it is giving back is no longer its own.
    #[tokio::test]
    async fn a_dispatch_that_outlived_its_slot_does_not_steal_the_next_one() {
        let started = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let release = Arc::new(tokio::sync::Notify::new());
        let host = Arc::new(Parked {
            started: Arc::clone(&started),
            release: Arc::clone(&release),
        });
        let (mesh, _out, _dir) = mesh_over(host);
        let mesh = Arc::new(mesh);
        let peer = DeviceId::new();
        let header = br#"{"method":"GET","path":"/devices"}"#;
        mesh.on_rpc(peer, &rpc::frame(rpc::OPEN, 7, header));
        mesh.on_rpc(peer, &rpc::frame(rpc::END, 7, b""));
        tokio::time::sleep(Duration::from_millis(100)).await;
        // The sweep takes the slot while the work still runs, exactly as it
        // does for a peer that froze; the peer then takes the id again.
        {
            let mut st = mesh.state();
            st.inbound
                .get_mut(&(peer, 7))
                .expect("the running slot")
                .deadline = Instant::now() - Duration::from_secs(1);
        }
        mesh.on_rpc(peer, &rpc::frame(rpc::OPEN, 7, header));
        assert_eq!(
            mesh.state().inbound.len(),
            1,
            "the reclaimed id was taken by a new request"
        );
        release.notify_waiters();
        tokio::time::sleep(Duration::from_millis(250)).await;
        let st = mesh.state();
        assert_eq!(
            st.inbound.len(),
            1,
            "the old dispatch answered without removing a slot it no longer owned"
        );
        assert!(
            !st.inbound.get(&(peer, 7)).unwrap().dispatched,
            "and the new request is still waiting for its END"
        );
    }

    /// A nudge on a link that is already up is ordinary: the other side's
    /// reconnect went unnoticed here. Nothing has to be attempted, and
    /// spawning a task per frame to discover that is churn; the window exists
    /// for attempts that have somewhere to go.
    #[tokio::test]
    async fn a_nudge_on_a_link_that_is_up_spawns_no_attempt() {
        let (mesh, _dir) = test_mesh();
        let mesh = Arc::new(mesh);
        // The higher-numbered device nudges, so ours has to be the lower one
        // for the frame to be acted on at all.
        let mut peer = DeviceId::new();
        while peer.to_bytes() <= mesh.me.to_bytes() {
            peer = DeviceId::new();
        }
        let up = up_link(&mesh, peer);
        {
            let mut st = mesh.state();
            st.peers.insert(peer, listed(peer));
            st.online.insert(peer);
            st.links.insert(peer, up);
        }
        for _ in 0..50 {
            mesh.on_frame(peer, &[NUDGE]);
        }
        // Read before anything is awaited: the arm decides synchronously.
        assert!(
            !mesh.state().attempts.contains_key(&peer),
            "a nudge on a live link records no attempt and spawns no task"
        );
    }

    /// A mesh that cannot be built says so. `mesh::start` used to answer
    /// `None` in silence, which left the console at "connecting" for good
    /// whenever a `cloud/` file was damaged while the daemon was running.
    #[test]
    fn a_mesh_that_cannot_be_built_moves_the_phase_and_says_why() {
        let dir = std::env::temp_dir().join(format!("or-mesh-open-{}", uuid::Uuid::new_v4()));
        openremote_core::fsx::create_private_dir(&dir).unwrap();
        openremote_core::fsx::create_private_dir(&dir.join("cloud")).unwrap();
        crate::identity::Files::new(&dir)
            .save_credentials(&crate::identity::Credentials {
                device_credential: Some("a device credential".into()),
                account: Some(crate::identity::Account {
                    id: "acc-1".into(),
                    email: "owner@example.test".into(),
                    name: "owner".into(),
                }),
                kind: "desktop".into(),
                ..Default::default()
            })
            .unwrap();
        // Truncated the way a crash mid-write leaves it.
        std::fs::write(dir.join("cloud").join("identity.json"), b"{\"device_id\": ").unwrap();
        let config = crate::CloudConfig {
            api_base: "http://127.0.0.1:9/v1/openremote".into(),
            auth_issuer: "http://127.0.0.1:9/v1/auth".into(),
            client_id: crate::config::CLIENT_ID.into(),
            device_name: "this-box".into(),
            platform: "windows".into(),
            redirect_uri: None,
        };
        let cloud = Cloud::open(config, &dir);
        assert_eq!(cloud.phase(), Phase::Relink, "open names the damage");

        // The same belt for a file that is damaged under a running daemon:
        // starting the mesh must not return in silence either.
        cloud.set_phase(Phase::Connecting, None);
        assert!(
            start(&cloud, Arc::new(Nop)).is_none(),
            "no mesh can be built from a damaged identity"
        );
        assert_eq!(
            cloud.phase(),
            Phase::Relink,
            "the console is not left connecting with nothing to read"
        );
        assert_eq!(
            cloud.view()["error"].as_str(),
            Some(crate::IDENTITY_BROKEN),
            "and it says which piece is broken"
        );

        // The file disappearing is the other shape, and it must not borrow the
        // sentence that tells the user to reset their files: signing in again
        // fixes this one.
        crate::identity::Files::new(&dir).forget_identity().unwrap();
        cloud.set_phase(Phase::Connecting, None);
        assert!(
            start(&cloud, Arc::new(Nop)).is_none(),
            "no mesh can be built from a missing identity either"
        );
        assert_eq!(
            cloud.view()["error"].as_str(),
            Some(crate::IDENTITY_MISSING),
            "a missing identity is offered the sign-in that joins again"
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// A host that writes down which peers it was told are online.
    #[derive(Default)]
    struct OnlineLog(Mutex<Vec<DeviceId>>);
    impl OnlineLog {
        fn told(&self) -> Vec<DeviceId> {
            self.0.lock().unwrap().clone()
        }
    }
    impl CloudHost for OnlineLog {
        fn dispatch(&self, _: PeerContext, _: Request) -> BoxFuture<'static, Response> {
            Box::pin(std::future::ready(Response {
                status: 200,
                content_type: None,
                body: Vec::new(),
            }))
        }
        fn note(&self, _: PeerContext, _: Value) {}
        fn linked(&self, _: DeviceId) {}
        fn online(&self, peer: DeviceId) {
            self.0.lock().unwrap().push(peer);
        }
        fn revoked(&self) {}
    }

    fn devices(ids: &[DeviceId]) -> Value {
        json!({"devices": ids.iter().map(|id| json!({
            "id": id.to_string(), "noise_pubkey": key(), "name": "d", "kind": "machine",
        })).collect::<Vec<_>>()})
    }

    #[test]
    fn a_peer_online_before_the_list_names_it_is_announced_when_it_does() {
        // A device that just signed in hears from the relay who is online
        // before its first device list arrives. The host's compare with those
        // peers failed for want of a key; when the list names them, the host
        // must hear again, or the two devices meet only on the periodic
        // compare minutes later.
        let log = Arc::new(OnlineLog::default());
        let (mesh, _out, dir) = mesh_over(Arc::clone(&log) as Arc<dyn CloudHost>);
        let known = DeviceId::new();
        let early = DeviceId::new();
        let fleeting = DeviceId::new();
        mesh.update_devices(&devices(&[known]));
        mesh.set_online(&[known, early, mesh.me]);
        mesh.presence(fleeting, true);
        mesh.presence(fleeting, false);
        assert!(log.told().is_empty(), "the bookkeeping itself tells nobody");

        mesh.update_devices(&devices(&[known, early, fleeting]));
        assert_eq!(
            log.told(),
            vec![early],
            "only the peer that was online and unlisted; not the one already \
             listed, not one that left, not this device"
        );

        // The next refresh does not say it again.
        mesh.update_devices(&devices(&[known, early, fleeting]));
        assert_eq!(log.told(), vec![early]);

        // A peer that comes online unlisted later is caught the same way.
        let late = DeviceId::new();
        mesh.presence(late, true);
        mesh.update_devices(&devices(&[known, early, fleeting, late]));
        assert_eq!(log.told(), vec![early, late]);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[tokio::test]
    async fn a_nudge_from_a_device_the_list_does_not_know_fetches_the_list() {
        let (mut mesh, _out, dir) = mesh_over(Arc::new(Nop));
        // The higher-numbered device sends the nudge to the initiator.
        mesh.me = DeviceId::from_bytes([0; 16]);
        let stranger = DeviceId::from_bytes([1; 16]);
        let mesh = Arc::new(mesh);
        mesh.on_frame(stranger, &[NUDGE]);
        assert!(
            mesh.state().attempts.is_empty(),
            "no attempt that could only fail for want of a key"
        );
        assert!(
            tokio::time::timeout(Duration::from_millis(100), mesh.refresh.notified())
                .await
                .is_ok(),
            "the list is asked for again"
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
