//! The built-in browser on Android. Tauri can't add a second webview to a
//! window on Android, so each tab is a native android.webkit.WebView that
//! the app's own Kotlin plugin lays over the console
//! (gen/android/app/src/main/java/space/moreweb/openremote/browser/). This
//! side gives the console the desktop shell's `browser_*` commands and
//! `browser:*` events (src/browser/), so the console drives either the same
//! way, and connects the pages to the daemon running in this process, so an
//! agent can work in the phone's tabs when you chat from the phone.
//!
//! The Kotlin plugin's commands are reached only from here: no capability
//! grants them to a webview, and the pages in the tabs have no IPC at all.
//!
//! The commands keep the desktop's names and argument shapes, so the
//! console's browser store speaks to either shell the same way:
//! `browser_share` hands a private tab to agents, as on the desktop, and the
//! phone's share sheet is `browser_share_sheet`.

use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock, Weak};

use openremote_browser::{Caps, DownloadInfo, FromHost, ToHost, AGENT_SCRIPT};
use openremote_daemon::app::App;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::ipc::{Channel, InvokeResponseBody};
use tauri::plugin::{Builder, PluginHandle, TauriPlugin};
use tauri::{AppHandle, Emitter, EventTarget, Manager, Wry};

use crate::phone_browser::{agent_news, News};

/// The Kotlin plugin.
const PACKAGE: &str = "space.moreweb.openremote.browser";
const CLASS: &str = "BrowserPlugin";

/// The console's webview: the only one that hears the browser's events.
const CONSOLE: &str = "main";

struct Shared {
    handle: PluginHandle<Wry>,
    /// The daemon in this process, once it is up.
    daemon: OnceLock<Weak<App>>,
    /// What the Kotlin side said it is, once it started.
    hello: Mutex<Option<(String, Caps)>>,
    /// The daemon has heard hello: tabs and downloads reach it from now on.
    greeted: AtomicBool,
    /// The tabs the daemon has been told about. Private tabs are never told,
    /// unless the person hands one to agents.
    reported: Mutex<HashSet<String>>,
}

impl Shared {
    fn daemon(&self) -> Option<Arc<App>> {
        self.daemon.get().and_then(Weak::upgrade)
    }
}

/// The browser's state, for its commands.
pub struct PhoneBrowser(Arc<Shared>);

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

pub fn init() -> TauriPlugin<Wry> {
    Builder::new("browser")
        .setup(|app, api| {
            let handle = api.register_android_plugin(PACKAGE, CLASS)?;
            let shared = Arc::new(Shared {
                handle,
                daemon: OnceLock::new(),
                hello: Mutex::new(None),
                greeted: AtomicBool::new(false),
                reported: Mutex::new(HashSet::new()),
            });
            app.manage(PhoneBrowser(Arc::clone(&shared)));
            start(app.clone(), shared);
            Ok(())
        })
        .build()
}

/// Hands the Kotlin side its events channel and the agent helper every page
/// runs, and hears what kind of browser it is.
fn start(app: AppHandle, shared: Arc<Shared>) {
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<Value>();
    // Events arrive on the phone's UI thread: handed on at once, dealt with
    // off it (the daemon writes history to disk).
    let events = Channel::<Value>::new(move |body: InvokeResponseBody| {
        if let InvokeResponseBody::Json(json) = body {
            if let Ok(message) = serde_json::from_str::<Value>(&json) {
                let _ = tx.send(message);
            }
        }
        Ok(())
    });
    let worker = Arc::clone(&shared);
    tauri::async_runtime::spawn(async move {
        while let Some(message) = rx.recv().await {
            deliver(&app, &worker, message);
        }
    });
    tauri::async_runtime::spawn(async move {
        let payload = json!({ "events": events, "agentScript": AGENT_SCRIPT });
        match shared
            .handle
            .run_mobile_plugin_async::<Value>("start", payload)
            .await
        {
            Ok(hello) => {
                let engine = hello
                    .get("engine")
                    .and_then(Value::as_str)
                    .unwrap_or("Android WebView")
                    .to_string();
                let caps = hello
                    .get("caps")
                    .cloned()
                    .and_then(|caps| serde_json::from_value(caps).ok())
                    .unwrap_or_default();
                *lock(&shared.hello) = Some((engine, caps));
                greet(&shared);
            }
            Err(e) => eprintln!("openremote: the browser didn't start: {e}"),
        }
    });
}

/// One event from the pages: to the console under the desktop's name, and
/// to the daemon when agents may know of it.
fn deliver(app: &AppHandle, shared: &Shared, message: Value) {
    let Some(event) = message.get("event").and_then(Value::as_str) else {
        return;
    };
    let payload = message.get("payload").cloned().unwrap_or(Value::Null);
    let target = EventTarget::Webview {
        label: CONSOLE.into(),
    };
    let _ = app.emit_to(target, &format!("browser:{event}"), &payload);
    match event {
        "tab" => report_tab(shared, &payload),
        "closed" => forget_tab(shared, &payload),
        "download" => report_download(shared, &payload),
        _ => {}
    }
}

fn report_tab(shared: &Shared, payload: &Value) {
    if !shared.greeted.load(Ordering::SeqCst) {
        return;
    }
    let Some(daemon) = shared.daemon() else {
        return;
    };
    let Some(id) = payload.get("id").and_then(Value::as_str) else {
        return;
    };
    let reported = lock(&shared.reported).contains(id);
    match agent_news(payload, reported) {
        News::Tab(tab) => {
            lock(&shared.reported).insert(tab.id.clone());
            daemon.browser.receive(FromHost::Tab { tab });
        }
        News::Gone(tab) => {
            lock(&shared.reported).remove(&tab);
            daemon.browser.receive(FromHost::Closed { tab });
        }
        News::Nothing => {}
    }
}

fn forget_tab(shared: &Shared, payload: &Value) {
    let Some(id) = payload.get("id").and_then(Value::as_str) else {
        return;
    };
    if lock(&shared.reported).remove(id) {
        if let Some(daemon) = shared.daemon() {
            daemon.browser.receive(FromHost::Closed {
                tab: id.to_string(),
            });
        }
    }
}

fn report_download(shared: &Shared, payload: &Value) {
    if !shared.greeted.load(Ordering::SeqCst)
        || !payload
            .get("agents")
            .and_then(Value::as_bool)
            .unwrap_or(false)
    {
        return;
    }
    let Some(daemon) = shared.daemon() else {
        return;
    };
    let id = match payload.get("id") {
        Some(Value::String(id)) => id.clone(),
        Some(other) => other.to_string(),
        None => return,
    };
    let info = json!({
        "id": id,
        "tab": payload.get("tab"),
        "url": payload.get("url"),
        "path": payload.get("path"),
        "state": payload.get("state"),
    });
    if let Ok(download) = serde_json::from_value::<DownloadInfo>(info) {
        daemon.browser.receive(FromHost::Download { download });
    }
}

/// Once both are up: the daemon hears the browser's hello, then the tabs
/// that are already open.
fn greet(shared: &Arc<Shared>) {
    let Some(daemon) = shared.daemon() else {
        return;
    };
    let Some((engine, caps)) = lock(&shared.hello).clone() else {
        return;
    };
    if shared.greeted.swap(true, Ordering::SeqCst) {
        return;
    }
    daemon.browser.receive(FromHost::Hello { engine, caps });
    let shared = Arc::clone(shared);
    tauri::async_runtime::spawn(async move {
        if let Ok(answer) = shared
            .handle
            .run_mobile_plugin_async::<Value>("tabs", json!({}))
            .await
        {
            for tab in answer
                .get("tabs")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                report_tab(&shared, tab);
            }
        }
    });
}

/// The daemon in this process is up: it hosts the phone's browser, and each
/// of its calls (an agent's) runs on the pages and answers back.
pub fn attach_daemon(app: &AppHandle, daemon: &Arc<App>) {
    let Some(state) = app.try_state::<PhoneBrowser>() else {
        return;
    };
    let shared = Arc::clone(&state.0);
    daemon.browser.set_hosted();
    let sender = Arc::clone(&shared);
    let weak = Arc::downgrade(daemon);
    daemon.browser.connect(Arc::new(move |message: ToHost| {
        let shared = Arc::clone(&sender);
        let weak = weak.clone();
        tauri::async_runtime::spawn(async move {
            let ToHost { id, call } = message;
            let answer = shared
                .handle
                .run_mobile_plugin_async::<Value>("agent", json!({ "id": id, "call": call }))
                .await;
            let reply = match answer {
                Ok(mut answer) => FromHost::Reply {
                    id,
                    ok: Some(answer.get_mut("ok").map(Value::take).unwrap_or(Value::Null)),
                    error: None,
                },
                Err(e) => FromHost::Reply {
                    id,
                    ok: None,
                    error: Some(e.to_string()),
                },
            };
            if let Some(daemon) = weak.upgrade() {
                daemon.browser.receive(reply);
            }
        });
    }));
    let _ = shared.daemon.set(Arc::downgrade(daemon));
    greet(&shared);
}

async fn ask(app: &AppHandle, command: &str, payload: Value) -> Result<Value, String> {
    let shared = app
        .try_state::<PhoneBrowser>()
        .map(|state| Arc::clone(&state.0))
        .ok_or("the browser isn't ready yet")?;
    shared
        .handle
        .run_mobile_plugin_async::<Value>(command, payload)
        .await
        .map_err(|e| e.to_string())
}

/// Where the console's page area is, in CSS pixels: the desktop's bounds,
/// and what only the phone needs.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Bounds {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    /// How far the page may slide up under the console's bar as it scrolls.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bar: Option<f64>,
    /// The console's devicePixelRatio.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scale: Option<f64>,
    /// Reduced motion: the bar snaps instead of easing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub still: Option<bool>,
    /// Whether Android's back goes to the page's history (false while one of
    /// the console's own strips, like find in page, needs it).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub back: Option<bool>,
}

/// A part of the page, in CSS pixels.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Clip {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

#[tauri::command]
pub async fn browser_open(
    app: AppHandle,
    url: String,
    bounds: Bounds,
    show: bool,
    private: Option<bool>,
) -> Result<String, String> {
    let payload = json!({
        "url": url,
        "bounds": bounds,
        "show": show,
        "private": private.unwrap_or(false),
    });
    let opened = ask(&app, "open", payload).await?;
    opened
        .get("id")
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| "the browser answered oddly".to_string())
}

/// The tabs the browser holds, oldest first: a console that reloaded takes
/// them back.
#[tauri::command]
pub async fn browser_tabs(app: AppHandle) -> Result<Vec<Value>, String> {
    let answer = ask(&app, "tabs", json!({})).await?;
    Ok(answer
        .get("tabs")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default())
}

#[tauri::command]
pub async fn browser_close(app: AppHandle, tab: String) -> Result<(), String> {
    ask(&app, "close", json!({ "tab": tab })).await.map(drop)
}

#[tauri::command]
pub async fn browser_set_bounds(app: AppHandle, tab: String, bounds: Bounds) -> Result<(), String> {
    ask(&app, "setBounds", json!({ "tab": tab, "bounds": bounds }))
        .await
        .map(drop)
}

#[tauri::command]
pub async fn browser_show(app: AppHandle, tab: String, focus: Option<bool>) -> Result<(), String> {
    let payload = json!({ "tab": tab, "focus": focus.unwrap_or(false) });
    ask(&app, "show", payload).await.map(drop)
}

#[tauri::command]
pub async fn browser_hide(app: AppHandle, tab: String) -> Result<(), String> {
    ask(&app, "hide", json!({ "tab": tab })).await.map(drop)
}

#[tauri::command]
pub async fn browser_navigate(app: AppHandle, tab: String, url: String) -> Result<(), String> {
    ask(&app, "navigate", json!({ "tab": tab, "url": url }))
        .await
        .map(drop)
}

#[tauri::command]
pub async fn browser_back(app: AppHandle, tab: String) -> Result<(), String> {
    ask(&app, "back", json!({ "tab": tab })).await.map(drop)
}

#[tauri::command]
pub async fn browser_forward(app: AppHandle, tab: String) -> Result<(), String> {
    ask(&app, "forward", json!({ "tab": tab })).await.map(drop)
}

#[tauri::command]
pub async fn browser_reload(app: AppHandle, tab: String) -> Result<(), String> {
    ask(&app, "reload", json!({ "tab": tab })).await.map(drop)
}

#[tauri::command]
pub async fn browser_stop(app: AppHandle, tab: String) -> Result<(), String> {
    ask(&app, "stop", json!({ "tab": tab })).await.map(drop)
}

/// A picture of the tab as a JPEG data URL: all of it or `clip`, at most
/// `max` CSS pixels wide (the tab grid's thumbnails).
#[tauri::command]
pub async fn browser_capture(
    app: AppHandle,
    tab: String,
    clip: Option<Clip>,
    max: Option<f64>,
) -> Result<String, String> {
    let answer = ask(
        &app,
        "capture",
        json!({ "tab": tab, "clip": clip, "max": max }),
    )
    .await?;
    answer
        .get("data")
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| "no picture came back".to_string())
}

/// The page's zoom, 1.0 being 100% (text size on the phone, as in Chrome).
#[tauri::command]
pub async fn browser_zoom(app: AppHandle, tab: String, factor: f64) -> Result<(), String> {
    ask(&app, "zoom", json!({ "tab": tab, "factor": factor }))
        .await
        .map(drop)
}

/// Find in page: the matches arrive as `browser:find` events; an empty term
/// ends the search.
#[tauri::command]
pub async fn browser_find(
    app: AppHandle,
    tab: String,
    term: String,
    step: i32,
) -> Result<(), String> {
    ask(
        &app,
        "find",
        json!({ "tab": tab, "term": term, "step": step }),
    )
    .await
    .map(drop)
}

/// The answer to a `browser:permission` request.
#[tauri::command]
pub async fn browser_permission(
    app: AppHandle,
    tab: String,
    id: Value,
    allow: bool,
) -> Result<(), String> {
    ask(
        &app,
        "permission",
        json!({ "tab": tab, "id": id, "allow": allow }),
    )
    .await
    .map(drop)
}

/// Forgets the current site: its cookies and storage.
#[tauri::command]
pub async fn browser_clear_site(app: AppHandle, tab: String) -> Result<(), String> {
    ask(&app, "clearSite", json!({ "tab": tab }))
        .await
        .map(drop)
}

/// Clears what the browser itself keeps, by kind: "cookies", "site_data",
/// "cache", "permissions" or "all" (history and bookmarks are the daemon's).
#[tauri::command]
pub async fn browser_clear_data(app: AppHandle, kinds: Vec<String>) -> Result<(), String> {
    ask(&app, "clearData", json!({ "kinds": kinds }))
        .await
        .map(drop)
}

/// The page in the phone's own browser.
#[tauri::command]
pub async fn browser_open_external(app: AppHandle, url: String) -> Result<(), String> {
    ask(&app, "openExternal", json!({ "url": url }))
        .await
        .map(drop)
}

/// What this browser can do, under the desktop's names, so the console
/// draws only controls that work here.
#[tauri::command]
pub fn browser_features() -> Value {
    crate::phone_browser::features()
}

/// The phone only: the share sheet with a page's address (`url`, or the
/// tab's).
#[tauri::command]
pub async fn browser_share_sheet(
    app: AppHandle,
    tab: Option<String>,
    url: Option<String>,
    title: Option<String>,
) -> Result<(), String> {
    ask(
        &app,
        "share",
        json!({ "tab": tab, "url": url, "title": title }),
    )
    .await
    .map(drop)
}

/// The console's own file (a data URL or base64) into Downloads; answers
/// where it landed.
#[tauri::command]
pub async fn browser_save_file(
    app: AppHandle,
    name: String,
    data: String,
) -> Result<String, String> {
    let answer = ask(&app, "saveFile", json!({ "name": name, "data": data })).await?;
    answer
        .get("path")
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| "the file wasn't saved".to_string())
}

/// Opens a finished download.
#[tauri::command]
pub async fn browser_reveal(app: AppHandle, path: String) -> Result<(), String> {
    ask(&app, "reveal", json!({ "path": path })).await.map(drop)
}

/// The phone only: asks sites for their desktop pages, or the phone's again.
#[tauri::command]
pub async fn browser_desktop_site(app: AppHandle, tab: String, on: bool) -> Result<(), String> {
    ask(&app, "desktopSite", json!({ "tab": tab, "on": on }))
        .await
        .map(drop)
}

/// Lets agents use a private tab, or takes it back: the desktop's
/// `browser_share` and its arguments.
#[tauri::command]
pub async fn browser_share(app: AppHandle, tab: String, on: bool) -> Result<(), String> {
    ask(&app, "shareWithAgents", json!({ "tab": tab, "shared": on }))
        .await
        .map(drop)
}

/// The phone only: downloads a link or picture chosen from its menu.
#[tauri::command]
pub async fn browser_download(app: AppHandle, tab: String, url: String) -> Result<(), String> {
    ask(&app, "download", json!({ "tab": tab, "url": url }))
        .await
        .map(drop)
}
