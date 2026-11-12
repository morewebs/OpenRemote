//! The browser's link to the daemon, over the sidecar's own pipes: the
//! daemon's calls arrive as `BROWSER {json}` lines on its stdout (beside
//! READY and DATA_DIR), and the shell writes its answers and its tabs'
//! news to the daemon's stdin the same way. The daemon serves the agents'
//! browser tools through this; the console never sees it.

use openremote_browser::{Call, Caps, Controller, DownloadInfo, FromHost, TabInfo, ToHost};
use tauri::{AppHandle, Manager};

use super::{Browser, Tab};

/// Whether this tab's news reaches the daemon: a private tab only once the
/// user hands it to agents (and then it still never reaches history).
fn shared(tab: &Tab) -> bool {
    !tab.private || tab.shared
}

pub fn info(tab: &Tab) -> TabInfo {
    TabInfo {
        id: tab.id.clone(),
        url: tab.url.clone(),
        title: tab.title.clone(),
        loading: tab.loading,
        opener: tab.opener.clone(),
        controller: tab.controller.clone(),
        private: tab.private,
    }
}

fn send(app: &AppHandle, message: &FromHost) {
    crate::desktop::write_to_daemon(app, &openremote_browser::line(message));
}

/// A daemon started (or restarted): tell it what this browser can do and
/// which tabs are open.
pub fn hello(app: &AppHandle) {
    let caps = Caps {
        trusted_input: cfg!(windows),
        console: cfg!(windows),
        network: cfg!(windows),
        upload: cfg!(windows),
        adblock: cfg!(windows),
    };
    let engine = if cfg!(windows) {
        "webview2"
    } else if cfg!(target_os = "macos") {
        "wkwebview"
    } else {
        "webkitgtk"
    };
    send(
        app,
        &FromHost::Hello {
            engine: engine.into(),
            caps,
        },
    );
    let tabs: Vec<Tab> = app
        .state::<Browser>()
        .tabs
        .lock()
        .expect("browser lock")
        .values()
        .cloned()
        .collect();
    for tab in tabs.iter().filter(|t| shared(t)) {
        send(app, &FromHost::Tab { tab: info(tab) });
    }
}

pub fn tab_changed(app: &AppHandle, tab: &Tab) {
    if shared(tab) {
        send(app, &FromHost::Tab { tab: info(tab) });
    } else {
        // A private tab taken back from agents: gone, as far as they know.
        send(
            app,
            &FromHost::Closed {
                tab: tab.id.clone(),
            },
        );
    }
}

pub fn closed(app: &AppHandle, id: &str) {
    send(app, &FromHost::Closed { tab: id.into() });
}

pub fn download(app: &AppHandle, download: &super::Download) {
    send(
        app,
        &FromHost::Download {
            download: DownloadInfo {
                id: download.id.to_string(),
                tab: download.tab.clone(),
                url: download.url.clone(),
                path: download.path.clone(),
                state: download.state.to_string(),
            },
        },
    );
}

/// One line from the daemon's stdout: a browser call is run and answered;
/// anything else is not ours.
pub fn receive(app: &AppHandle, line: &str) -> bool {
    let Some(ToHost { id, call }) = openremote_browser::parse::<ToHost>(line) else {
        return false;
    };
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        // Marks need no answer (the daemon doesn't wait on them), but a
        // reply costs nothing and keeps every call alike.
        let reply = match super::agent::run(&app, call).await {
            Ok(ok) => FromHost::Reply {
                id,
                ok: Some(ok),
                error: None,
            },
            Err(error) => FromHost::Reply {
                id,
                ok: None,
                error: Some(error),
            },
        };
        send(&app, &reply);
    });
    true
}

/// Who holds a tab, for the tab strip's badge.
pub fn mark(app: &AppHandle, tab: &str, controller: Option<Controller>) {
    super::update(app, tab, |t| t.controller = controller);
}

/// The calls that are only bookkeeping, answered without touching a page.
pub fn bookkeeping(app: &AppHandle, call: &Call) -> Option<Result<serde_json::Value, String>> {
    match call {
        Call::Mark { tab, controller } => {
            mark(app, tab, controller.clone());
            Some(Ok(serde_json::Value::Null))
        }
        Call::Adblock {
            on,
            lists,
            updated,
            allow,
        } => {
            #[cfg(windows)]
            super::adblock::configure(app, *on, lists.clone(), *updated, allow.clone());
            #[cfg(not(windows))]
            let _ = (on, lists, updated, allow);
            Some(Ok(serde_json::Value::Null))
        }
        Call::Tabs => {
            let browser = app.state::<Browser>();
            let tabs = browser.tabs.lock().expect("browser lock");
            let mut list: Vec<TabInfo> = tabs.values().filter(|t| shared(t)).map(info).collect();
            list.sort_by_key(|t| t.id.trim_start_matches("tab-").parse::<u64>().unwrap_or(0));
            Some(Ok(serde_json::to_value(list).unwrap_or_default()))
        }
        _ => None,
    }
}
