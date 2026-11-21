use super::{Supervisor, SupervisorError, backend_resolution, lock_recovering};
use crate::app_accounts::AppAccountView;
use crate::signin::{SignInRun, SignInView};
use serde::Serialize;
use std::path::PathBuf;
use std::sync::Arc;
use uuid::Uuid;

pub(super) struct AccountLogin {
    id: String,
    request_id: String,
    run: Arc<SignInRun>,
}

impl AccountLogin {
    fn view(&self) -> AccountSignInView {
        AccountSignInView {
            run_id: self.id.clone(),
            view: self.run.view(),
        }
    }
}

#[derive(Serialize)]
pub(crate) struct AccountSignInView {
    run_id: String,
    #[serde(flatten)]
    view: SignInView,
}

impl Supervisor {
    fn require_accounts(&self, harness: &str) -> Result<(), SupervisorError> {
        if harness == "codex"
            && self
                .harnesses()
                .iter()
                .any(|app| app.id == harness && app.available && app.app_accounts_supported)
        {
            return Ok(());
        }
        Err(SupervisorError::Conflict(
            "managed accounts are unavailable for this App on this machine".into(),
        ))
    }

    pub(super) fn account_home(
        &self,
        harness: &str,
        id: Option<&str>,
        endpoint: Option<&str>,
    ) -> Result<Option<PathBuf>, SupervisorError> {
        let Some(id) = id else {
            return Ok(None);
        };
        self.require_accounts(harness)?;
        if endpoint.is_some_and(|endpoint| !endpoint.trim().is_empty()) {
            return Err(SupervisorError::Conflict(
                "choose an App account or a custom endpoint for this chat".into(),
            ));
        }
        self.app_accounts
            .resolve(id)
            .map(Some)
            .map_err(SupervisorError::Conflict)
    }

    pub(crate) fn app_accounts(
        &self,
        harness: &str,
    ) -> Result<Vec<AppAccountView>, SupervisorError> {
        self.require_accounts(harness)?;
        self.app_accounts.list().map_err(SupervisorError::Conflict)
    }

    pub(crate) fn create_app_account(
        &self,
        harness: &str,
        label: &str,
    ) -> Result<AppAccountView, SupervisorError> {
        self.require_accounts(harness)?;
        self.app_accounts
            .create(label)
            .map_err(SupervisorError::Conflict)
    }

    // Called with the account mutations lock held, so disconnect cannot race a login commit.
    pub(crate) fn update_app_account(
        &self,
        harness: &str,
        id: &str,
        label: Option<&str>,
        disconnect: bool,
    ) -> Result<AppAccountView, SupervisorError> {
        self.require_accounts(harness)?;
        if disconnect
            && lock_recovering(&self.app_signins)
                .get(id)
                .is_some_and(|login| login.run.view().running)
        {
            return Err(SupervisorError::Conflict(
                "stop the active sign-in before disconnecting this account".into(),
            ));
        }
        self.app_accounts
            .update(id, label, disconnect)
            .map_err(SupervisorError::Conflict)
    }

    pub(crate) fn start_app_account_sign_in(
        self: &Arc<Self>,
        harness: &str,
        id: &str,
        request_id: String,
    ) -> Result<AccountSignInView, SupervisorError> {
        self.require_accounts(harness)?;
        let _mutation = lock_recovering(&self.account_mutations);
        let mut signins = lock_recovering(&self.app_signins);
        if let Some(login) = signins.get(id) {
            if login.request_id == request_id {
                return Ok(login.view());
            }
            if login.run.view().running {
                return Err(SupervisorError::Conflict(
                    "a sign-in for this account is already running".into(),
                ));
            }
        }
        let resolution = self
            .backend(harness)
            .and_then(|backend| backend_resolution(&backend).cloned())
            .ok_or_else(|| {
                SupervisorError::Conflict("Codex is unavailable on this machine".into())
            })?;
        let run_id = Uuid::new_v4().to_string();
        let stage = self
            .app_accounts
            .begin_sign_in(id, &run_id)
            .map_err(SupervisorError::Conflict)?;
        let (run, mut child) = match crate::signin::spawn_managed_login(&resolution, &stage) {
            Ok(login) => login,
            Err(message) => {
                self.app_accounts.discard_sign_in(&stage);
                return Err(SupervisorError::Conflict(message));
            }
        };
        let login = AccountLogin {
            id: run_id,
            request_id,
            run: run.clone(),
        };
        let initial = login.view();
        signins.insert(id.into(), login);

        let supervisor = self.clone();
        let id = id.to_string();
        tokio::spawn(async move {
            let result = tokio::select! {
                biased;
                _ = run.stopped() => {
                    let _ = child.kill().await;
                    Err("Sign-in cancelled.".into())
                }
                _ = tokio::time::sleep(std::time::Duration::from_secs(15 * 60)) => {
                    let _ = child.kill().await;
                    Err("Codex sign-in timed out. Try again.".into())
                }
                status = child.wait() => match status {
                    Ok(status) if status.success() => Ok(()),
                    _ if run.view().lines.is_empty() => Err(
                        "Codex could not start device sign-in. Check that device code sign-in is enabled for your ChatGPT account and try again.".into()
                    ),
                    _ => Err("Codex could not finish signing in. Try again.".into()),
                }
            };
            let _mutation = lock_recovering(&supervisor.account_mutations);
            let result =
                result.and_then(|()| supervisor.app_accounts.complete_sign_in(&id, &stage));
            supervisor.app_accounts.discard_sign_in(&stage);
            run.finish_managed(result);
        });
        Ok(initial)
    }

    pub(crate) fn app_account_sign_in(
        &self,
        harness: &str,
        id: &str,
        run_id: &str,
        stop: bool,
    ) -> Result<AccountSignInView, SupervisorError> {
        if harness != "codex" {
            return Err(SupervisorError::NotFound("No such sign-in.".into()));
        }
        let signins = lock_recovering(&self.app_signins);
        let login = signins
            .get(id)
            .filter(|login| login.id == run_id)
            .ok_or_else(|| SupervisorError::NotFound("No such sign-in.".into()))?;
        if stop {
            login.run.stop();
        }
        Ok(login.view())
    }
}
