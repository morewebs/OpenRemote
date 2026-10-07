//! The desktop shell runs the daemon as a sidecar. The shell owns one
//! daemon for the app's lifetime; the daemon's stdin watchdog (it exits
//! when stdin closes) kills it transitively on app exit.

use std::sync::Mutex;

use tauri::{Manager, State, WindowEvent};
use tauri_plugin_shell::process::CommandEvent;
use tauri_plugin_shell::ShellExt;

use crate::{tray, DaemonInfo, DaemonState};

const DAEMON_NAME: &str = "openremote-daemon";

/// The live sidecar child. Held so a requested restart can stop the old
/// daemon before spawning the new one; the stdin watchdog (the daemon exits
/// when stdin closes) still owns the app-exit path.
#[derive(Default)]
struct Sidecar(Mutex<Option<tauri_plugin_shell::process::CommandChild>>);

/// Restart the daemon sidecar: kill the old child, drop what the shell
/// knew about it, spawn fresh. The console heals on its own - on
/// connection error it re-polls daemon_info every 2.5s and follows a
/// new port - so this returns before the new daemon is ready.
#[tauri::command]
fn daemon_restart(
    app: tauri::AppHandle,
    state: State<'_, DaemonState>,
    sidecar: State<'_, Sidecar>,
) {
    if let Some(child) = sidecar.0.lock().expect("daemon state lock").take() {
        let _ = child.kill();
    }
    *state.info.lock().expect("daemon state lock") = DaemonInfo::default();
    spawn_daemon(app);
}

pub fn run() {
    let builder = tauri::Builder::default();
    // First, so a second launch is answered before anything else starts.
    #[cfg(any(windows, target_os = "linux"))]
    let builder = builder.plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
        tray::show_main(app);
    }));
    builder
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_shell::init())
        .manage(DaemonState::default())
        .manage(Sidecar::default())
        .manage(tray::TrayState::default())
        .invoke_handler(tauri::generate_handler![
            crate::daemon_info,
            daemon_restart,
            tray::tray_prefs,
            tray::set_tray_prefs
        ])
        .setup(|app| {
            tray::load(app.handle());
            tray::apply(app.handle());
            spawn_daemon(app.handle().clone());
            Ok(())
        })
        // With the tray on, closing the window hides it: the daemon keeps
        // serving this computer's synced chats and the user's other devices.
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                if window.label() == "main" && tray::keeps_running(window.app_handle()) {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
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
        let sidecar = match system_env() {
            Some(vars) => sidecar.env_clear().envs(vars),
            None => sidecar,
        };
        let (mut rx, child) = sidecar.spawn().expect("daemon sidecar spawns");
        app.state::<Sidecar>()
            .0
            .lock()
            .expect("daemon state lock")
            .replace(child);

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

/// Under an AppImage the shell runs with the image's own library and data
/// paths. The daemon and every harness it starts (node, python, git...)
/// must see the system's instead, or they load the image's libraries and
/// break. None outside an AppImage: the environment passes through as is.
fn system_env() -> Option<Vec<(String, String)>> {
    let appdir = std::env::var("APPDIR").ok().filter(|d| !d.is_empty())?;
    let mut vars = Vec::new();
    for (key, value) in std::env::vars() {
        // The AppImage runtime's own variables, and toolkit paths set only
        // for the image's GTK/WebKit.
        if matches!(key.as_str(), "APPDIR" | "APPIMAGE" | "ARGV0" | "OWD")
            || key.starts_with("GTK_")
            || key.starts_with("GIO_")
            || key.starts_with("GDK_PIXBUF_")
            || key == "GI_TYPELIB_PATH"
            || key == "GSETTINGS_SCHEMA_DIR"
        {
            continue;
        }
        if !value.contains(&appdir) {
            vars.push((key, value));
            continue;
        }
        // Path lists keep their entries from outside the image.
        let kept: Vec<&str> = value
            .split(':')
            .filter(|entry| !entry.is_empty() && !entry.starts_with(&appdir))
            .collect();
        if !kept.is_empty() {
            vars.push((key, kept.join(":")));
        }
    }
    Some(vars)
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
    *state.info.lock().expect("daemon state lock") = info.clone();
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
