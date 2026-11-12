//! Ad and tracker blocking for the browser's tabs: the glue between the
//! daemon's word and the tabs' hooks (`hooks`, which is all WebView2; the
//! decisions are in `rules`).
//!
//! Off, this does nothing at all: no tab is hooked, so no request is ever
//! held up to be looked at, and no filter engine exists. The daemon owns
//! the setting, the lists the user picked, the sites the user allows ads
//! on and the lists' freshness, and tells this module in one call
//! (`configure`) when the browser says hello and whenever any of it
//! changes. It says "on" only when a picked list has been downloaded; no
//! list ships with the app. Private tabs follow the same rule as the rest.
//!
//! Only the browser's tabs are ever hooked. The console's own webview never
//! is (`hookable`), and the daemon's requests are its own process's, which
//! no webview sees; a request to this machine or its network is never
//! looked at either (`rules::may_filter`).

mod hooks;
mod rules;

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, Webview};

use super::Browser;
use hooks::{Hooks, Sink};

/// Where the console hears how many requests a tab's page has had blocked.
const BLOCKED_EVENT: &str = "browser:blocked";

/// Bumped by every `configure`, so a slow build never replaces a newer one.
static ROUND: AtomicU64 = AtomicU64::new(0);

/// Held while blocking is turned on or off, so an engine that finished
/// building can never start after a later "off".
static TURN: Mutex<()> = Mutex::new(());

/// The tabs that are hooked, by the tokens to take their hooks off again.
static HOOKED: Mutex<Option<HashMap<String, Hooks>>> = Mutex::new(None);

fn locked<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Whether a webview may be hooked: the browser's tabs (`tab-N`) only,
/// never the console.
fn hookable(label: &str) -> bool {
    label != super::CONSOLE
        && label
            .strip_prefix("tab-")
            .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
}

/// What the console is told: a tab's page and how much of it was blocked.
#[derive(Clone, Serialize)]
struct Blocked {
    tab: String,
    count: u32,
}

impl Sink for AppHandle {
    fn blocked(&self, tab: &str, count: u32) {
        let _ = self.emit_to(
            super::console(),
            BLOCKED_EVENT,
            Blocked {
                tab: tab.to_string(),
                count,
            },
        );
    }
}

/// The daemon's word on blocking. Off takes every hook off every tab and
/// frees the engine; on builds the engine (off the window's thread) and
/// hooks the tabs that are open, and tabs opened later hook themselves.
pub fn configure(app: &AppHandle, on: bool, lists: Vec<String>, updated: i64, allow: Vec<String>) {
    let round = ROUND.fetch_add(1, Ordering::SeqCst) + 1;
    if !on || lists.is_empty() {
        let _turn = locked(&TURN);
        hooks::stop();
        unhook_all(app);
        return;
    }
    let source = (lists, updated);
    // Allowed sites change without a rebuild.
    if hooks::update(allow.clone(), &source) {
        return;
    }
    let app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let engine = rules::build(&source.0);
        let _turn = locked(&TURN);
        // A newer word came while this built: it is the one to keep.
        if ROUND.load(Ordering::SeqCst) != round {
            return;
        }
        match engine {
            Some(engine) => {
                hooks::start(Arc::new(engine), source, allow);
                hook_all(&app);
            }
            // Nothing readable to block with: plain browsing.
            None => {
                hooks::stop();
                unhook_all(&app);
            }
        }
    });
}

/// Hook every tab that is open now.
fn hook_all(app: &AppHandle) {
    let tabs: Vec<String> = app
        .state::<Browser>()
        .tabs
        .lock()
        .expect("browser lock")
        .keys()
        .cloned()
        .collect();
    for id in tabs {
        if let Some(webview) = app.get_webview(&id) {
            attach(app, &webview);
        }
    }
}

fn unhook_all(app: &AppHandle) {
    let hooked: Vec<(String, Hooks)> = locked(&HOOKED)
        .take()
        .map(|m| m.into_iter().collect())
        .unwrap_or_default();
    for (id, tokens) in hooked {
        if let Some(webview) = app.get_webview(&id) {
            let _ = webview.with_webview(move |platform| {
                // SAFETY: COM calls on the window's thread.
                unsafe { hooks::unhook(&platform.controller(), tokens) }
            });
        }
    }
}

/// A tab is gone: nothing of it is kept.
pub fn forget(tab: &str) {
    if let Some(hooked) = locked(&HOOKED).as_mut() {
        hooked.remove(tab);
    }
    hooks::forget(tab);
}

/// Hook one tab, if blocking is on (a tab just opened, private or not, or
/// one that was open when blocking was turned on). Off, it does nothing.
pub fn attach(app: &AppHandle, webview: &Webview) {
    if !hooks::is_on() || !hookable(webview.label()) {
        return;
    }
    let app = app.clone();
    let tab = webview.label().to_string();
    let _ = webview.with_webview(move |platform| {
        // Blocking may have been turned off while this waited its turn.
        if !hooks::is_on() {
            return;
        }
        // Every closure on the window's thread runs one at a time, so a
        // tab is hooked once however many times it was asked.
        if locked(&HOOKED)
            .get_or_insert_with(HashMap::new)
            .contains_key(&tab)
        {
            return;
        }
        // SAFETY: COM calls on the window's thread; the controller and
        // environment are this tab's own.
        let tokens =
            unsafe { hooks::hook(&app, &tab, &platform.controller(), &platform.environment()) };
        if let Some(tokens) = tokens {
            locked(&HOOKED)
                .get_or_insert_with(HashMap::new)
                .insert(tab, tokens);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_browsers_tabs_are_ever_hooked() {
        assert!(hookable("tab-1"));
        assert!(hookable("tab-42"));
        assert!(!hookable(super::super::CONSOLE));
        assert!(!hookable("main"));
        assert!(!hookable("tab-"));
        assert!(!hookable("tab-1x"));
        assert!(!hookable("signin"));
    }

    #[test]
    fn off_there_is_no_engine_and_nothing_to_hook_with() {
        hooks::stop();
        assert!(!hooks::is_on());
        // Allowed sites arriving while off never turn blocking on.
        assert!(!hooks::update(vec!["a.test".into()], &(vec![], 0)));
        assert!(!hooks::is_on());
    }
}
