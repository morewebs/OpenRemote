//! The mesh: this device's links to the account's other devices, over the
//! relay. Peers and their public keys come from the registry; the first key
//! seen for each device is pinned, and a device whose key later changes is
//! never linked again. Links come up on demand and drop whenever a frame may
//! have been lost (the peer went offline, the relay reconnected, a message
//! failed to decrypt); the next request simply links again.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

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

#[derive(Default)]
struct State {
    peers: HashMap<DeviceId, Peer>,
    links: HashMap<DeviceId, Link>,
    waiters: HashMap<DeviceId, Vec<oneshot::Sender<()>>>,
    calls: HashMap<(DeviceId, u32), Call>,
    inbound: HashMap<(DeviceId, u32), Request>,
    next_stream: u32,
    auto_update: bool,
    online: std::collections::HashSet<DeviceId>,
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

    /// Takes the registry's device list. New keys are pinned; a changed key
    /// marks the device and its key is not used.
    pub fn update_devices(&self, body: &Value) {
        let mut pins = self.pins();
        let mut pinned_new = false;
        let mut peers = HashMap::new();
        for d in body["devices"].as_array().into_iter().flatten() {
            let Some(id) = d["id"].as_str().and_then(DeviceId::parse) else {
                continue;
            };
            let key = d["noise_pubkey"].as_str().unwrap_or_default().to_string();
            let problem = match pins.get(&id.to_string()) {
                Some(pin) if *pin != key => Some("key_changed"),
                Some(_) => None,
                None => {
                    pins.insert(id.to_string(), key.clone());
                    pinned_new = true;
                    None
                }
            };
            if id == self.me {
                *self.kind.lock().unwrap_or_else(|e| e.into_inner()) =
                    d["kind"].as_str().unwrap_or("desktop").to_string();
            }
            use base64::Engine;
            let pubkey = base64::engine::general_purpose::URL_SAFE_NO_PAD
                .decode(&key)
                .unwrap_or_default();
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
    }

    fn set_online(&self, ids: &[DeviceId]) {
        self.state().online = ids.iter().copied().collect();
    }

    fn presence(&self, id: DeviceId, online: bool) {
        let mut st = self.state();
        if online {
            st.online.insert(id);
        } else {
            st.online.remove(&id);
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
        for attempt in 0..3 {
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
                let open = st.inbound.keys().filter(|(p, _)| *p == from).count();
                let request = serde_json::from_slice::<Value>(body).ok().map(|h| Request {
                    method: h["method"].as_str().unwrap_or("GET").to_string(),
                    path: h["path"].as_str().unwrap_or("/").to_string(),
                    content_type: h["content_type"].as_str().map(str::to_string),
                    body: Vec::new(),
                });
                match request {
                    Some(r) if open < MAX_INBOUND_PER_PEER => {
                        st.inbound.insert((from, stream), r);
                    }
                    _ => {
                        let reset = rpc::frame(rpc::RESET, stream, b"busy");
                        self.send_sealed(&mut st, from, &reset);
                    }
                }
            }
            rpc::DATA => {
                let too_big = match st.inbound.get_mut(&(from, stream)) {
                    Some(r) => {
                        r.body.extend_from_slice(body);
                        r.body.len() > rpc::MAX_REQUEST
                    }
                    None => false,
                };
                if too_big {
                    st.inbound.remove(&(from, stream));
                    let reset = rpc::frame(rpc::RESET, stream, b"too large");
                    self.send_sealed(&mut st, from, &reset);
                }
            }
            rpc::END => {
                let Some(request) = st.inbound.remove(&(from, stream)) else {
                    return;
                };
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
                st.inbound.remove(&(from, stream));
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

/// Drops a peer's link and fails everything in flight with it.
fn fail_peer(st: &mut State, peer: DeviceId, _why: &str) {
    st.links.remove(&peer);
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
}

/// Starts the mesh for a joined device: the relay connection (kept up with
/// backoff) and the device-list refresher.
pub fn start(
    cloud: &Arc<Cloud>,
    host: Arc<dyn CloudHost>,
) -> Option<(Arc<Mesh>, Vec<tokio::task::JoinHandle<()>>)> {
    let identity = cloud.identity()?;
    let account = cloud.view()["account"]["id"].as_str()?.to_string();
    let kind = cloud.view()["device"]["kind"]
        .as_str()
        .unwrap_or("desktop")
        .to_string();
    let (out, out_rx) = mpsc::unbounded_channel();
    let mesh = Arc::new(Mesh {
        me: identity.device_id,
        account,
        private: identity.private_key(),
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
    Some((mesh, vec![relay, refresher]))
}

/// Keeps the device list current: at start, when the relay says it
/// changed, when an unknown device knocks, and every few minutes.
async fn refresh_devices(cloud: Arc<Cloud>, mesh: Arc<Mesh>) {
    loop {
        if let Some(credential) = cloud.device_credential() {
            match cloud.registry().devices(&credential).await {
                Ok(body) => mesh.update_devices(&body),
                Err(crate::registry::RegistryError::Revoked) => {
                    cloud.removed(&*mesh.host);
                    return;
                }
                Err(_) => {}
            }
        }
        tokio::select! {
            _ = mesh.refresh.notified() => {}
            _ = tokio::time::sleep(Duration::from_secs(300)) => {}
        }
    }
}

async fn run_relay(
    cloud: Arc<Cloud>,
    mesh: Arc<Mesh>,
    mut out_rx: mpsc::UnboundedReceiver<(DeviceId, Vec<u8>)>,
) {
    let mut backoff = Duration::from_secs(1);
    loop {
        let Some(credential) = cloud.device_credential() else {
            return;
        };
        match relay_session(&cloud, &mesh, &credential, &mut out_rx).await {
            Ended::Stop => return,
            Ended::Retry => {}
        }
        mesh.drop_links();
        if cloud.phase() == Phase::Online {
            cloud.set_phase(Phase::Offline, None);
            // The link was up: this is a fresh drop, not a run of failures.
            backoff = Duration::from_secs(1);
        }
        // Jitter, so a relay restart isn't met by every device at once.
        let jitter = Duration::from_millis(u64::from(uuid::Uuid::new_v4().as_bytes()[0]) * 4);
        tokio::select! {
            _ = tokio::time::sleep(backoff + jitter) => {
                backoff = (backoff * 2).min(Duration::from_secs(60));
            }
            _ = mesh.wake.notified() => backoff = Duration::from_secs(1),
        }
        // Frames queued while away can't be delivered in order any more.
        while out_rx.try_recv().is_ok() {}
    }
}

async fn relay_session(
    cloud: &Arc<Cloud>,
    mesh: &Arc<Mesh>,
    credential: &str,
    out_rx: &mut mpsc::UnboundedReceiver<(DeviceId, Vec<u8>)>,
) -> Ended {
    let url = cloud.config().relay_url();
    let Ok(uri) = url.parse::<http::Uri>() else {
        return Ended::Stop;
    };
    let Ok(mut request) = url.as_str().into_client_request() else {
        return Ended::Stop;
    };
    let auth = format!("Bearer {credential}");
    if let Ok(value) = auth.parse() {
        request.headers_mut().insert("authorization", value);
    }
    if let Ok(value) = env!("CARGO_PKG_VERSION").parse() {
        request.headers_mut().insert("x-openremote-version", value);
    }
    let Ok(stream) = cloud.http().connect(&uri).await else {
        cloud.set_phase(Phase::Offline, Some("can't reach OpenRemote Cloud".into()));
        return Ended::Retry;
    };
    let ws = match tokio_tungstenite::client_async(request, stream).await {
        Ok((ws, _)) => ws,
        Err(tokio_tungstenite::tungstenite::Error::Http(response)) => {
            return match response.status().as_u16() {
                401 => {
                    cloud.set_phase(
                        Phase::Relink,
                        Some("sign in to OpenRemote Cloud again".into()),
                    );
                    Ended::Stop
                }
                410 => {
                    cloud.removed(&*mesh.host);
                    Ended::Stop
                }
                _ => Ended::Retry,
            };
        }
        Err(_) => return Ended::Retry,
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
                    _ => return Ended::Retry,
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
                                Ended::Stop
                            }
                            Some(4001) => {
                                cloud.set_phase(Phase::Offline, Some("this device connected from somewhere else".into()));
                                Ended::Stop
                            }
                            _ => Ended::Retry,
                        };
                    }
                    _ => {}
                }
            }
            outgoing = out_rx.recv() => {
                let Some((to, payload)) = outgoing else { return Ended::Stop };
                let mut frame = to.to_bytes().to_vec();
                frame.extend_from_slice(&payload);
                if sink.send(Message::Binary(frame.into())).await.is_err() {
                    return Ended::Retry;
                }
            }
            _ = ping.tick() => {
                if sink.send(Message::Ping(Default::default())).await.is_err() {
                    return Ended::Retry;
                }
            }
            _ = mesh.wake.notified() => {
                if sink.send(Message::Ping(Default::default())).await.is_err() {
                    return Ended::Retry;
                }
                probe.get_or_insert(tokio::time::Instant::now() + Duration::from_secs(10));
            }
        }
    }
}
