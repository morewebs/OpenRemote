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
pub mod oauth;
pub mod registry;

use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use openremote_core::DeviceId;
use serde::Serialize;
use serde_json::{Value, json};

pub use config::CloudConfig;
use https::Http;
use identity::{Account, Credentials, Files, Identity};
use registry::{DeviceInfo, Registry, RegistryError};

/// A sign-in left in the browser this long is abandoned.
const SIGN_IN_WINDOW: Duration = Duration::from_secs(600);

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

struct Inner {
    identity: Option<Identity>,
    credentials: Credentials,
    pending: Option<Pending>,
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
    inner: Mutex<Inner>,
}

fn now_s() -> i64 {
    openremote_core::now_ms() / 1000
}

impl Cloud {
    pub fn open(config: CloudConfig, data_dir: &Path) -> Arc<Self> {
        let http = Http::new();
        let files = Files::new(data_dir);
        let credentials = files.credentials();
        let phase = if credentials.device_credential.is_some() {
            Phase::Connecting
        } else {
            Phase::SignedOut
        };
        Arc::new(Self {
            registry: Registry::new(http.clone(), &config.api_base),
            config,
            http,
            inner: Mutex::new(Inner {
                identity: files.identity(),
                credentials,
                pending: None,
                phase,
                error: None,
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
                "platform": std::env::consts::OS,
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
    /// comes back to this daemon at `/cloud/callback` on `port`.
    pub fn begin_signin(&self, port: u16) -> String {
        let pkce = oauth::Pkce::new();
        let state = oauth::new_state();
        let redirect_uri = format!("http://127.0.0.1:{port}/cloud/callback");
        let url = oauth::authorize_url(
            &self.config.auth_issuer,
            &self.config.client_id,
            &redirect_uri,
            &state,
            &pkce.challenge,
        );
        let mut inner = self.inner();
        inner.pending = Some(Pending {
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
        inner.pending = None;
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
            match inner.pending.take() {
                Some(p) if p.state == state && p.started.elapsed() < SIGN_IN_WINDOW => p,
                other => {
                    // A wrong state must not cancel the sign-in that is
                    // really in progress.
                    inner.pending = other.filter(|p| p.started.elapsed() < SIGN_IN_WINDOW);
                    return Err(CloudError::StaleSignIn);
                }
            }
        };
        let result = self.join_with_code(code, &pending).await;
        let mut inner = self.inner();
        match &result {
            Ok(_) => {
                inner.phase = Phase::Connecting;
                inner.error = None;
            }
            Err(e) => {
                inner.phase = if inner.credentials.device_credential.is_some() {
                    Phase::Connecting
                } else {
                    Phase::SignedOut
                };
                inner.error = Some(e.to_string());
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
                            platform: std::env::consts::OS,
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
            return Ok(identity);
        }
        let identity = Identity::generate();
        self.files.save_identity(&identity)?;
        self.inner().identity = Some(identity.clone());
        Ok(identity)
    }

    /// A machine joins with the code from an install command.
    pub async fn enroll(&self, code: &str) -> Result<SignedIn, CloudError> {
        if self.enrolled() {
            return Err(CloudError::AlreadyEnrolled);
        }
        let identity = self.ensure_identity()?;
        let id = identity.device_id.to_string();
        let registered = self
            .registry
            .redeem(
                code,
                &DeviceInfo {
                    id: &id,
                    name: &self.config.device_name,
                    platform: std::env::consts::OS,
                    noise_pubkey: &identity.noise_public,
                    app_version: env!("CARGO_PKG_VERSION"),
                },
            )
            .await?;
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

    pub fn registry(&self) -> &Registry {
        &self.registry
    }

    pub fn http(&self) -> &Http {
        &self.http
    }

    pub fn set_phase(&self, phase: Phase, error: Option<String>) {
        let mut inner = self.inner();
        inner.phase = phase;
        inner.error = error;
    }
}
