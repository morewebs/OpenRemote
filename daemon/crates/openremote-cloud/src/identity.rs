//! What this device keeps about itself, in `<data>/cloud/` (owner-only):
//! - `identity.json`: its id and Noise key pair. Kept across sign-out, so a
//!   device signing in again is the same device.
//! - `credentials.json`: the device credential, the account it belongs to,
//!   and, on a desktop, the account's tokens. Dropped on sign-out.

use std::path::{Path, PathBuf};

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use openremote_core::DeviceId;
use serde::{Deserialize, Serialize};

pub const NOISE_PARAMS: &str = "Noise_KK_25519_ChaChaPoly_BLAKE2s";

#[derive(Clone, Serialize, Deserialize)]
pub struct Identity {
    pub device_id: DeviceId,
    /// base64url X25519 keys.
    pub noise_private: String,
    pub noise_public: String,
}

impl Identity {
    pub fn generate() -> Self {
        let keys = snow::Builder::new(NOISE_PARAMS.parse().expect("noise params"))
            .generate_keypair()
            .expect("a key pair");
        Self {
            device_id: DeviceId::new(),
            noise_private: URL_SAFE_NO_PAD.encode(keys.private),
            noise_public: URL_SAFE_NO_PAD.encode(keys.public),
        }
    }

    pub fn private_key(&self) -> Vec<u8> {
        URL_SAFE_NO_PAD
            .decode(&self.noise_private)
            .unwrap_or_default()
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Account {
    pub id: String,
    pub email: String,
    #[serde(default)]
    pub name: String,
}

#[derive(Clone, Default, Serialize, Deserialize)]
pub struct Credentials {
    pub device_credential: Option<String>,
    pub account: Option<Account>,
    /// `desktop` or `machine`.
    #[serde(default)]
    pub kind: String,
    /// A desktop's account tokens, for changes only the account may make.
    #[serde(default)]
    pub access_token: Option<String>,
    #[serde(default)]
    pub access_expires_at: i64,
    #[serde(default)]
    pub refresh_token: Option<String>,
}

pub struct Files {
    dir: PathBuf,
}

impl Files {
    pub fn new(data_dir: &Path) -> Self {
        Self {
            dir: data_dir.join("cloud"),
        }
    }

    fn read<T: for<'de> Deserialize<'de>>(&self, name: &str) -> Option<T> {
        let raw = std::fs::read(self.dir.join(name)).ok()?;
        serde_json::from_slice(&raw).ok()
    }

    fn write<T: Serialize>(&self, name: &str, value: &T) -> std::io::Result<()> {
        openremote_core::fsx::create_private_dir(&self.dir)?;
        let json = serde_json::to_vec_pretty(value).map_err(std::io::Error::other)?;
        openremote_core::fsx::write_private(&self.dir.join(name), &json)
    }

    pub fn identity(&self) -> Option<Identity> {
        self.read("identity.json")
    }

    pub fn save_identity(&self, identity: &Identity) -> std::io::Result<()> {
        self.write("identity.json", identity)
    }

    pub fn credentials(&self) -> Credentials {
        self.read("credentials.json").unwrap_or_default()
    }

    pub fn save_credentials(&self, credentials: &Credentials) -> std::io::Result<()> {
        self.write("credentials.json", credentials)
    }

    /// Removing this device from the account: a new identity next time.
    pub fn forget_identity(&self) -> std::io::Result<()> {
        match std::fs::remove_file(self.dir.join("identity.json")) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        }
    }
}
