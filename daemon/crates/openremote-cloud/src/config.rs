//! Where Cloud mode reaches moreweb. Production by default; the environment
//! points a daemon at a local backend or the test double.

#[derive(Clone, Debug)]
pub struct CloudConfig {
    /// The registry, e.g. `https://api.moreweb.space/v1/openremote`.
    pub api_base: String,
    /// The OAuth issuer, e.g. `https://auth.moreweb.space/v1/auth`.
    pub auth_issuer: String,
    pub client_id: String,
    /// This device's name in the user's device list.
    pub device_name: String,
    /// The OS this device registers as (`windows`, `linux`, `android`...).
    pub platform: String,
    /// Where the sign-in page sends the browser back. None is the loopback
    /// callback on this daemon's own port; the Android app, which the OS
    /// may freeze while the browser is in front, takes [`APP_REDIRECT`].
    pub redirect_uri: Option<String>,
}

pub const DEFAULT_API: &str = "https://api.moreweb.space/v1/openremote";
pub const DEFAULT_ISSUER: &str = "https://auth.moreweb.space/v1/auth";
pub const CLIENT_ID: &str = "openremote-desktop";
/// The Android app's sign-in return address: a private-use scheme named
/// after the app's id (RFC 8252 7.1), which the OS hands to the app.
pub const APP_REDIRECT: &str = "space.moreweb.openremote:/cloud/callback";

/// A phone drives machines and is never one: it can't run harnesses.
pub fn is_phone(platform: &str) -> bool {
    matches!(platform, "android" | "ios")
}

impl CloudConfig {
    /// `OPENREMOTE_CLOUD_API`, `OPENREMOTE_AUTH_ISSUER` and
    /// `OPENREMOTE_DEVICE_NAME` override the defaults.
    pub fn from_env() -> Self {
        let var = |name: &str| std::env::var(name).ok().filter(|v| !v.trim().is_empty());
        Self {
            api_base: var("OPENREMOTE_CLOUD_API")
                .unwrap_or_else(|| DEFAULT_API.to_string())
                .trim_end_matches('/')
                .to_string(),
            auth_issuer: var("OPENREMOTE_AUTH_ISSUER")
                .unwrap_or_else(|| DEFAULT_ISSUER.to_string())
                .trim_end_matches('/')
                .to_string(),
            client_id: CLIENT_ID.to_string(),
            device_name: var("OPENREMOTE_DEVICE_NAME").unwrap_or_else(openremote_core::hostname),
            platform: std::env::consts::OS.to_string(),
            redirect_uri: None,
        }
    }

    /// The relay's WebSocket URL, derived from the registry's.
    pub fn relay_url(&self) -> String {
        let base = if let Some(rest) = self.api_base.strip_prefix("https://") {
            format!("wss://{rest}")
        } else if let Some(rest) = self.api_base.strip_prefix("http://") {
            format!("ws://{rest}")
        } else {
            self.api_base.clone()
        };
        format!("{base}/relay")
    }
}
