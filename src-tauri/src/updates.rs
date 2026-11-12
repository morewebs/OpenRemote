//! Updates on the desktop: a newer signed release found through the endpoint
//! in tauri.conf.json (this project's own GitHub releases, over https),
//! downloaded, verified against the public key the app was built with, and
//! installed when the user says so. The console decides when to look (once a
//! day, `src/updates.js`) and shows what was found.
//!
//! The request is the updater plugin's own: the shell adds no header, no
//! query and no identifier of its own to it.

use std::future::Future;

use serde::Serialize;
use tauri::AppHandle;
use tauri_plugin_updater::UpdaterExt;

use crate::desktop;

/// What a look found, as the console shows it.
#[derive(Clone, Serialize)]
pub struct Check {
    /// False while the build carries no public key: releases are not signed
    /// for it yet, so nothing was asked of the network.
    pub enabled: bool,
    pub available: Option<Available>,
}

/// A newer release.
#[derive(Clone, Serialize)]
pub struct Available {
    pub version: String,
    pub notes: Option<String>,
    pub date: Option<String>,
}

/// Whether this build can update: the app carries the signing key's public
/// half. Until the key exists, the updater is never asked to look.
fn switched_on(app: &AppHandle) -> bool {
    app.config()
        .plugins
        .0
        .get("updater")
        .and_then(|u| u.get("pubkey"))
        .and_then(|k| k.as_str())
        .is_some_and(|k| !k.trim().is_empty())
}

#[tauri::command]
pub async fn update_check(app: AppHandle) -> Result<Check, String> {
    if !switched_on(&app) {
        return Ok(Check {
            enabled: false,
            available: None,
        });
    }
    let update = app
        .updater()
        .map_err(|e| e.to_string())?
        .check()
        .await
        .map_err(|e| format!("couldn't check for updates: {e}"))?;
    Ok(Check {
        enabled: true,
        available: update.map(|u| Available {
            version: u.version,
            notes: u.body,
            date: u.date.map(|d| d.to_string()),
        }),
    })
}

/// Download and verify the newer release, stop the daemon, install, and
/// restart into the new version.
#[tauri::command]
pub async fn update_install(app: AppHandle) -> Result<(), String> {
    if !switched_on(&app) {
        return Err("updates aren't switched on in this build".into());
    }
    let Some(update) = app
        .updater()
        .map_err(|e| e.to_string())?
        .check()
        .await
        .map_err(|e| format!("couldn't check for updates: {e}"))?
    else {
        return Err("this is already the newest version".into());
    };
    // The download is verified against the public key here, before anything
    // is stopped: a bad signature leaves the running app and its chats alone.
    let bytes = update
        .download(|_, _| {}, || {})
        .await
        .map_err(|e| format!("the update didn't download: {e}"))?;
    stop_then_install(
        || desktop::stop_daemon(&app),
        || {
            update
                .install(&bytes)
                .map_err(|e| format!("the update didn't install: {e}"))
        },
        || desktop::start_daemon(&app),
    )
    .await?;
    // macOS and Linux replace the app in place and return here; the Windows
    // installer has already taken over and the process has exited.
    app.restart();
}

/// The order an install runs in. The daemon is stopped first, and waited
/// for: the installer replaces the daemon's binary too, and on Windows a
/// running daemon holds that file open, so an installer started beside it
/// fails or leaves the old daemon behind. An install that fails brings the
/// daemon back, so the app the user is left with still runs its chats.
pub(crate) async fn stop_then_install<S, F, I, R>(
    stop: S,
    install: I,
    resume: R,
) -> Result<(), String>
where
    S: FnOnce() -> F,
    F: Future<Output = ()>,
    I: FnOnce() -> Result<(), String>,
    R: FnOnce(),
{
    stop().await;
    let installed = install();
    if installed.is_err() {
        resume();
    }
    installed
}

#[cfg(test)]
mod tests {
    use super::stop_then_install;
    use std::cell::RefCell;

    #[test]
    fn the_daemon_is_stopped_and_gone_before_the_installer_runs() {
        let steps = RefCell::new(Vec::new());
        let done = tauri::async_runtime::block_on(stop_then_install(
            || async {
                steps.borrow_mut().push("stop begins");
                // The stop is awaited to its end, not merely started: it
                // yields once before the daemon is gone.
                let mut yielded = false;
                std::future::poll_fn(|cx| {
                    if yielded {
                        std::task::Poll::Ready(())
                    } else {
                        yielded = true;
                        cx.waker().wake_by_ref();
                        std::task::Poll::Pending
                    }
                })
                .await;
                steps.borrow_mut().push("daemon gone");
            },
            || {
                steps.borrow_mut().push("install");
                Ok(())
            },
            || steps.borrow_mut().push("daemon back"),
        ));
        assert_eq!(done, Ok(()));
        assert_eq!(
            *steps.borrow(),
            ["stop begins", "daemon gone", "install"],
            "an install that lands restarts the app, not the daemon"
        );
    }

    #[test]
    fn a_failed_install_brings_the_daemon_back() {
        let steps = RefCell::new(Vec::new());
        let done = tauri::async_runtime::block_on(stop_then_install(
            || async { steps.borrow_mut().push("stop") },
            || {
                steps.borrow_mut().push("install");
                Err("the installer refused".to_string())
            },
            || steps.borrow_mut().push("daemon back"),
        ));
        assert_eq!(done, Err("the installer refused".to_string()));
        assert_eq!(*steps.borrow(), ["stop", "install", "daemon back"]);
    }
}
