//! moreweb sign-in for a desktop app (RFC 8252): the system browser goes to
//! the authorize page, and the code comes back to this daemon's own loopback
//! address. PKCE (S256) binds the code to this sign-in.

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use sha2::{Digest, Sha256};

use crate::https::{Http, HttpError, form_encode};

/// 48 random bytes from three v4 UUIDs (the OS RNG through getrandom).
fn random_token() -> String {
    let mut bytes = Vec::with_capacity(48);
    for _ in 0..3 {
        bytes.extend_from_slice(uuid::Uuid::new_v4().as_bytes());
    }
    URL_SAFE_NO_PAD.encode(bytes)
}

pub struct Pkce {
    pub verifier: String,
    pub challenge: String,
}

impl Pkce {
    pub fn new() -> Self {
        let verifier = random_token();
        Self {
            challenge: challenge_for(&verifier),
            verifier,
        }
    }
}

impl Default for Pkce {
    fn default() -> Self {
        Self::new()
    }
}

pub fn challenge_for(verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

/// A fresh, unguessable `state` for one sign-in.
pub fn new_state() -> String {
    random_token()
}

pub fn authorize_url(
    issuer: &str,
    client_id: &str,
    redirect_uri: &str,
    state: &str,
    challenge: &str,
) -> String {
    format!(
        "{issuer}/oauth/authorize?{}",
        form_encode(&[
            ("response_type", "code"),
            ("client_id", client_id),
            ("redirect_uri", redirect_uri),
            ("scope", "openid profile email"),
            ("state", state),
            ("code_challenge", challenge),
            ("code_challenge_method", "S256"),
        ])
    )
}

#[derive(Debug, Clone)]
pub struct Tokens {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_in: i64,
}

#[derive(Debug, thiserror::Error)]
pub enum OAuthError {
    #[error(transparent)]
    Http(#[from] HttpError),
    /// The server refused: the code, verifier or refresh token is no good.
    #[error("{0}")]
    Refused(String),
}

fn tokens_from(response: crate::https::Response) -> Result<Tokens, OAuthError> {
    let body = response.json();
    if response.status != 200 {
        let why = body["error_description"]
            .as_str()
            .or(body["error"].as_str())
            .unwrap_or("sign-in was refused")
            .to_string();
        return Err(OAuthError::Refused(why));
    }
    let access_token = body["access_token"]
        .as_str()
        .ok_or_else(|| OAuthError::Refused("no access token in the answer".into()))?
        .to_string();
    Ok(Tokens {
        access_token,
        refresh_token: body["refresh_token"].as_str().map(str::to_string),
        expires_in: body["expires_in"].as_i64().unwrap_or(3600),
    })
}

pub async fn exchange(
    http: &Http,
    issuer: &str,
    client_id: &str,
    code: &str,
    redirect_uri: &str,
    verifier: &str,
) -> Result<Tokens, OAuthError> {
    let response = http
        .form(
            &format!("{issuer}/oauth/token"),
            &[
                ("grant_type", "authorization_code"),
                ("code", code),
                ("redirect_uri", redirect_uri),
                ("client_id", client_id),
                ("code_verifier", verifier),
            ],
        )
        .await?;
    tokens_from(response)
}

/// Refresh tokens rotate: the answer carries the one to keep.
pub async fn refresh(
    http: &Http,
    issuer: &str,
    client_id: &str,
    refresh_token: &str,
) -> Result<Tokens, OAuthError> {
    let response = http
        .form(
            &format!("{issuer}/oauth/token"),
            &[
                ("grant_type", "refresh_token"),
                ("refresh_token", refresh_token),
                ("client_id", client_id),
            ],
        )
        .await?;
    tokens_from(response)
}

/// Ends the sign-in on the server (RFC 7009). Best effort.
pub async fn revoke(http: &Http, issuer: &str, client_id: &str, token: &str) {
    let _ = http
        .form(
            &format!("{issuer}/oauth/revoke"),
            &[("token", token), ("client_id", client_id)],
        )
        .await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn s256_matches_the_rfc_7636_example() {
        assert_eq!(
            challenge_for("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
        let pkce = Pkce::new();
        assert_eq!(pkce.verifier.len(), 64, "inside RFC 7636's 43-128");
        assert_ne!(Pkce::new().verifier, pkce.verifier);
    }
}
