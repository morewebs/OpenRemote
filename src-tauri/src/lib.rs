//! OpenRemote's shell: wraps the console (`src/`) and gives it a daemon.
//! On the desktop the daemon is a sidecar process (`desktop.rs`). Android
//! has no sidecars, so the app runs the daemon in its own process
//! (`mobile.rs`).

use std::sync::Mutex;

use tauri::State;

#[cfg(desktop)]
mod desktop;
#[cfg(target_os = "android")]
mod mobile;
#[cfg(desktop)]
mod tray;

/// What the shell knows about the running daemon. The console asks for
/// this once on boot.
#[derive(Clone, serde::Serialize, Default)]
pub struct DaemonInfo {
    /// `http://127.0.0.1:<port>` - ready when Some.
    pub url: Option<String>,
    /// The daemon's bearer token.
    pub token: Option<String>,
    /// The daemon's data dir.
    pub data_dir: Option<String>,
    /// The Android app: no window chrome, no tray, and Cloud only, since a
    /// phone runs no chats of its own.
    pub mobile: bool,
}

#[derive(Default)]
struct DaemonState {
    info: Mutex<DaemonInfo>,
}

#[tauri::command]
fn daemon_info(state: State<'_, DaemonState>) -> DaemonInfo {
    state.info.lock().expect("daemon state lock").clone()
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    #[cfg(desktop)]
    desktop::run();
    #[cfg(target_os = "android")]
    mobile::run();
}
