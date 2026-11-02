//! Whether OpenRemote keeps its window in front of other windows.
//!
//! The topmost flag belongs to the window, and Windows keeps it until
//! something clears it: a window can arrive with the flag already set by
//! whatever set it last, and it stays set for the window's whole life. So the
//! setting only means anything if the app writes the flag itself - once at
//! launch, and again whenever the row in Settings changes. Off unless the user
//! turns it on; stored beside the tray preference (`tray.json`) and the
//! window's own geometry (`window.json`).

use std::path::PathBuf;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

/// What the user chose. A file from before this setting, or no file at all,
/// is off: the flag is never applied without a choice that says so.
#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize)]
pub struct WindowPrefs {
    #[serde(default)]
    pub keep_above: bool,
}

#[derive(Default)]
pub struct WindowPrefsState {
    prefs: Mutex<WindowPrefs>,
}

/// What the console reads and writes: the setting as the shell holds it.
#[derive(Serialize)]
pub struct WindowPrefsView {
    keep_above: bool,
}

fn prefs_path(app: &AppHandle) -> Option<PathBuf> {
    app.path()
        .app_config_dir()
        .ok()
        .map(|dir| dir.join("window_prefs.json"))
}

/// Read the stored choice. Called from setup, before the window is shown, so
/// the first paint already has the z-order the person asked for.
pub fn load(app: &AppHandle) {
    let prefs = prefs_path(app)
        .and_then(|path| std::fs::read(path).ok())
        .and_then(|raw| serde_json::from_slice(&raw).ok())
        .unwrap_or_default();
    *app.state::<WindowPrefsState>()
        .prefs
        .lock()
        .expect("window prefs lock") = prefs;
}

fn save(app: &AppHandle, prefs: &WindowPrefs) {
    if let Some(path) = prefs_path(app) {
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(json) = serde_json::to_vec_pretty(prefs) {
            let _ = std::fs::write(path, json);
        }
    }
}

/// Put the window where the setting says. Run at launch and after every
/// change: a window that arrived with the flag set from outside is corrected
/// here, which is what "off by default" has to mean in practice.
pub fn apply(app: &AppHandle) {
    let keep_above = app
        .state::<WindowPrefsState>()
        .prefs
        .lock()
        .expect("window prefs lock")
        .keep_above;
    // get_window, not get_webview_window: once the built-in browser adds a
    // tab, the window holds more than one webview (tray.rs has the same note).
    if let Some(window) = app.get_window("main") {
        let _ = window.set_always_on_top(keep_above);
    }
}

fn view(app: &AppHandle) -> WindowPrefsView {
    WindowPrefsView {
        keep_above: app
            .state::<WindowPrefsState>()
            .prefs
            .lock()
            .expect("window prefs lock")
            .keep_above,
    }
}

#[tauri::command]
pub fn window_prefs(app: AppHandle) -> WindowPrefsView {
    view(&app)
}

/// The console's own toggle. Answered with the setting the shell now holds,
/// so a row that did not take can say so instead of looking changed.
#[tauri::command]
pub fn set_window_prefs(app: AppHandle, keep_above: bool) -> WindowPrefsView {
    {
        let state = app.state::<WindowPrefsState>();
        let mut prefs = state.prefs.lock().expect("window prefs lock");
        prefs.keep_above = keep_above;
        save(&app, &prefs);
    }
    apply(&app);
    view(&app)
}

#[cfg(test)]
mod tests {
    use super::WindowPrefs;

    #[test]
    fn nothing_stored_is_off() {
        // The whole point of the setting: a window is not pinned because the
        // app was installed, and a file with no key is the same as no file.
        let empty: WindowPrefs = serde_json::from_slice(b"{}").expect("an empty record reads");
        assert!(!empty.keep_above);
        assert!(!WindowPrefs::default().keep_above);
    }

    #[test]
    fn a_stored_choice_round_trips() {
        let on = WindowPrefs { keep_above: true };
        let raw = serde_json::to_vec(&on).expect("the setting writes");
        let back: WindowPrefs = serde_json::from_slice(&raw).expect("and reads back");
        assert!(back.keep_above);
    }
}
