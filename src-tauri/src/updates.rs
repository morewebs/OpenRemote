//! Updates on the desktop: a newer signed release found through the endpoint
//! in tauri.conf.json (this project's own GitHub releases, over https),
//! downloaded, verified against the public key the app was built with, and
//! installed when the user says so. The console decides when to look
//! (`src/updates.js`) and shows what was found.
//!
//! The request is the updater plugin's own: the shell adds no header, no
//! query and no identifier of its own to it.
//!
//! Windows installs in place. The installer is fetched in the background as
//! soon as a check finds it, and held in memory until the user confirms. The
//! other systems are sent to the release page instead.

use std::future::Future;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use serde::Serialize;
use tauri::{AppHandle, Url};
use tauri_plugin_updater::UpdaterExt;

use crate::{browser, desktop};

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
    /// The release page the console's badge opens, from the feed's `page` key.
    pub page: Option<String>,
    /// Whether this build installs the update itself. Only Windows does; the
    /// other systems send the user to `page`.
    pub can_install: bool,
    /// Whether the installer for this version is already in memory, so an
    /// install starts without a wait.
    pub ready: bool,
}

/// The Windows installer for one version, held in memory until an install
/// takes it or the app quits.
struct Staged {
    version: String,
    bytes: Vec<u8>,
}

/// The installer fetched in the background. One slot: a copy for a newer
/// version replaces the one it finds.
static STAGED: Mutex<Option<Staged>> = Mutex::new(None);

/// Set while a background download runs, so a second check does not start
/// another one beside it.
static STAGING: AtomicBool = AtomicBool::new(false);

/// The release page of the most recent check that got an answer. It is kept
/// here, not sent back to the console, so the open command takes no argument
/// and can only open the page the feed announced.
static PAGE: Mutex<Option<String>> = Mutex::new(None);

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

/// Looks for a newer release. On Windows a newer one starts its download in
/// the background here, so the console's install is one click away.
#[tauri::command]
pub async fn update_check(app: AppHandle) -> Result<Check, String> {
    if !switched_on(&app) {
        remember_page(None);
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
    let Some(update) = update else {
        remember_page(None);
        return Ok(Check {
            enabled: true,
            available: None,
        });
    };
    let page = page_from(&update.raw_json);
    remember_page(page.clone());
    let can_install = cfg!(target_os = "windows");
    let version = update.version.clone();
    let available = Available {
        version: version.clone(),
        notes: update.body.clone(),
        date: update.date.map(|d| d.to_string()),
        page,
        can_install,
        ready: can_install && staged_for(&STAGED, &version),
    };
    // Fetched now, so the install is one click away. A failed fetch is dropped
    // without a word: the next check tries again, and an install downloads on
    // its own when no copy is held.
    if can_install && !available.ready {
        if let Some(flight) = InFlight::begin(&STAGING) {
            tauri::async_runtime::spawn(async move {
                let download = update.download(|_, _| {}, || {});
                stage_with(flight, &STAGED, version, download).await;
            });
        }
    }
    Ok(Check {
        enabled: true,
        available: Some(available),
    })
}

/// Download and verify the newer release, stop the daemon, install, and
/// restart into the new version. Only Windows installs in place; the other
/// systems open the release page instead (`update_open_page`).
#[tauri::command]
pub async fn update_install(app: AppHandle) -> Result<(), String> {
    if !cfg!(target_os = "windows") {
        return Err("install from the release page".into());
    }
    if !switched_on(&app) {
        return Err("updates aren't switched on in this build".into());
    }
    let Some(_slot) = InstallSlot::take(&INSTALLING) else {
        return Err("an update is already being installed".into());
    };
    let Some(update) = app
        .updater()
        .map_err(|e| e.to_string())?
        .check()
        .await
        .map_err(|e| format!("couldn't check for updates: {e}"))?
    else {
        return Err("this is already the newest version".into());
    };
    // Taken, not copied: a failed install must not leave the same package
    // behind for the next click to reuse. A retry downloads afresh. A copy for
    // another version is dropped here too, since it can never be right for this
    // update.
    //
    // Both paths are verified against the public key before anything is
    // stopped. download() checks the signature before it returns, and install()
    // checks the bytes again before the installer runs, so a staged copy is
    // checked twice. A bad signature leaves the running app and its chats alone.
    let bytes = match take_staged(&STAGED, &update.version) {
        Some(bytes) => bytes,
        None => update
            .download(|_, _| {}, || {})
            .await
            .map_err(|e| format!("the update didn't download: {e}"))?,
    };
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

/// Opens the release page of the most recent check. It takes no argument: the
/// page is the one the feed announced, so the console cannot send a link. It
/// goes through the same opener the built-in browser uses for pages outside
/// the app, which the desktop already registers.
#[tauri::command]
pub fn update_open_page(app: AppHandle) -> Result<(), String> {
    let page = PAGE
        .lock()
        .expect("update page lock")
        .clone()
        .ok_or("no release page is known yet")?;
    browser::browser_open_external(app, page)
}

/// Keeps the page of a check that got an answer. A failed check returns before
/// this is called, so it leaves the page of the last good check in place.
fn remember_page(page: Option<String>) {
    *PAGE.lock().expect("update page lock") = page;
}

/// The release page from the feed's `page` key, or None when it is missing or
/// is not usable. The feed is not signed, so only an https address is kept:
/// the page is handed to the system's browser, and a file path or a script
/// link must never travel that way.
fn page_from(raw: &serde_json::Value) -> Option<String> {
    let page = Url::parse(raw.get("page")?.as_str()?).ok()?;
    (page.scheme() == "https").then(|| page.to_string())
}

/// Whether `slot` holds the installer for `version`.
fn staged_for(slot: &Mutex<Option<Staged>>, version: &str) -> bool {
    slot.lock()
        .expect("staged update lock")
        .as_ref()
        .is_some_and(|s| s.version == version)
}

/// Hands out the installer for `version`, and empties the slot whatever the
/// version is: the bytes leave the slot once, and only for the version asked.
fn take_staged(slot: &Mutex<Option<Staged>>, version: &str) -> Option<Vec<u8>> {
    let staged = slot.lock().expect("staged update lock").take()?;
    (staged.version == version).then_some(staged.bytes)
}

fn keep_staged(slot: &Mutex<Option<Staged>>, version: String, bytes: Vec<u8>) {
    *slot.lock().expect("staged update lock") = Some(Staged { version, bytes });
}

/// One background download. Its bytes are kept only when the download
/// succeeds. The flag is released when this returns, however it ended, so a
/// failed download never blocks the next check.
async fn stage_with<E, F>(
    _flight: InFlight<'_>,
    slot: &Mutex<Option<Staged>>,
    version: String,
    download: F,
) where
    F: Future<Output = Result<Vec<u8>, E>>,
{
    if let Ok(bytes) = download.await {
        keep_staged(slot, version, bytes);
    }
}

/// Holds the background download's flag until dropped. It follows the same
/// rule as `InstallSlot`, but a download ends by itself rather than by
/// restarting the app, so the flag is given back however the download ends.
struct InFlight<'a>(&'a AtomicBool);

impl<'a> InFlight<'a> {
    fn begin(flag: &'a AtomicBool) -> Option<Self> {
        flag.compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .ok()
            .map(|_| Self(flag))
    }
}

impl Drop for InFlight<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
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

/// One install at a time. Two clicks, or a click and a second window's call,
/// would otherwise both stop the daemon and both start an installer over the
/// same files.
static INSTALLING: AtomicBool = AtomicBool::new(false);

/// Holds the install slot until dropped. An install that lands restarts the
/// app, so only a failed one ever gives the slot back.
pub(crate) struct InstallSlot<'a>(&'a AtomicBool);

impl<'a> InstallSlot<'a> {
    pub(crate) fn take(flag: &'a AtomicBool) -> Option<Self> {
        flag.compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .ok()
            .map(|_| Self(flag))
    }
}

impl Drop for InstallSlot<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}

#[cfg(test)]
mod tests {
    use super::{
        keep_staged, page_from, stage_with, staged_for, stop_then_install, take_staged, InFlight,
        InstallSlot,
    };
    use serde_json::json;
    use std::sync::atomic::AtomicBool;
    use std::sync::Mutex;

    #[test]
    fn a_second_install_is_refused_while_one_runs() {
        let flag = AtomicBool::new(false);
        let first = InstallSlot::take(&flag).expect("the first install takes the slot");
        assert!(
            InstallSlot::take(&flag).is_none(),
            "an overlapping install is refused"
        );
        drop(first);
        assert!(
            InstallSlot::take(&flag).is_some(),
            "a failed install gives the slot back"
        );
    }
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

    #[test]
    fn the_release_page_is_read_from_the_feed() {
        let feed = json!({
            "version": "0.5.1",
            "page": "https://github.com/morewebs/OpenRemote/releases/tag/v0.5.1"
        });
        assert_eq!(
            page_from(&feed).as_deref(),
            Some("https://github.com/morewebs/OpenRemote/releases/tag/v0.5.1")
        );
    }

    #[test]
    fn a_feed_without_a_usable_page_gives_none() {
        assert_eq!(
            page_from(&json!({ "version": "0.5.1" })),
            None,
            "no page key"
        );
        assert_eq!(page_from(&json!({ "page": 7 })), None, "not a string");
        assert_eq!(page_from(&json!({ "page": "" })), None, "an empty page");
        for page in [
            "http://example.com/release",
            "file:///C:/Windows/win.ini",
            "javascript:alert(1)",
        ] {
            assert_eq!(
                page_from(&json!({ "page": page })),
                None,
                "{page} is not an https page"
            );
        }
    }

    #[test]
    fn a_staged_installer_is_handed_out_once_and_only_for_its_version() {
        let slot = Mutex::new(None);
        keep_staged(&slot, "0.5.1".into(), vec![1, 2, 3]);
        assert!(staged_for(&slot, "0.5.1"));
        assert_eq!(
            take_staged(&slot, "0.5.2"),
            None,
            "another version's installer is never handed out"
        );
        assert!(!staged_for(&slot, "0.5.1"), "the attempt empties the slot");

        keep_staged(&slot, "0.5.1".into(), vec![1, 2, 3]);
        assert_eq!(take_staged(&slot, "0.5.1"), Some(vec![1, 2, 3]));
        assert_eq!(
            take_staged(&slot, "0.5.1"),
            None,
            "the bytes are handed out once"
        );
    }

    #[test]
    fn a_second_background_download_is_refused_while_one_is_in_flight() {
        let flag = AtomicBool::new(false);
        let first = InFlight::begin(&flag).expect("the first download starts");
        assert!(
            InFlight::begin(&flag).is_none(),
            "a second check does not start another"
        );
        drop(first);
        assert!(
            InFlight::begin(&flag).is_some(),
            "a finished download frees the flag"
        );
    }

    #[test]
    fn a_failed_background_download_clears_the_flag_and_stages_nothing() {
        let flag = AtomicBool::new(false);
        let slot = Mutex::new(None);
        let flight = InFlight::begin(&flag).expect("the download starts");
        tauri::async_runtime::block_on(stage_with(flight, &slot, "0.5.1".into(), async {
            Err::<Vec<u8>, &str>("the connection dropped")
        }));
        assert!(
            !staged_for(&slot, "0.5.1"),
            "nothing is kept from a failed download"
        );
        assert!(InFlight::begin(&flag).is_some(), "the next check may retry");
    }

    #[test]
    fn a_finished_background_download_keeps_its_bytes_and_frees_the_flag() {
        let flag = AtomicBool::new(false);
        let slot = Mutex::new(None);
        let flight = InFlight::begin(&flag).expect("the download starts");
        tauri::async_runtime::block_on(stage_with(flight, &slot, "0.5.1".into(), async {
            Ok::<Vec<u8>, &str>(vec![7, 7])
        }));
        assert!(staged_for(&slot, "0.5.1"));
        assert!(InFlight::begin(&flag).is_some());
    }
}
