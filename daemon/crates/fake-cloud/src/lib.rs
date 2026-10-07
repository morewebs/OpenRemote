//! A stand-in for moreweb: the OAuth sign-in, the device registry under
//! `/v1/openremote`, and (with the relay) everything Cloud mode talks to,
//! in memory and on loopback. It serves the same contract as the real
//! backend, so the daemon's Cloud code is tested end to end without it.
//!
//! The authorize page approves at once as the current test user; that is
//! the one liberty it takes.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Redirect, Response};
use axum::routing::{get, patch, post};
use axum::{Form, Json, Router};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

pub mod relay;

pub const CLIENT_ID: &str = "openremote-desktop";

#[derive(Clone)]
pub struct Device {
    pub id: String,
    pub account: String,
    pub name: String,
    pub platform: String,
    pub kind: String,
    pub pubkey: String,
    pub credential: String,
    pub app_version: Option<String>,
    pub created_via: &'static str,
    pub revoked: bool,
    pub auto_update_override: Option<bool>,
}

struct Code {
    account: String,
    redirect_uri: String,
    challenge: String,
}

#[derive(Default)]
struct World {
    /// email → account id
    accounts: HashMap<String, String>,
    current_user: String,
    codes: HashMap<String, Code>,
    access: HashMap<String, String>,
    refresh: HashMap<String, String>,
    devices: HashMap<String, Device>,
    enrollments: HashMap<String, (String, bool)>,
    no_auto_update: std::collections::HashSet<String>,
}

#[derive(Clone)]
pub struct FakeCloud {
    pub base: String,
    world: Arc<Mutex<World>>,
    pub hub: Arc<relay::Hub>,
}

fn token(prefix: &str) -> String {
    format!("{prefix}{}", uuid::Uuid::new_v4().simple())
}

impl FakeCloud {
    /// Serves on a random loopback port until the process ends.
    pub async fn start() -> Self {
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .expect("bind loopback");
        let base = format!("http://{}", listener.local_addr().expect("addr"));
        let cloud = Self {
            base,
            world: Arc::new(Mutex::new(World {
                current_user: "user@example.test".into(),
                ..World::default()
            })),
            hub: Arc::new(relay::Hub::default()),
        };
        let app = router(cloud.clone());
        tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        cloud
    }

    pub fn issuer(&self) -> String {
        format!("{}/v1/auth", self.base)
    }

    pub fn api(&self) -> String {
        format!("{}/v1/openremote", self.base)
    }

    fn world(&self) -> std::sync::MutexGuard<'_, World> {
        self.world.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Who the authorize page signs in as from now on.
    pub fn set_user(&self, email: &str) {
        self.world().current_user = email.to_string();
    }

    pub fn account_id(&self, email: &str) -> String {
        let mut world = self.world();
        world
            .accounts
            .entry(email.to_string())
            .or_insert_with(|| uuid::Uuid::new_v4().to_string())
            .clone()
    }

    pub fn devices(&self) -> Vec<Device> {
        self.world().devices.values().cloned().collect()
    }

    /// An install code for `email`'s account, as the Machines tab mints one.
    pub fn enrollment_code(&self, email: &str) -> String {
        let account = self.account_id(email);
        let code = token("mwe_");
        self.world()
            .enrollments
            .insert(code.clone(), (account, false));
        code
    }

    /// Removes a device, as another of the user's devices would.
    pub fn remove_device(&self, id: &str) {
        let account = {
            let mut world = self.world();
            let Some(device) = world.devices.get_mut(id) else {
                return;
            };
            device.revoked = true;
            device.account.clone()
        };
        self.hub.revoke(&account, id);
        self.hub.devices_changed(&account);
    }

    fn account_of_token(&self, headers: &HeaderMap) -> Option<String> {
        let bearer = bearer(headers)?;
        self.world().access.get(bearer).cloned()
    }

    /// Ok(device) for a live credential, Err(status) otherwise.
    pub fn device_of(&self, headers: &HeaderMap) -> Result<Device, StatusCode> {
        let bearer = bearer(headers).ok_or(StatusCode::UNAUTHORIZED)?;
        let world = self.world();
        let device = world
            .devices
            .values()
            .find(|d| d.credential == bearer)
            .ok_or(StatusCode::UNAUTHORIZED)?;
        if device.revoked {
            return Err(StatusCode::GONE);
        }
        Ok(device.clone())
    }

    fn account_json(&self, account: &str) -> Value {
        let world = self.world();
        let email = world
            .accounts
            .iter()
            .find(|(_, id)| *id == account)
            .map(|(email, _)| email.clone())
            .unwrap_or_default();
        json!({"id": account, "email": email, "name": email.split('@').next().unwrap_or("")})
    }

    fn device_json(&self, d: &Device, this: Option<&str>) -> Value {
        let account_auto = !self.world().no_auto_update.contains(&d.account);
        json!({
            "id": d.id, "name": d.name, "platform": d.platform, "kind": d.kind,
            "noise_pubkey": d.pubkey, "app_version": d.app_version,
            "created_via": d.created_via, "online": self.hub.online(&d.account).contains(&d.id),
            "auto_update_override": d.auto_update_override,
            "auto_update": d.auto_update_override.unwrap_or(account_auto),
            "this_device": this == Some(d.id.as_str()),
        })
    }
}

fn bearer(headers: &HeaderMap) -> Option<&str> {
    headers
        .get("authorization")?
        .to_str()
        .ok()?
        .strip_prefix("Bearer ")
}

fn error(status: StatusCode, code: &str) -> Response {
    (status, Json(json!({"error": code, "message": code}))).into_response()
}

fn router(cloud: FakeCloud) -> Router {
    Router::new()
        .route("/v1/auth/oauth/authorize", get(authorize))
        .route("/v1/auth/oauth/token", post(token_endpoint))
        .route("/v1/auth/oauth/revoke", post(revoke))
        .route("/v1/openremote/devices", get(list_devices).post(register))
        .route(
            "/v1/openremote/devices/{id}",
            patch(update_device).delete(delete_device),
        )
        .route("/v1/openremote/enrollments", post(create_enrollment))
        .route("/v1/openremote/enrollments/redeem", post(redeem))
        .route("/v1/openremote/prefs", get(get_prefs).put(put_prefs))
        .route("/v1/openremote/relay", get(relay::relay_ws))
        .with_state(cloud)
}

#[derive(Deserialize)]
struct AuthorizeQuery {
    client_id: String,
    redirect_uri: String,
    state: String,
    code_challenge: String,
    code_challenge_method: String,
}

async fn authorize(State(cloud): State<FakeCloud>, Query(q): Query<AuthorizeQuery>) -> Response {
    let loopback = q.redirect_uri.starts_with("http://127.0.0.1:")
        && q.redirect_uri.ends_with("/cloud/callback");
    if q.client_id != CLIENT_ID || !loopback || q.code_challenge_method != "S256" {
        return error(StatusCode::BAD_REQUEST, "invalid_request");
    }
    let email = cloud.world().current_user.clone();
    let account = cloud.account_id(&email);
    let code = token("code_");
    cloud.world().codes.insert(
        code.clone(),
        Code {
            account,
            redirect_uri: q.redirect_uri.clone(),
            challenge: q.code_challenge,
        },
    );
    Redirect::to(&format!(
        "{}?code={}&state={}",
        q.redirect_uri, code, q.state
    ))
    .into_response()
}

#[derive(Deserialize)]
struct TokenForm {
    grant_type: String,
    code: Option<String>,
    redirect_uri: Option<String>,
    client_id: Option<String>,
    code_verifier: Option<String>,
    refresh_token: Option<String>,
}

fn tokens(cloud: &FakeCloud, account: String) -> Response {
    let access = token("at_");
    let refresh = token("rt_");
    let mut world = cloud.world();
    world.access.insert(access.clone(), account.clone());
    world.refresh.insert(refresh.clone(), account);
    Json(json!({"access_token": access, "refresh_token": refresh, "token_type": "Bearer", "expires_in": 3600}))
        .into_response()
}

async fn token_endpoint(State(cloud): State<FakeCloud>, Form(f): Form<TokenForm>) -> Response {
    if f.client_id.as_deref() != Some(CLIENT_ID) {
        return error(StatusCode::BAD_REQUEST, "invalid_grant");
    }
    match f.grant_type.as_str() {
        "authorization_code" => {
            let code = cloud.world().codes.remove(f.code.as_deref().unwrap_or(""));
            let Some(code) = code else {
                return error(StatusCode::BAD_REQUEST, "invalid_grant");
            };
            let verifier = f.code_verifier.unwrap_or_default();
            let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
            if challenge != code.challenge || f.redirect_uri.as_deref() != Some(&code.redirect_uri)
            {
                return error(StatusCode::BAD_REQUEST, "invalid_grant");
            }
            tokens(&cloud, code.account)
        }
        "refresh_token" => {
            let account = cloud
                .world()
                .refresh
                .remove(f.refresh_token.as_deref().unwrap_or(""));
            match account {
                Some(account) => tokens(&cloud, account),
                None => error(StatusCode::BAD_REQUEST, "invalid_grant"),
            }
        }
        _ => error(StatusCode::BAD_REQUEST, "unsupported_grant_type"),
    }
}

#[derive(Deserialize)]
struct RevokeForm {
    token: String,
}

async fn revoke(State(cloud): State<FakeCloud>, Form(f): Form<RevokeForm>) -> StatusCode {
    cloud.world().refresh.remove(&f.token);
    StatusCode::OK
}

#[derive(Deserialize)]
struct RegisterBody {
    id: String,
    name: String,
    platform: String,
    noise_pubkey: String,
    app_version: Option<String>,
    kind: Option<String>,
}

async fn register(
    State(cloud): State<FakeCloud>,
    headers: HeaderMap,
    Json(b): Json<RegisterBody>,
) -> Response {
    let Some(account) = cloud.account_of_token(&headers) else {
        return error(StatusCode::UNAUTHORIZED, "invalid_token");
    };
    let credential = token("mwd_");
    let (status, device) = {
        let mut world = cloud.world();
        let key_taken = world
            .devices
            .values()
            .any(|d| d.pubkey == b.noise_pubkey && d.id != b.id);
        match world.devices.get_mut(&b.id) {
            Some(d) if d.revoked => return error(StatusCode::GONE, "device_revoked"),
            Some(d) if d.account != account || d.pubkey != b.noise_pubkey => {
                return error(StatusCode::CONFLICT, "device_conflict");
            }
            Some(d) => {
                d.credential = credential.clone();
                d.name = b.name;
                d.app_version = b.app_version;
                (StatusCode::OK, d.clone())
            }
            None if key_taken => return error(StatusCode::CONFLICT, "device_conflict"),
            None => {
                let d = Device {
                    id: b.id.clone(),
                    account: account.clone(),
                    name: b.name,
                    platform: b.platform,
                    kind: b.kind.unwrap_or_else(|| "desktop".into()),
                    pubkey: b.noise_pubkey,
                    credential: credential.clone(),
                    app_version: b.app_version,
                    created_via: "login",
                    revoked: false,
                    auto_update_override: None,
                };
                world.devices.insert(b.id.clone(), d.clone());
                (StatusCode::CREATED, d)
            }
        }
    };
    cloud
        .hub
        .close_device(&account, &device.id, relay::CLOSE_ROTATED);
    cloud.hub.devices_changed(&account);
    (
        status,
        Json(json!({
            "device": cloud.device_json(&device, Some(&device.id)),
            "credential": credential,
            "account": cloud.account_json(&account),
        })),
    )
        .into_response()
}

/// The caller's account, and its device when it called with a credential;
/// or the status and error code that refuse it.
fn caller(
    cloud: &FakeCloud,
    headers: &HeaderMap,
) -> Result<(String, Option<Device>), (StatusCode, &'static str)> {
    if let Some(account) = cloud.account_of_token(headers) {
        return Ok((account, None));
    }
    match cloud.device_of(headers) {
        Ok(d) => Ok((d.account.clone(), Some(d))),
        Err(StatusCode::GONE) => Err((StatusCode::GONE, "device_revoked")),
        Err(status) => Err((status, "invalid_token")),
    }
}

async fn list_devices(State(cloud): State<FakeCloud>, headers: HeaderMap) -> Response {
    let (account, me) = match caller(&cloud, &headers) {
        Ok(c) => c,
        Err((status, code)) => return error(status, code),
    };
    let devices: Vec<Device> = cloud
        .devices()
        .into_iter()
        .filter(|d| d.account == account && !d.revoked)
        .collect();
    let this = me.as_ref().map(|d| d.id.as_str());
    let auto = !cloud.world().no_auto_update.contains(&account);
    Json(json!({
        "account": cloud.account_json(&account),
        "devices": devices.iter().map(|d| cloud.device_json(d, this)).collect::<Vec<_>>(),
        "prefs": {"auto_update": auto},
    }))
    .into_response()
}

async fn update_device(
    State(cloud): State<FakeCloud>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Response {
    let (account, me) = match caller(&cloud, &headers) {
        Ok(c) => c,
        Err((status, code)) => return error(status, code),
    };
    if let Some(me) = &me {
        if me.id != id || body.get("kind").is_some() || body.get("auto_update_override").is_some() {
            return error(StatusCode::FORBIDDEN, "account_token_required");
        }
    }
    let device = {
        let mut world = cloud.world();
        let Some(d) = world
            .devices
            .get_mut(&id)
            .filter(|d| d.account == account && !d.revoked)
        else {
            return error(StatusCode::NOT_FOUND, "not_found");
        };
        if let Some(name) = body["name"].as_str() {
            d.name = name.to_string();
        }
        if let Some(kind) = body["kind"].as_str() {
            d.kind = kind.to_string();
        }
        if let Some(version) = body["app_version"].as_str() {
            d.app_version = Some(version.to_string());
        }
        match body["auto_update_override"].as_str() {
            Some("on") => d.auto_update_override = Some(true),
            Some("off") => d.auto_update_override = Some(false),
            Some("inherit") => d.auto_update_override = None,
            _ => {}
        }
        d.clone()
    };
    cloud.hub.devices_changed(&account);
    Json(cloud.device_json(&device, me.as_ref().map(|d| d.id.as_str()))).into_response()
}

async fn delete_device(
    State(cloud): State<FakeCloud>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response {
    let (account, me) = match caller(&cloud, &headers) {
        Ok(c) => c,
        Err((status, code)) => return error(status, code),
    };
    if me.as_ref().is_some_and(|d| d.id != id) {
        return error(StatusCode::FORBIDDEN, "account_token_required");
    }
    let found = cloud
        .devices()
        .iter()
        .any(|d| d.id == id && d.account == account && !d.revoked);
    if !found {
        return error(StatusCode::NOT_FOUND, "not_found");
    }
    cloud.remove_device(&id);
    StatusCode::NO_CONTENT.into_response()
}

async fn create_enrollment(State(cloud): State<FakeCloud>, headers: HeaderMap) -> Response {
    let Some(account) = cloud.account_of_token(&headers) else {
        return error(StatusCode::FORBIDDEN, "account_token_required");
    };
    let code = token("mwe_");
    cloud
        .world()
        .enrollments
        .insert(code.clone(), (account, false));
    (
        StatusCode::CREATED,
        Json(json!({
            "id": uuid::Uuid::new_v4().to_string(),
            "code": code,
            "expires_at": 0,
            "install_command": format!("curl -fsSL https://moreweb.space/openremote/install.sh | sh -s -- {code}"),
        })),
    )
        .into_response()
}

#[derive(Deserialize)]
struct RedeemBody {
    code: String,
    device: RegisterBody,
}

async fn redeem(State(cloud): State<FakeCloud>, Json(b): Json<RedeemBody>) -> Response {
    let credential = token("mwd_");
    let account = {
        let mut world = cloud.world();
        let account = match world.enrollments.get(&b.code) {
            Some((account, false)) => account.clone(),
            _ => return error(StatusCode::BAD_REQUEST, "invalid_code"),
        };
        if world
            .devices
            .values()
            .any(|d| d.id == b.device.id || d.pubkey == b.device.noise_pubkey)
        {
            return error(StatusCode::CONFLICT, "device_conflict");
        }
        world
            .enrollments
            .insert(b.code.clone(), (account.clone(), true));
        world.devices.insert(
            b.device.id.clone(),
            Device {
                id: b.device.id.clone(),
                account: account.clone(),
                name: b.device.name.clone(),
                platform: b.device.platform.clone(),
                kind: "machine".into(),
                pubkey: b.device.noise_pubkey.clone(),
                credential: credential.clone(),
                app_version: b.device.app_version.clone(),
                created_via: "enrollment",
                revoked: false,
                auto_update_override: None,
            },
        );
        account
    };
    cloud.hub.devices_changed(&account);
    let device = cloud.devices().into_iter().find(|d| d.id == b.device.id);
    (
        StatusCode::CREATED,
        Json(json!({
            "device": device.map(|d| cloud.device_json(&d, Some(&b.device.id))),
            "credential": credential,
            "account": cloud.account_json(&account),
        })),
    )
        .into_response()
}

async fn get_prefs(State(cloud): State<FakeCloud>, headers: HeaderMap) -> Response {
    match caller(&cloud, &headers) {
        Ok((account, _)) => {
            let on = !cloud.world().no_auto_update.contains(&account);
            Json(json!({"auto_update": on})).into_response()
        }
        Err((status, code)) => error(status, code),
    }
}

#[derive(Deserialize)]
struct Prefs {
    auto_update: bool,
}

async fn put_prefs(
    State(cloud): State<FakeCloud>,
    headers: HeaderMap,
    Json(p): Json<Prefs>,
) -> Response {
    let Some(account) = cloud.account_of_token(&headers) else {
        return error(StatusCode::FORBIDDEN, "account_token_required");
    };
    {
        let mut world = cloud.world();
        if p.auto_update {
            world.no_auto_update.remove(&account);
        } else {
            world.no_auto_update.insert(account.clone());
        }
    }
    cloud.hub.devices_changed(&account);
    Json(json!({"auto_update": p.auto_update})).into_response()
}
