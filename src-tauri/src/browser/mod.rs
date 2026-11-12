//! The built-in browser's engine on the desktop. Each tab is a native
//! webview (WebView2 on Windows, WebKit on macOS and Linux) laid over a
//! placeholder the console draws, in a profile folder of its own, so web
//! pages never share storage with the console. The console owns the
//! browser's chrome (tab strip, address bar, menus); the shell owns the
//! pages and tells the console what they do through `browser:*` events.
//!
//! Tabs are webviews in the console's own window, so two rules keep them
//! away from the app: the app's commands answer only the console's webview
//! (the ACL in build.rs and capabilities/default.json), and a tab never
//! loads the app's own addresses (`allowed`).

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};

use openremote_browser::Controller;
use serde::{Deserialize, Serialize};
use tauri::webview::{DownloadEvent, NewWindowResponse, PageLoadEvent, WebviewBuilder};
use tauri::{
    AppHandle, Emitter, EventTarget, LogicalPosition, LogicalSize, Manager, State, Url, Webview,
    WebviewUrl,
};

#[cfg(windows)]
mod adblock;
mod agent;
mod downloads;
pub mod pipe;
#[cfg(all(windows, debug_assertions))]
pub mod selftest;
#[cfg(windows)]
mod win;

/// The console's webview: the only one that hears the browser's events.
const CONSOLE: &str = "main";

/// Every tab's downloads in one ledger: ids are unique across the app, and a
/// finish from one tab can never close the row another tab opened.
static DOWNLOADS: LazyLock<Mutex<downloads::Ledger>> =
    LazyLock::new(|| Mutex::new(downloads::Ledger::default()));

/// The tabs this window holds, by id.
#[derive(Default)]
pub struct Browser {
    next: AtomicU64,
    tabs: Mutex<HashMap<String, Tab>>,
    /// Where the console last put a page: an agent's new tab takes its size.
    last_bounds: Mutex<Option<Bounds>>,
}

/// A tab nobody has looked at for this long sleeps (background tabs cost
/// nothing), unless an agent holds it.
const SLEEP_AFTER: Duration = Duration::from_secs(5 * 60);

/// What the console is told about a tab.
#[derive(Clone, Debug, Default, Serialize)]
pub struct Tab {
    pub id: String,
    pub url: String,
    pub title: String,
    pub loading: bool,
    /// The page's icon as a data URL (the console shows no remote images).
    pub favicon: Option<String>,
    pub can_back: bool,
    pub can_forward: bool,
    /// The tab that opened this one (a link with target=_blank or
    /// window.open), so the console can place it beside its opener.
    pub opener: Option<String>,
    /// Who holds the tab besides the user: a chat's agent, or the user who
    /// took it over from one (the tab strip's badge).
    pub controller: Option<Controller>,
    /// A private tab: a throwaway profile, no history, hidden from agents.
    pub private: bool,
    /// A private tab the user handed to agents.
    pub shared: bool,
    /// The page is showing something fullscreen (a video).
    pub fullscreen: bool,
    #[serde(skip)]
    bounds: Option<Bounds>,
    /// The console is showing it.
    #[serde(skip)]
    pub shown: bool,
    /// Drawn off screen for an agent while the console shows another tab,
    /// so its pictures and layout stay real.
    #[serde(skip)]
    pub parked: bool,
    #[serde(skip)]
    pub suspended: bool,
    #[serde(skip)]
    pub hidden_since: Option<Instant>,
}

/// A download a tab started, as the console lists it.
#[derive(Clone, Debug, Serialize)]
pub struct Download {
    pub id: u64,
    pub tab: String,
    pub url: String,
    pub path: Option<String>,
    /// "started", "done" or "failed".
    pub state: &'static str,
}

/// Where the console's placeholder sits, in CSS pixels. The console's
/// webview fills the window from its top-left corner, so CSS pixels are
/// the window's logical pixels and Tauri scales them for the display.
#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
pub struct Bounds {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// A browser key pressed while a page had focus: the page never sees it
/// and the console acts on it (`Mod+T`, the app's own `Mod+K`...).
///
/// Built only by the Windows backend, where WebView2's accelerator handler is
/// what reports the key; the other platforms let the console's own listener
/// see it, so on Linux this type is declared and never constructed, which
/// `-D warnings` reads as dead code. The allow is that fact, not a silence.
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
#[derive(Clone, Debug, Serialize)]
pub struct KeyPress {
    pub tab: String,
    pub chord: String,
}

/// The names Tauri answers on `<name>.localhost` in every webview of the
/// app: the console's own pages, its IPC and its assets.
const APP_HOSTS: [&str; 3] = ["tauri.localhost", "ipc.localhost", "asset.localhost"];

/// Whether a tab may load this address. The console is served from
/// `tauri.localhost` (and in development from the dev server), and Tauri
/// answers its IPC and asset schemes on a few `*.localhost` names in every
/// webview of the app, so those stay out of reach. Every other address on
/// this machine stays open - plain `localhost`, 127.0.0.1 and names like
/// `myapp.localhost` are the user's own dev servers.
pub fn allowed(url: &Url, dev: Option<&Url>) -> bool {
    match url.scheme() {
        "http" | "https" => {
            let host = url.host_str().unwrap_or("").trim_end_matches('.');
            if APP_HOSTS.iter().any(|app| host.eq_ignore_ascii_case(app)) {
                return false;
            }
            !dev.is_some_and(|dev| dev.origin() == url.origin())
        }
        "about" | "blob" | "data" | "file" => true,
        _ => false,
    }
}

/// The browser's profile: cookies, storage and cache live here, apart
/// from the console's own.
fn profile_dir(app: &AppHandle) -> Result<PathBuf, String> {
    // A test run points the profile somewhere disposable.
    #[cfg(debug_assertions)]
    if let Some(dir) = std::env::var_os("OPENREMOTE_BROWSER_DIR") {
        return Ok(PathBuf::from(dir));
    }
    let base = app.path().app_local_data_dir().map_err(|e| e.to_string())?;
    Ok(base.join("browser").join("profile"))
}

/// The console's own webview, as an event target: the daemon's status and the browser's
/// news both go there and nowhere else. `pub(crate)` because the sidecar code speaks to
/// the console through it.
pub(crate) fn console() -> EventTarget {
    EventTarget::Webview {
        label: CONSOLE.into(),
    }
}

/// Where an agent's tab is drawn while the console isn't showing it: off
/// screen, at the size pages are shown at.
pub fn parked_bounds(app: &AppHandle) -> Bounds {
    let last = *app
        .state::<Browser>()
        .last_bounds
        .lock()
        .expect("browser lock");
    let size = last.unwrap_or(Bounds {
        x: 0.0,
        y: 0.0,
        width: 1280.0,
        height: 800.0,
    });
    Bounds {
        x: -30000.0,
        y: 0.0,
        width: size.width.max(320.0),
        height: size.height.max(240.0),
    }
}

fn tab(app: &AppHandle, id: &str) -> Result<Webview, String> {
    app.get_webview(id)
        .filter(|_| id.starts_with("tab-"))
        .ok_or_else(|| format!("no tab {id}"))
}

/// Change what the shell knows about a tab and tell the console.
fn update(app: &AppHandle, id: &str, change: impl FnOnce(&mut Tab)) {
    let browser = app.state::<Browser>();
    let snapshot = {
        let mut tabs = browser.tabs.lock().expect("browser lock");
        let Some(tab) = tabs.get_mut(id) else {
            return;
        };
        change(tab);
        tab.clone()
    };
    let _ = app.emit_to(console(), "browser:tab", snapshot.clone());
    pipe::tab_changed(app, &snapshot);
}

/// A page went fullscreen (a video's button) or came back: the whole
/// window follows, and the console hides its chrome.
///
/// Only the Windows engine reports it, so on the other platforms nothing calls
/// this. The allow says that, rather than hiding a function that became dead
/// on Windows too.
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub fn fullscreen_changed(app: &AppHandle, tab: &str, on: bool) {
    update(app, tab, |t| t.fullscreen = on);
    if let Some(window) = app.get_window(CONSOLE) {
        let _ = window.set_fullscreen(on);
    }
    let _ = app.emit_to(
        console(),
        "browser:fullscreen",
        serde_json::json!({ "tab": tab, "on": on }),
    );
}

/// Close a tab: its page, its record, and what agents knew of it.
pub fn close_tab(app: &AppHandle, id: &str) -> Result<(), String> {
    let webview = self::tab(app, id)?;
    app.state::<Browser>()
        .tabs
        .lock()
        .expect("browser lock")
        .remove(id);
    #[cfg(windows)]
    {
        agent::forget(id);
        adblock::forget(id);
    }
    pipe::closed(app, id);
    webview.close().map_err(|e| e.to_string())
}

/// Open a tab. It blocks until the window's thread has built the webview
/// (Tauri's add_child waits on it), so it must never run on that thread:
/// commands call it from the async runtime, and a page's popup from the
/// blocking pool.
pub fn open_tab(
    app: AppHandle,
    url: Url,
    bounds: Bounds,
    show: bool,
    opener: Option<String>,
    private: bool,
) -> Result<String, String> {
    if !allowed(&url, app.config().build.dev_url.as_ref()) {
        return Err(format!("{url} can't be opened here"));
    }
    let browser = app.state::<Browser>();
    let id = format!("tab-{}", browser.next.fetch_add(1, Ordering::Relaxed) + 1);
    let window = app.get_window(CONSOLE).ok_or("the window is gone")?;
    let dev = app.config().build.dev_url.clone();
    let (on_load, on_title, on_popup, on_download) =
        (app.clone(), app.clone(), app.clone(), app.clone());
    let popup_from = id.clone();
    let builder = WebviewBuilder::new(&id, WebviewUrl::External(url.clone()))
        .data_directory(profile_dir(&app)?)
        .on_navigation(move |to| allowed(to, dev.as_ref()))
        .on_page_load(move |webview, payload| {
            let loading = matches!(payload.event(), PageLoadEvent::Started);
            let url = payload.url().to_string();
            update(&on_load, webview.label(), |tab| {
                tab.loading = loading;
                tab.url = url;
            });
        })
        .on_document_title_changed(move |webview, title| {
            update(&on_title, webview.label(), |tab| tab.title = title);
        })
        // A page asking for a new window gets a tab instead.
        .on_new_window(move |url, _features| {
            let app = on_popup.clone();
            let opener = popup_from.clone();
            tauri::async_runtime::spawn_blocking(move || {
                let bounds = app
                    .state::<Browser>()
                    .tabs
                    .lock()
                    .expect("browser lock")
                    .get(&opener)
                    .and_then(|tab| tab.bounds);
                let private = app
                    .state::<Browser>()
                    .tabs
                    .lock()
                    .expect("browser lock")
                    .get(&opener)
                    .is_some_and(|tab| tab.private);
                if let Some(bounds) = bounds {
                    // A private tab's popups stay private.
                    let _ = open_tab(app, url, bounds, true, Some(opener), private);
                }
            });
            NewWindowResponse::Deny
        })
        // Downloads land where WebView2 puts them (the user's Downloads
        // folder, a free name); handling them here keeps Edge's own
        // download bubble away, and the console lists them instead.
        .on_download(move |webview, event| {
            let tab = webview.label().to_string();
            let download = {
                let mut ledger = DOWNLOADS.lock().expect("downloads lock");
                match event {
                    DownloadEvent::Requested { url, destination } => {
                        let path = destination.display().to_string();
                        ledger.requested(&tab, url.as_ref(), Some(path))
                    }
                    DownloadEvent::Finished { url, path, success } => {
                        let path = path.map(|path| path.display().to_string());
                        ledger.finished(&tab, url.as_ref(), path, success)
                    }
                    _ => return true,
                }
            };
            let _ = on_download.emit_to(console(), "browser:download", download.clone());
            pipe::download(&on_download, &download);
            true
        })
        // A private tab keeps nothing: WebView2's InPrivate, WebKit's
        // non-persistent data store.
        .incognito(private);
    #[cfg(windows)]
    let builder = builder.additional_browser_args(win::BROWSER_ARGS);
    browser.tabs.lock().expect("browser lock").insert(
        id.clone(),
        Tab {
            id: id.clone(),
            url: url.to_string(),
            loading: true,
            opener: opener.clone(),
            private,
            bounds: Some(bounds),
            shown: show,
            hidden_since: (!show).then(Instant::now),
            ..Tab::default()
        },
    );
    let webview = window
        .add_child(
            builder,
            LogicalPosition::new(bounds.x, bounds.y),
            LogicalSize::new(bounds.width, bounds.height),
        )
        .map_err(|e| {
            browser.tabs.lock().expect("browser lock").remove(&id);
            e.to_string()
        })?;
    if !show {
        let _ = webview.hide();
    }
    #[cfg(windows)]
    {
        win::attach(&app, &webview);
        win::watch_requests(&app, &webview);
        // Hooks only while ad blocking is on; off, this does nothing.
        adblock::attach(&app, &webview);
    }
    update(&app, &id, |_| {});
    Ok(id)
}

#[tauri::command]
pub async fn browser_open(
    app: AppHandle,
    url: String,
    bounds: Bounds,
    show: bool,
    private: Option<bool>,
) -> Result<String, String> {
    let url = Url::parse(&url).map_err(|e| e.to_string())?;
    *app.state::<Browser>()
        .last_bounds
        .lock()
        .expect("browser lock") = Some(bounds);
    open_tab(app, url, bounds, show, None, private.unwrap_or(false))
}

/// The tabs the shell holds, oldest first: a console that reloaded takes
/// them back instead of leaving them on screen, orphaned.
#[tauri::command]
pub fn browser_tabs(browser: State<'_, Browser>) -> Vec<Tab> {
    let mut tabs: Vec<Tab> = browser
        .tabs
        .lock()
        .expect("browser lock")
        .values()
        .cloned()
        .collect();
    tabs.sort_by_key(|t| t.id.trim_start_matches("tab-").parse::<u64>().unwrap_or(0));
    tabs
}

#[tauri::command]
pub fn browser_close(app: AppHandle, tab: String) -> Result<(), String> {
    close_tab(&app, &tab)
}

#[tauri::command]
pub fn browser_set_bounds(app: AppHandle, tab: String, bounds: Bounds) -> Result<(), String> {
    let webview = self::tab(&app, &tab)?;
    let browser = app.state::<Browser>();
    if let Some(known) = browser.tabs.lock().expect("browser lock").get_mut(&tab) {
        known.bounds = Some(bounds);
        // The console placed it, so it is no longer parked off screen.
        known.parked = false;
    }
    // Off-screen placements (the console's own picture swap) don't count.
    if bounds.x > -10000.0 {
        *browser.last_bounds.lock().expect("browser lock") = Some(bounds);
    }
    webview
        .set_bounds(tauri::Rect {
            position: LogicalPosition::new(bounds.x, bounds.y).into(),
            size: LogicalSize::new(bounds.width, bounds.height).into(),
        })
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn browser_show(app: AppHandle, tab: String, focus: bool) -> Result<(), String> {
    let webview = self::tab(&app, &tab)?;
    let suspended = {
        let browser = app.state::<Browser>();
        let mut tabs = browser.tabs.lock().expect("browser lock");
        tabs.get_mut(&tab).is_some_and(|t| {
            t.shown = true;
            t.parked = false;
            t.hidden_since = None;
            std::mem::replace(&mut t.suspended, false)
        })
    };
    #[cfg(windows)]
    if suspended {
        let _ = win::resume(&webview);
    }
    #[cfg(not(windows))]
    let _ = suspended;
    webview.show().map_err(|e| e.to_string())?;
    if focus {
        let _ = webview.set_focus();
    }
    Ok(())
}

#[tauri::command]
pub fn browser_hide(app: AppHandle, tab: String) -> Result<(), String> {
    let webview = self::tab(&app, &tab)?;
    if let Some(t) = app
        .state::<Browser>()
        .tabs
        .lock()
        .expect("browser lock")
        .get_mut(&tab)
    {
        t.shown = false;
        t.parked = false;
        t.hidden_since = Some(Instant::now());
    }
    webview.hide().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn browser_navigate(app: AppHandle, tab: String, url: String) -> Result<(), String> {
    let url = Url::parse(&url).map_err(|e| e.to_string())?;
    if !allowed(&url, app.config().build.dev_url.as_ref()) {
        return Err(format!("{url} can't be opened here"));
    }
    self::tab(&app, &tab)?
        .navigate(url)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn browser_back(app: AppHandle, tab: String) -> Result<(), String> {
    history(&self::tab(&app, &tab)?, -1)
}

#[tauri::command]
pub fn browser_forward(app: AppHandle, tab: String) -> Result<(), String> {
    history(&self::tab(&app, &tab)?, 1)
}

fn history(webview: &Webview, step: i32) -> Result<(), String> {
    #[cfg(windows)]
    return win::history(webview, step);
    #[cfg(not(windows))]
    webview
        .eval(format!("history.go({step})"))
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn browser_reload(app: AppHandle, tab: String) -> Result<(), String> {
    self::tab(&app, &tab)?.reload().map_err(|e| e.to_string())
}

/// A part of a page, in its own CSS pixels (relative to what's in view).
///
/// Its fields are read only where a picture can be taken, which is the Windows
/// engine; the other platforms accept the argument and answer that a picture is
/// not available yet, so the four numbers are never read there. The allow says
/// that, and it is scoped so that a field becoming unread on Windows still
/// fails the lint.
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
#[derive(Clone, Copy, Debug, Deserialize)]
pub struct Clip {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// A picture of the tab as it is now, as a data URL: the console shows it
/// in the tab's place while one of its own menus or dialogs covers the
/// page (a native webview always draws above the console's HTML), and a
/// screenshot sent to a chat. `clip` takes a part of what's in view.
#[tauri::command]
pub async fn browser_capture(
    app: AppHandle,
    tab: String,
    clip: Option<Clip>,
) -> Result<String, String> {
    let webview = self::tab(&app, &tab)?;
    #[cfg(windows)]
    {
        let mut params = serde_json::json!({ "format": "jpeg", "quality": 80 });
        if let Some(clip) = clip {
            // The clip is in the page's coordinates: what's in view plus
            // how far the page has scrolled.
            let scrolled = win::cdp(&webview, "Page.getLayoutMetrics", serde_json::json!({}))
                .await
                .unwrap_or_default();
            let (sx, sy) = (
                scrolled["cssVisualViewport"]["pageX"]
                    .as_f64()
                    .unwrap_or(0.0),
                scrolled["cssVisualViewport"]["pageY"]
                    .as_f64()
                    .unwrap_or(0.0),
            );
            params["clip"] = serde_json::json!({
                "x": clip.x + sx, "y": clip.y + sy,
                "width": clip.width.max(1.0), "height": clip.height.max(1.0), "scale": 1,
            });
        }
        let shot = win::cdp(&webview, "Page.captureScreenshot", params).await?;
        let data = shot["data"].as_str().ok_or("no picture came back")?;
        Ok(format!("data:image/jpeg;base64,{data}"))
    }
    #[cfg(not(windows))]
    {
        let _ = (webview, clip);
        Err("not on this platform yet".into())
    }
}

/// One DevTools protocol call on a tab, in-process: no debugging port is
/// ever opened, so nothing else on the machine can attach to the browser.
#[tauri::command]
pub async fn browser_cdp(
    app: AppHandle,
    tab: String,
    method: String,
    params: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let webview = self::tab(&app, &tab)?;
    #[cfg(windows)]
    return win::cdp(&webview, &method, params).await;
    #[cfg(not(windows))]
    {
        let _ = (webview, method, params);
        Err("not on this platform yet".into())
    }
}

/// Stop loading the page.
#[tauri::command]
pub fn browser_stop(app: AppHandle, tab: String) -> Result<(), String> {
    let webview = self::tab(&app, &tab)?;
    #[cfg(windows)]
    return win::stop(&webview);
    #[cfg(not(windows))]
    webview.eval("window.stop()").map_err(|e| e.to_string())
}

/// The page's zoom, 1.0 being 100%.
#[tauri::command]
pub fn browser_zoom(app: AppHandle, tab: String, factor: f64) -> Result<(), String> {
    self::tab(&app, &tab)?
        .set_zoom(factor.clamp(0.25, 5.0))
        .map_err(|e| e.to_string())
}

/// Find in page: a term to look for (and step through with `step`, 1 or
/// -1), the matches arriving as `browser:find` events. An empty term ends
/// the search.
#[tauri::command]
pub fn browser_find(app: AppHandle, tab: String, term: String, step: i32) -> Result<(), String> {
    let webview = self::tab(&app, &tab)?;
    #[cfg(windows)]
    return win::find(&app, &webview, term, step);
    #[cfg(not(windows))]
    {
        let _ = (app, webview, term, step);
        Err("not on this platform yet".into())
    }
}

#[tauri::command]
pub fn browser_print(app: AppHandle, tab: String) -> Result<(), String> {
    let webview = self::tab(&app, &tab)?;
    #[cfg(windows)]
    return win::print(&webview);
    #[cfg(not(windows))]
    webview.print().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn browser_devtools(app: AppHandle, tab: String) -> Result<(), String> {
    let webview = self::tab(&app, &tab)?;
    // Release builds gain DevTools with the browser's first release; the
    // console only offers the item in development until then.
    #[cfg(debug_assertions)]
    {
        webview.open_devtools();
        Ok(())
    }
    #[cfg(not(debug_assertions))]
    {
        let _ = webview;
        Err("DevTools aren't in this build yet".into())
    }
}

/// Show the page as another screen shows it: a phone's or any width and
/// height, drawn at `scale` when the space is smaller than the screen, and
/// for a phone with touch and a phone's user agent (what the browser's
/// own developer tools do). No size: the tab's own size again.
#[tauri::command]
pub async fn browser_viewport(
    app: AppHandle,
    tab: String,
    width: Option<u32>,
    height: Option<u32>,
    mobile: bool,
    scale: Option<f64>,
) -> Result<(), String> {
    let webview = self::tab(&app, &tab)?;
    #[cfg(windows)]
    {
        let Some((w, h)) = width.zip(height) else {
            win::cdp(
                &webview,
                "Emulation.clearDeviceMetricsOverride",
                serde_json::json!({}),
            )
            .await?;
            win::cdp(
                &webview,
                "Emulation.setTouchEmulationEnabled",
                serde_json::json!({ "enabled": false }),
            )
            .await?;
            win::cdp(
                &webview,
                "Emulation.setUserAgentOverride",
                serde_json::json!({ "userAgent": "" }),
            )
            .await?;
            return Ok(());
        };
        win::cdp(
            &webview,
            "Emulation.setDeviceMetricsOverride",
            serde_json::json!({
                "width": w.clamp(200, 3840),
                "height": h.clamp(200, 2160),
                "deviceScaleFactor": 0,
                "mobile": mobile,
                "scale": scale.unwrap_or(1.0).clamp(0.1, 1.0),
            }),
        )
        .await?;
        win::cdp(
            &webview,
            "Emulation.setTouchEmulationEnabled",
            serde_json::json!({ "enabled": mobile, "maxTouchPoints": 5 }),
        )
        .await?;
        let agent = if mobile {
            // The engine's own user agent, as its Android build words it.
            let found = win::cdp(&webview, "Browser.getVersion", serde_json::json!({})).await?;
            phone_agent(found["userAgent"].as_str().unwrap_or_default())
        } else {
            String::new()
        };
        win::cdp(
            &webview,
            "Emulation.setUserAgentOverride",
            serde_json::json!({ "userAgent": agent }),
        )
        .await
        .map(|_| ())
    }
    #[cfg(not(windows))]
    {
        let _ = (webview, width, height, mobile, scale);
        Err("not on this platform yet".into())
    }
}

/// A desktop Chromium user agent as the same engine's Android build sends
/// it, so sites serve the phone layout in device view.
#[cfg_attr(not(windows), allow(dead_code))]
fn phone_agent(desktop: &str) -> String {
    let Some(open) = desktop.find('(') else {
        return String::new();
    };
    let Some(close) = desktop[open..].find(')').map(|i| open + i) else {
        return String::new();
    };
    let rest = desktop[close + 1..].replace(" Safari/", " Mobile Safari/");
    // Edge's token is not on Android's Chromium.
    let rest = rest.split(" Edg/").next().unwrap_or(&rest).to_string();
    format!("{}(Linux; Android 10; K){rest}", &desktop[..open])
}

/// Forget the current site: its cookies, storage and cache.
#[tauri::command]
pub async fn browser_clear_site(app: AppHandle, tab: String) -> Result<(), String> {
    let webview = self::tab(&app, &tab)?;
    let url = webview.url().map_err(|e| e.to_string())?;
    let origin = url.origin().ascii_serialization();
    if origin == "null" {
        return Err("this page has no site to clear".into());
    }
    #[cfg(windows)]
    {
        win::cdp(
            &webview,
            "Storage.clearDataForOrigin",
            serde_json::json!({ "origin": origin, "storageTypes": "all" }),
        )
        .await?;
        webview.reload().map_err(|e| e.to_string())
    }
    #[cfg(not(windows))]
    {
        let _ = webview;
        Err("not on this platform yet".into())
    }
}

/// Open the page in the system's default browser.
#[tauri::command]
pub fn browser_open_external(app: AppHandle, url: String) -> Result<(), String> {
    let url = Url::parse(&url).map_err(|e| e.to_string())?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err("only web pages open outside".into());
    }
    #[allow(deprecated)]
    tauri_plugin_shell::ShellExt::shell(&app)
        .open(url.as_str(), None)
        .map_err(|e| e.to_string())
}

/// Show a finished download in the file manager.
#[tauri::command]
pub fn browser_reveal(path: String) -> Result<(), String> {
    let path = PathBuf::from(path);
    if !path.is_absolute() || !path.exists() {
        return Err("that file is gone".into());
    }
    #[cfg(windows)]
    let shown = std::process::Command::new("explorer")
        .arg("/select,")
        .arg(&path)
        .spawn();
    #[cfg(target_os = "macos")]
    let shown = std::process::Command::new("open")
        .arg("-R")
        .arg(&path)
        .spawn();
    #[cfg(all(unix, not(target_os = "macos")))]
    let shown = std::process::Command::new("xdg-open")
        .arg(path.parent().unwrap_or(&path))
        .spawn();
    shown.map(|_| ()).map_err(|e| e.to_string())
}

/// The user's answer to a site's permission request (`browser:permission`).
#[tauri::command]
pub fn browser_permission(app: AppHandle, tab: String, id: u64, allow: bool) -> Result<(), String> {
    let webview = self::tab(&app, &tab)?;
    #[cfg(windows)]
    return win::answer_permission(&webview, id, allow);
    #[cfg(not(windows))]
    {
        let _ = (webview, id, allow);
        Err("not on this platform yet".into())
    }
}

/// Start or stop the element picker: the element under the pointer is
/// outlined and a click picks it (`browser:picked`), Esc gives up.
#[tauri::command]
pub async fn browser_pick(app: AppHandle, tab: String, on: bool) -> Result<(), String> {
    let webview = self::tab(&app, &tab)?;
    #[cfg(windows)]
    return agent::pick(&app, &webview, on).await;
    #[cfg(not(windows))]
    {
        let _ = (app, webview, on);
        Err("not on this platform yet".into())
    }
}

/// Start or stop a tab's stream of frames (`browser:frame`), for recording.
#[tauri::command]
pub async fn browser_screencast(app: AppHandle, tab: String, on: bool) -> Result<(), String> {
    let webview = self::tab(&app, &tab)?;
    #[cfg(windows)]
    return agent::screencast(&app, &webview, on).await;
    #[cfg(not(windows))]
    {
        let _ = (app, webview, on);
        Err("not on this platform yet".into())
    }
}

/// A file the console made (a recording) saved into Downloads under a
/// name of its own; answers where it landed.
#[tauri::command]
pub fn browser_save_file(app: AppHandle, name: String, data: String) -> Result<String, String> {
    use base64::Engine;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(data.as_bytes())
        .map_err(|_| "the file came through damaged".to_string())?;
    let dir = app.path().download_dir().map_err(|e| e.to_string())?;
    // Only a file name: no folders, nothing a name could climb out with.
    let clean: String = name
        .chars()
        .map(|c| {
            if matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') {
                '-'
            } else {
                c
            }
        })
        .collect();
    let clean = clean.trim().trim_start_matches('.').to_string();
    let clean = if clean.is_empty() {
        "download".to_string()
    } else {
        clean
    };
    let (stem, ext) = match clean.rsplit_once('.') {
        Some((stem, ext)) => (stem.to_string(), format!(".{ext}")),
        None => (clean.clone(), String::new()),
    };
    let mut path = dir.join(&clean);
    let mut n = 1;
    while path.exists() {
        path = dir.join(format!("{stem} ({n}){ext}"));
        n += 1;
    }
    std::fs::write(&path, bytes).map_err(|e| e.to_string())?;
    Ok(path.display().to_string())
}

/// Clear the browser's data for every site: "cookies", "cache",
/// "storage" or "all". History lives in the daemon and is cleared there.
#[tauri::command]
pub async fn browser_clear_data(app: AppHandle, kinds: Vec<String>) -> Result<(), String> {
    #[cfg(windows)]
    {
        // Every tab shares the profile, so any page can clear it; with none
        // open, a blank one is made for the moment.
        let open = app
            .state::<Browser>()
            .tabs
            .lock()
            .expect("browser lock")
            .values()
            .find(|t| !t.private)
            .map(|t| t.id.clone());
        let (id, temporary) = match open {
            Some(id) => (id, false),
            None => {
                let maker = app.clone();
                let blank = Url::parse("about:blank").expect("a fixed address");
                let bounds = parked_bounds(&app);
                let id = tauri::async_runtime::spawn_blocking(move || {
                    open_tab(maker, blank, bounds, false, None, false)
                })
                .await
                .map_err(|e| e.to_string())??;
                (id, true)
            }
        };
        let result = win::clear_data(&self::tab(&app, &id)?, &kinds).await;
        if temporary {
            let _ = close_tab(&app, &id);
        }
        result
    }
    #[cfg(not(windows))]
    {
        let _ = (app, kinds);
        Err("not on this platform yet".into())
    }
}

/// A private tab handed to agents, or taken back (agents see private tabs
/// only while they're handed over; such tabs never reach history).
#[tauri::command]
pub fn browser_share(app: AppHandle, tab: String, on: bool) -> Result<(), String> {
    self::tab(&app, &tab)?;
    update(&app, &tab, |t| t.shared = on && t.private);
    Ok(())
}

/// Let tabs nobody has looked at for a while sleep, unless an agent holds
/// them or they're drawn for one.
pub fn start_sweeper(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(60)).await;
            let sleepy: Vec<String> = {
                let browser = app.state::<Browser>();
                let mut tabs = browser.tabs.lock().expect("browser lock");
                tabs.values_mut()
                    .filter(|t| {
                        !t.shown
                            && !t.parked
                            && !t.suspended
                            && t.controller.is_none()
                            && t.hidden_since
                                .is_some_and(|since| since.elapsed() > SLEEP_AFTER)
                    })
                    .map(|t| {
                        t.suspended = true;
                        t.id.clone()
                    })
                    .collect()
            };
            #[cfg(windows)]
            for id in sleepy {
                if let Ok(webview) = self::tab(&app, &id) {
                    let _ = win::suspend(&webview);
                }
            }
            #[cfg(not(windows))]
            let _ = sleepy;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn url(s: &str) -> Url {
        Url::parse(s).unwrap()
    }

    #[test]
    fn tabs_never_load_the_apps_own_addresses() {
        let dev = url("http://127.0.0.1:5175/");
        for app in [
            "http://tauri.localhost/",
            "https://tauri.localhost/index.html",
            "http://ipc.localhost/daemon_info",
            "http://asset.localhost/x",
            "http://tauri.localhost./",
            "http://127.0.0.1:5175/",
            "tauri://localhost/",
            "ipc://localhost/",
            "javascript:alert(1)",
        ] {
            assert!(!allowed(&url(app), Some(&dev)), "{app} should be refused");
        }
    }

    #[test]
    fn the_web_and_the_users_dev_servers_stay_open() {
        let dev = url("http://127.0.0.1:5175/");
        for page in [
            "https://example.com/",
            "http://localhost:5173/",
            "http://myapp.localhost:3000/",
            "http://127.0.0.1:3000/",
            "http://127.0.0.1:5176/",
            "about:blank",
            "file:///C:/site/index.html",
            "data:text/html,hi",
        ] {
            assert!(allowed(&url(page), Some(&dev)), "{page} should open");
        }
        // Without a dev server, its address is just another local port.
        assert!(allowed(&url("http://127.0.0.1:5175/"), None));
    }

    #[test]
    fn device_view_speaks_as_the_engines_android_build() {
        let desktop = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/141.0.0.0 Safari/537.36 Edg/141.0.0.0";
        assert_eq!(
            phone_agent(desktop),
            "Mozilla/5.0 (Linux; Android 10; K) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/141.0.0.0 Mobile Safari/537.36"
        );
        assert_eq!(phone_agent("odd"), "");
    }
}

/// What this build's browser can actually do, so the console offers only controls that
/// work here. Ported from the console-side line: the browser work moved the module, the
/// question did not change.
#[derive(Clone, Copy, Debug, Serialize)]
pub struct Features {
    pub find: bool,
    /// Screenshots and the DevTools protocol: element picking.
    pub cdp: bool,
    /// Device view, which drives the page's emulation over the bridge.
    pub viewport: bool,
    pub clear_site: bool,
    pub print: bool,
    pub devtools: bool,
}

#[tauri::command]
pub fn browser_features() -> Features {
    #[cfg(windows)]
    return Features {
        find: true,
        cdp: true,
        viewport: true,
        clear_site: true,
        print: true,
        devtools: cfg!(debug_assertions),
    };
    #[cfg(not(windows))]
    Features {
        find: false,
        cdp: false,
        viewport: false,
        clear_site: false,
        // Printing is not a Windows-only feature: wry implements it as the
        // system's own dialog on WebKitGTK and WKWebView too, and this crate
        // has the non-Windows arm for it below. Gating it here disabled a
        // control that works.
        print: true,
        devtools: cfg!(debug_assertions),
    }
}

/// Keep the engine's own context menu out of a tab: the console draws the browser's
/// chrome, and a right-click menu from WebView2 would be a second one.
pub fn no_engine_menu(webview: &Webview) {
    #[cfg(windows)]
    win::no_default_menu(webview);
    #[cfg(not(windows))]
    let _ = webview;
}
