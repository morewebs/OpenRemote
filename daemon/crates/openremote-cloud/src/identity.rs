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

pub use openremote_noise::NOISE_PARAMS;

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

    /// This device's Noise private key, when the stored text is the 32 bytes
    /// an X25519 private key is. Anything else answers `None`: an empty or
    /// shortened key would make every handshake fail as "that device doesn't
    /// answer", which blames the peer for what is this computer's damaged
    /// file, and no mesh can be built without it.
    pub fn private_key(&self) -> Option<Vec<u8>> {
        let key = URL_SAFE_NO_PAD.decode(&self.noise_private).ok()?;
        (key.len() == 32).then_some(key)
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

/// What one of the `cloud/` files holds: the value, nothing at all, or bytes
/// this daemon cannot read (truncated by a crash, corrupted by the disk, moved
/// aside by a virus scanner, or overwritten with something that is not this
/// file). The three cases must stay apart: an absent file is a fresh install
/// or a device that never joined, while an unreadable one is damaged state the
/// user has to be told about instead of quietly starting over.
pub enum Stored<T> {
    Missing,
    Unreadable,
    Value(T),
}

impl<T> Stored<T> {
    pub fn value(self) -> Option<T> {
        match self {
            Self::Value(value) => Some(value),
            Self::Missing | Self::Unreadable => None,
        }
    }
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

    fn read<T: for<'de> Deserialize<'de>>(&self, name: &str) -> Stored<T> {
        let raw = match std::fs::read(self.dir.join(name)) {
            Ok(raw) => raw,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Stored::Missing,
            // Anything else (permissions, a directory, an I/O error) is not
            // proof of absence either: the file is there and cannot be read.
            Err(_) => return Stored::Unreadable,
        };
        match serde_json::from_slice(&raw) {
            Ok(value) => Stored::Value(value),
            Err(_) => Stored::Unreadable,
        }
    }

    fn write<T: Serialize>(&self, name: &str, value: &T) -> std::io::Result<()> {
        openremote_core::fsx::create_private_dir(&self.dir)?;
        let json = serde_json::to_vec_pretty(value).map_err(std::io::Error::other)?;
        openremote_core::fsx::write_private(&self.dir.join(name), &json)
    }

    pub fn identity(&self) -> Option<Identity> {
        self.identity_state().value()
    }

    /// What `identity.json` holds, damage included: the identity, nothing (a
    /// fresh install), or bytes that are not this file any more.
    pub fn identity_state(&self) -> Stored<Identity> {
        self.read("identity.json")
    }

    /// This device's identity, minted exactly once. `enroll`, `status` and
    /// the daemon are separate processes that can all reach an empty
    /// `cloud/` together; the mint is exclusive, so exactly one of them
    /// creates the file and the others come back holding the winner's
    /// identity instead of pairing their own candidate with the winner's
    /// credentials.
    pub fn mint_identity(&self) -> std::io::Result<Identity> {
        if let Some(identity) = self.identity() {
            return Ok(identity);
        }
        let candidate = Identity::generate();
        match self.link_new_identity(&candidate) {
            Ok(()) => Ok(candidate),
            // Another process minted first. This machine's identity is
            // theirs, and ours must never reach the registry.
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                self.identity().ok_or_else(|| {
                    std::io::Error::other("cloud/identity.json is present but unreadable")
                })
            }
            Err(e) => Err(e),
        }
    }

    /// Puts `identity` in place only if no identity exists yet. The
    /// candidate is written whole to a file of its own and then hard-linked
    /// under its final name: the link is exclusive (it fails when the name
    /// is taken, the create_new/O_EXCL guarantee) and atomic (an identity
    /// only ever appears complete), so a crash cannot leave a half-written
    /// identity behind for every later mint to trip over.
    fn link_new_identity(&self, identity: &Identity) -> std::io::Result<()> {
        openremote_core::fsx::create_private_dir(&self.dir)?;
        let json = serde_json::to_vec_pretty(identity).map_err(std::io::Error::other)?;
        let candidate = self
            .dir
            .join(format!("identity.json.{}", uuid::Uuid::new_v4()));
        openremote_core::fsx::write_private(&candidate, &json)?;
        let linked = std::fs::hard_link(&candidate, self.dir.join("identity.json"));
        let _ = std::fs::remove_file(&candidate);
        linked
    }

    pub fn save_identity(&self, identity: &Identity) -> std::io::Result<()> {
        self.write("identity.json", identity)
    }

    pub fn credentials(&self) -> Credentials {
        self.credentials_state().value().unwrap_or_default()
    }

    /// What `credentials.json` holds, damage included: the credentials, nothing
    /// (a fresh install, or a device that signed out), or bytes that are not
    /// this file any more. A damaged file reads as the empty credentials every
    /// caller expects, and `Cloud::open` is the one place that looks at the
    /// difference and tells the user.
    pub fn credentials_state(&self) -> Stored<Credentials> {
        self.read("credentials.json")
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn scratch_dir() -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("or-identity-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Two processes that mint at the same moment must end up as one
    /// device, not two: the loser of the exclusive create re-reads the
    /// winner's identity, so both redeem with the same id and key.
    #[test]
    fn two_concurrent_mints_agree_on_one_identity() {
        let data = scratch_dir();
        let start = std::sync::Arc::new(std::sync::Barrier::new(2));
        let mint = |data: &std::path::Path, start: std::sync::Arc<std::sync::Barrier>| {
            let data = data.to_path_buf();
            std::thread::spawn(move || {
                start.wait();
                Files::new(&data).mint_identity()
            })
        };
        let a = mint(&data, Arc::clone(&start));
        let b = mint(&data, Arc::clone(&start));
        let (a, b) = (a.join().unwrap().unwrap(), b.join().unwrap().unwrap());
        assert_eq!(
            a.device_id, b.device_id,
            "the loser of the mint re-read the winner's identity"
        );
        assert_eq!(a.noise_public, b.noise_public);
        let disk = Files::new(&data)
            .identity()
            .expect("the winner's identity is on disk");
        assert_eq!(disk.device_id, a.device_id);
        std::fs::remove_dir_all(&data).unwrap();
    }

    /// A mint against a file that already exists reuses it, and the
    /// exclusive step itself refuses to place a second identity over the
    /// first.
    #[test]
    fn an_existing_identity_refuses_a_second_mint() {
        let data = scratch_dir();
        let first = Files::new(&data).mint_identity().unwrap();
        let again = Files::new(&data).mint_identity().unwrap();
        assert_eq!(again.device_id, first.device_id);
        let refused = Files::new(&data)
            .link_new_identity(&Identity::generate())
            .unwrap_err();
        assert_eq!(refused.kind(), std::io::ErrorKind::AlreadyExists);
        std::fs::remove_dir_all(&data).unwrap();
    }
}
