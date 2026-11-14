//! OpenRemote Cloud on this device: signing in to moreweb, joining the
//! account's device list, and (next) the end-to-end encrypted link to the
//! user's other devices through the relay.
//!
//! A desktop joins by signing in (OAuth in the system browser, the code
//! coming back to this daemon's loopback address). A machine joins by
//! spending a one-time install code. Either way the device ends up with its
//! own credential and Noise key pair; the account's tokens stay on desktops.

pub mod config;
pub mod https;
pub mod identity;
pub mod link;
pub mod mesh;
pub mod oauth;
pub mod registry;
pub mod rpc;

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use openremote_core::DeviceId;
use serde::Serialize;
use serde_json::{Value, json};

pub use config::CloudConfig;
use https::Http;
use identity::{Account, Credentials, Files, Identity, Stored};
pub use mesh::{CloudHost, Mesh, MeshError, PeerContext};
use registry::{DeviceInfo, Registry, RegistryError};

/// A sign-in left in the browser this long is abandoned.
const SIGN_IN_WINDOW: Duration = Duration::from_secs(600);

/// What the console is told when this computer's own Cloud state cannot be
/// used. The phase moves to "sign in again" and the sentence names the damage,
/// so a file the daemon cannot read is never mistaken for a device that is
/// still connecting (which is a phase that would never end), and never for
/// anything the user did.
///
/// Each sentence also says what signing in again will get the user, because the
/// three cases differ. A missing identity mints a new one, so the account gains
/// a second device row and the old one is a ghost nobody on this computer can
/// remove: saying so is the honest thing to do. A damaged file cannot be minted
/// over (the mint never overwrites), so that sentence must not promise a sign-in
/// that would fail, and it names the only real way out. A damaged sign-in keeps
/// the identity, so signing in again returns as the same device.
pub(crate) const IDENTITY_MISSING: &str = "this computer's Cloud identity is missing - signing in again joins your account as a new device";
pub(crate) const IDENTITY_BROKEN: &str = "this computer's Cloud identity is damaged - it cannot rejoin until this computer's Cloud files are reset";
pub(crate) const KEY_BROKEN: &str = "this computer's Cloud key is damaged - it cannot rejoin until this computer's Cloud files are reset";
pub(crate) const SIGNIN_BROKEN: &str = "this computer's Cloud sign-in is damaged - sign in again";

/// What the console is told when something reaches this device with a
/// credential the backend no longer accepts. The row may still be in the
/// account, but nothing here can prove this computer is the device that owns
/// it, and only a sign-in can. The relay and the registry say it the same way,
/// so one const keeps them from drifting apart.
pub(crate) const SIGN_IN_AGAIN: &str = "sign in to OpenRemote Cloud again";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    SignedOut,
    SigningIn,
    /// Signed in; the relay link comes up next.
    Connecting,
    Online,
    Offline,
    /// The device credential stopped working: sign in again.
    Relink,
    /// This device was removed from the account.
    Revoked,
}

#[derive(Debug, thiserror::Error)]
pub enum CloudError {
    #[error("that sign-in expired or was already used - start again")]
    StaleSignIn,
    #[error("this computer is already in OpenRemote Cloud")]
    AlreadyEnrolled,
    #[error("sign in to OpenRemote Cloud first")]
    SignedOut,
    #[error("this computer's Cloud identity changed while joining - try again")]
    IdentityChanged,
    /// This computer's own `cloud/` files cannot carry a sign-in. The sentence
    /// is the one to show: it names the damage, not a request that failed and
    /// not a filesystem path.
    #[error("{0}")]
    CloudFilesBroken(String),
    #[error(transparent)]
    OAuth(#[from] oauth::OAuthError),
    #[error(transparent)]
    Registry(#[from] RegistryError),
    #[error("can't save this device's Cloud files: {0}")]
    Io(#[from] std::io::Error),
}

struct Pending {
    state: String,
    verifier: String,
    redirect_uri: String,
    started: Instant,
}

/// Sign-ins the browser may still finish. Every click on "Sign in" opens its
/// own tab with its own state, so a second click must not strand the first
/// tab; a handful is plenty and the oldest goes first.
const MAX_PENDING_SIGN_INS: usize = 4;

struct Inner {
    identity: Option<Identity>,
    credentials: Credentials,
    pending: Vec<Pending>,
    phase: Phase,
    error: Option<String>,
}

/// What a finished sign-in changed, for the daemon to act on.
pub struct SignedIn {
    pub account: Account,
    pub device_id: DeviceId,
    /// The account this computer was signed in to before, when it differs:
    /// that account's synced chats don't belong to the new one.
    pub previous_account: Option<Account>,
}

pub struct Cloud {
    config: CloudConfig,
    http: Http,
    registry: Registry,
    files: Files,
    data_dir: PathBuf,
    inner: Mutex<Inner>,
    running: Mutex<Option<Running>>,
}

/// One mesh run. It is alive exactly as long as its relay loop is: the
/// refresher on its own never ends (it polls the device list forever),
/// so the loop's handle is the run's liveness.
struct Running {
    mesh: Arc<Mesh>,
    relay: tokio::task::JoinHandle<()>,
    refresher: tokio::task::JoinHandle<()>,
}

fn now_s() -> i64 {
    openremote_core::now_ms() / 1000
}

/// A sha256 digest, raw bytes. The daemon's webhook route keys its
/// delivery-dedup on the body's digest - hook senders are outside the
/// console's request_id contract, so the body is the only idempotency
/// key they bring - and this is the crate that already hashes (the OAuth
/// PKCE challenge), so the digest lives here instead of a new
/// dependency somewhere else.
pub fn sha256(bytes: &[u8]) -> [u8; 32] {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(bytes);
    let mut out = [0u8; 32];
    out.copy_from_slice(&digest);
    out
}

impl Cloud {
    pub fn open(config: CloudConfig, data_dir: &Path) -> Arc<Self> {
        let http = Http::new();
        let files = Files::new(data_dir);
        let credentials = files.credentials_state();
        let identity = files.identity_state();
        // A credential only means this device is in Cloud when the identity
        // behind it can actually carry a mesh: the relay needs this device's
        // own key and the registry's row is keyed by its id. Opening as
        // "connecting" on the credential alone made a damaged or missing
        // `identity.json` a phase that never ended and never said why, so every
        // shape that cannot work opens as "sign in again" with its own reason.
        // Absence of both files is a fresh install, not damage: signed out, no
        // error, exactly as before.
        let (phase, error) = match (&credentials, &identity) {
            (Stored::Unreadable, _) => (Phase::SignedOut, Some(SIGNIN_BROKEN)),
            (Stored::Value(c), _) if c.device_credential.is_none() => (Phase::SignedOut, None),
            (Stored::Value(c), _) if c.account.is_none() => (Phase::Relink, Some(SIGNIN_BROKEN)),
            (Stored::Value(_), Stored::Value(id)) => match id.private_key() {
                Some(_) => (Phase::Connecting, None),
                None => (Phase::Relink, Some(KEY_BROKEN)),
            },
            (Stored::Value(_), Stored::Missing) => (Phase::Relink, Some(IDENTITY_MISSING)),
            (Stored::Value(_), Stored::Unreadable) => (Phase::Relink, Some(IDENTITY_BROKEN)),
            (Stored::Missing, _) => (Phase::SignedOut, None),
        };
        Arc::new(Self {
            registry: Registry::new(http.clone(), &config.api_base),
            data_dir: data_dir.to_path_buf(),
            running: Mutex::new(None),
            config,
            http,
            inner: Mutex::new(Inner {
                identity: identity.value(),
                credentials: credentials.value().unwrap_or_default(),
                pending: Vec::new(),
                phase,
                error: error.map(str::to_string),
            }),
            files,
        })
    }

    fn inner(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn config(&self) -> &CloudConfig {
        &self.config
    }

    pub fn phase(&self) -> Phase {
        self.inner().phase
    }

    /// The phase and the notice beside it, read together: a caller that
    /// decides what to write next must never act on half of a pair that
    /// someone else just replaced.
    pub(crate) fn phase_and_error(&self) -> (Phase, Option<String>) {
        let inner = self.inner();
        (inner.phase, inner.error.clone())
    }

    /// The console's picture of Cloud on this device.
    pub fn view(&self) -> Value {
        let inner = self.inner();
        let device = inner
            .identity
            .as_ref()
            .filter(|_| inner.credentials.device_credential.is_some());
        json!({
            "state": inner.phase,
            "error": inner.error,
            "account": inner.credentials.account,
            "device": device.map(|identity| json!({
                "id": identity.device_id,
                "name": self.config.device_name,
                "kind": inner.credentials.kind,
                "platform": self.config.platform,
            })),
        })
    }

    /// This device's id, once it has joined an account.
    pub fn device_id(&self) -> Option<DeviceId> {
        let inner = self.inner();
        inner
            .credentials
            .device_credential
            .as_ref()
            .and(inner.identity.as_ref().map(|i| i.device_id))
    }

    pub fn enrolled(&self) -> bool {
        self.inner().credentials.device_credential.is_some()
    }

    /// Starts a sign-in: the URL to open in the system browser. The code
    /// comes back to this daemon at `/cloud/callback` on `port`, or through
    /// the configured redirect (the app passes it on to that same route).
    pub fn begin_signin(&self, port: u16) -> String {
        let pkce = oauth::Pkce::new();
        let state = oauth::new_state();
        let redirect_uri = self
            .config
            .redirect_uri
            .clone()
            .unwrap_or_else(|| format!("http://127.0.0.1:{port}/cloud/callback"));
        let url = oauth::authorize_url(
            &self.config.auth_issuer,
            &self.config.client_id,
            &redirect_uri,
            &state,
            &pkce.challenge,
        );
        let mut inner = self.inner();
        inner
            .pending
            .retain(|p| p.started.elapsed() < SIGN_IN_WINDOW);
        if inner.pending.len() >= MAX_PENDING_SIGN_INS {
            inner.pending.remove(0);
        }
        inner.pending.push(Pending {
            state,
            verifier: pkce.verifier,
            redirect_uri,
            started: Instant::now(),
        });
        inner.error = None;
        if matches!(
            inner.phase,
            Phase::SignedOut | Phase::Relink | Phase::Revoked
        ) {
            inner.phase = Phase::SigningIn;
        }
        url
    }

    pub fn cancel_signin(&self) {
        let mut inner = self.inner();
        inner.pending.clear();
        if inner.phase == Phase::SigningIn {
            inner.phase = if inner.credentials.device_credential.is_some() {
                Phase::Connecting
            } else {
                Phase::SignedOut
            };
        }
    }

    /// The browser came back with a code: trade it for the account's
    /// tokens, then join (or re-join) the account as this device.
    pub async fn finish_signin(&self, code: &str, state: &str) -> Result<SignedIn, CloudError> {
        let pending = {
            let mut inner = self.inner();
            // A state that came back from an older tab is not a reason to
            // cancel the newer sign-in, and neither is a stale one.
            inner
                .pending
                .retain(|p| p.started.elapsed() < SIGN_IN_WINDOW);
            match inner.pending.iter().position(|p| p.state == state) {
                Some(i) => inner.pending.remove(i),
                None => return Err(CloudError::StaleSignIn),
            }
        };
        let result = self.join_with_code(code, &pending).await;
        let mut inner = self.inner();
        match &result {
            Ok(_) => {
                // The account is in; every other tab's flow is spent.
                inner.pending.clear();
                inner.phase = Phase::Connecting;
                inner.error = None;
            }
            Err(e) => {
                // A damaged cloud/ file is not a failed sign-in. Its sentence
                // and its phase are already set, and rewriting them to
                // "connecting" beside an io message would hide the one thing
                // the user can act on.
                if !matches!(e, CloudError::CloudFilesBroken(_)) {
                    inner.phase = if inner.credentials.device_credential.is_some() {
                        Phase::Connecting
                    } else {
                        Phase::SignedOut
                    };
                    inner.error = Some(e.to_string());
                }
            }
        }
        result
    }

    async fn join_with_code(&self, code: &str, pending: &Pending) -> Result<SignedIn, CloudError> {
        let tokens = oauth::exchange(
            &self.http,
            &self.config.auth_issuer,
            &self.config.client_id,
            code,
            &pending.redirect_uri,
            &pending.verifier,
        )
        .await?;
        let previous_account = self.inner().credentials.account.clone();
        let mut identity = self.ensure_identity()?;
        let version = env!("CARGO_PKG_VERSION");
        let register = |identity: &Identity| {
            let id = identity.device_id.to_string();
            let key = identity.noise_public.clone();
            let name = self.config.device_name.clone();
            let token = tokens.access_token.clone();
            async move {
                self.registry
                    .register(
                        &token,
                        &DeviceInfo {
                            id: &id,
                            name: &name,
                            platform: &self.config.platform,
                            noise_pubkey: &key,
                            app_version: version,
                        },
                    )
                    .await
            }
        };
        let registered = match register(&identity).await {
            // The id belongs to another account (this computer changed
            // accounts) or was removed: join as a new device.
            Err(RegistryError::Revoked) | Err(RegistryError::Refused { status: 409, .. }) => {
                identity = Identity::generate();
                self.files.save_identity(&identity)?;
                self.inner().identity = Some(identity.clone());
                register(&identity).await?
            }
            other => other?,
        };
        let credentials = Credentials {
            device_credential: Some(registered.credential),
            account: Some(registered.account.clone()),
            kind: registered.device["kind"]
                .as_str()
                .unwrap_or("desktop")
                .to_string(),
            access_token: Some(tokens.access_token),
            access_expires_at: now_s() + tokens.expires_in,
            refresh_token: tokens.refresh_token,
        };
        self.files.save_credentials(&credentials)?;
        self.inner().credentials = credentials;
        Ok(SignedIn {
            previous_account: previous_account.filter(|p| p.id != registered.account.id),
            account: registered.account,
            device_id: identity.device_id,
        })
    }

    fn ensure_identity(&self) -> Result<Identity, CloudError> {
        if let Some(identity) = self.inner().identity.clone() {
            // The file parses but holds no usable key: registering with its
            // public half would put a device in the account that can never
            // complete a handshake, and every later start would say
            // "connecting" with nothing to read.
            if identity.private_key().is_none() {
                return Err(self.cloud_files(KEY_BROKEN));
            }
            return Ok(identity);
        }
        // Nothing in memory: a fresh install, or a file this daemon cannot
        // read. The mint refuses to overwrite, so a damaged file fails inside
        // it with its own path in the message, and the console would show a
        // path where the user needs a sentence. Name the damage here instead,
        // and let the browser flow end with the same reason the open-time
        // check already gave, so a sign-in that cannot work does not quietly
        // promise one that does.
        match self.files.identity_state() {
            Stored::Unreadable => Err(self.cloud_files(IDENTITY_BROKEN)),
            // A file that parses but holds no usable key is the same dead end
            // reached by the check above, and it cannot be left to the mint: a
            // parsable file comes back from it unchanged, so the device would
            // register a public key whose private half is gone and nothing
            // could ever link to it.
            Stored::Value(identity) if identity.private_key().is_none() => {
                Err(self.cloud_files(KEY_BROKEN))
            }
            _ => {
                // The mint is exclusive across processes: an enroll or agent
                // that started beside this one may have minted first, and that
                // identity is the one this device must join with. Two
                // identities on one machine never pair with one credential,
                // and every Noise handshake after that fails with nothing to
                // say why.
                let identity = self.files.mint_identity()?;
                self.inner().identity = Some(identity.clone());
                Ok(identity)
            }
        }
    }

    /// The one way damaged `cloud/` state reaches the console: the phase that
    /// offers a sign-in, and the sentence that says why. Both are set here so
    /// the failure cannot be reported as a name-shaped file path.
    fn cloud_files(&self, sentence: &'static str) -> CloudError {
        self.set_phase(Phase::Relink, Some(sentence.to_string()));
        CloudError::CloudFilesBroken(sentence.to_string())
    }

    /// A machine joins with the code from an install command.
    pub async fn enroll(&self, code: &str) -> Result<SignedIn, CloudError> {
        if self.enrolled() {
            return Err(CloudError::AlreadyEnrolled);
        }
        let mut identity = self.ensure_identity()?;
        let redeem = |identity: &Identity| {
            let id = identity.device_id.to_string();
            let key = identity.noise_public.clone();
            let name = self.config.device_name.clone();
            async move {
                self.registry
                    .redeem(
                        code,
                        &DeviceInfo {
                            id: &id,
                            name: &name,
                            platform: &self.config.platform,
                            noise_pubkey: &key,
                            app_version: env!("CARGO_PKG_VERSION"),
                        },
                    )
                    .await
            }
        };
        let registered = match redeem(&identity).await {
            // The identity's row is dead on the account's side (the device
            // was removed, or an earlier attempt registered the id and
            // never saved its credential). Join as a new device, the same
            // recovery the desktop sign-in makes; without it every future
            // enroll collides with the dead row and the machine stays
            // un-enrollable until identity.json is deleted by hand.
            Err(RegistryError::Revoked) | Err(RegistryError::Refused { status: 409, .. }) => {
                identity = Identity::generate();
                self.files.save_identity(&identity)?;
                self.inner().identity = Some(identity.clone());
                redeem(&identity).await?
            }
            other => other?,
        };
        // The redeem flew without the identity lock: another process may
        // have replaced the identity file while it was in flight. Saving a
        // credential for one id over another id's keys would pair the
        // files badly and fail every Noise handshake from now on, so the
        // credential is dropped and the enroll starts over instead.
        if self
            .files
            .identity()
            .is_none_or(|kept| kept.device_id != identity.device_id)
        {
            return Err(CloudError::IdentityChanged);
        }
        let credentials = Credentials {
            device_credential: Some(registered.credential),
            account: Some(registered.account.clone()),
            kind: "machine".to_string(),
            ..Credentials::default()
        };
        self.files.save_credentials(&credentials)?;
        let mut inner = self.inner();
        inner.credentials = credentials;
        inner.phase = Phase::Connecting;
        inner.error = None;
        Ok(SignedIn {
            account: registered.account,
            device_id: identity.device_id,
            previous_account: None,
        })
    }

    /// Signs this device out: the account's tokens are revoked and the
    /// device credential dropped. The identity stays, so signing in again is
    /// the same device, and synced chats stay on this computer.
    pub async fn signout(&self) -> Result<(), CloudError> {
        self.stop();
        let refresh = self.inner().credentials.refresh_token.clone();
        if let Some(token) = refresh {
            oauth::revoke(
                &self.http,
                &self.config.auth_issuer,
                &self.config.client_id,
                &token,
            )
            .await;
        }
        self.files.save_credentials(&Credentials::default())?;
        let mut inner = self.inner();
        inner.credentials = Credentials::default();
        inner.phase = Phase::SignedOut;
        inner.error = None;
        Ok(())
    }

    /// The account's access token, refreshed when it is about to expire.
    /// Only desktops hold one; a machine acts with its device credential.
    pub async fn account_token(&self) -> Result<String, CloudError> {
        let (access, expires, refresh) = {
            let inner = self.inner();
            let c = &inner.credentials;
            (
                c.access_token.clone(),
                c.access_expires_at,
                c.refresh_token.clone(),
            )
        };
        if let Some(access) = access.filter(|_| expires - 60 > now_s()) {
            return Ok(access);
        }
        let refresh = refresh.ok_or(CloudError::SignedOut)?;
        let tokens = oauth::refresh(
            &self.http,
            &self.config.auth_issuer,
            &self.config.client_id,
            &refresh,
        )
        .await?;
        let mut inner = self.inner();
        // A signout or leave finished while the refresh was in flight and
        // cleared or replaced these credentials. Writing the minted tokens
        // now would put a live, brand-new refresh token into the signed-out
        // credentials on disk, so the mint is dropped: the caller sees the
        // sign-out, not a token nobody will ever revoke.
        if inner.credentials.refresh_token.as_deref() != Some(refresh.as_str()) {
            return Err(CloudError::SignedOut);
        }
        inner.credentials.access_token = Some(tokens.access_token.clone());
        inner.credentials.access_expires_at = now_s() + tokens.expires_in;
        if tokens.refresh_token.is_some() {
            inner.credentials.refresh_token = tokens.refresh_token;
        }
        self.files.save_credentials(&inner.credentials)?;
        Ok(tokens.access_token)
    }

    /// The device credential, for the registry and the relay.
    pub fn device_credential(&self) -> Option<String> {
        self.inner().credentials.device_credential.clone()
    }

    pub fn identity(&self) -> Option<Identity> {
        self.inner().identity.clone()
    }

    /// What is on disk for this computer's identity, for the one caller that
    /// has to name the damage rather than act on it: "missing" and "damaged"
    /// look the same from `identity()` but need different sentences, because
    /// only one of them lets a sign-in join the account again.
    pub(crate) fn identity_state(&self) -> Stored<Identity> {
        self.files.identity_state()
    }

    pub fn registry(&self) -> &Registry {
        &self.registry
    }

    pub fn http(&self) -> &Http {
        &self.http
    }

    pub fn data_dir(&self) -> &Path {
        &self.data_dir
    }

    /// Brings up the mesh (the relay link and the device list) for a device
    /// that has joined. Does nothing when it isn't joined or is already up.
    ///
    /// A run whose relay loop stopped itself is rebuilt, not skipped: the
    /// loop exits for good when the credential stops working (sign in
    /// again), the account removes this device, or this device connects
    /// from elsewhere, and the user's next sign-in must find the slot
    /// free. Before it looked here, a dead run's handles parked in the
    /// slot forever and every later start was turned away until the
    /// process restarted.
    pub fn start(self: &Arc<Self>, host: Arc<dyn CloudHost>) {
        let mut running = self.running.lock().unwrap_or_else(|e| e.into_inner());
        if running.as_ref().is_some_and(|r| !r.relay.is_finished()) {
            return;
        }
        if let Some(dead) = running.take() {
            // The loop is gone but the slot still holds it (its exit path
            // hasn't released the slot yet, or it never will: it panicked).
            // Retire the leftovers here. Only a finished loop opens this
            // door; rebuilding under a live one would run two meshes.
            Self::teardown(dead);
        }
        if !self.enrolled() {
            return;
        }
        if let Some((mesh, relay, refresher)) = mesh::start(self, host) {
            *running = Some(Running {
                mesh,
                relay,
                refresher,
            });
        }
    }

    /// Reconnects to the relay now instead of when the backoff says: the
    /// app came back to the front, and the link it held may have died while
    /// the OS had it frozen.
    pub fn reconnect_now(&self) {
        if let Some(mesh) = self.mesh() {
            mesh.wake();
        }
    }

    /// Takes the mesh down (signing out, or a removal taking this device's
    /// credential with it). Safe when nothing is running.
    pub fn stop(&self) {
        if let Some(running) = self
            .running
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take()
        {
            Self::teardown(running);
        }
    }

    pub fn mesh(&self) -> Option<Arc<Mesh>> {
        self.running
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
            .map(|r| Arc::clone(&r.mesh))
    }

    /// Stops a run's tasks. A task aborting itself is safe (removed() stops
    /// the very run that reports the removal): the cancellation is only
    /// delivered at the task's next await, and every caller of removed()
    /// returns without another one.
    fn teardown(running: Running) {
        running.relay.abort();
        running.refresher.abort();
    }

    /// The relay loop that ran `mesh` has exited for good: release the slot
    /// it filled so a later start can rebuild, and stop the refresher that
    /// would otherwise idle forever polling for a credential that no
    /// longer works. The slot is matched by mesh identity, so a loop whose
    /// teardown was delayed (the run was stopped and rebuilt around it)
    /// can never tear down the run that took its place.
    pub(crate) fn reap(&self, mesh: &Arc<Mesh>) {
        if let Some(running) = self
            .running
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take_if(|r| Arc::ptr_eq(&r.mesh, mesh))
        {
            Self::teardown(running);
        }
    }

    /// This device was removed from the account: its credential is gone for
    /// good, and the daemon wipes the synced chats it held.
    pub(crate) fn removed(&self, host: &dyn CloudHost) {
        // The run's tasks stop with the credential: the refresher would
        // otherwise keep waking every few minutes for an account this
        // device no longer belongs to.
        self.stop();
        let _ = self.files.save_credentials(&Credentials::default());
        {
            let mut inner = self.inner();
            inner.credentials = Credentials::default();
            inner.phase = Phase::Revoked;
            inner.error = Some("This device was removed from your account.".into());
        }
        host.revoked();
    }

    /// The user removed this device from their account here: it leaves for
    /// good, and joins as a new device if it signs in again.
    ///
    /// The registry's delete is the part that cannot be redone later: the
    /// device's row lives in the account, and once the credential that can
    /// remove it has been forgotten here, only another device can clean it up.
    /// So a delete that fails is reported and nothing local is discarded:
    /// the next leave (or unenroll) still has what it needs to finish. A 410
    /// is that same delete already succeeded - the row is gone - and the
    /// local side goes ahead.
    pub async fn leave(&self) -> Result<(), CloudError> {
        // The delete comes first, and the mesh keeps running until it lands.
        // Tearing the run down before the gate left a refused leave looking
        // enrolled and online while connected to nothing, with no way back: the
        // only start is the boot path, and the refresher that would notice a
        // row deleted elsewhere was just aborted. The delete does not need the
        // link down, so a failure now leaves the device exactly as it was.
        if let (Some(credential), Some(id)) = (self.device_credential(), self.device_id()) {
            match self.registry.delete(&credential, &id.to_string()).await {
                Ok(()) | Err(RegistryError::Revoked) => {}
                Err(e @ RegistryError::Unauthorized) => {
                    // The registry rejecting this credential is the same proof
                    // the relay's own 401 is, and leave is the only writer that
                    // holds it right now. Leaving the phase alone would keep
                    // "online" on screen beside an error that says the
                    // credential is dead, and the correction would wait for a
                    // handshake that may not come until the socket does. The
                    // row is untouched, so nothing local is discarded.
                    self.set_phase(Phase::Relink, Some(SIGN_IN_AGAIN.into()));
                    return Err(e.into());
                }
                Err(e) => return Err(e.into()),
            }
        }
        // The row is gone or never existed: nothing may keep talking to the
        // relay with a credential that no longer names a device.
        self.stop();
        let refresh = self.inner().credentials.refresh_token.clone();
        if let Some(token) = refresh {
            oauth::revoke(
                &self.http,
                &self.config.auth_issuer,
                &self.config.client_id,
                &token,
            )
            .await;
        }
        self.files.save_credentials(&Credentials::default())?;
        self.files.forget_identity()?;
        let mut inner = self.inner();
        inner.credentials = Credentials::default();
        inner.identity = None;
        inner.phase = Phase::SignedOut;
        inner.error = None;
        Ok(())
    }

    pub fn set_phase(&self, phase: Phase, error: Option<String>) {
        let mut inner = self.inner();
        inner.phase = phase;
        inner.error = error;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt};

    /// A backend whose interesting requests (the token refresh, the code
    /// redeem) are held until the test lets them go, after first saying
    /// they arrived. The hold is what makes the races under test
    /// deterministic: the test changes Cloud's state while the request is
    /// in flight, which is exactly the moment the races bite.
    async fn gated_backend() -> (String, Arc<tokio::sync::Notify>, Arc<tokio::sync::Notify>) {
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let arrived = Arc::new(tokio::sync::Notify::new());
        let release = Arc::new(tokio::sync::Notify::new());
        tokio::spawn({
            let (arrived, release) = (Arc::clone(&arrived), Arc::clone(&release));
            async move {
                loop {
                    let Ok((stream, _)) = listener.accept().await else {
                        break;
                    };
                    let (arrived, release) = (Arc::clone(&arrived), Arc::clone(&release));
                    tokio::spawn(async move {
                        serve(stream, arrived, release).await;
                    });
                }
            }
        });
        (base, arrived, release)
    }

    /// One request: the head and body are read whole before anything is
    /// answered (closing with unread request bytes can reset the
    /// connection), then the OAuth revocation answers at once and the
    /// gated endpoints wait for the test.
    async fn serve(
        stream: tokio::net::TcpStream,
        arrived: Arc<tokio::sync::Notify>,
        release: Arc<tokio::sync::Notify>,
    ) {
        // Buffered for read_until; the raw stream underneath still writes.
        let mut stream = tokio::io::BufReader::new(stream);
        let mut head = Vec::new();
        let mut line = Vec::new();
        loop {
            line.clear();
            if stream.read_until(b'\n', &mut line).await.unwrap_or(0) == 0 {
                return;
            }
            if matches!(line.as_slice(), b"\r\n" | b"\n") {
                break;
            }
            head.extend_from_slice(&line);
        }
        let head = String::from_utf8_lossy(&head).into_owned();
        let mut body = vec![0u8; content_length(&head)];
        if stream.read_exact(&mut body).await.is_err() {
            return;
        }
        let path = head
            .lines()
            .next()
            .and_then(|l| l.split_whitespace().nth(1))
            .unwrap_or_default()
            .to_string();
        if path.ends_with("/oauth/revoke") {
            reply(stream.get_mut(), br#"{"revoked":true}"#).await;
            return;
        }
        arrived.notify_one();
        release.notified().await;
        if path.ends_with("/oauth/token") {
            reply(stream.get_mut(), br#"{"access_token":"at-new","refresh_token":"rt-new","token_type":"Bearer","expires_in":3600}"#)
                .await;
        } else {
            // The registry's redeem: a join that succeeded server-side.
            reply(stream.get_mut(), br#"{"device":{"id":"whatever","kind":"machine"},"credential":"a credential","account":{"id":"acc-1","email":"owner@example.test","name":"owner"}}"#)
                .await;
        }
    }

    async fn reply(stream: &mut tokio::net::TcpStream, body: &[u8]) {
        let head = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        let _ = stream.write_all(head.as_bytes()).await;
        let _ = stream.write_all(body).await;
    }

    fn content_length(head: &str) -> usize {
        for line in head.lines() {
            let Some((name, value)) = line.split_once(':') else {
                continue;
            };
            if name.trim().eq_ignore_ascii_case("content-length") {
                if let Ok(len) = value.trim().parse() {
                    return len;
                }
            }
        }
        0
    }

    fn test_config(base: &str) -> CloudConfig {
        CloudConfig {
            api_base: format!("{base}/v1/openremote"),
            auth_issuer: format!("{base}/v1/auth"),
            client_id: crate::config::CLIENT_ID.into(),
            device_name: "this-box".into(),
            platform: std::env::consts::OS.into(),
            redirect_uri: None,
        }
    }

    fn scratch_dir() -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("or-cloud-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Credentials whose access token is spent, so the next account
    /// action must refresh - and the refresh is what the sign-out races.
    fn seed_refresh(dir: &std::path::Path) {
        Files::new(dir)
            .save_credentials(&Credentials {
                kind: "desktop".into(),
                access_token: Some("a spent access token".into()),
                access_expires_at: 0,
                refresh_token: Some("the refresh token in play".into()),
                ..Credentials::default()
            })
            .unwrap();
    }

    /// A sign-out that finishes while a refresh is in flight must not let
    /// the minted tokens reach the cleared credentials: the device shows
    /// signed out, and no live, freshly-minted refresh token survives on
    /// disk waiting for a future sign-in to overwrite it.
    #[tokio::test]
    async fn a_refresh_that_outlives_signout_writes_nothing_back() {
        let (base, arrived, release) = gated_backend().await;
        let dir = scratch_dir();
        seed_refresh(&dir);
        let cloud = Cloud::open(test_config(&base), &dir);
        let pending = {
            let cloud = Arc::clone(&cloud);
            tokio::spawn(async move { cloud.account_token().await })
        };
        arrived.notified().await; // the refresh is in flight
        cloud.signout().await.unwrap(); // the user signs out mid-flight
        release.notify_one(); // the minted tokens come back late
        assert!(matches!(pending.await.unwrap(), Err(CloudError::SignedOut)));
        let saved = Files::new(&dir).credentials();
        assert!(saved.access_token.is_none(), "no minted token was written");
        assert!(saved.refresh_token.is_none());
        assert_eq!(cloud.phase(), Phase::SignedOut);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// The same refresh undisturbed still lands: the write-back is gated
    /// on the credentials, not on luck.
    #[tokio::test]
    async fn an_undisturbed_refresh_persists_its_tokens() {
        let (base, arrived, release) = gated_backend().await;
        let dir = scratch_dir();
        seed_refresh(&dir);
        let cloud = Cloud::open(test_config(&base), &dir);
        let pending = {
            let cloud = Arc::clone(&cloud);
            tokio::spawn(async move { cloud.account_token().await })
        };
        arrived.notified().await;
        release.notify_one();
        let token = pending.await.unwrap().unwrap();
        assert_eq!(token, "at-new");
        let saved = Files::new(&dir).credentials();
        assert_eq!(saved.access_token.as_deref(), Some("at-new"));
        assert_eq!(saved.refresh_token.as_deref(), Some("rt-new"));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// A redeem that succeeds while another process replaces the identity
    /// file must not save its credential: the id it registered and the id
    /// on disk no longer match, and a mismatched pair fails every Noise
    /// handshake from now on with nothing to say why.
    #[tokio::test]
    async fn an_enroll_whose_identity_changed_under_it_saves_nothing() {
        let (base, arrived, release) = gated_backend().await;
        let dir = scratch_dir();
        let cloud = Cloud::open(test_config(&base), &dir);
        let pending = {
            let cloud = Arc::clone(&cloud);
            tokio::spawn(async move { cloud.enroll("a fresh install code").await })
        };
        arrived.notified().await; // the redeem is in flight
        let other = Identity::generate();
        Files::new(&dir).save_identity(&other).unwrap(); // the other process's identity
        release.notify_one(); // the redeem answers: joined, server-side
        assert!(matches!(
            pending.await.unwrap(),
            Err(CloudError::IdentityChanged)
        ));
        let saved = Files::new(&dir).credentials();
        assert!(
            saved.device_credential.is_none(),
            "no credential was paired with the wrong identity"
        );
        assert_eq!(
            Files::new(&dir).identity().unwrap().device_id,
            other.device_id,
            "the identity on disk is left alone"
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// A backend whose every answer is one JSON body. A sign-in has to get past
    /// the token exchange before it reaches this computer's own files, so the
    /// damage tests need an exchange that succeeds and nothing more.
    async fn json_backend(body: &'static str) -> String {
        backend(move |_| (200, body.as_bytes())).await
    }

    /// A dead device credential behind a relay socket that is merely
    /// unavailable: the registry's delete is answered 401 while the run's own
    /// connection is not, so what leave does to the phase and to the run can be
    /// watched at the same time.
    async fn dead_credential_backend() -> String {
        backend(|head| {
            if head.starts_with("DELETE ") {
                (401, UNAVAILABLE)
            } else {
                (500, UNAVAILABLE)
            }
        })
        .await
    }

    /// A test backend's answer, picked from the request it is answering.
    type Answer = dyn Fn(&str) -> (u16, &'static [u8]) + Send + Sync;

    /// One answer per request, chosen from the request line and headers. The
    /// head and body are read whole first: closing with unread request bytes
    /// resets the connection instead of answering it.
    async fn backend(
        pick: impl Fn(&str) -> (u16, &'static [u8]) + Send + Sync + 'static,
    ) -> String {
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let pick: Arc<Answer> = Arc::new(pick);
        tokio::spawn(async move {
            loop {
                let Ok((stream, _)) = listener.accept().await else {
                    break;
                };
                let pick = Arc::clone(&pick);
                tokio::spawn(async move {
                    let mut stream = tokio::io::BufReader::new(stream);
                    let mut head = Vec::new();
                    let mut line = Vec::new();
                    loop {
                        line.clear();
                        if stream.read_until(b'\n', &mut line).await.unwrap_or(0) == 0 {
                            return;
                        }
                        if matches!(line.as_slice(), b"\r\n" | b"\n") {
                            break;
                        }
                        head.extend_from_slice(&line);
                    }
                    let head = String::from_utf8_lossy(&head).into_owned();
                    let mut body = vec![0u8; content_length(&head)];
                    if stream.read_exact(&mut body).await.is_err() {
                        return;
                    }
                    let (status, body) = pick(&head);
                    let head = format!(
                        "HTTP/1.1 {status} Status\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        body.len()
                    );
                    let _ = stream.write_all(head.as_bytes()).await;
                    let _ = stream.write_all(body).await;
                });
            }
        });
        base
    }

    /// What every backend says when it has nothing useful to say.
    const UNAVAILABLE: &[u8] = br#"{"message":"OpenRemote Cloud is unavailable"}"#;

    /// A backend that answers every request with the status the test sets:
    /// the registry's self-delete while the network is down, and once it is
    /// back, the 410 a row that is already gone comes back as.
    async fn status_backend(status: Arc<std::sync::atomic::AtomicU16>) -> String {
        backend(move |_| {
            (
                status.load(std::sync::atomic::Ordering::SeqCst),
                UNAVAILABLE,
            )
        })
        .await
    }

    /// A device that is in the account: its identity, its credential, and the
    /// desktop tokens a sign-out would otherwise leave behind.
    fn seed_enrolled(dir: &std::path::Path, identity: &Identity) {
        Files::new(dir).save_identity(identity).unwrap();
        Files::new(dir)
            .save_credentials(&Credentials {
                device_credential: Some("the device credential".into()),
                account: Some(Account {
                    id: "acc-1".into(),
                    email: "owner@example.test".into(),
                    name: "owner".into(),
                }),
                kind: "desktop".into(),
                access_token: Some("an account access token".into()),
                access_expires_at: 0,
                refresh_token: Some("the account refresh token".into()),
            })
            .unwrap();
    }

    /// A host that does nothing: these tests never reach a peer's request.
    /// Only a run needs one, and a run is what a refused leave must not break.
    struct Silent;
    impl CloudHost for Silent {
        fn dispatch(
            &self,
            _: PeerContext,
            _: crate::rpc::Request,
        ) -> futures_util::future::BoxFuture<'static, crate::rpc::Response> {
            Box::pin(std::future::ready(crate::rpc::Response {
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

    /// Leaving is only done when the registry has dropped the device's row:
    /// the row outlives everything local, so forgetting the credential while
    /// it stays in the account leaves a device nobody can remove from here
    /// again. A delete that fails is reported and keeps what a retry needs; a
    /// row the registry says is already gone finishes the leave.
    #[tokio::test]
    async fn a_leave_the_registry_refuses_keeps_its_credential_and_says_so() {
        use std::sync::atomic::{AtomicU16, Ordering};
        let status = Arc::new(AtomicU16::new(500));
        let base = status_backend(Arc::clone(&status)).await;
        let dir = scratch_dir();
        let identity = Identity::generate();
        seed_enrolled(&dir, &identity);
        let cloud = Cloud::open(test_config(&base), &dir);
        // A run is up, the way it is for a device that is in Cloud and
        // connected. Nothing rebuilds a run except the boot path, so a refused
        // leave that stopped this one would leave the console reading "online"
        // beside a device that is connected to nothing, with no way back.
        cloud.start(Arc::new(Silent));
        assert!(cloud.mesh().is_some(), "a run for this device");
        let phase_before = cloud.phase();

        assert!(
            matches!(cloud.leave().await, Err(CloudError::Registry(_))),
            "the console is told the device could not be removed"
        );
        let kept = Files::new(&dir).credentials();
        assert_eq!(
            kept.device_credential.as_deref(),
            Some("the device credential"),
            "the credential that can still remove the row is kept"
        );
        assert_eq!(
            kept.refresh_token.as_deref(),
            Some("the account refresh token"),
            "the token a retry revokes is kept too"
        );
        assert!(cloud.enrolled(), "this device is still in the account");
        assert_eq!(
            Files::new(&dir).identity().unwrap().device_id,
            identity.device_id,
            "the identity the row belongs to is kept with it"
        );
        assert_eq!(
            cloud.phase(),
            phase_before,
            "the console shows what it showed before: still in the account, still connected"
        );
        assert!(
            cloud.mesh().is_some(),
            "the refused leave did not tear the run down, so the relay's own 401 or removal can still move the phase"
        );

        // The registry is reachable again and no longer has the row.
        status.store(410, Ordering::SeqCst);
        cloud
            .leave()
            .await
            .expect("a row the registry no longer has is out of the account");
        assert!(!cloud.enrolled(), "the leave finished this time");
        assert_eq!(cloud.phase(), Phase::SignedOut);
        assert!(
            cloud.mesh().is_none(),
            "the finished leave stops the run: nothing talks to the relay with a dead credential"
        );
        assert!(
            Files::new(&dir).identity().is_none(),
            "a sign-in from here is a new device"
        );
        assert!(
            Files::new(&dir).credentials().refresh_token.is_none(),
            "no token survives the leave"
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// A credential the registry rejects is the one failure no retry can fix:
    /// every later leave answers 401 until the user signs in again. Nothing may
    /// be forgotten locally in the meantime, the run may not be torn down so
    /// the relay can still have its say, and the phase may not keep saying
    /// "online" now that leave holds proof the credential is dead.
    #[tokio::test]
    async fn a_leave_a_dead_credential_refuses_forgets_nothing_and_stops_claiming_online() {
        let base = dead_credential_backend().await;
        let dir = scratch_dir();
        let identity = Identity::generate();
        seed_enrolled(&dir, &identity);
        let cloud = Cloud::open(test_config(&base), &dir);

        for attempt in ["the first", "a second"] {
            cloud.start(Arc::new(Silent));
            assert!(cloud.mesh().is_some(), "{attempt}: a run for this device");
            assert!(
                matches!(
                    cloud.leave().await,
                    Err(CloudError::Registry(RegistryError::Unauthorized))
                ),
                "{attempt} leave reports a dead credential as one, not as a generic failure"
            );
            assert!(
                cloud.mesh().is_some(),
                "{attempt} leave leaves the run up: only the relay's own 401 or removal may end it"
            );
            // The relay loop retries its own connection in the background and
            // writes its phase when an attempt fails, so it is stopped before
            // reading what leave itself wrote.
            cloud.stop();
            assert_eq!(
                cloud.phase(),
                Phase::Relink,
                "{attempt} leave stops claiming a link the backend no longer trusts"
            );
            assert_eq!(
                cloud.view()["error"].as_str(),
                Some(SIGN_IN_AGAIN),
                "{attempt} leave says what the relay would have said"
            );
        }
        assert!(cloud.enrolled(), "this device is still in the account");
        let kept = Files::new(&dir).credentials();
        assert_eq!(
            kept.device_credential.as_deref(),
            Some("the device credential"),
            "the credential is kept: another device can still remove the row"
        );
        assert_eq!(
            kept.refresh_token.as_deref(),
            Some("the account refresh token")
        );
        assert_eq!(
            Files::new(&dir).identity().unwrap().device_id,
            identity.device_id
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// Every click on "Sign in" opens its own tab with its own state. If the
    /// user finishes the tab they opened first, that flow must still be
    /// accepted - it used to be answered with "expired or was already used"
    /// while a perfectly good code sat unredeemed.
    #[tokio::test]
    async fn a_second_sign_in_leaves_the_first_tab_finishable() {
        let dir = scratch_dir();
        let cloud = Cloud::open(test_config("http://127.0.0.1:9"), &dir);
        let state_of = |url: &str| {
            url.split_once("state=")
                .and_then(|(_, rest)| rest.split('&').next())
                .expect("a state on the authorize url")
                .to_string()
        };
        let first = state_of(&cloud.begin_signin(52882));
        let second = state_of(&cloud.begin_signin(52882));
        assert_ne!(first, second, "each tab carries its own state");
        assert_eq!(cloud.inner().pending.len(), 2, "both flows are live");

        // The exchange itself cannot land here - nothing answers on port 9 -
        // so what is asserted is that the older tab was accepted at all.
        let result = cloud
            .finish_signin("a code from the older tab", &first)
            .await;
        assert!(
            !matches!(result, Err(CloudError::StaleSignIn)),
            "the older tab is not told its sign-in expired"
        );
        assert_eq!(
            cloud.inner().pending.len(),
            1,
            "only the flow that came back is spent"
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// A first sign-in must save a usable identity and its credentials before
    /// it can survive reopening the app. Android denies hard links even in
    /// private storage, so this also exercises how a new identity is placed.
    #[tokio::test]
    async fn a_fresh_sign_in_persists_its_identity_and_credentials() {
        let base = backend(|head| {
            if head.starts_with("POST /v1/auth/oauth/token ") {
                (200, br#"{"access_token":"test-access","refresh_token":"test-refresh","expires_in":3600}"#)
            } else if head.starts_with("POST /v1/openremote/devices ") {
                (200, br#"{"device":{"kind":"desktop"},"credential":"test-device","account":{"id":"acc-1","email":"owner@example.test","name":"owner"}}"#)
            } else {
                (404, UNAVAILABLE)
            }
        })
        .await;
        let dir = scratch_dir();
        let config = test_config(&base);
        let cloud = Cloud::open(config.clone(), &dir);
        let state = state_of(&cloud.begin_signin(52882));
        let signed_in = cloud.finish_signin("test-code", &state).await.unwrap();
        assert_eq!(cloud.phase(), Phase::Connecting);

        let reopened = Cloud::open(config, &dir);
        assert!(reopened.enrolled());
        assert_eq!(reopened.device_id(), Some(signed_in.device_id));
        assert!(reopened.identity().unwrap().private_key().is_some());
        assert_eq!(reopened.device_credential().as_deref(), Some("test-device"));
        assert_eq!(reopened.account_token().await.unwrap(), "test-access");
        assert_eq!(
            Files::new(&dir).credentials().refresh_token.as_deref(),
            Some("test-refresh")
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// Damage on disk is what a crash, a full disk or a virus scanner leaves,
    /// never something `save_identity` writes: the file is put there as raw
    /// bytes, not through `Files`.
    fn damage(dir: &std::path::Path, name: &str, bytes: &[u8]) {
        let cloud = dir.join("cloud");
        std::fs::create_dir_all(&cloud).unwrap();
        std::fs::write(cloud.join(name), bytes).unwrap();
    }

    /// The `state` the authorize URL carries, which is what the browser sends
    /// back when it returns.
    fn state_of(url: &str) -> String {
        url.split_once("state=")
            .and_then(|(_, rest)| rest.split('&').next())
            .expect("a state on the authorize url")
            .to_string()
    }

    fn enrolled_credentials() -> Credentials {
        Credentials {
            device_credential: Some("the device credential".into()),
            account: Some(Account {
                id: "acc-1".into(),
                email: "owner@example.test".into(),
                name: "owner".into(),
            }),
            kind: "desktop".into(),
            ..Credentials::default()
        }
    }

    fn enrolled_on_disk(dir: &std::path::Path) -> Identity {
        let identity = Identity::generate();
        Files::new(dir).save_identity(&identity).unwrap();
        Files::new(dir)
            .save_credentials(&enrolled_credentials())
            .unwrap();
        identity
    }

    /// A credential with no usable identity behind it is not a device that is
    /// connecting: no mesh can be built from it, so the phase must say "sign in
    /// again" and the sentence must name the piece that is broken. Every one of
    /// these shapes used to open as "connecting", with no error, for good.
    #[test]
    fn a_credential_without_a_usable_identity_opens_as_sign_in_again() {
        let cases: [(&str, Option<&[u8]>, &str); 3] = [
            ("no identity.json at all", None, IDENTITY_MISSING),
            (
                "identity.json truncated by a crash",
                Some(b"{\"device_id\": \"half"),
                IDENTITY_BROKEN,
            ),
            (
                "identity.json replaced by something else",
                Some(b"<html>quarantined</html>"),
                IDENTITY_BROKEN,
            ),
        ];
        for (what, on_disk, sentence) in cases {
            let dir = scratch_dir();
            Files::new(&dir)
                .save_credentials(&enrolled_credentials())
                .unwrap();
            if let Some(bytes) = on_disk {
                damage(&dir, "identity.json", bytes);
            }
            let cloud = Cloud::open(test_config("http://127.0.0.1:9"), &dir);
            assert_eq!(
                cloud.phase(),
                Phase::Relink,
                "{what} is not a device that is connecting"
            );
            assert_eq!(
                cloud.view()["error"].as_str(),
                Some(sentence),
                "{what} says which piece is broken"
            );
            assert!(
                cloud.enrolled(),
                "{what}: the credential survived, so a sign-in has the row it names"
            );
            std::fs::remove_dir_all(&dir).unwrap();
        }

        // The file parses but its key is not key material: nothing can ever
        // link with it, and that is a different sentence.
        let dir = scratch_dir();
        Files::new(&dir)
            .save_credentials(&enrolled_credentials())
            .unwrap();
        let broken = json!({
            "device_id": DeviceId::new().to_string(),
            "noise_private": "not base64!",
            "noise_public": "not base64!",
        });
        damage(&dir, "identity.json", broken.to_string().as_bytes());
        let cloud = Cloud::open(test_config("http://127.0.0.1:9"), &dir);
        assert_eq!(cloud.phase(), Phase::Relink);
        assert_eq!(
            cloud.view()["error"].as_str(),
            Some(KEY_BROKEN),
            "an empty key is named as a damaged key, not as a connecting device"
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// A damaged `credentials.json` is a signed-out device whose identity
    /// survives, so signing in again is the same device - and the console is
    /// told it was damage, not a sign-out the user asked for.
    #[test]
    fn a_damaged_sign_in_file_keeps_the_identity_and_says_it_is_damaged() {
        let dir = scratch_dir();
        let identity = Identity::generate();
        Files::new(&dir).save_identity(&identity).unwrap();
        damage(&dir, "credentials.json", b"{\"device_credential\": ");
        let cloud = Cloud::open(test_config("http://127.0.0.1:9"), &dir);
        assert_eq!(cloud.phase(), Phase::SignedOut);
        assert_eq!(
            cloud.view()["error"].as_str(),
            Some(SIGNIN_BROKEN),
            "the user is told why the sign-in is gone instead of guessing"
        );
        assert_eq!(
            Files::new(&dir).identity().unwrap().device_id,
            identity.device_id,
            "the identity is untouched, so signing in again is the same device"
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// The sentence a damaged identity shows offers a sign-in, so that sign-in
    /// has to meet the damage and say so. It used to reach the mint, which
    /// refuses to overwrite, and hand the console its io error: a filesystem
    /// path in the same alert, with no control left that could act on it. The
    /// exchange succeeds here (it is the only part a backend answers), so what
    /// this pins is the step after it.
    #[tokio::test]
    async fn a_sign_in_on_a_damaged_identity_says_it_is_damaged_and_shows_no_path() {
        let base = json_backend(
            r#"{"access_token":"an access token","refresh_token":"a refresh token","expires_in":3600,"token_type":"Bearer"}"#,
        )
        .await;
        let dir = scratch_dir();
        Files::new(&dir)
            .save_credentials(&enrolled_credentials())
            .unwrap();
        damage(&dir, "identity.json", b"{\"device_id\": \"half");
        let cloud = Cloud::open(test_config(&base), &dir);
        assert_eq!(cloud.phase(), Phase::Relink, "the open-time check");

        let state = state_of(&cloud.begin_signin(52882));
        let error = cloud.finish_signin("a code", &state).await.err();
        let error = error.expect("a damaged identity cannot be signed in over");
        assert_eq!(
            error.to_string(),
            IDENTITY_BROKEN,
            "the sentence the user already had, not an io message"
        );
        assert!(
            !error.to_string().contains("identity.json"),
            "no filesystem path reaches the alert"
        );
        assert_eq!(
            cloud.view()["error"].as_str(),
            Some(IDENTITY_BROKEN),
            "the failed sign-in does not replace the sentence with an io path"
        );
        assert_eq!(
            cloud.phase(),
            Phase::Relink,
            "and does not claim the device is connecting"
        );
        assert!(
            matches!(Files::new(&dir).identity_state(), Stored::Unreadable),
            "the damaged file is left exactly where it is"
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// The other shape of the same damage: a file that parses but holds no
    /// usable key, put there after the daemon opened. The mint hands a parsable
    /// file back unchanged rather than overwriting it, so the device would
    /// register a public key whose private half is gone, finish the sign-in,
    /// and then fail every handshake with nothing to say why.
    #[tokio::test]
    async fn a_sign_in_on_a_key_that_cannot_be_read_says_the_key_is_damaged() {
        let base = json_backend(
            r#"{"access_token":"an access token","refresh_token":"a refresh token","expires_in":3600,"token_type":"Bearer"}"#,
        )
        .await;
        let dir = scratch_dir();
        Files::new(&dir)
            .save_credentials(&enrolled_credentials())
            .unwrap();
        // Opened with no identity at all, then the file arrives damaged.
        let cloud = Cloud::open(test_config(&base), &dir);
        let broken = json!({
            "device_id": DeviceId::new().to_string(),
            "noise_private": "not base64!",
            "noise_public": "not base64!",
        });
        damage(&dir, "identity.json", broken.to_string().as_bytes());

        let state = state_of(&cloud.begin_signin(52882));
        let error = cloud.finish_signin("a code", &state).await.err();
        let error = error.expect("a key that cannot be read cannot be signed in with");
        assert_eq!(
            error.to_string(),
            KEY_BROKEN,
            "the sentence, not a device that joined"
        );
        assert_eq!(
            cloud.view()["error"].as_str(),
            Some(KEY_BROKEN),
            "the console is told which piece is broken"
        );
        assert_eq!(cloud.phase(), Phase::Relink, "and is offered a sign-in");
        assert_eq!(
            Files::new(&dir).credentials().device_credential.as_deref(),
            Some("the device credential"),
            "no registration replaced the credential with one for a device that could never link"
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// Health is unchanged: nothing on disk is a fresh install (signed out, no
    /// error to show), and identity plus credential together is connecting.
    /// This is the test that fails if the damage checks ever cry wolf.
    #[test]
    fn a_fresh_install_and_a_healthy_enrollment_open_as_before() {
        let fresh = scratch_dir();
        let cloud = Cloud::open(test_config("http://127.0.0.1:9"), &fresh);
        assert_eq!(cloud.phase(), Phase::SignedOut);
        assert_eq!(
            cloud.view()["error"],
            Value::Null,
            "a fresh install has nothing wrong with it"
        );
        std::fs::remove_dir_all(&fresh).unwrap();

        let healthy = scratch_dir();
        enrolled_on_disk(&healthy);
        let cloud = Cloud::open(test_config("http://127.0.0.1:9"), &healthy);
        assert_eq!(cloud.phase(), Phase::Connecting);
        assert_eq!(cloud.view()["error"], Value::Null);
        assert!(cloud.enrolled());
        std::fs::remove_dir_all(&healthy).unwrap();
    }
}
