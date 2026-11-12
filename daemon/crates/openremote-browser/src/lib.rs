//! What the OpenRemote daemon and a device's built-in browser say to each
//! other.
//!
//! The browser lives in the app (the desktop shell, the Android app), the
//! agents' tools and the browser's own data live in the daemon. On the
//! desktop the two talk over the sidecar's stdio pipes: one JSON message per
//! line, each line starting with [`LINE_PREFIX`], so the pipe can carry
//! other lines too. The daemon sends [`ToHost`] calls; the browser answers
//! each with a [`FromHost::Reply`] and tells the daemon about its tabs as
//! they change.
//!
//! Also here, so every engine reads pages the same way: [`AGENT_SCRIPT`],
//! the page-side helper that turns a page into the text agents read (with a
//! ref for each element they can act on), and [`keys`], Chrome's key names
//! for the DevTools protocol.

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub mod keys;

/// Every browser line on the pipe starts with this.
pub const LINE_PREFIX: &str = "BROWSER ";

/// The page-side helper. Runs in an isolated world of each page (never the
/// world the page's own scripts share), so a page can neither see nor
/// break it, and holds no privilege: it reads and nudges the page as the
/// page's own scripts could.
pub const AGENT_SCRIPT: &str = include_str!("agent.js");

/// The daemon's call to the browser: answered by a reply with the same id.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ToHost {
    pub id: u64,
    pub call: Call,
}

/// What the browser says to the daemon.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum FromHost {
    /// The browser is up (the app started, or the daemon was restarted under
    /// it). The daemon talks to no browser before this, so a daemon started
    /// by hand or as a service knows it has none.
    Hello { engine: String, caps: Caps },
    /// The answer to the call with this id: `ok` or `error`.
    Reply {
        id: u64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        ok: Option<Value>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        error: Option<String>,
    },
    /// A tab opened or changed.
    Tab { tab: TabInfo },
    /// A tab closed.
    Closed { tab: String },
    /// A download started, moved on or finished.
    Download { download: DownloadInfo },
}

/// What a browser can do beyond the basics, so tools that need more say
/// so plainly instead of failing oddly.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Caps {
    /// Clicks and keys arrive as real input (the page can't tell them
    /// from a person's), not as script events.
    #[serde(default)]
    pub trusted_input: bool,
    /// The page's console messages can be read.
    #[serde(default)]
    pub console: bool,
    /// The page's network requests can be read.
    #[serde(default)]
    pub network: bool,
    /// Files can be put into a page's file picker.
    #[serde(default)]
    pub upload: bool,
    /// The browser can block ads and trackers: it answers [`Call::Adblock`].
    /// A browser that never says so is never told about blocking.
    #[serde(default)]
    pub adblock: bool,
}

/// One tab, as the daemon and agents see it.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct TabInfo {
    pub id: String,
    pub url: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub loading: bool,
    /// The tab another tab opened (a popup), if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub opener: Option<String>,
    /// Who is using the tab now, when not just the user.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub controller: Option<Controller>,
    /// A private tab the user handed to agents: it never reaches history.
    /// (Private tabs not handed over never reach the daemon at all.)
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub private: bool,
}

/// Who holds a tab.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Controller {
    /// A chat's agent is using it: the tab shows the chat's name.
    Agent { chat: String, title: String },
    /// The user took it over from that chat's agent: the agent can still
    /// read it but not act until the user hands it back.
    Paused { chat: String, title: String },
}

impl Controller {
    pub fn chat(&self) -> &str {
        match self {
            Controller::Agent { chat, .. } | Controller::Paused { chat, .. } => chat,
        }
    }
}

/// One download.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct DownloadInfo {
    pub id: String,
    #[serde(default)]
    pub tab: String,
    pub url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// `started`, `done`, `failed` or `cancelled`.
    pub state: String,
}

/// One thing the daemon asks of the browser. Tabs are named by the
/// browser's own ids; elements by the refs a snapshot handed out.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Call {
    /// Every open tab.
    Tabs,
    /// Open a tab; `controller` marks it as an agent's from the start.
    Open {
        url: String,
        #[serde(default)]
        controller: Option<Controller>,
    },
    Close {
        tab: String,
    },
    /// Show this tab in the browser view.
    Select {
        tab: String,
    },
    Navigate {
        tab: String,
        url: String,
    },
    Back {
        tab: String,
    },
    Forward {
        tab: String,
    },
    Reload {
        tab: String,
    },
    /// The page as text, each element an agent can act on with a ref.
    Snapshot {
        tab: String,
    },
    Click {
        tab: String,
        target: String,
        #[serde(default)]
        double: bool,
        /// `left` (the default), `right` or `middle`.
        #[serde(default)]
        button: Option<String>,
    },
    Hover {
        tab: String,
        target: String,
    },
    /// Type into an element, after focusing it; `clear` empties it first.
    Type {
        tab: String,
        target: String,
        text: String,
        #[serde(default)]
        clear: bool,
        /// Press Enter after.
        #[serde(default)]
        submit: bool,
    },
    /// Press a key or a chord (`Enter`, `Control+A`) where focus is.
    Press {
        tab: String,
        key: String,
    },
    /// Choose options of a list by value or label.
    SelectOption {
        tab: String,
        target: String,
        values: Vec<String>,
    },
    /// Scroll the page (or the element) by this many pixels.
    Scroll {
        tab: String,
        #[serde(default)]
        target: Option<String>,
        #[serde(default)]
        dx: f64,
        #[serde(default)]
        dy: f64,
    },
    /// A picture of the visible page, the whole page, or one element.
    Screenshot {
        tab: String,
        #[serde(default)]
        full_page: bool,
        #[serde(default)]
        target: Option<String>,
    },
    /// The page's console messages since the last read.
    Console {
        tab: String,
    },
    /// The page's network requests since the last read.
    Network {
        tab: String,
    },
    /// Run script in the page and answer its result.
    Evaluate {
        tab: String,
        script: String,
    },
    /// Wait for text to appear, or to go, or for a time.
    Wait {
        tab: String,
        #[serde(default)]
        text: Option<String>,
        #[serde(default)]
        gone: Option<String>,
        #[serde(default)]
        ms: Option<u64>,
    },
    /// Show the page at a size (a phone's, say); `None` puts it back.
    Viewport {
        tab: String,
        #[serde(default)]
        size: Option<(u32, u32)>,
    },
    /// Answer the page's alert, confirm or prompt.
    Dialog {
        tab: String,
        accept: bool,
        #[serde(default)]
        text: Option<String>,
    },
    /// Put files into a file picker.
    Upload {
        tab: String,
        target: String,
        paths: Vec<String>,
    },
    /// Mark who holds a tab (the badge the tab shows).
    Mark {
        tab: String,
        #[serde(default)]
        controller: Option<Controller>,
    },
    /// Ad and tracker blocking, as the user set it on this computer: sent
    /// when a browser that can block says hello, and whenever any of it
    /// changes. Off means the browser hooks no tab and builds no engine.
    Adblock {
        on: bool,
        /// The filter lists to block with: files on this device the daemon
        /// downloaded from the lists the user picked (EasyList-format text).
        #[serde(default)]
        lists: Vec<String>,
        /// When the lists last changed (ms), so a refresh is noticed even
        /// though the files keep their names.
        #[serde(default)]
        updated: i64,
        /// Sites the user allows ads on (`example.com`): nothing is blocked
        /// on their pages.
        #[serde(default)]
        allow: Vec<String>,
    },
}

impl Call {
    /// The tab a call names, if it names one.
    pub fn tab(&self) -> Option<&str> {
        match self {
            Call::Tabs | Call::Open { .. } | Call::Adblock { .. } => None,
            Call::Close { tab }
            | Call::Select { tab }
            | Call::Navigate { tab, .. }
            | Call::Back { tab }
            | Call::Forward { tab }
            | Call::Reload { tab }
            | Call::Snapshot { tab }
            | Call::Click { tab, .. }
            | Call::Hover { tab, .. }
            | Call::Type { tab, .. }
            | Call::Press { tab, .. }
            | Call::SelectOption { tab, .. }
            | Call::Scroll { tab, .. }
            | Call::Screenshot { tab, .. }
            | Call::Console { tab }
            | Call::Network { tab }
            | Call::Evaluate { tab, .. }
            | Call::Wait { tab, .. }
            | Call::Viewport { tab, .. }
            | Call::Dialog { tab, .. }
            | Call::Upload { tab, .. }
            | Call::Mark { tab, .. } => Some(tab),
        }
    }
}

/// One message as a pipe line (no newline).
pub fn line<T: Serialize>(message: &T) -> String {
    let json = serde_json::to_string(message).expect("browser messages serialize");
    format!("{LINE_PREFIX}{json}")
}

/// A pipe line's message, if the line is a browser line and parses.
pub fn parse<T: for<'de> Deserialize<'de>>(line: &str) -> Option<T> {
    let json = line
        .trim_end_matches(['\r', '\n'])
        .strip_prefix(LINE_PREFIX)?;
    serde_json::from_str(json).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn calls_and_replies_ride_one_line_each() {
        let call = ToHost {
            id: 7,
            call: Call::Click {
                tab: "3".into(),
                target: "e12".into(),
                double: false,
                button: None,
            },
        };
        let text = line(&call);
        assert!(text.starts_with("BROWSER {"));
        assert!(!text.contains('\n'));
        assert_eq!(parse::<ToHost>(&format!("{text}\r\n")), Some(call));

        let reply: FromHost =
            parse(r#"BROWSER {"type":"reply","id":7,"ok":{"text":"done"}}"#).unwrap();
        assert_eq!(
            reply,
            FromHost::Reply {
                id: 7,
                ok: Some(json!({"text": "done"})),
                error: None
            }
        );
        // Other lines on the same pipe are not browser lines.
        assert_eq!(parse::<FromHost>("READY 127.0.0.1:4000"), None);
        assert_eq!(parse::<FromHost>("BROWSER not json"), None);
    }

    #[test]
    fn a_tab_says_who_holds_it() {
        let tab: TabInfo = serde_json::from_value(json!({
            "id": "2",
            "url": "https://example.com/",
            "controller": {"kind": "agent", "chat": "c1", "title": "Fix the login"},
        }))
        .unwrap();
        assert_eq!(tab.controller.as_ref().map(Controller::chat), Some("c1"));
        assert_eq!(
            Call::Mark {
                tab: "2".into(),
                controller: None
            }
            .tab(),
            Some("2")
        );
        assert_eq!(Call::Tabs.tab(), None);
    }

    #[test]
    fn blocking_off_needs_nothing_but_the_word() {
        let off: ToHost = parse(r#"BROWSER {"id":3,"call":{"op":"adblock","on":false}}"#).unwrap();
        assert_eq!(
            off.call,
            Call::Adblock {
                on: false,
                lists: vec![],
                updated: 0,
                allow: vec![]
            }
        );
        assert_eq!(off.call.tab(), None);
    }

    #[test]
    fn a_browser_that_never_mentions_blocking_cannot_block() {
        let caps: Caps = serde_json::from_value(json!({"console": true})).unwrap();
        assert!(!caps.adblock);
        let caps: Caps = serde_json::from_value(json!({"adblock": true})).unwrap();
        assert!(caps.adblock);
    }
}
