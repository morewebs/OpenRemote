//! Machine-local Codex accounts. The steady home keeps transcripts and
//! settings; a sign-in writes credentials in a separate home until its
//! provider identity has been checked.

use std::collections::BTreeMap;
use std::fs;
use std::io::{ErrorKind, Read};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, SystemTime};

use openremote_core::fsx::{create_private_dir, write_private};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

const MAX_AUTH_BYTES: u64 = 128 * 1024;
const AUTH_FILE: &str = "auth.json";
const STALE_SIGN_IN_AGE: Duration = Duration::from_secs(60 * 60);

#[derive(Clone, Debug, Serialize)]
pub struct AppAccountView {
    pub id: String,
    pub label: String,
    pub status: &'static str,
}

#[derive(Serialize, Deserialize)]
struct AccountRecord {
    label: String,
    provider_identity: Option<String>,
    connected: bool,
}

type Registry = BTreeMap<Uuid, AccountRecord>;

pub struct AppAccounts {
    root: PathBuf,
    lock: Mutex<()>,
}

impl AppAccounts {
    pub fn open(data_dir: &Path) -> Self {
        let accounts = Self {
            root: data_dir.join("app-accounts").join("codex"),
            lock: Mutex::new(()),
        };
        accounts.cleanup_stale_sign_ins(SystemTime::now());
        accounts
    }

    pub fn list(&self) -> Result<Vec<AppAccountView>, String> {
        let _lock = self.lock()?;
        let mut views = self
            .load()?
            .iter()
            .map(|(id, row)| self.view(*id, row))
            .collect::<Vec<_>>();
        views.sort_by_key(|row| row.label.to_lowercase());
        Ok(views)
    }

    pub fn create(&self, label: &str) -> Result<AppAccountView, String> {
        let _lock = self.lock()?;
        let mut registry = self.load()?;
        let id = Uuid::new_v4();
        let row = AccountRecord {
            label: valid_label(label)?,
            provider_identity: None,
            connected: false,
        };
        let view = self.view(id, &row);
        registry.insert(id, row);
        self.save(&registry)?;
        Ok(view)
    }

    pub fn update(
        &self,
        id: &str,
        label: Option<&str>,
        disconnect: bool,
    ) -> Result<AppAccountView, String> {
        let _lock = self.lock()?;
        let id = account_id(id)?;
        let mut registry = self.load()?;
        let row = registry.get_mut(&id).ok_or("App account does not exist.")?;
        if let Some(label) = label {
            row.label = valid_label(label)?;
        }
        if disconnect {
            row.connected = false;
        }
        let view = self.view(id, row);
        self.save(&registry)?;
        if disconnect {
            match fs::remove_file(self.home(id).join(AUTH_FILE)) {
                Ok(()) => {}
                Err(err) if err.kind() == ErrorKind::NotFound => {}
                Err(_) => {
                    return Err(
                        "The account was disconnected, but its local credentials could not be cleared."
                            .to_string(),
                    );
                }
            }
        }
        Ok(view)
    }

    pub fn resolve(&self, id: &str) -> Result<PathBuf, String> {
        let _lock = self.lock()?;
        let id = account_id(id)?;
        let registry = self.load()?;
        let row = registry.get(&id).ok_or("App account does not exist.")?;
        if !self.is_ready(id, row) {
            return Err(format!(
                "Reconnect the Codex account ‘{}’ to continue.",
                row.label
            ));
        }
        Ok(self.home(id))
    }

    pub fn begin_sign_in(&self, id: &str, run_id: &str) -> Result<PathBuf, String> {
        let _lock = self.lock()?;
        let id = account_id(id)?;
        let run = Uuid::parse_str(run_id).map_err(|_| "Invalid sign-in ID.")?;
        if !self.load()?.contains_key(&id) {
            return Err("App account does not exist.".to_string());
        }
        let parent = self.staging().join(id.to_string());
        let stage = parent.join(run.to_string());
        // Make the parents private before the provider can write its credentials.
        for directory in [&self.staging(), &parent] {
            create_private_dir(directory).map_err(|_| "Couldn't prepare Codex sign-in.")?;
        }
        fs::create_dir(&stage).map_err(|_| "Couldn't prepare Codex sign-in.")?;
        create_private_dir(&stage).map_err(|_| "Couldn't prepare Codex sign-in.")?;
        Ok(stage)
    }

    pub fn complete_sign_in(&self, id: &str, stage: &Path) -> Result<(), String> {
        let _lock = self.lock()?;
        let id = account_id(id)?;
        if self.stage_account(stage) != Some(id) {
            return Err("This sign-in belongs to a different App account.".to_string());
        }
        let mut registry = self.load()?;
        let row = registry.get_mut(&id).ok_or("App account does not exist.")?;
        // Validate the same snapshot that will replace the live credentials.
        let bytes = read_auth(stage).ok_or("Codex didn't save sign-in credentials.")?;
        let identity = openremote_codex::managed_identity_from_auth(&bytes)
            .ok_or("Couldn't verify the Codex identity. Try signing in again.")?;
        if row
            .provider_identity
            .as_ref()
            .is_some_and(|existing| existing != &identity)
        {
            return Err(
                "Sign in with the original Codex account to reconnect. Use Add account for a different sign-in."
                    .to_string(),
            );
        }
        let home = self.home(id);
        for directory in [&self.homes(), &home] {
            create_private_dir(directory).map_err(|_| "Couldn't save the Codex sign-in.")?;
        }
        write_private(&home.join(AUTH_FILE), &bytes)
            .map_err(|_| "Couldn't save the Codex sign-in.")?;
        row.provider_identity = Some(identity);
        row.connected = true;
        self.save(&registry)
    }

    pub fn discard_sign_in(&self, stage: &Path) {
        // Only delete the staging tree we created, even if part of it
        // was replaced with a link while the provider was running.
        if self.stage_account(stage).is_none() {
            return;
        }
        let Ok(relative) = stage.strip_prefix(&self.root) else {
            return;
        };
        let (Some(root), Ok(stage)) = (self.resolved_root(), stage.canonicalize()) else {
            return;
        };
        if stage == root.join(relative) {
            let _ = fs::remove_dir_all(stage);
        }
    }

    fn cleanup_stale_sign_ins(&self, now: SystemTime) {
        let staging = self.staging();
        let (Some(root), Ok(resolved_staging)) = (self.resolved_root(), staging.canonicalize())
        else {
            return;
        };
        if resolved_staging != root.join("sign-ins") {
            return;
        }
        let Ok(accounts) = fs::read_dir(&staging) else {
            return;
        };
        for account in accounts.flatten() {
            let name = account.file_name();
            if !name.to_str().is_some_and(|name| account_id(name).is_ok())
                || !account.file_type().is_ok_and(|kind| kind.is_dir())
                || !account
                    .path()
                    .canonicalize()
                    .is_ok_and(|path| path == resolved_staging.join(name))
            {
                continue;
            }
            let Ok(stages) = fs::read_dir(account.path()) else {
                continue;
            };
            for stage in stages.flatten() {
                // A fresh stage could belong to another daemon still using
                // this directory. Leave more than the device login timeout.
                let old_directory = stage.metadata().is_ok_and(|metadata| {
                    metadata.is_dir()
                        && metadata.modified().is_ok_and(|modified| {
                            now.duration_since(modified)
                                .is_ok_and(|age| age >= STALE_SIGN_IN_AGE)
                        })
                });
                if old_directory {
                    self.discard_sign_in(&stage.path());
                }
            }
        }
    }

    fn lock(&self) -> Result<MutexGuard<'_, ()>, String> {
        self.lock
            .lock()
            .map_err(|_| "App accounts are temporarily unavailable.".to_string())
    }

    fn load(&self) -> Result<Registry, String> {
        match fs::read(self.root.join("registry.json")) {
            Ok(bytes) => serde_json::from_slice(&bytes)
                .map_err(|_| "Couldn't read the saved App accounts.".to_string()),
            Err(err) if err.kind() == ErrorKind::NotFound => Ok(Registry::new()),
            Err(_) => Err("Couldn't read the saved App accounts.".to_string()),
        }
    }

    fn save(&self, registry: &Registry) -> Result<(), String> {
        let err = "Couldn't save the App accounts.";
        create_private_dir(&self.root).map_err(|_| err)?;
        let bytes = serde_json::to_vec(registry).map_err(|_| err)?;
        write_private(&self.root.join("registry.json"), &bytes).map_err(|_| err.to_string())
    }

    fn view(&self, id: Uuid, row: &AccountRecord) -> AppAccountView {
        AppAccountView {
            id: id.to_string(),
            label: row.label.clone(),
            status: if self.is_ready(id, row) {
                "ready"
            } else {
                "disconnected"
            },
        }
    }

    fn is_ready(&self, id: Uuid, row: &AccountRecord) -> bool {
        row.connected
            && row.provider_identity.as_ref().is_some_and(|identity| {
                read_auth(&self.home(id))
                    .and_then(|bytes| openremote_codex::managed_identity_from_auth(&bytes))
                    .as_ref()
                    == Some(identity)
            })
    }

    fn homes(&self) -> PathBuf {
        self.root.join("homes")
    }

    fn home(&self, id: Uuid) -> PathBuf {
        self.homes().join(id.to_string())
    }

    fn staging(&self) -> PathBuf {
        self.root.join("sign-ins")
    }

    fn resolved_root(&self) -> Option<PathBuf> {
        let data = self.root.parent()?.parent()?.canonicalize().ok()?;
        let root = self.root.canonicalize().ok()?;
        // The configured data directory may itself be a link. The paths
        // we append must remain under its resolved location.
        (root == data.join("app-accounts").join("codex")).then_some(root)
    }

    fn stage_account(&self, stage: &Path) -> Option<Uuid> {
        let parent = stage.parent()?;
        let account = Uuid::parse_str(parent.file_name()?.to_str()?).ok()?;
        let run = Uuid::parse_str(stage.file_name()?.to_str()?).ok()?;
        (stage
            == self
                .staging()
                .join(account.to_string())
                .join(run.to_string()))
        .then_some(account)
    }
}

fn account_id(id: &str) -> Result<Uuid, String> {
    // Run lookup and persistence must use the same spelling for an ID.
    Uuid::parse_str(id)
        .ok()
        .filter(|parsed| parsed.to_string() == id)
        .ok_or_else(|| "App account does not exist.".to_string())
}

fn valid_label(label: &str) -> Result<String, String> {
    let label = label.trim();
    if label.is_empty() || label.chars().any(char::is_control) {
        return Err("Enter a name for the account.".to_string());
    }
    Ok(label.to_string())
}

fn read_auth(home: &Path) -> Option<Vec<u8>> {
    let path = home.join(AUTH_FILE);
    let metadata = fs::symlink_metadata(&path).ok()?;
    if !metadata.is_file() || metadata.len() > MAX_AUTH_BYTES {
        return None;
    }
    let mut bytes = Vec::new();
    fs::File::open(path)
        .ok()?
        .take(MAX_AUTH_BYTES + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    (bytes.len() as u64 <= MAX_AUTH_BYTES).then_some(bytes)
}

#[cfg(test)]
mod tests {
    use super::AppAccounts;
    use base64::Engine;
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use serde_json::json;
    use std::path::{Path, PathBuf};

    fn auth(home: &Path, user: &str, workspace: &str, token: &str) {
        let claims = json!({
            "https://api.openai.com/auth": {
                "chatgpt_user_id": user, "chatgpt_account_id": workspace
            }
        });
        let id_token = format!("e30.{}.fixture", URL_SAFE_NO_PAD.encode(claims.to_string()));
        let bytes = serde_json::to_vec(&json!({
            "auth_mode": "chatgpt",
            "tokens": {
                "id_token": id_token, "access_token": token,
                "refresh_token": "fixture-refresh", "account_id": workspace
            }
        }))
        .unwrap();
        std::fs::write(home.join("auth.json"), bytes).unwrap();
    }

    fn stage(accounts: &AppAccounts, id: &str, user: &str, workspace: &str) -> PathBuf {
        let stage = accounts
            .begin_sign_in(id, &uuid::Uuid::new_v4().to_string())
            .unwrap();
        auth(&stage, user, workspace, "fixture-access");
        stage
    }

    #[test]
    fn accounts_support_custom_names_and_more_than_two_sign_ins() {
        let data = tempfile::tempdir().unwrap();
        let accounts = AppAccounts::open(data.path());
        let label = "研究 / Team #3 ✨".repeat(20);
        let named = accounts.create(&label).unwrap();
        for number in 1..=20 {
            accounts.create(&format!("Client {number}")).unwrap();
        }
        let renamed = format!("{label} (archive)");
        accounts.update(&named.id, Some(&renamed), false).unwrap();
        let saved = AppAccounts::open(data.path()).list().unwrap();
        assert_eq!(saved.len(), 21);
        assert!(
            saved
                .iter()
                .any(|row| row.id == named.id && row.label == renamed)
        );
    }

    #[test]
    fn account_homes_and_labels_survive_restart_without_sharing_credentials() {
        let data = tempfile::tempdir().unwrap();
        let accounts = AppAccounts::open(data.path());
        let work = accounts.create(" Work ").unwrap();
        let personal = accounts.create("Personal").unwrap();
        assert_eq!(work.label, "Work");
        assert_eq!(work.status, "disconnected");
        assert!(accounts.resolve(&work.id).is_err());
        for (account, identity) in [(&work, "work-user"), (&personal, "personal-user")] {
            let stage = stage(&accounts, &account.id, identity, "workspace-1");
            accounts.complete_sign_in(&account.id, &stage).unwrap();
            accounts.discard_sign_in(&stage);
            assert!(!stage.exists());
        }

        let accounts = AppAccounts::open(data.path());
        let work_home = accounts.resolve(&work.id).unwrap();
        let personal_home = accounts.resolve(&personal.id).unwrap();
        assert_ne!(work_home, personal_home);
        assert_ne!(
            std::fs::read(work_home.join("auth.json")).unwrap(),
            std::fs::read(personal_home.join("auth.json")).unwrap()
        );
        let visible = serde_json::to_value(accounts.list().unwrap()).unwrap();
        assert_eq!(
            visible,
            json!([
                { "id": personal.id, "label": "Personal", "status": "ready" },
                { "id": work.id, "label": "Work", "status": "ready" }
            ])
        );
        let other_machine = tempfile::tempdir().unwrap();
        assert!(
            AppAccounts::open(other_machine.path())
                .resolve(&work.id)
                .is_err()
        );
    }

    #[test]
    fn reconnect_rejects_another_provider_user_or_workspace_and_keeps_chat_home() {
        let data = tempfile::tempdir().unwrap();
        let accounts = AppAccounts::open(data.path());
        let account = accounts.create("Work").unwrap();
        let first = stage(&accounts, &account.id, "user-1", "workspace-1");
        accounts.complete_sign_in(&account.id, &first).unwrap();
        let home = accounts.resolve(&account.id).unwrap();
        let auth_path = home.join("auth.json");
        let original_auth = std::fs::read(&auth_path).unwrap();
        let chat = home.join("fixture-session.jsonl");
        std::fs::write(&chat, "previous chat").unwrap();
        for (user, workspace) in [("user-2", "workspace-1"), ("user-1", "workspace-2")] {
            let candidate = stage(&accounts, &account.id, user, workspace);
            assert!(accounts.complete_sign_in(&account.id, &candidate).is_err());
            assert_eq!(std::fs::read(&auth_path).unwrap(), original_auth);
        }
        let row = accounts
            .update(&account.id, Some("New name"), true)
            .unwrap();
        assert_eq!(row.status, "disconnected");
        assert_eq!(row.label, "New name");
        assert!(!auth_path.exists());
        // A provider child may still be shutting down and write credentials
        // after disconnect. That can't make the account available again.
        std::fs::write(&auth_path, original_auth).unwrap();
        assert!(accounts.resolve(&account.id).is_err());

        let existing = stage(&accounts, &account.id, "user-1", "workspace-1");
        auth(
            &existing,
            "user-1",
            "workspace-1",
            "fixture-refreshed-token",
        );
        accounts.complete_sign_in(&account.id, &existing).unwrap();
        assert_eq!(accounts.resolve(&account.id).unwrap(), home);
        assert_eq!(std::fs::read_to_string(chat).unwrap(), "previous chat");
    }

    #[test]
    fn rejects_foreign_stages_and_unrecognized_or_replaced_credentials() {
        let data = tempfile::tempdir().unwrap();
        let accounts = AppAccounts::open(data.path());
        let first = accounts.create("First").unwrap();
        let second = accounts.create("Second").unwrap();
        let stage = stage(&accounts, &first.id, "user-1", "workspace-1");
        assert!(accounts.complete_sign_in(&second.id, &stage).is_err());
        assert!(accounts.begin_sign_in(&first.id, "../elsewhere").is_err());
        let outside = tempfile::tempdir().unwrap();
        accounts.discard_sign_in(outside.path());
        assert!(outside.path().exists());
        accounts.complete_sign_in(&first.id, &stage).unwrap();
        let home = accounts.resolve(&first.id).unwrap();
        let alias = format!("urn:uuid:{}", first.id);
        assert!(accounts.resolve(&alias).is_err());
        assert!(
            accounts
                .begin_sign_in(&alias, &uuid::Uuid::new_v4().to_string())
                .is_err()
        );
        auth(&home, "unexpected-user", "workspace-1", "fixture-access");
        assert!(accounts.resolve(&first.id).is_err());
        assert_eq!(accounts.list().unwrap()[0].status, "disconnected");
        assert!(accounts.resolve("../../home").is_err());
        assert!(accounts.create(" \n ").is_err());
    }

    #[test]
    fn startup_reclaims_stale_stages_without_touching_fresh_or_unrelated_paths() {
        use std::time::{Duration, SystemTime};

        let data = tempfile::tempdir().unwrap();
        let accounts = AppAccounts::open(data.path());
        let row = accounts.create("Work").unwrap();
        let completed = stage(&accounts, &row.id, "user-1", "workspace-1");
        accounts.complete_sign_in(&row.id, &completed).unwrap();
        let durable = accounts.resolve(&row.id).unwrap();
        let unrelated = completed.with_file_name("keep-this");
        std::fs::create_dir(&unrelated).unwrap();

        let reopened = AppAccounts::open(data.path());
        assert!(completed.exists());
        reopened.cleanup_stale_sign_ins(SystemTime::now() + Duration::from_secs(3601));
        assert!(!completed.exists());
        assert!(unrelated.exists());
        assert_eq!(reopened.resolve(&row.id).unwrap(), durable);
    }
}
