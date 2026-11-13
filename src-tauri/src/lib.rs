//! OpenRemote's shell: wraps the console (`src/`) and gives it a daemon.
//! On the desktop the daemon is a sidecar process (`desktop.rs`). Android
//! has no sidecars, so the app runs the daemon in its own process
//! (`mobile.rs`).

use std::sync::Mutex;

use tauri::State;

#[cfg(desktop)]
mod browser;
#[cfg(desktop)]
mod desktop;
// Where the window opens and what it remembers. Desktop only: a phone has no
// free-floating window.
#[cfg(desktop)]
mod geometry;
#[cfg(target_os = "android")]
mod mobile;
// The phone's built-in browser: the Kotlin plugin's pages, under the
// desktop's browser_* commands and browser:* events.
#[cfg(target_os = "android")]
mod mobile_browser;
// Its rules that need no phone, so the desktop's test build checks them too.
#[cfg(any(target_os = "android", test))]
mod phone_browser;
// The rule for the sign-in's in-app browser. Only the phone app opens pages,
// so on the desktop it exists for the test build alone.
#[cfg(any(target_os = "android", test))]
mod signin;
#[cfg(desktop)]
mod tray;
// Updates: a newer signed release, checked every two hours by the console and
// installed on the user's click. Desktop only; a phone updates from its store.
#[cfg(desktop)]
mod updates;
// Whether the window stays in front of other apps. Desktop only, like the
// window itself.
#[cfg(desktop)]
mod window_prefs;

/// What the shell knows about the running daemon. The console asks for
/// this on boot and while it has no connection, and the shell pushes it
/// whenever the answer changes.
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
    /// Why the last attempt to run the daemon failed, when one did. The
    /// console shows this instead of waiting in silence; it is cleared as
    /// soon as a daemon says READY, and never set while one runs.
    pub error: Option<DaemonFailure>,
}

/// Which failure a reason describes. The reason text is the shell's own
/// words, and the title above it has to match them: "did not start" is a
/// lie about a daemon that ran for an hour and then exited.
#[derive(Clone, Copy, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum DaemonFailureKind {
    /// The binary never ran: missing, quarantined, locked, truncated.
    Start,
    /// It ran, and then exited on its own.
    Stopped,
    /// It is running and has not said READY.
    Silent,
}

/// A daemon that is not running: which failure it was, and the shell's own
/// words for it. The console shows the words under the title the kind picks.
#[derive(Clone, serde::Serialize)]
pub struct DaemonFailure {
    pub kind: DaemonFailureKind,
    /// The reason, in the shell's words.
    pub text: String,
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
