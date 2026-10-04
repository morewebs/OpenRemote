//! OpenRemote desktop shell.
//!
//! Wraps the console (`src/`) and runs the daemon as a sidecar. The shell
//! owns one daemon for the app's lifetime; the daemon's stdin watchdog
//! (it exits when stdin closes) kills it transitively on app exit.

use std::sync::Mutex;

use tauri::{Manager, State};
use tauri_plugin_shell::process::CommandEvent;
use tauri_plugin_shell::ShellExt;

const DAEMON_NAME: &str = "openremote-daemon";

/// What the shell knows about the running daemon, learned from its READY
/// line. The console asks for this once on boot.
#[derive(Clone, serde::Serialize, Default)]
pub struct DaemonInfo {
    /// `http://127.0.0.1:<port>` - ready when Some.
    pub url: Option<String>,
    /// The auth token read from the daemon's data dir on first run.
    pub token: Option<String>,
    /// The data dir the daemon reports on its DATA_DIR line.
    pub data_dir: Option<String>,
}

#[derive(Default)]
struct DaemonState(Mutex<DaemonInfo>);

#[tauri::command]
fn daemon_info(state: State<'_, DaemonState>) -> DaemonInfo {
    state.0.lock().expect("daemon state lock").clone()
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_shell::init())
        .manage(DaemonState::default())
        .invoke_handler(tauri::generate_handler![daemon_info])
        .setup(|app| {
            spawn_daemon(app.handle().clone());
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running OpenRemote");
}

/// Spawn the daemon sidecar and watch its stdout lines (READY / DATA_DIR).
/// The daemon prints both at startup; the info publishes once both are
/// seen (or when the stream ends, for a daemon that never said DATA_DIR).
fn spawn_daemon(app: tauri::AppHandle) {
    tauri::async_runtime::spawn(async move {
        let sidecar = app
            .shell()
            .sidecar(DAEMON_NAME)
            .expect("daemon sidecar registered in tauri.conf.json");
        let (mut rx, _child) = sidecar.spawn().expect("daemon sidecar spawns");

        let mut info = DaemonInfo::default();
        let mut published = false;
        while let Some(event) = rx.recv().await {
            if let CommandEvent::Stdout(line) = event {
                let line = String::from_utf8_lossy(&line).trim().to_string();
                if let Some(addr) = line.strip_prefix("READY ") {
                    info.url = Some(format!("http://{addr}"));
                } else if let Some(dir) = line.strip_prefix("DATA_DIR ") {
                    info.data_dir = Some(dir.to_string());
                }
                if info.url.is_some() && info.data_dir.is_some() && !published {
                    publish(&app, &mut info);
                    published = true;
                }
            }
        }
        if !published && info.url.is_some() {
            publish(&app, &mut info);
        }
    });
}

/// Fill in the token (from the daemon's data dir) and hand the info to the
/// console.
fn publish(app: &tauri::AppHandle, info: &mut DaemonInfo) {
    info.token = info
        .data_dir
        .as_deref()
        .and_then(read_daemon_token_at)
        .or_else(read_daemon_token_default);
    let state = app.state::<DaemonState>();
    *state.0.lock().expect("daemon state lock") = info.clone();
}

/// The daemon's default data dir is `~/.openremote`; its `token` file holds
/// the bearer secret (the daemon logs it on first run as well).
fn read_daemon_token_default() -> Option<String> {
    let home = std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(std::path::PathBuf::from)?;
    read_daemon_token_at(home.join(".openremote").to_str()?)
}

fn read_daemon_token_at(dir: &str) -> Option<String> {
    let token = std::fs::read_to_string(std::path::Path::new(dir).join("token")).ok()?;
    let token = token.trim().to_string();
    if token.is_empty() { None } else { Some(token) }
}
