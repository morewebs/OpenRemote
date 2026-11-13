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
//!
//! On Windows the fetch takes the smallest route the feed offers: a patch from
//! the installed version's uncompressed installer (kept, packed, from the last
//! update), else the whole uncompressed installer packed, else the release's own
//! installer. Every route is checked against its signature, the same check the
//! plugin makes on the installer it downloads, so a route that does not verify
//! is passed over. The uncompressed installer is what the patches are made from.

use std::fs;
use std::future::Future;
use std::io;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use serde::Serialize;
use serde_json::{json, Value};
use tauri::{AppHandle, Manager, Url};
use tauri_plugin_updater::{Update, UpdaterExt};

use crate::{browser, delta_codec, desktop};

/// The updater's name for this system, as the feed keys its platforms.
const TARGET: &str = "windows-x86_64";

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

/// The installer for one version, held in memory until an install takes it or
/// the app quits.
struct Staged {
    version: String,
    /// The installer's bytes.
    bytes: Vec<u8>,
    /// Set when `bytes` is the uncompressed installer a patch was applied to or
    /// that was unpacked: the feed entry it is checked against, which the install
    /// restores. None when `bytes` is the release's own installer, which the
    /// feed's entry (already held by the check) covers.
    entry: Option<Value>,
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
    forget_stale_bases(&app);
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
    // without a word: the next check tries again, and an install fetches on
    // its own when no copy is held.
    if can_install && !available.ready {
        if let Some(flight) = InFlight::begin(&STAGING) {
            let app = app.clone();
            tauri::async_runtime::spawn(async move {
                stage_with(flight, &STAGED, fetch(&app, &update)).await;
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
    // behind for the next click to reuse. A retry fetches afresh. A copy for
    // another version is dropped here too, since it can never be right for this
    // update.
    //
    // Every route is verified before anything is stopped: a fetched or staged
    // copy is checked against its signature when it is fetched, and install()
    // checks the bytes again before the installer runs. A bad signature leaves
    // the running app and its chats alone.
    let staged = match take_staged(&STAGED, &update.version) {
        Some(staged) => staged,
        None => fetch(&app, &update).await?,
    };
    let Staged {
        version,
        bytes,
        entry,
    } = staged;
    // The uncompressed installer is the base of the next patch, so it is kept
    // for the version it installs. Kept before the install: the app restarts
    // into that version, and the copy is what it will patch from. A failure to
    // keep it only means the next update downloads a full package.
    if entry.is_some() {
        let _ = write_base(&app, &version, &bytes);
    }
    stop_then_install(
        || desktop::stop_daemon(&app),
        || install(&app, &update, entry.as_ref(), &bytes),
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
fn page_from(raw: &Value) -> Option<String> {
    let page = Url::parse(raw.get("page")?.as_str()?).ok()?;
    (page.scheme() == "https").then(|| page.to_string())
}

/// The installer for the newest release, by the smallest route the app has:
/// a patch from the installed version's base, then the whole uncompressed
/// installer packed, then the release's own installer. A route that fails is
/// passed over: the next one is its fallback, so only the last route's failure
/// is reported.
async fn fetch(app: &AppHandle, update: &Update) -> Result<Staged, String> {
    if let Some(staged) = fetch_delta(app, update).await {
        return Ok(staged);
    }
    let bytes = update
        .download(|_, _| {}, || {})
        .await
        .map_err(|e| format!("the update didn't download: {e}"))?;
    Ok(Staged {
        version: update.version.clone(),
        bytes,
        entry: None,
    })
}

/// The patch or the full package, when the feed offers one and it verifies.
/// None sends the caller to the release's own installer.
async fn fetch_delta(app: &AppHandle, update: &Update) -> Option<Staged> {
    let delta = update.raw_json.get("delta")?;
    let installed = app.package_info().version.to_string();
    let installer_signature = delta.pointer("/installer/signature")?.as_str()?;
    let installer = entry(
        &update.version,
        update.download_url.as_str(),
        installer_signature,
    );

    // A patch only reaches from the version it was made from, and only from the
    // base this app kept for it. Any other case, or a patch that does not
    // verify, goes on to the full package.
    if delta.get("from").and_then(Value::as_str) == Some(installed.as_str()) {
        if let Some(base) = read_base(app, &installed) {
            let patch = delta
                .get("patch")
                .and_then(|p| file_entry(&update.version, p));
            if let Some(patch) = patch {
                if let Some(bytes) = patched(app, &base, &patch, &installer).await {
                    return Some(uncompressed(update, bytes, installer));
                }
            }
        }
    }
    let full = file_entry(&update.version, delta.get("full")?)?;
    let bytes = unpacked(app, &full, &installer).await?;
    Some(uncompressed(update, bytes, installer))
}

fn uncompressed(update: &Update, bytes: Vec<u8>, installer: Value) -> Staged {
    Staged {
        version: update.version.clone(),
        bytes,
        entry: Some(installer),
    }
}

/// The uncompressed installer made by applying a verified patch to the base.
async fn patched(
    app: &AppHandle,
    base: &[u8],
    patch: &Value,
    installer: &Value,
) -> Option<Vec<u8>> {
    let patch = download(app, patch).await?;
    let bytes = delta_codec::apply_patch(base, &patch).ok()?;
    verified(app, installer, &bytes)?;
    Some(bytes)
}

/// The uncompressed installer unpacked from a verified full package.
async fn unpacked(app: &AppHandle, full: &Value, installer: &Value) -> Option<Vec<u8>> {
    let packed = download(app, full).await?;
    let bytes = delta_codec::unpack(&packed).ok()?;
    verified(app, installer, &bytes)?;
    Some(bytes)
}

/// Downloads one file of the feed. The plugin checks its signature before it
/// returns the bytes.
async fn download(app: &AppHandle, entry: &Value) -> Option<Vec<u8>> {
    restore(app, entry)
        .ok()?
        .download(|_, _| {}, || {})
        .await
        .ok()
}

/// Whether `bytes` is the file `entry` was signed for.
fn verified(app: &AppHandle, entry: &Value, bytes: &[u8]) -> Option<()> {
    restore(app, entry).ok()?.verify(bytes).ok()
}

/// Installs `bytes` as the release's installer: the feed's own update when
/// there is no entry, else the entry the bytes were verified against.
fn install(
    app: &AppHandle,
    update: &Update,
    entry: Option<&Value>,
    bytes: &[u8],
) -> Result<(), String> {
    let result = match entry {
        None => update.install(bytes),
        Some(entry) => restore(app, entry)?.install(bytes),
    };
    result.map_err(|e| format!("the update didn't install: {e}"))
}

/// An update for one file the feed names, restored from its entry. The plugin's
/// own download and install take their signature from the entry, which is how
/// a patch, a full package and the uncompressed installer are each checked
/// with the same code that checks the release's installer.
fn restore(app: &AppHandle, entry: &Value) -> Result<Update, String> {
    app.updater()
        .map_err(|e| e.to_string())?
        .restore_update(entry.clone())
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "the update is not newer than this app".to_string())
}

/// The feed entry for one file of the delta: its URL and signature, under the
/// version the update announces. The entry is what the plugin verifies against.
fn entry(version: &str, url: &str, signature: &str) -> Value {
    json!({
        "version": version,
        "platforms": { TARGET: { "url": url, "signature": signature.trim() } },
    })
}

/// The entry for a file the delta object names, when it has a URL and a
/// signature. None when it does not, so a half-written feed is passed over.
fn file_entry(version: &str, file: &Value) -> Option<Value> {
    Some(entry(
        version,
        file.get("url")?.as_str()?,
        file.get("signature")?.as_str()?,
    ))
}

/// The base file name for an installed version's uncompressed installer.
fn base_file_name(version: &str) -> String {
    format!("base-{version}.zst")
}

/// Whether a file in the updates folder is a base the app no longer needs: any
/// base but the installed version's, and any half-written one.
fn is_stale_base(name: &str, installed: &str) -> bool {
    name.starts_with("base-") && name != base_file_name(installed)
}

/// The folder the bases are kept in: the app's own data folder, so each channel
/// (stable, nightly) keeps its own.
fn bases_dir(app: &AppHandle) -> Option<PathBuf> {
    Some(app.path().app_data_dir().ok()?.join("updates"))
}

/// The installed version's base, unpacked. None when the app has none, which is
/// the case after a fresh install: its first update is a full package.
fn read_base(app: &AppHandle, installed: &str) -> Option<Vec<u8>> {
    let packed = fs::read(bases_dir(app)?.join(base_file_name(installed))).ok()?;
    delta_codec::unpack(&packed).ok()
}

/// Keeps `raw` as the base for `version`, packed. Written to a part file and
/// renamed, so a base is never half there.
fn write_base(app: &AppHandle, version: &str, raw: &[u8]) -> io::Result<()> {
    let dir = bases_dir(app).ok_or_else(|| io::Error::other("the app has no data folder"))?;
    fs::create_dir_all(&dir)?;
    let packed = delta_codec::pack(raw, delta_codec::STORE_LEVEL)?;
    let part = dir.join(format!("{}.part", base_file_name(version)));
    fs::write(&part, packed)?;
    fs::rename(part, dir.join(base_file_name(version)))
}

/// Removes the bases the installed version does not need. Run on each check,
/// which is before any install, so the base of a version that was just staged
/// is never removed: it is written only when its install starts.
fn forget_stale_bases(app: &AppHandle) {
    let Some(dir) = bases_dir(app) else {
        return;
    };
    let installed = app.package_info().version.to_string();
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        if is_stale_base(&name.to_string_lossy(), &installed) {
            let _ = fs::remove_file(entry.path());
        }
    }
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
fn take_staged(slot: &Mutex<Option<Staged>>, version: &str) -> Option<Staged> {
    let staged = slot.lock().expect("staged update lock").take()?;
    (staged.version == version).then_some(staged)
}

fn keep_staged(slot: &Mutex<Option<Staged>>, staged: Staged) {
    *slot.lock().expect("staged update lock") = Some(staged);
}

/// One background fetch. Its result is kept only when the fetch succeeds. The
/// flag is released when this returns, however it ended, so a failed fetch
/// never blocks the next check.
async fn stage_with<E>(
    _flight: InFlight<'_>,
    slot: &Mutex<Option<Staged>>,
    fetched: impl Future<Output = Result<Staged, E>>,
) {
    if let Ok(staged) = fetched.await {
        keep_staged(slot, staged);
    }
}

/// Holds the background fetch's flag until dropped. It follows the same rule as
/// `InstallSlot`, but a fetch ends by itself rather than by restarting the app,
/// so the flag is given back however the fetch ends.
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
        base_file_name, entry, file_entry, is_stale_base, keep_staged, page_from, stage_with,
        staged_for, stop_then_install, take_staged, InFlight, InstallSlot, Staged, TARGET,
    };
    use serde_json::json;
    use std::sync::atomic::AtomicBool;
    use std::sync::Mutex;

    fn staged(version: &str, bytes: Vec<u8>) -> Staged {
        Staged {
            version: version.into(),
            bytes,
            entry: None,
        }
    }

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
        keep_staged(&slot, staged("0.5.1", vec![1, 2, 3]));
        assert!(staged_for(&slot, "0.5.1"));
        assert!(
            take_staged(&slot, "0.5.2").is_none(),
            "another version's installer is never handed out"
        );
        assert!(!staged_for(&slot, "0.5.1"), "the attempt empties the slot");

        keep_staged(&slot, staged("0.5.1", vec![1, 2, 3]));
        let taken = take_staged(&slot, "0.5.1").expect("the version asked for");
        assert_eq!(taken.bytes, vec![1, 2, 3]);
        assert!(
            take_staged(&slot, "0.5.1").is_none(),
            "the bytes are handed out once"
        );
    }

    #[test]
    fn a_second_background_fetch_is_refused_while_one_is_in_flight() {
        let flag = AtomicBool::new(false);
        let first = InFlight::begin(&flag).expect("the first fetch starts");
        assert!(
            InFlight::begin(&flag).is_none(),
            "a second check does not start another"
        );
        drop(first);
        assert!(
            InFlight::begin(&flag).is_some(),
            "a finished fetch frees the flag"
        );
    }

    #[test]
    fn a_failed_background_fetch_clears_the_flag_and_stages_nothing() {
        let flag = AtomicBool::new(false);
        let slot = Mutex::new(None);
        let flight = InFlight::begin(&flag).expect("the fetch starts");
        tauri::async_runtime::block_on(stage_with(flight, &slot, async {
            Err::<Staged, &str>("the connection dropped")
        }));
        assert!(
            !staged_for(&slot, "0.5.1"),
            "nothing is kept from a failed fetch"
        );
        assert!(InFlight::begin(&flag).is_some(), "the next check may retry");
    }

    #[test]
    fn a_finished_background_fetch_keeps_its_bytes_and_frees_the_flag() {
        let flag = AtomicBool::new(false);
        let slot = Mutex::new(None);
        let flight = InFlight::begin(&flag).expect("the fetch starts");
        tauri::async_runtime::block_on(stage_with(flight, &slot, async {
            Ok::<Staged, &str>(staged("0.5.1", vec![7, 7]))
        }));
        assert!(staged_for(&slot, "0.5.1"));
        assert!(InFlight::begin(&flag).is_some());
    }

    #[test]
    fn a_file_of_the_delta_is_restored_under_the_update_it_belongs_to() {
        let e = entry("0.5.1", "https://example.com/p.zst", " sig \n");
        assert_eq!(e["version"], "0.5.1");
        assert_eq!(
            e["platforms"][TARGET],
            json!({ "url": "https://example.com/p.zst", "signature": "sig" }),
            "the signature is trimmed, and the entry is keyed for this system"
        );
        let file = json!({ "url": "https://example.com/p.zst", "signature": "sig" });
        assert_eq!(file_entry("0.5.1", &file), Some(e));
    }

    #[test]
    fn a_delta_file_without_a_url_or_signature_is_passed_over() {
        assert_eq!(file_entry("0.5.1", &json!({ "url": "https://x" })), None);
        assert_eq!(file_entry("0.5.1", &json!({ "signature": "s" })), None);
        assert_eq!(
            file_entry("0.5.1", &json!({ "url": 1, "signature": "s" })),
            None
        );
    }

    #[test]
    fn a_base_is_kept_for_the_installed_version_only() {
        let installed = "0.5.1-nightly.202611130215";
        assert_eq!(
            base_file_name(installed),
            "base-0.5.1-nightly.202611130215.zst"
        );
        assert!(!is_stale_base(&base_file_name(installed), installed));
        assert!(
            is_stale_base(&base_file_name("0.5.0"), installed),
            "an older base goes"
        );
        assert!(is_stale_base(
            &format!("{}.part", base_file_name(installed)),
            "0.5.0"
        ));
        assert!(
            !is_stale_base("settings.json", installed),
            "other files are not touched"
        );
    }
}
