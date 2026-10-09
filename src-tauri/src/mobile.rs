//! The Android app. There are no sidecars on Android, so the daemon runs in
//! this process: the same API on loopback for the console, no harnesses (a
//! phone drives the user's machines and runs nothing itself), and data in
//! the app's own private directory. Cloud signs in through the app's own
//! scheme, which the OS hands back here.

use std::sync::{Arc, OnceLock};
use std::time::Duration;

use openremote_cloud::config::APP_REDIRECT;
use openremote_cloud::CloudConfig;
use openremote_daemon::app::App;
use tauri::{Manager, RunEvent};
use tauri_plugin_deep_link::DeepLinkExt;
use tauri_plugin_opener::OpenerExt;

use crate::{DaemonInfo, DaemonState};

/// The daemon, once it has started.
static DAEMON: OnceLock<Arc<App>> = OnceLock::new();

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_deep_link::init())
        .plugin(tauri_plugin_opener::init())
        .manage(DaemonState {
            info: std::sync::Mutex::new(DaemonInfo {
                mobile: true,
                ..DaemonInfo::default()
            }),
        })
        .invoke_handler(tauri::generate_handler![crate::daemon_info, open_url])
        .setup(|app| {
            start_daemon(app.handle().clone(), app.path().app_data_dir()?);
            app.deep_link().on_open_url(move |event| {
                for url in event.urls() {
                    sign_in_returned(url.to_string());
                }
            });
            // The return address can also be what started the app: the OS
            // may have stopped it while the browser was in front.
            if let Ok(Some(urls)) = app.deep_link().get_current() {
                for url in urls {
                    sign_in_returned(url.to_string());
                }
            }
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building OpenRemote")
        .run(|_app, event| {
            // Back in front: the relay link may have died while the OS had
            // the app frozen. Check it now rather than when the backoff
            // says, so the chats catch up as the screen appears.
            if let RunEvent::Resumed = event {
                if let Some(daemon) = DAEMON.get() {
                    daemon.cloud.reconnect_now();
                }
            }
        });
}

fn start_daemon(app: tauri::AppHandle, data_dir: std::path::PathBuf) {
    eprintln!("openremote: starting the in-app daemon");
    tauri::async_runtime::spawn(async move {
        match openremote_daemon::embedded::start(data_dir, cloud_config()).await {
            Ok(daemon) => {
                eprintln!("openremote: the in-app daemon is up at {}", daemon.url);
                let state = app.state::<DaemonState>();
                *state.info.lock().expect("daemon state lock") = DaemonInfo {
                    url: Some(daemon.url),
                    token: Some(daemon.token),
                    data_dir: Some(daemon.data_dir.display().to_string()),
                    mobile: true,
                };
                let _ = DAEMON.set(daemon.app);
            }
            Err(e) => eprintln!("openremote: the daemon didn't start: {e}"),
        }
    });
}

fn cloud_config() -> CloudConfig {
    let mut config = CloudConfig::from_env();
    // A build made to try the app against a local stand-in (fake-cloud)
    // carries its address; release builds never set these.
    if let Some(api) = option_env!("OPENREMOTE_CLOUD_API") {
        config.api_base = api.trim_end_matches('/').to_string();
    }
    if let Some(issuer) = option_env!("OPENREMOTE_AUTH_ISSUER") {
        config.auth_issuer = issuer.trim_end_matches('/').to_string();
    }
    config.platform = "android".to_string();
    config.redirect_uri = Some(APP_REDIRECT.to_string());
    config.device_name = device_name();
    config
}

/// The phone's own name for itself ("Google Pixel 8"): the hostname every
/// Android device reports is "localhost".
fn device_name() -> String {
    let props = android_system_properties::AndroidSystemProperties::new();
    let maker = props.get("ro.product.manufacturer").unwrap_or_default();
    let model = props.get("ro.product.model").unwrap_or_default();
    let (maker, model) = (maker.trim(), model.trim());
    let name = if model.to_lowercase().starts_with(&maker.to_lowercase()) {
        model.to_string()
    } else {
        let mut chars = maker.chars();
        let maker: String = chars
            .next()
            .map(|c| c.to_uppercase().chain(chars).collect())
            .unwrap_or_default();
        format!("{maker} {model}")
    };
    let name = name.trim();
    if name.is_empty() {
        "Android phone".to_string()
    } else {
        name.chars().take(64).collect()
    }
}

/// The browser came back to the app's scheme: hand the address to the
/// daemon, which finishes the sign-in as its loopback callback would. The
/// console's poll sees the result.
fn sign_in_returned(url: String) {
    if !url.starts_with(APP_REDIRECT) {
        return;
    }
    tauri::async_runtime::spawn(async move {
        // The address can arrive before the daemon is up (a cold start).
        for _ in 0..300 {
            if let Some(daemon) = DAEMON.get() {
                openremote_daemon::embedded::sign_in_returned(daemon, &url).await;
                return;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    });
}

/// Opens a web page outside the app: Cloud's sign-in page in an in-app
/// browser tab (it comes back through the app's scheme), anything else (a
/// link in a chat) in the phone's browser.
#[tauri::command]
fn open_url(app: tauri::AppHandle, url: String, sign_in: Option<bool>) -> Result<(), String> {
    if !(url.starts_with("https://") || url.starts_with("http://")) {
        return Err("only web pages open from here".into());
    }
    let with = sign_in.unwrap_or(false).then_some("inAppBrowser");
    app.opener().open_url(url, with).map_err(|e| e.to_string())
}
