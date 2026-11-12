//! What a JavaScript plugin's frame reaches through the console that hosts
//! it. The frame itself reaches nothing: it is sandboxed to scripts alone,
//! holds no token and has no address of its own. The console answers its
//! calls, and whatever needs this machine - the network, the chats - comes
//! here, where each route checks the plugin's install record: the powers
//! the user approved, whether it is on, and, for a project's plugin, that a
//! chat it names is one of that project's.
//!
//! A plugin's secrets never leave this daemon except inside the plugin's
//! own request, written into the header that names it - and, when the
//! plugin declares a `sendTo`, only toward the address the user saved in
//! that other setting (its default never counts: without a saved address
//! the secret stays out, or the request is refused when it is required).
//!
//! Console-only, like the rest of the plugin routes: none of these answers
//! the user's other devices.

use std::sync::Arc;
use std::time::Duration;

use axum::extract::{DefaultBodyLimit, Path as AxumPath, State};
use axum::http::{StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::{Json, Router};
use base64::Engine;
use openremote_core::{
    InstalledPlugin, PluginPower, PluginRuntime as Runtime, SettingField, SettingKind,
};
use serde::Deserialize;
use serde_json::{Map, Value, json};

use crate::app::App;

/// A plugin's request arrives as base64 inside JSON: room for about 32 MB
/// of bytes, past the 25 MB the common transcription services accept.
const MAX_FETCH_BODY: usize = 48 * 1024 * 1024;
const DEFAULT_FETCH_TIME: Duration = Duration::from_secs(120);
const MAX_FETCH_TIME: Duration = Duration::from_secs(300);

/// How a header names one of the plugin's secrets: `{{secret:api_key}}`.
const SECRET_OPEN: &str = "{{secret:";
const SECRET_CLOSE: &str = "}}";

/// Headers a plugin's request may not set: the connection's own, and the
/// ones the client writes itself.
const FORBIDDEN_HEADERS: &[&str] = &[
    "host",
    "content-length",
    "connection",
    "keep-alive",
    "transfer-encoding",
    "upgrade",
    "te",
    "trailer",
    "user-agent",
    "cookie",
];

const METHODS: &[&str] = &["GET", "POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"];

/// The calls a frame's bridge carries here, the same ones (and the same
/// gate) a process plugin's program makes over its own pipe. Nothing else
/// is offered: the list is the whole of it.
pub const BRIDGE_METHODS: &[&str] = &[
    "chats/list",
    "chats/read",
    "chats/start",
    "chats/prompt",
    "chats/stop",
];

pub fn routes() -> Router<Arc<App>> {
    Router::new()
        .route(
            "/plugins/{id}/fetch",
            post(fetch).layer(DefaultBodyLimit::max(MAX_FETCH_BODY)),
        )
        .route("/plugins/{id}/call", post(call))
}

fn bad(status: StatusCode, message: &str) -> Response {
    (status, Json(json!({"error": message}))).into_response()
}

/// The installed JavaScript plugin an id names, on, with its files still
/// what was approved. The refusal is boxed: a Response is large, and the
/// Ok side is the common one.
fn js_plugin(app: &App, id: &str) -> Result<InstalledPlugin, Box<Response>> {
    let plugin = app.plugin_runtime.plugin(id).ok_or_else(|| {
        Box::new(bad(
            StatusCode::NOT_FOUND,
            &format!("{id} is not installed"),
        ))
    })?;
    if plugin.runtime != Runtime::Js {
        return Err(Box::new(bad(
            StatusCode::NOT_FOUND,
            &format!("{} does not run in the console", plugin.name),
        )));
    }
    if !plugin.enabled {
        return Err(Box::new(bad(
            StatusCode::FORBIDDEN,
            &format!("{} is turned off", plugin.name),
        )));
    }
    if !app.plugin_runtime.approved(&plugin) {
        return Err(Box::new(bad(
            StatusCode::CONFLICT,
            "its files in the project changed since you approved them; review it again under Plugins",
        )));
    }
    Ok(plugin)
}

// ---- calls: the chats, through the same gate as a program's ----

#[derive(Deserialize)]
struct CallBody {
    method: String,
    #[serde(default)]
    params: Value,
}

/// One of the bridge's calls into the chats. The power it needs is read
/// from the install record, and a project's plugin reaches only that
/// project's chats.
async fn call(
    State(app): State<Arc<App>>,
    AxumPath(id): AxumPath<String>,
    Json(body): Json<CallBody>,
) -> Response {
    let plugin = match js_plugin(&app, &id) {
        Ok(plugin) => plugin,
        Err(refused) => return *refused,
    };
    if !BRIDGE_METHODS.contains(&body.method.as_str()) {
        return bad(
            StatusCode::NOT_FOUND,
            &format!("OpenRemote offers a plugin no call named {}", body.method),
        );
    }
    let params = if body.params.is_null() {
        json!({})
    } else {
        body.params
    };
    match app
        .plugin_runtime
        .answer(&plugin.id(), &body.method, params)
        .await
    {
        Ok(result) => Json(json!({"result": result})).into_response(),
        Err(e) => {
            let status = match e.code {
                -32003 => StatusCode::FORBIDDEN,
                -32601 => StatusCode::NOT_FOUND,
                -32602 => StatusCode::BAD_REQUEST,
                _ => StatusCode::CONFLICT,
            };
            bad(status, &e.message)
        }
    }
}

// ---- fetch: the network, for a plugin given it ----

/// A plugin's request, as its frame made it: the console carries it here
/// unchanged, the body as base64.
#[derive(Deserialize)]
struct FetchBody {
    url: String,
    #[serde(default)]
    method: Option<String>,
    #[serde(default)]
    headers: Vec<(String, String)>,
    #[serde(default)]
    body: Option<String>,
    #[serde(default)]
    timeout_ms: Option<u64>,
}

/// Whether `url` is at or under `base`: the same scheme, host and port, and
/// a path that continues the base's at a segment boundary - so
/// `https://api.example.com/v1` admits `/v1/audio` but neither `/v1x` nor
/// another host that merely starts the same way.
pub fn goes_to(url: &str, base: &str) -> bool {
    let (Ok(url), Ok(base)) = (url.parse::<Uri>(), base.trim().parse::<Uri>()) else {
        return false;
    };
    let scheme = |u: &Uri| u.scheme_str().map(str::to_ascii_lowercase);
    let host = |u: &Uri| u.host().map(str::to_ascii_lowercase);
    let port = |u: &Uri| {
        u.port_u16().or(match scheme(u).as_deref() {
            Some("https") => Some(443),
            Some("http") => Some(80),
            _ => None,
        })
    };
    if scheme(&url).is_none()
        || scheme(&url) != scheme(&base)
        || host(&url).is_none()
        || host(&url) != host(&base)
        || port(&url) != port(&base)
    {
        return false;
    }
    let prefix = base.path().trim_end_matches('/');
    let path = url.path();
    prefix.is_empty() || path == prefix || path.starts_with(&format!("{prefix}/"))
}

/// Write the plugin's secrets into a header value where it names them.
/// Answers None when a named secret is not set: the header is then left
/// out, so a service that needs no key gets none. `declared` is the
/// install record's settings, never the folder's.
pub fn fill_secrets(
    value: &str,
    declared: &[SettingField],
    stored: &Map<String, Value>,
    url: &str,
) -> Result<Option<String>, (StatusCode, String)> {
    let mut out = String::new();
    let mut rest = value;
    while let Some(start) = rest.find(SECRET_OPEN) {
        out.push_str(&rest[..start]);
        let after = &rest[start + SECRET_OPEN.len()..];
        let Some(end) = after.find(SECRET_CLOSE) else {
            return Err((
                StatusCode::BAD_REQUEST,
                "a secret named in a header is not closed".into(),
            ));
        };
        let name = after[..end].trim();
        let Some(setting) = declared
            .iter()
            .find(|s| s.key == name && s.kind == SettingKind::Secret)
        else {
            return Err((
                StatusCode::BAD_REQUEST,
                format!("{name} is not one of the plugin's secrets"),
            ));
        };
        let Some(secret) = stored
            .get(name)
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
        else {
            return Ok(None);
        };
        if let Some(guard) = &setting.send_to {
            // Only an address the user saved: the plugin's own default is
            // never the user's word (Settings > Plugins offers "Use
            // default", which saves it by a click).
            let address = stored
                .get(guard)
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|a| !a.is_empty())
                .map(str::to_string);
            let Some(address) = address else {
                if !setting.required {
                    return Ok(None);
                }
                let label = declared
                    .iter()
                    .find(|s| &s.key == guard)
                    .map(|s| s.label.clone())
                    .unwrap_or_else(|| guard.clone());
                return Err((
                    StatusCode::FORBIDDEN,
                    format!(
                        "Save an address in {label} first (Settings > Plugins): {} is sent only to an address you saved",
                        setting.label
                    ),
                ));
            };
            if !goes_to(url, &address) {
                return Err((
                    StatusCode::FORBIDDEN,
                    format!("the {name} setting is sent only to the address in {guard}"),
                ));
            }
        }
        out.push_str(secret);
        rest = &after[end + SECRET_CLOSE.len()..];
    }
    out.push_str(rest);
    Ok(Some(out))
}

/// Whether `url` is at or under an address the user saved in one of the
/// plugin's text settings. A default is the plugin's own word and never
/// counts; nor does a secret.
pub fn typed_by_user(url: &str, declared: &[SettingField], stored: &Map<String, Value>) -> bool {
    declared
        .iter()
        .filter(|s| s.kind == SettingKind::String)
        .filter_map(|s| stored.get(&s.key).and_then(Value::as_str))
        .filter(|typed| {
            let lower = typed.trim().to_ascii_lowercase();
            lower.starts_with("http://") || lower.starts_with("https://")
        })
        .any(|typed| goes_to(url, typed))
}

/// The address a url's host is at, looked up once (the request connects
/// to it, never to a second lookup's).
async fn any_address(url: &str) -> Result<std::net::SocketAddr, String> {
    let uri: Uri = url
        .parse()
        .map_err(|_| format!("'{url}' is not an address"))?;
    let host = uri
        .host()
        .ok_or("the address names no host")?
        .trim_start_matches('[')
        .trim_end_matches(']')
        .to_string();
    let port = uri
        .port_u16()
        .unwrap_or(if uri.scheme_str() == Some("https") {
            443
        } else {
            80
        });
    if let Ok(ip) = host.parse::<std::net::IpAddr>() {
        return Ok(std::net::SocketAddr::new(ip, port));
    }
    tokio::net::lookup_host((host.as_str(), port))
        .await
        .map_err(|_| format!("{host} could not be found"))?
        .next()
        .ok_or_else(|| format!("{host} could not be found"))
}

/// Make a plugin's request, if it was given the network. Like a WASM
/// plugin's, it reaches the internet only - never this machine or its
/// network's private addresses, unless the user typed that address into
/// one of its settings - connecting to the very address that was checked,
/// and nothing is redirected for it. The answer comes back whole:
/// status, headers and body (base64).
async fn fetch(
    State(app): State<Arc<App>>,
    AxumPath(id): AxumPath<String>,
    Json(body): Json<FetchBody>,
) -> Response {
    let plugin = match js_plugin(&app, &id) {
        Ok(plugin) => plugin,
        Err(refused) => return *refused,
    };
    if !plugin.powers.contains(&PluginPower::Network) {
        return bad(
            StatusCode::FORBIDDEN,
            &format!(
                "{} may not reach the internet: that power was not approved when it was installed",
                plugin.name
            ),
        );
    }
    let url = body.url.trim().to_string();
    let lower = url.to_ascii_lowercase();
    if !(lower.starts_with("https://") || lower.starts_with("http://")) {
        return bad(
            StatusCode::BAD_REQUEST,
            "a plugin reaches http and https addresses only",
        );
    }
    let method = body
        .method
        .as_deref()
        .unwrap_or("GET")
        .trim()
        .to_ascii_uppercase();
    if !METHODS.contains(&method.as_str()) {
        return bad(
            StatusCode::BAD_REQUEST,
            &format!("{method} is not a request a plugin may make"),
        );
    }
    let bytes = match body.body.as_deref().filter(|b| !b.is_empty()) {
        None => None,
        Some(encoded) => match base64::engine::general_purpose::STANDARD.decode(encoded) {
            Ok(bytes) => Some(bytes),
            Err(_) => return bad(StatusCode::BAD_REQUEST, "the request body is not base64"),
        },
    };
    let stored = app.plugin_runtime.settings.load(&plugin.id());
    let mut headers: Vec<(String, String)> = Vec::new();
    let mut content_type = None;
    for (name, value) in &body.headers {
        let name = name.trim().to_ascii_lowercase();
        if FORBIDDEN_HEADERS.contains(&name.as_str())
            || name.starts_with("proxy-")
            || name.starts_with("sec-")
        {
            continue;
        }
        let value = match fill_secrets(value, &plugin.contributes.settings, &stored, &url) {
            Ok(Some(value)) => value,
            Ok(None) => continue,
            Err((status, why)) => return bad(status, &why),
        };
        if name == "content-type" {
            content_type = Some(value);
        } else {
            headers.push((name, value));
        }
    }
    let limit = body
        .timeout_ms
        .map(Duration::from_millis)
        .unwrap_or(DEFAULT_FETCH_TIME)
        .min(MAX_FETCH_TIME);
    let answer = tokio::time::timeout(limit, async {
        let addr = match super::wasm::public_address(&url).await {
            Ok(addr) => addr,
            // This machine or its network only where the user typed that
            // address into one of the plugin's settings (a transcription
            // server on this computer, say): the user's word, never the
            // plugin's.
            Err(_) if typed_by_user(&url, &plugin.contributes.settings, &stored) => {
                any_address(&url)
                    .await
                    .map_err(|why| (StatusCode::BAD_GATEWAY, why))?
            }
            Err(why) => return Err((StatusCode::FORBIDDEN, why)),
        };
        let pairs: Vec<(&str, &str)> = headers
            .iter()
            .map(|(n, v)| (n.as_str(), v.as_str()))
            .collect();
        let payload = bytes.map(|b| {
            (
                content_type
                    .as_deref()
                    .unwrap_or("application/octet-stream"),
                b,
            )
        });
        openremote_cloud::https::Http::new()
            .request_to(limit, addr, &method, &url, &pairs, payload)
            .await
            .map_err(|e| (StatusCode::BAD_GATEWAY, e.to_string()))
    })
    .await
    .unwrap_or_else(|_| {
        Err((
            StatusCode::GATEWAY_TIMEOUT,
            format!("{url} did not answer in time"),
        ))
    });
    match answer {
        Ok(response) => {
            let headers: Vec<(String, String)> = response
                .headers
                .iter()
                .filter_map(|(n, v)| {
                    v.to_str()
                        .ok()
                        .map(|v| (n.as_str().to_string(), v.to_string()))
                })
                .collect();
            Json(json!({
                "status": response.status,
                "headers": headers,
                "body": base64::engine::general_purpose::STANDARD.encode(&response.body),
            }))
            .into_response()
        }
        Err((status, why)) => bad(status, &why),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn field(key: &str, kind: SettingKind) -> SettingField {
        SettingField {
            key: key.into(),
            kind,
            label: key.into(),
            help: None,
            default: None,
            options: Vec::new(),
            min: None,
            max: None,
            required: false,
            send_to: None,
        }
    }

    #[test]
    fn an_address_is_under_its_base_only_at_a_segment_on_the_same_host() {
        let base = "https://api.example.com/v1";
        assert!(goes_to("https://api.example.com/v1", base));
        assert!(goes_to("https://api.example.com/v1/audio", base));
        assert!(goes_to("https://API.example.com:443/v1/audio", base));
        assert!(!goes_to("https://api.example.com/v1x", base));
        assert!(!goes_to("https://api.example.com/v2", base));
        assert!(!goes_to("http://api.example.com/v1", base));
        assert!(!goes_to("https://api.example.com:8443/v1", base));
        assert!(!goes_to("https://api.example.com.evil.net/v1", base));
        assert!(!goes_to("https://evil.net/v1?api.example.com", base));
        assert!(goes_to(
            "https://api.example.com/anything",
            "https://api.example.com"
        ));
        assert!(!goes_to("not an address", base));
    }

    #[test]
    fn this_machine_is_reached_only_at_an_address_the_user_typed() {
        let mut address = field("base_url", SettingKind::String);
        address.default = Some(json!("http://127.0.0.1:9000/v1"));
        let declared = vec![address, field("token", SettingKind::Secret)];
        let mut stored = Map::new();
        // The plugin's own default is not the user's word.
        assert!(!typed_by_user(
            "http://127.0.0.1:9000/v1/x",
            &declared,
            &stored
        ));
        // A secret's value is not an address the user chose either.
        stored.insert("token".into(), json!("http://127.0.0.1:9000"));
        assert!(!typed_by_user(
            "http://127.0.0.1:9000/v1/x",
            &declared,
            &stored
        ));
        stored.insert("base_url".into(), json!("http://127.0.0.1:9000/v1"));
        assert!(typed_by_user(
            "http://127.0.0.1:9000/v1/audio",
            &declared,
            &stored
        ));
        assert!(!typed_by_user(
            "http://127.0.0.1:9001/v1/audio",
            &declared,
            &stored
        ));
        assert!(!typed_by_user(
            "http://127.0.0.1:9000/admin",
            &declared,
            &stored
        ));
    }

    #[test]
    fn a_secret_goes_only_into_its_header_and_only_toward_its_address() {
        let mut key = field("api_key", SettingKind::Secret);
        key.send_to = Some("address".into());
        let mut address = field("address", SettingKind::String);
        address.default = Some(json!("https://api.example.com/v1"));
        let loose = field("loose", SettingKind::Secret);
        let declared = vec![key, address, loose, field("plain", SettingKind::String)];
        let mut stored = Map::new();
        stored.insert("api_key".into(), json!("sk-1"));
        stored.insert("loose".into(), json!("lk-2"));
        let to = "https://api.example.com/v1/audio";

        // No address saved: the plugin's default is not the user's word,
        // so the key is not written in and the header stays out.
        assert_eq!(
            fill_secrets("Bearer {{secret:api_key}}", &declared, &stored, to).unwrap(),
            None
        );
        // A required key with no saved address is refused, saying what to
        // save.
        let mut required = declared.clone();
        required[0].required = true;
        let (status, why) =
            fill_secrets("Bearer {{secret:api_key}}", &required, &stored, to).unwrap_err();
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert!(why.contains("Save"), "{why}");
        assert!(!why.contains("sk-1"), "{why}");
        // The default saved by the user's own click ("Use default") counts.
        stored.insert("address".into(), json!("https://api.example.com/v1"));
        assert_eq!(
            fill_secrets("Bearer {{secret:api_key}}", &declared, &stored, to).unwrap(),
            Some("Bearer sk-1".into())
        );
        // Anywhere else is refused, the secret never written.
        let (status, why) = fill_secrets(
            "Bearer {{secret:api_key}}",
            &declared,
            &stored,
            "https://evil.net/v1",
        )
        .unwrap_err();
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert!(!why.contains("sk-1"), "{why}");
        // The user's own address wins over the default.
        stored.insert("address".into(), json!("https://local.example.org"));
        assert!(fill_secrets("{{secret:api_key}}", &declared, &stored, to).is_err());
        assert!(
            fill_secrets(
                "{{secret:api_key}}",
                &declared,
                &stored,
                "https://local.example.org/x"
            )
            .is_ok()
        );
        // A secret with no sendTo goes where the plugin sends it.
        assert_eq!(
            fill_secrets(
                "{{secret:loose}}",
                &declared,
                &stored,
                "https://any.example.net"
            )
            .unwrap(),
            Some("lk-2".into())
        );
        // An unset secret leaves the header out.
        stored.remove("loose");
        assert_eq!(
            fill_secrets("{{secret:loose}}", &declared, &stored, to).unwrap(),
            None
        );
        // Only a declared secret: a plain setting or an unknown name is not.
        for named in ["{{secret:plain}}", "{{secret:nope}}", "{{secret:api_key"] {
            assert_eq!(
                fill_secrets(named, &declared, &stored, to).unwrap_err().0,
                StatusCode::BAD_REQUEST,
                "{named}"
            );
        }
        // A header with no secret passes as it is.
        assert_eq!(
            fill_secrets("text/plain", &declared, &stored, to).unwrap(),
            Some("text/plain".into())
        );
    }
}
