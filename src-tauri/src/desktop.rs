//! The desktop shell runs the daemon as a sidecar. The shell owns one
//! daemon for the app's lifetime and respawns it when it dies on its
//! own; the daemon's stdin watchdog (it exits when stdin closes) kills it
//! transitively on app exit.

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tauri::{Emitter, Manager, State, WindowEvent};
use tauri_plugin_shell::process::CommandEvent;
use tauri_plugin_shell::ShellExt;

use crate::{
    browser, geometry, tray, window_prefs, DaemonFailure, DaemonFailureKind, DaemonInfo,
    DaemonState,
};

const DAEMON_NAME: &str = "openremote-daemon";

/// How long the shell waits before respawning a daemon that died on its
/// own. A daemon that got as far as READY was healthy, so its death was
/// a blip and the wait resets to the minimum; each daemon that dies (or
/// fails to start) without ever saying READY doubles the wait, capped,
/// so a daemon that cannot start is retried slowly instead of in a tight
/// loop.
const RESPAWN_WAIT_MIN: Duration = Duration::from_secs(2);
const RESPAWN_WAIT_MAX: Duration = Duration::from_secs(30);

/// How long a daemon the shell started has to say READY before the shell
/// stops calling it slow. A process that is alive and silent is a
/// different failure from one that never ran, and the panel has no other
/// way to tell them apart. The number is the console's own patience for a
/// first answer (it asks for 90 seconds before it shows the waiting
/// screen): a verdict earlier than that calls a boot stuck while the app
/// is still willing to wait, and the only thing the panel can offer is a
/// restart, which would kill it.
const READY_DEADLINE: Duration = Duration::from_secs(90);

/// How long a restart waits for the old daemon to end itself after its stdin
/// is closed.
///
/// It must stay above the daemon's own drain bound, `STOP_DRAIN_BOUND` in
/// `openremote-daemon`'s supervisor (ten seconds): a shorter wait kills a
/// daemon that is still sweeping, which is the orphan this whole path exists
/// to prevent. Twelve is that bound plus two seconds of scheduling slack.
///
/// That bound is per harness and the sweep is sequential, so a machine with
/// several slow chats can legitimately take longer than one bound; the cap
/// only ever costs a wedged daemon a slower restart, and the unit test below
/// pins it against the daemon's constant for exactly that reason.
const GRACEFUL_STOP_WAIT: Duration = Duration::from_secs(12);

/// The most of the daemon's own words that is kept for a diagnosis: forty
/// lines, and no more than 8 KiB of them.
const SAID_LINES: usize = 40;
const SAID_BYTES: usize = 8 * 1024;

/// The sidecar child the shell owns: the running daemon's handle, or None
/// while none runs (dead, and not yet respawned). A watcher tells its own
/// daemon by the child's pid. Storing a replacement drops the child it
/// replaces, and with it the daemon's stdin pipe: the daemon exits when
/// stdin closes, so a replaced daemon stops itself, and the app-exit path
/// rides the same watchdog.
#[derive(Default)]
struct Sidecar {
    child: Mutex<Option<tauri_plugin_shell::process::CommandChild>>,
    /// Set by a watcher once its process is gone. `daemon_restart` closes
    /// the old daemon's stdin and waits on this, so a replacement is not
    /// started while the old daemon is still stopping the harnesses it ran.
    exited: Arc<AtomicBool>,
}

/// Lines for the daemon's stdin (the built-in browser's news and answers),
/// written in order by one thread so a busy pipe never holds up the window.
static TO_DAEMON: std::sync::OnceLock<std::sync::mpsc::Sender<String>> = std::sync::OnceLock::new();

/// Send one line to the running daemon; dropped when none runs (the next
/// daemon hears everything again in its hello).
pub fn write_to_daemon(app: &tauri::AppHandle, line: &str) {
    let tx = TO_DAEMON.get_or_init(|| {
        let (tx, rx) = std::sync::mpsc::channel::<String>();
        let app = app.clone();
        std::thread::spawn(move || {
            for line in rx {
                let sidecar = app.state::<Sidecar>();
                let mut slot = sidecar.child.lock().expect("daemon state lock");
                if let Some(child) = slot.as_mut() {
                    let _ = child.write(format!("{line}\n").as_bytes());
                }
            }
        });
        tx
    });
    let _ = tx.send(line.to_string());
}

/// Restart the daemon sidecar: close the old daemon's stdin so it stops
/// itself, wait a bounded moment, then spawn a fresh one.
///
/// Not `kill`: a killed process runs no destructors, so the shutdown sweep
/// in the daemon's stdin watchdog never fires and every harness CLI it
/// started keeps running with no daemon to talk to - still working, still
/// spending, still holding its folder. Closing stdin is the daemon's own
/// stop button, and the sweep is what it does with it. The console heals on
/// its own (on a connection error it re-polls daemon_info every 2.5s and
/// follows a new port), so this returns before the new daemon is ready.
#[tauri::command]
async fn daemon_restart(
    app: tauri::AppHandle,
    state: State<'_, DaemonState>,
    sidecar: State<'_, Sidecar>,
) -> Result<(), String> {
    let old = sidecar.child.lock().expect("daemon state lock").take();
    if let Some(child) = old {
        let pid = child.pid();
        let exited = Arc::clone(&sidecar.exited);
        // Dropping the handle is what closes the pipe: the plugin's child
        // can be written to and killed, and has no way to close stdin on its
        // own.
        drop(child);
        let gone = tauri::async_runtime::spawn_blocking(move || {
            let deadline = std::time::Instant::now() + GRACEFUL_STOP_WAIT;
            while std::time::Instant::now() < deadline {
                if exited.load(Ordering::SeqCst) {
                    return true;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            exited.load(Ordering::SeqCst)
        })
        .await
        .unwrap_or(false);
        if !gone {
            // The daemon never read its closed stdin: a stuck harness, a
            // store that will not release, a process under a debugger. Its
            // handle went with the drop, so the pid is all that is left.
            stop_by_pid(pid);
        }
    }
    *state.info.lock().expect("daemon state lock") = DaemonInfo::default();
    tell_console(&app);
    spawn_daemon(app);
    Ok(())
}

/// Stop a daemon by pid, for the case the graceful path could not.
///
/// The pid is the only handle left: dropping the child was what closed its
/// stdin, and the plugin's child cannot be both kept and closed. A pid can be
/// reused, so ownership is checked as close to the signal as the platform
/// allows:
///
/// - Windows: one call. `taskkill` takes the pid and the image name as filters
///   in the same invocation, so the image name *is* the ownership check inside
///   the kill: a reused pid is only hit if its new owner is also another
///   `openremote-daemon.exe`.
/// - Linux: `/proc/<pid>/comm` is read, and only a name that starts with
///   `openremote-daem` (the kernel truncates the name at fifteen characters)
///   is signalled. The check and the kill are two syscalls, so the window is
///   narrowed to the microseconds between them rather than closed.
/// - macOS: no `/proc`, so a plain pid kill goes out; the window there is
///   narrowed by nothing, which is the one place it is still open.
///
/// No console window is created; the shell is a GUI process and would only
/// flash one.
fn stop_by_pid(pid: u32) {
    let pid = pid.to_string();
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let by_pid = format!("PID eq {pid}");
        let _ = std::process::Command::new("taskkill")
            .args([
                "/F",
                "/FI",
                by_pid.as_str(),
                "/FI",
                "IMAGENAME eq openremote-daemon.exe",
            ])
            .creation_flags(CREATE_NO_WINDOW)
            .status();
    }
    #[cfg(target_os = "linux")]
    {
        let named = std::fs::read_to_string(format!("/proc/{pid}/comm"))
            .map(|comm| comm.trim_start().starts_with("openremote-daem"))
            .unwrap_or(false);
        if named {
            let _ = std::process::Command::new("kill")
                .args(["-9", &pid])
                .status();
        }
    }
    #[cfg(all(unix, not(target_os = "linux")))]
    {
        let _ = std::process::Command::new("kill")
            .args(["-9", &pid])
            .status();
    }
}

/// Open a folder in the system's file manager: the sidebar's "Open folder"
/// on a project. Only a folder that exists is opened - handed a file, the
/// file manager would run it instead of showing it.
#[tauri::command]
fn reveal_in_folder(path: String) -> Result<(), String> {
    let path = std::path::PathBuf::from(path);
    if !path.is_absolute() || !path.is_dir() {
        return Err("that folder is gone".into());
    }
    // Explorer wants the platform's own separators: a folder stored with
    // forward slashes is respelled with backslashes before it is handed over.
    let path: std::path::PathBuf = path.components().collect();
    #[cfg(windows)]
    let shown = std::process::Command::new("explorer").arg(&path).spawn();
    #[cfg(target_os = "macos")]
    let shown = std::process::Command::new("open").arg(&path).spawn();
    #[cfg(all(unix, not(target_os = "macos")))]
    let shown = std::process::Command::new("xdg-open").arg(&path).spawn();
    shown.map(|_| ()).map_err(|e| e.to_string())
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
        .manage(window_prefs::WindowPrefsState::default())
        .manage(browser::Browser::default())
        .invoke_handler(tauri::generate_handler![
            crate::daemon_info,
            daemon_restart,
            reveal_in_folder,
            tray::tray_prefs,
            tray::set_tray_prefs,
            window_prefs::window_prefs,
            window_prefs::set_window_prefs,
            browser::browser_open,
            browser::browser_features,
            browser::browser_tabs,
            browser::browser_close,
            browser::browser_set_bounds,
            browser::browser_show,
            browser::browser_hide,
            browser::browser_navigate,
            browser::browser_back,
            browser::browser_forward,
            browser::browser_reload,
            browser::browser_capture,
            browser::browser_cdp,
            browser::browser_stop,
            browser::browser_zoom,
            browser::browser_find,
            browser::browser_print,
            browser::browser_devtools,
            browser::browser_viewport,
            browser::browser_clear_site,
            browser::browser_open_external,
            browser::browser_reveal,
            // The browser's surface grew with the daemon-side work: a page's permission
            // prompt, the element picker, a tab's frames, saving a file into Downloads,
            // clearing the profile, and handing a private tab over to agents.
            browser::browser_permission,
            browser::browser_pick,
            browser::browser_screencast,
            browser::browser_save_file,
            browser::browser_clear_data,
            browser::browser_share
        ])
        .setup(|app| {
            // Before the first paint: fit the window to the screen it is on
            // and put it back where the user left it. The configured size is
            // larger than some laptop screens, and the part that would fall
            // off is the titlebar's own close button.
            geometry::open(app.handle());
            // The window's z-order is the shell's to hold, not something it
            // inherits from whatever set it last: the stored choice is written
            // on every launch, so a flag left behind is corrected here.
            window_prefs::load(app.handle());
            window_prefs::apply(app.handle());
            // The console's webview gets no engine context menu: every action
            // on the console is a control the app draws (browser/mod.rs has
            // the longer reason). Tab pages keep theirs on purpose.
            if let Some(console) = app.get_webview("main") {
                browser::no_engine_menu(&console);
            }
            // A dev build gives the console's webview a debugging port
            // (main.rs). The window exists by now, so the variable has done
            // its job; dropping it keeps the port off the browser's own
            // profile, which is driven in-process and never opens one.
            #[cfg(all(windows, debug_assertions))]
            std::env::remove_var("WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS");
            #[cfg(all(windows, debug_assertions))]
            if let Some(out) = std::env::var_os("OPENREMOTE_BROWSER_SELFTEST") {
                browser::selftest::start(app.handle().clone(), out.into());
            }
            tray::load(app.handle());
            tray::apply(app.handle());
            spawn_daemon(app.handle().clone());
            // A tab nobody has looked at for five minutes is suspended: it keeps its page
            // and its place, and stops costing memory and battery while hidden.
            browser::start_sweeper(app.handle().clone());
            Ok(())
        })
        // Where the window is, after the user moves or resizes it. The close
        // path writes regardless: a drag that ends in a quit is still a
        // choice worth keeping.
        .on_window_event(|window, event| match event {
            WindowEvent::CloseRequested { api, .. } if window.label() == "main" => {
                geometry::save_now(window);
                if tray::keeps_running(window.app_handle()) {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
            WindowEvent::Moved(_) | WindowEvent::Resized(_) if window.label() == "main" => {
                geometry::remember(window);
            }
            _ => {}
        })
        .run(tauri::generate_context!())
        .expect("error while running OpenRemote");
}

/// Spawn the daemon sidecar and keep it alive for the app's lifetime:
/// watch its stdout lines (READY / DATA_DIR) and publish the info once
/// both are seen. When the event stream ends while the child is still
/// this watcher's own (the daemon crashed, or something outside the app
/// killed it), clear the published info and spawn a replacement after a
/// bounded wait: the console's poll then sees a not-ready daemon instead
/// of retrying a dead address, and follows the replacement once it
/// publishes. A deliberate restart replaces the child itself, which the
/// watcher sees and stands down for.
fn spawn_daemon(app: tauri::AppHandle) {
    tauri::async_runtime::spawn(async move {
        // The delay before the next spawn after a death.
        let mut wait = RESPAWN_WAIT_MIN;
        loop {
            // The flag describes the child this pass is about to watch, not
            // the one before it.
            app.state::<Sidecar>().exited.store(false, Ordering::SeqCst);
            let sidecar = app
                .shell()
                .sidecar(DAEMON_NAME)
                .expect("daemon sidecar registered in tauri.conf.json");
            let sidecar = match system_env() {
                Some(vars) => sidecar.env_clear().envs(vars),
                None => sidecar,
            };
            let (mut rx, child) = match sidecar.spawn() {
                Ok(spawned) => spawned,
                Err(err) => {
                    // The binary could not be started at all:
                    // quarantined by a scanner, truncated by an
                    // interrupted copy, locked by another process. That
                    // can be transient while the app lives, so retry on
                    // the growing wait, and give the console the reason so
                    // it says something instead of waiting in silence.
                    eprintln!("openremote: the daemon could not be started: {err}");
                    report_failure(&app, DaemonFailureKind::Start, err.to_string());
                    respawn_delay(wait).await;
                    if daemon_slot_claimed(&app) {
                        return;
                    }
                    wait = (wait * 2).min(RESPAWN_WAIT_MAX);
                    continue;
                }
            };
            let pid = child.pid();
            app.state::<Sidecar>()
                .child
                .lock()
                .expect("daemon state lock")
                .replace(child);
            // A daemon that is alive and quiet is named at the deadline. A
            // daemon that dies first aborts the timer: its death is the
            // truer reason, and the death path below writes that one.
            let deadline = tauri::async_runtime::spawn({
                let app = app.clone();
                async move {
                    respawn_delay(READY_DEADLINE).await;
                    report_silent(&app);
                }
            });

            let mut info = DaemonInfo::default();
            let mut published = false;
            let mut exit = None;
            // What the daemon says on stderr: its own words for a panic, a
            // bind that failed, the reason it stopped. Kept bounded as it
            // arrives, because a daemon that dies in a loop can talk.
            let mut said: VecDeque<String> = VecDeque::new();
            while let Some(event) = rx.recv().await {
                match event {
                    CommandEvent::Terminated(payload) => exit = payload.code,
                    CommandEvent::Stdout(line) => {
                        let line = String::from_utf8_lossy(&line).trim().to_string();
                        // The daemon's calls to the built-in browser share this pipe, so a
                        // line meant for the browser is taken here and never parsed as a
                        // daemon announcement.
                        if browser::pipe::receive(&app, &line) {
                            continue;
                        }
                        if let Some(addr) = line.strip_prefix("READY ") {
                            info.url = Some(format!("http://{addr}"));
                        } else if let Some(dir) = line.strip_prefix("DATA_DIR ") {
                            info.data_dir = Some(dir.to_string());
                        }
                        if info.url.is_some() && info.data_dir.is_some() && !published {
                            publish(&app, &mut info);
                            published = true;
                            // A new daemon learns about the browser and its tabs.
                            browser::pipe::hello(&app);
                        }
                    }
                    CommandEvent::Stderr(bytes) => keep_lines(&mut said, &bytes),
                    _ => {}
                }
            }
            deadline.abort();
            // A deliberate restart waits on this: it closed this process's
            // stdin and has to know the daemon finished stopping the
            // harnesses it ran before a replacement starts.
            app.state::<Sidecar>().exited.store(true, Ordering::SeqCst);
            // The stream ends only when the process is gone. If the
            // child in the slot is still this one's, the daemon died on
            // its own: take the dead handle out so the slot says what is
            // true (no daemon running). Otherwise a deliberate restart
            // or another watcher already replaced it.
            let was_mine = {
                let sidecar = app.state::<Sidecar>();
                let mut slot = sidecar.child.lock().expect("daemon state lock");
                let mine = slot.as_ref().is_some_and(|child| child.pid() == pid);
                if mine {
                    let _ = slot.take();
                }
                mine
            };
            if !was_mine {
                return;
            }
            // Clear the dead daemon's info so daemon_info stops answering
            // with an address nothing listens on, and say why it went. The
            // replacement repopulates both when it says READY and DATA_DIR.
            *app.state::<DaemonState>()
                .info
                .lock()
                .expect("daemon state lock") = DaemonInfo::default();
            let said_log = write_stderr_log(&app, &said, exit);
            report_failure(
                &app,
                DaemonFailureKind::Stopped,
                match (exit, said_log) {
                    (Some(code), Some(path)) => format!(
                        "the daemon stopped (exit code {code}); what it said last is in {}",
                        path.display()
                    ),
                    (Some(code), None) => format!("the daemon stopped (exit code {code})"),
                    (None, Some(path)) => format!(
                        "the daemon stopped; what it said last is in {}",
                        path.display()
                    ),
                    (None, None) => "the daemon stopped".to_string(),
                },
            );
            eprintln!("openremote: the daemon exited, respawning");
            respawn_delay(wait).await;
            if daemon_slot_claimed(&app) {
                return;
            }
            let was_ready = info.url.is_some();
            wait = if was_ready {
                RESPAWN_WAIT_MIN
            } else {
                (wait * 2).min(RESPAWN_WAIT_MAX)
            };
        }
    });
}

/// Whether another spawn claimed the sidecar slot while a dead daemon's
/// watcher was waiting to respawn it. Only daemon_restart and a
/// respawning watcher store a child, so a claimed slot means one of them
/// ran; the waiting watcher stands down rather than double-spawn.
fn daemon_slot_claimed(app: &tauri::AppHandle) -> bool {
    app.state::<Sidecar>()
        .child
        .lock()
        .expect("daemon state lock")
        .is_some()
}

/// Hand what the shell knows about the daemon to the console. The console
/// also asks `daemon_info` on its own, and that is what makes this safe to
/// miss: a spawn that fails before the console has loaded its listener is
/// still there when the console asks. This is the same event bus the
/// browser reports its tabs on, not a second way of talking.
fn tell_console(app: &tauri::AppHandle) {
    let info = app
        .state::<DaemonState>()
        .info
        .lock()
        .expect("daemon state lock")
        .clone();
    let _ = app.emit_to(browser::console(), "daemon:status", info);
}

/// Record why the daemon is not running, and tell the console. A daemon
/// that is up is never described as failed: the address it published
/// stays, so a stale reason from another spawn cannot sit over a live one.
fn report_failure(app: &tauri::AppHandle, kind: DaemonFailureKind, text: String) {
    {
        let state = app.state::<DaemonState>();
        let mut info = state.info.lock().expect("daemon state lock");
        if info.url.is_none() {
            *info = DaemonInfo {
                error: Some(DaemonFailure { kind, text }),
                ..DaemonInfo::default()
            };
        }
    }
    tell_console(app);
}

/// The daemon is running and has not said READY. Its stdout says nothing
/// either way, so this is the one failure the shell has to notice on a
/// clock. Only fills an empty slot: a real reason from the same attempt is
/// fresher, and this one has to be harmless if it races a death.
fn report_silent(app: &tauri::AppHandle) {
    let wrote = {
        let state = app.state::<DaemonState>();
        let mut info = state.info.lock().expect("daemon state lock");
        if info.url.is_none() && info.error.is_none() {
            *info = DaemonInfo {
                error: Some(DaemonFailure {
                    kind: DaemonFailureKind::Silent,
                    // The number comes from the timer itself, so the
                    // sentence cannot promise a deadline the shell does not
                    // keep.
                    text: format!(
                        "it started and has not answered in {} seconds",
                        READY_DEADLINE.as_secs()
                    ),
                }),
                ..DaemonInfo::default()
            };
            true
        } else {
            false
        }
    };
    if wrote {
        tell_console(app);
    }
}

/// Sleep out a respawn wait on the runtime's blocking pool. The shell
/// carries no timer of its own, and sleeping inline would park an async
/// worker that other work still needs; the blocking pool exists for
/// exactly this kind of wait.
async fn respawn_delay(wait: Duration) {
    let _ = tauri::async_runtime::spawn_blocking(move || std::thread::sleep(wait)).await;
}

/// Keep the newest lines of the daemon's stderr, bounded both ways.
fn keep_lines(tail: &mut VecDeque<String>, bytes: &[u8]) {
    for line in String::from_utf8_lossy(bytes).lines() {
        let mut line = line.trim_end().to_string();
        if line.is_empty() {
            continue;
        }
        // One line can be as long as the daemon likes, and the bound is on
        // what is kept: an overlong line keeps its newest end.
        if line.len() > SAID_BYTES {
            let mut cut = line.len() - SAID_BYTES;
            while !line.is_char_boundary(cut) {
                cut += 1;
            }
            line = line[cut..].to_string();
        }
        tail.push_back(line);
    }
    while tail.len() > SAID_LINES {
        tail.pop_front();
    }
    while tail.iter().map(String::len).sum::<usize>() > SAID_BYTES && tail.len() > 1 {
        tail.pop_front();
    }
}

/// Write the daemon's last words where a person can find them, and answer
/// the path (None when it said nothing).
///
/// A packaged shell has no console to print them to, and the one place in
/// the app that could show them is the console's, so a file under the app's
/// own log directory is the whole mechanism: the failure the panel shows
/// names the path. Overwritten per death rather than appended, so the file
/// always describes the failure the panel is talking about.
fn write_stderr_log(
    app: &tauri::AppHandle,
    tail: &VecDeque<String>,
    exit: Option<i32>,
) -> Option<PathBuf> {
    if tail.is_empty() {
        return None;
    }
    let dir = app.path().app_log_dir().ok()?;
    std::fs::create_dir_all(&dir).ok()?;
    let path = dir.join("daemon-stderr.log");
    let mut body = String::new();
    for line in tail {
        body.push_str(line);
        body.push('\n');
    }
    if body.len() > SAID_BYTES {
        // Cut from the front, on a character boundary: the newest end is the
        // part worth keeping.
        let mut cut = body.len() - SAID_BYTES;
        while !body.is_char_boundary(cut) {
            cut += 1;
        }
        body = body[cut..].to_string();
    }
    let head = format!(
        "openremote: what the daemon said last before the shell saw it stop (exit code {}).\n\n",
        exit.map(|code| code.to_string())
            .unwrap_or_else(|| "unknown".to_string())
    );
    std::fs::write(&path, head + &body).ok()?;
    eprintln!(
        "openremote: the daemon's last words are in {}",
        path.display()
    );
    Some(path)
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
    // A daemon that answered is not a failure: whatever a failed attempt
    // left here stops being true the moment one says READY.
    info.error = None;
    let state = app.state::<DaemonState>();
    *state.info.lock().expect("daemon state lock") = info.clone();
    tell_console(app);
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
    if token.is_empty() {
        None
    } else {
        Some(token)
    }
}

#[cfg(test)]
mod tests {
    use super::GRACEFUL_STOP_WAIT;
    use std::time::Duration;

    /// The daemon's drain bound lives in another workspace and cannot be
    /// imported here, so it is read from its source. The two numbers only make
    /// sense together - the shell waits for the daemon to stop, the daemon
    /// bounds how long that stop takes - and moving either without the other
    /// turns this red instead of silently orphaning harnesses.
    fn daemon_drain_bound() -> Duration {
        let source = include_str!("../../daemon/crates/openremote-daemon/src/supervisor.rs");
        let line = source
            .lines()
            .find(|line| line.contains("STOP_DRAIN_BOUND:"))
            .expect("the daemon's STOP_DRAIN_BOUND");
        let secs = line
            .split("from_secs(")
            .nth(1)
            .and_then(|rest| rest.split(')').next())
            .and_then(|n| n.trim().parse::<u64>().ok())
            .expect("STOP_DRAIN_BOUND is written in whole seconds");
        Duration::from_secs(secs)
    }

    #[test]
    fn the_graceful_wait_outlasts_the_daemons_drain_bound() {
        let bound = daemon_drain_bound();
        assert!(
            GRACEFUL_STOP_WAIT > bound,
            "the shell waits {GRACEFUL_STOP_WAIT:?} for a daemon whose sweep may take {bound:?}"
        );
    }
}
