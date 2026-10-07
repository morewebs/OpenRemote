//! The registry client: the contract moreweb serves under `/v1/openremote`
//! (and the test double serves the same). A 401 means sign in again and is
//! never a reason to delete anything; only a 410, this device was removed,
//! is.

use serde_json::{Value, json};

use crate::https::{Http, HttpError, Response};
use crate::identity::Account;

#[derive(Debug, thiserror::Error)]
pub enum RegistryError {
    #[error(transparent)]
    Http(#[from] HttpError),
    #[error("sign in to OpenRemote Cloud again")]
    Unauthorized,
    #[error("this device was removed from your account")]
    Revoked,
    #[error("{message}")]
    Refused {
        status: u16,
        code: String,
        message: String,
    },
}

pub struct Registered {
    pub device: Value,
    pub credential: String,
    pub account: Account,
}

#[derive(Clone)]
pub struct Registry {
    http: Http,
    base: String,
}

fn check(response: Response) -> Result<Value, RegistryError> {
    let body = response.json();
    match response.status {
        200..=299 => Ok(body),
        401 => Err(RegistryError::Unauthorized),
        410 => Err(RegistryError::Revoked),
        status => Err(RegistryError::Refused {
            status,
            code: body["error"].as_str().unwrap_or("refused").to_string(),
            message: body["message"]
                .as_str()
                .unwrap_or("OpenRemote Cloud refused the request")
                .to_string(),
        }),
    }
}

fn registered(body: Value) -> Result<Registered, RegistryError> {
    let refused = || RegistryError::Refused {
        status: 502,
        code: "bad_answer".into(),
        message: "OpenRemote Cloud answered without a device credential".into(),
    };
    let credential = body["credential"].as_str().ok_or_else(refused)?.to_string();
    let account: Account =
        serde_json::from_value(body["account"].clone()).map_err(|_| refused())?;
    Ok(Registered {
        device: body["device"].clone(),
        credential,
        account,
    })
}

/// What a device says about itself when it joins.
pub struct DeviceInfo<'a> {
    pub id: &'a str,
    pub name: &'a str,
    pub platform: &'a str,
    pub noise_pubkey: &'a str,
    pub app_version: &'a str,
}

impl Registry {
    pub fn new(http: Http, base: &str) -> Self {
        Self {
            http,
            base: base.trim_end_matches('/').to_string(),
        }
    }

    fn url(&self, path: &str) -> String {
        format!("{}{}", self.base, path)
    }

    /// A desktop joins (or re-joins, same id and key) with an account token.
    pub async fn register(
        &self,
        account_token: &str,
        device: &DeviceInfo<'_>,
    ) -> Result<Registered, RegistryError> {
        let body = json!({
            "id": device.id,
            "name": device.name,
            "platform": device.platform,
            "noise_pubkey": device.noise_pubkey,
            "app_version": device.app_version,
        });
        let response = self
            .http
            .json(
                "POST",
                &self.url("/devices"),
                Some(account_token),
                Some(&body),
            )
            .await?;
        registered(check(response)?)
    }

    /// A machine joins by spending an install code.
    pub async fn redeem(
        &self,
        code: &str,
        device: &DeviceInfo<'_>,
    ) -> Result<Registered, RegistryError> {
        let body = json!({
            "code": code,
            "device": {
                "id": device.id,
                "name": device.name,
                "platform": device.platform,
                "noise_pubkey": device.noise_pubkey,
                "app_version": device.app_version,
            },
        });
        let response = self
            .http
            .json("POST", &self.url("/enrollments/redeem"), None, Some(&body))
            .await?;
        registered(check(response)?)
    }

    /// The account's devices, with who is online and the preferences.
    pub async fn devices(&self, bearer: &str) -> Result<Value, RegistryError> {
        let response = self
            .http
            .json("GET", &self.url("/devices"), Some(bearer), None)
            .await?;
        check(response)
    }

    pub async fn patch(
        &self,
        bearer: &str,
        id: &str,
        body: &Value,
    ) -> Result<Value, RegistryError> {
        let response = self
            .http
            .json(
                "PATCH",
                &self.url(&format!("/devices/{id}")),
                Some(bearer),
                Some(body),
            )
            .await?;
        check(response)
    }

    pub async fn delete(&self, bearer: &str, id: &str) -> Result<(), RegistryError> {
        let response = self
            .http
            .json(
                "DELETE",
                &self.url(&format!("/devices/{id}")),
                Some(bearer),
                None,
            )
            .await?;
        check(response).map(|_| ())
    }

    pub async fn create_enrollment(
        &self,
        account_token: &str,
        body: &Value,
    ) -> Result<Value, RegistryError> {
        let response = self
            .http
            .json(
                "POST",
                &self.url("/enrollments"),
                Some(account_token),
                Some(body),
            )
            .await?;
        check(response)
    }
}
