//! Keeping OpenRemote in the tray when its window closes. A computer that is
//! a machine serves the user's other devices only while OpenRemote runs, and
//! synced chats running here stop with it - so closing the window can leave
//! it running in the tray instead. A setting, on by default when this
//! computer is a machine; Quit is always one click away in the tray menu.

use std::path::PathBuf;
use std::sync::Mutex;

use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager};

#[derive(Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct TrayPrefs {
    /// The user's choice; None follows `machine`.
    pub keep_running: Option<bool>,
    /// This computer is a machine (the console reports it).
    #[serde(default)]
    pub machine: bool,
}

impl TrayPrefs {
    pub fn effective(&self) -> bool {
        self.keep_running.unwrap_or(self.machine)
    }
}

#[derive(serde::Serialize)]
pub struct TrayView {
    keep_running: bool,
    machine: bool,
    /// False where no tray could be shown (no appindicator on this Linux).
    available: bool,
}

#[derive(Default)]
pub struct TrayState {
    prefs: Mutex<TrayPrefs>,
    icon: Mutex<Option<TrayIcon>>,
    unavailable: Mutex<bool>,
}

fn prefs_path(app: &AppHandle) -> Option<PathBuf> {
    app.path().app_config_dir().ok().map(|d| d.join("tray.json"))
}

pub fn load(app: &AppHandle) {
    let prefs = prefs_path(app)
        .and_then(|p| std::fs::read(p).ok())
        .and_then(|raw| serde_json::from_slice(&raw).ok())
        .unwrap_or_default();
    *app.state::<TrayState>().prefs.lock().expect("tray lock") = prefs;
}

fn save(app: &AppHandle, prefs: &TrayPrefs) {
    if let Some(path) = prefs_path(app) {
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(json) = serde_json::to_vec_pretty(prefs) {
            let _ = std::fs::write(path, json);
        }
    }
}

/// Whether closing the window should hide it instead of quitting.
pub fn keeps_running(app: &AppHandle) -> bool {
    let state = app.state::<TrayState>();
    let effective = state.prefs.lock().expect("tray lock").effective();
    effective && state.icon.lock().expect("tray lock").is_some()
}

pub fn show_main(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

fn build(app: &AppHandle) -> tauri::Result<TrayIcon> {
    let open = MenuItem::with_id(app, "open", "Open OpenRemote", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit OpenRemote", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &quit])?;
    let mut builder = TrayIconBuilder::with_id("main")
        .tooltip("OpenRemote")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "open" => show_main(app),
            "quit" => app.exit(0),
            _ => {}
        })
        // Windows: a left click brings the window back. (Linux tray icons
        // send no clicks; the menu is the way there.)
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_main(tray.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon().cloned() {
        builder = builder.icon(icon);
    }
    builder.build(app)
}

/// Shows or hides the tray icon to match the setting. Built on first need:
/// on Linux the tray loads appindicator at runtime and panics without it,
/// so the build is caught and the setting quietly becomes unavailable.
pub fn apply(app: &AppHandle) {
    let state = app.state::<TrayState>();
    let effective = state.prefs.lock().expect("tray lock").effective();
    let mut icon = state.icon.lock().expect("tray lock");
    match (effective, icon.as_ref()) {
        (true, None) => {
            if *state.unavailable.lock().expect("tray lock") {
                return;
            }
            let handle = app.clone();
            match std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || build(&handle))) {
                Ok(Ok(built)) => *icon = Some(built),
                _ => *state.unavailable.lock().expect("tray lock") = true,
            }
        }
        (true, Some(existing)) => {
            let _ = existing.set_visible(true);
        }
        (false, Some(existing)) => {
            let _ = existing.set_visible(false);
        }
        (false, None) => {}
    }
}

fn view(app: &AppHandle) -> TrayView {
    let state = app.state::<TrayState>();
    let prefs = state.prefs.lock().expect("tray lock").clone();
    let unavailable = *state.unavailable.lock().expect("tray lock");
    TrayView {
        keep_running: prefs.effective(),
        machine: prefs.machine,
        available: !unavailable,
    }
}

#[tauri::command]
pub fn tray_prefs(app: AppHandle) -> TrayView {
    view(&app)
}

/// The console reports `machine` as it changes, and the user's own choice
/// as `keep_running`.
#[tauri::command]
pub fn set_tray_prefs(app: AppHandle, keep_running: Option<bool>, machine: Option<bool>) -> TrayView {
    {
        let state = app.state::<TrayState>();
        let mut prefs = state.prefs.lock().expect("tray lock");
        if let Some(keep) = keep_running {
            prefs.keep_running = Some(keep);
        }
        if let Some(machine) = machine {
            prefs.machine = machine;
        }
        save(&app, &prefs);
    }
    apply(&app);
    view(&app)
}
