//! Account state for an explicitly selected Codex sign-in.

use std::fs::File;
use std::io::Read;
use std::path::Path;

use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::Deserialize;
use tokio::process::Command;

const AUTH_MAX_BYTES: usize = 128 * 1024;

/// Use the same provider and storage when signing in and running a chat.
/// Existing sign-ins and environment-provided credentials stay outside
/// the managed profile.
pub fn configure_managed_account(command: &mut Command, home: &Path) {
    command
        .env("CODEX_HOME", home)
        .env("CODEX_SQLITE_HOME", home)
        .args([
            "-c",
            "cli_auth_credentials_store=\"file\"",
            "-c",
            "forced_login_method=\"chatgpt\"",
            "-c",
            "model_provider=\"openai\"",
            // The workspace may set custom endpoints. Managed sign-ins
            // only send their credentials to the official provider.
            "-c",
            "chatgpt_base_url=\"https://chatgpt.com/backend-api/\"",
            "-c",
            "openai_base_url=\"https://chatgpt.com/backend-api/codex\"",
        ]);
    for key in [
        "OPENAI_API_KEY",
        "OPENAI_FEDERATION_RULE_ID",
        "OPENAI_IDENTITY_TOKEN_FILE",
        "OPENAI_WORKLOAD_IDENTITY_CONTEXT",
        "CODEX_API_KEY",
        "CODEX_ACCESS_TOKEN",
        "CODEX_EXEC_SERVER_URL",
        "CODEX_CLOUD_TASKS_MODE",
        "OPENAI_BASE_URL",
        "CODEX_APP_SERVER_CHATGPT_BASE_URL",
        "CODEX_AUTHAPI_BASE_URL",
        "CODEX_APP_SERVER_LOGIN_CLIENT_ID",
        "CODEX_REFRESH_TOKEN_URL_OVERRIDE",
        "CODEX_REVOKE_TOKEN_URL_OVERRIDE",
    ] {
        command.env_remove(key);
    }
}

#[derive(Deserialize)]
struct AuthFile {
    auth_mode: String,
    tokens: Tokens,
}

#[derive(Deserialize)]
struct Tokens {
    id_token: String,
    access_token: String,
    refresh_token: String,
    account_id: Option<String>,
}

#[derive(Deserialize)]
struct IdClaims {
    #[serde(rename = "https://api.openai.com/auth")]
    auth: OpenAiClaims,
}

#[derive(Deserialize)]
struct OpenAiClaims {
    chatgpt_user_id: Option<String>,
    user_id: Option<String>,
    chatgpt_account_id: Option<String>,
}

fn valid_id(value: &str) -> bool {
    (1..=256).contains(&value.len())
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'))
}

/// Read the identity previously authenticated by Codex, from the private
/// credential file the CLI owns. This only compares the user's account
/// after provider login. It must not be used to authenticate arbitrary JWTs.
pub fn managed_identity_from_auth(contents: &[u8]) -> Option<String> {
    if contents.len() > AUTH_MAX_BYTES {
        return None;
    }
    let AuthFile { auth_mode, tokens } = serde_json::from_slice(contents).ok()?;
    if auth_mode != "chatgpt"
        || tokens.access_token.trim().is_empty()
        || tokens.refresh_token.trim().is_empty()
    {
        return None;
    }
    let mut jwt = tokens.id_token.split('.');
    let (header, payload, signature) = (jwt.next()?, jwt.next()?, jwt.next()?);
    if header.is_empty() || signature.is_empty() || jwt.next().is_some() {
        return None;
    }
    let payload = URL_SAFE_NO_PAD.decode(payload).ok()?;
    let IdClaims { auth } = serde_json::from_slice(&payload).ok()?;
    let user = auth.chatgpt_user_id.or(auth.user_id)?;
    // Earlier Codex releases wrote the selected workspace into the
    // token record as well. Conflicting selectors cannot identify one account.
    if let (Some(claim), Some(saved)) = (&auth.chatgpt_account_id, &tokens.account_id) {
        if claim != saved {
            return None;
        }
    }
    let workspace = auth.chatgpt_account_id.or(tokens.account_id)?;
    if !valid_id(&user) || !valid_id(&workspace) {
        return None;
    }
    Some(format!("{user}:{workspace}"))
}

/// Read an identity without accepting an unbounded credential file.
pub fn managed_identity(home: &Path) -> Option<String> {
    let mut contents = Vec::new();
    File::open(home.join("auth.json"))
        .ok()?
        .take((AUTH_MAX_BYTES + 1) as u64)
        .read_to_end(&mut contents)
        .ok()?;
    managed_identity_from_auth(&contents)
}
