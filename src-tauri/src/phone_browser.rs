//! The phone browser's rules that need no phone: what the daemon is told
//! about a tab, and what the phone's browser can do. `mobile_browser.rs`
//! applies them on Android; they live apart so the desktop's test build
//! checks them too.

use openremote_browser::TabInfo;
use serde_json::{json, Value};

/// What one `browser:tab` event from the phone's pages means to the daemon.
#[derive(Debug, PartialEq)]
pub enum News {
    /// A tab agents may see, as they see it. A private tab the user handed
    /// over keeps `private`, so the daemon keeps it out of history.
    Tab(TabInfo),
    /// A tab the daemon was told about and may no longer see (a private tab
    /// taken back from agents): gone, as far as they know.
    Gone(String),
    /// Nothing for the daemon.
    Nothing,
}

/// The daemon's news in one tab event. `reported` is whether the daemon was
/// already told about this tab. Only the tab's own word `agents` lets it
/// through, so a private tab nobody handed over never reaches the daemon.
pub fn agent_news(payload: &Value, reported: bool) -> News {
    let Some(id) = payload.get("id").and_then(Value::as_str) else {
        return News::Nothing;
    };
    let for_agents = payload
        .get("agents")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if for_agents {
        let Ok(mut tab) = serde_json::from_value::<TabInfo>(payload.clone()) else {
            return News::Nothing;
        };
        // Private unless the page said plainly it is not: a missing word must
        // not let a private page's address into history.
        tab.private = payload.get("private").and_then(Value::as_bool) != Some(false);
        return News::Tab(tab);
    }
    if reported {
        News::Gone(id.to_string())
    } else {
        News::Nothing
    }
}

/// What the phone's browser can do, under the desktop's `browser_features`
/// names: find in page and forgetting a site, but no DevTools bridge (so no
/// element picking, region shots or recording), no device view, no printing
/// and no DevTools window.
pub fn features() -> Value {
    json!({
        "find": true,
        "cdp": false,
        "viewport": false,
        "clear_site": true,
        "print": false,
        "devtools": false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tab(extra: Value) -> Value {
        let mut base = json!({
            "id": "tab-1",
            "url": "https://example.com/",
            "title": "Example",
            "loading": false,
            "opener": null,
            "controller": null,
            "favicon": null,
        });
        for (k, v) in extra.as_object().unwrap() {
            base[k] = v.clone();
        }
        base
    }

    #[test]
    fn a_normal_tab_reaches_the_daemon_as_a_normal_tab() {
        let news = agent_news(&tab(json!({ "agents": true, "private": false })), false);
        let News::Tab(info) = news else {
            panic!("expected a tab, got {news:?}")
        };
        assert_eq!(info.id, "tab-1");
        assert_eq!(info.url, "https://example.com/");
        assert!(!info.private);
    }

    #[test]
    fn a_private_tab_nobody_handed_over_never_reaches_the_daemon() {
        let payload = tab(json!({ "agents": false, "private": true }));
        assert_eq!(agent_news(&payload, false), News::Nothing);
        // No word about agents at all is the same as no.
        let silent = tab(json!({ "private": true }));
        assert_eq!(agent_news(&silent, false), News::Nothing);
    }

    #[test]
    fn a_handed_over_private_tab_stays_private_so_history_skips_it() {
        let news = agent_news(&tab(json!({ "agents": true, "private": true })), false);
        let News::Tab(info) = news else {
            panic!("expected a tab, got {news:?}")
        };
        assert!(info.private);
        // A tab that does not say is treated as private too.
        let unsaid = agent_news(&tab(json!({ "agents": true })), false);
        let News::Tab(info) = unsaid else {
            panic!("expected a tab, got {unsaid:?}")
        };
        assert!(info.private);
    }

    #[test]
    fn a_private_tab_taken_back_is_gone_for_agents() {
        let payload = tab(json!({ "agents": false, "private": true }));
        assert_eq!(agent_news(&payload, true), News::Gone("tab-1".into()));
    }

    #[test]
    fn an_event_without_an_id_is_nothing() {
        assert_eq!(agent_news(&json!({ "agents": true }), false), News::Nothing);
    }

    #[test]
    fn the_phone_offers_no_devtools_bridge() {
        let f = features();
        assert_eq!(f["find"], true);
        assert_eq!(f["clear_site"], true);
        assert_eq!(f["cdp"], false);
        assert_eq!(f["viewport"], false);
        assert_eq!(f["print"], false);
        assert_eq!(f["devtools"], false);
    }
}
