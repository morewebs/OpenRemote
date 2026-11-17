//! The tabs open on the user's other devices: Chrome's "Recent tabs".
//!
//! Each device sends its own saved session (the tabs it would bring back at
//! its next start) as a `sync.browser.tabs` note. That session never holds a
//! private tab: the console leaves them out of it (`sessionOf` in
//! `src/browser-tabs.js`), and this note is built from the session alone, so
//! nothing private has a way in. A receiver re-checks every row anyway.
//!
//! This is not the `sync.browser` merge. A session changes with every
//! navigation, and a peer's rows in the op log are never compacted, so the
//! open tabs would grow the log for ever. Each device describes only itself,
//! and only its latest description matters, so a receiver keeps exactly one
//! list per device in `devices.json`, rewritten whole. Which device a list
//! belongs to, and its name, come from the mesh's peer context and never from
//! the note: a device cannot speak for another.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::store::BrowserStore;

/// The note's `type`, as the sync engine matches it.
pub const NOTE_TYPE: &str = "sync.browser.tabs";
/// The note's shape version.
pub const VERSION: u64 = 1;
/// The account's opt-in setting for sharing open tabs across devices.
pub const SETTING: &str = "sync_tabs";

/// The most devices kept: an account has a handful, and more would be a peer
/// inventing them (it cannot, the id is the mesh's, but the file stays small).
pub const MAX_DEVICES: usize = 32;
/// The most tabs one device's list may hold. A sender with more sends its
/// first ones; a note with more is refused.
pub const MAX_TABS: usize = 100;
/// The longest address kept, in bytes.
pub const MAX_URL: usize = 2048;
/// The longest title kept, in characters: a longer one is cut.
pub const MAX_TITLE: usize = 300;
/// A device silent this long drops out of the list (Chrome's is similar).
pub const STALE_MS: i64 = 28 * 24 * 60 * 60 * 1000;

/// One open tab, as another device sees it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct OpenTab {
    pub url: String,
    #[serde(default)]
    pub title: String,
}

/// One device's open tabs as last heard.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DeviceTabs {
    pub device: String,
    pub name: String,
    /// When that device saved this list (its clock).
    pub at: i64,
    /// When this device heard it (this clock): what staleness is judged by.
    pub heard: i64,
    pub tabs: Vec<OpenTab>,
}

/// Why a peer's note was not taken. The file is left as it was.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refused {
    Type,
    Version,
    TooMany,
}

impl Refused {
    pub fn reason(self) -> &'static str {
        match self {
            Self::Type => "not an open-tabs note",
            Self::Version => "another shape version",
            Self::TooMany => "more tabs than one device keeps",
        }
    }
}

/// Whether a peer note is one of these, for the engine's dispatch.
pub fn is_tabs_note(note: &Value) -> bool {
    note.get("type").and_then(Value::as_str) == Some(NOTE_TYPE)
}

/// Whether this device lets its open tabs reach the others.
pub fn sharing(store: &BrowserStore) -> bool {
    store.settings().get(SETTING).and_then(Value::as_bool) == Some(true)
}

/// Only web pages travel: never `file:`, `data:` or the app's own addresses.
fn usable_url(url: &str) -> bool {
    let lower = url.to_ascii_lowercase();
    url.len() <= MAX_URL
        && (lower.starts_with("https://") || lower.starts_with("http://"))
        && !url.chars().any(char::is_control)
}

fn cut(title: &str) -> String {
    title
        .chars()
        .filter(|c| !c.is_control())
        .take(MAX_TITLE)
        .collect()
}

/// The tabs a session value holds, as they may travel: web pages only, at
/// most [`MAX_TABS`]. A session the console wrote has no private tabs; a row
/// that says it is private is dropped all the same.
pub fn session_tabs(session: &Value) -> Vec<OpenTab> {
    session
        .get("tabs")
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter()
                .filter(|row| row.get("private").and_then(Value::as_bool) != Some(true))
                .filter_map(|row| {
                    let url = row.get("url")?.as_str()?;
                    usable_url(url).then(|| OpenTab {
                        url: url.to_string(),
                        title: cut(row.get("title").and_then(Value::as_str).unwrap_or("")),
                    })
                })
                .take(MAX_TABS)
                .collect()
        })
        .unwrap_or_default()
}

/// The note to send: this device's open tabs, or an empty `off` note when
/// the user turned sharing off, so the others drop this device.
pub fn note(store: &BrowserStore) -> Value {
    let on = sharing(store);
    let tabs = if on {
        session_tabs(&store.session())
    } else {
        Vec::new()
    };
    json!({
        "type": NOTE_TYPE,
        "v": VERSION,
        "at": store.session_at(),
        "off": !on,
        "tabs": tabs,
    })
}

/// A cheap summary of what the note would say, so the watcher sends only
/// when it changed.
pub fn fingerprint(store: &BrowserStore) -> u64 {
    let text = note(store).to_string();
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in text.bytes() {
        hash = (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3);
    }
    hash
}

/// The other devices' open tabs, kept in one small file.
pub struct Devices {
    path: Option<PathBuf>,
    list: BTreeMap<String, DeviceTabs>,
}

impl Devices {
    /// Reads `devices.json` in the browser's folder; a missing or broken file
    /// is an empty list.
    pub fn open(dir: &Path) -> Self {
        let path = dir.join("devices.json");
        let list = std::fs::read(&path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Vec<DeviceTabs>>(&bytes).ok())
            .unwrap_or_default()
            .into_iter()
            .take(MAX_DEVICES)
            .map(|d| (d.device.clone(), d))
            .collect();
        Self {
            path: Some(path),
            list,
        }
    }

    /// A list kept in memory only, for tests.
    #[cfg(test)]
    pub fn in_memory() -> Self {
        Self {
            path: None,
            list: BTreeMap::new(),
        }
    }

    /// Takes a peer's note about itself. `device` and `name` are the mesh's
    /// word for the sender. Answers whether the list changed.
    pub fn take(
        &mut self,
        device: &str,
        name: &str,
        note: &Value,
        now: i64,
    ) -> Result<bool, Refused> {
        if !is_tabs_note(note) {
            return Err(Refused::Type);
        }
        if note.get("v").and_then(Value::as_u64) != Some(VERSION) {
            return Err(Refused::Version);
        }
        if note.get("off").and_then(Value::as_bool) == Some(true) {
            let changed = self.list.remove(device).is_some();
            if changed {
                self.save();
            }
            return Ok(changed);
        }
        let rows = note
            .get("tabs")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        if rows.len() > MAX_TABS {
            return Err(Refused::TooMany);
        }
        let at = note.get("at").and_then(Value::as_i64).unwrap_or(0);
        if self.list.get(device).is_some_and(|known| known.at > at) {
            // An older list than the one held: a late repeat.
            return Ok(false);
        }
        let tabs = session_tabs(&json!({ "tabs": rows }));
        if !self.list.contains_key(device) && self.list.len() >= MAX_DEVICES {
            // Make room by the device heard from longest ago.
            if let Some(oldest) = self
                .list
                .values()
                .min_by_key(|d| d.heard)
                .map(|d| d.device.clone())
            {
                self.list.remove(&oldest);
            }
        }
        let entry = DeviceTabs {
            device: device.to_string(),
            name: cut(name),
            at,
            heard: now,
            tabs,
        };
        if self.list.get(device).is_some_and(|known| {
            known.at == entry.at && known.tabs == entry.tabs && known.name == entry.name
        }) {
            return Ok(false);
        }
        self.list.insert(device.to_string(), entry);
        self.save();
        Ok(true)
    }

    /// The devices to show, newest first, without this one and without any
    /// silent past [`STALE_MS`]. Devices with no tabs open are left out.
    pub fn list(&self, me: &str, now: i64) -> Vec<DeviceTabs> {
        let mut out: Vec<DeviceTabs> = self
            .list
            .values()
            .filter(|d| d.device != me && now - d.heard <= STALE_MS && !d.tabs.is_empty())
            .cloned()
            .collect();
        out.sort_by_key(|device| std::cmp::Reverse(device.at));
        out
    }

    /// Writes the file whole, through a temporary file, so a crash leaves the
    /// old list or the new one and never half of one.
    fn save(&self) {
        let Some(path) = &self.path else { return };
        let rows: Vec<&DeviceTabs> = self.list.values().collect();
        let Ok(bytes) = serde_json::to_vec(&rows) else {
            return;
        };
        let tmp = path.with_extension("json.tmp");
        if std::fs::write(&tmp, bytes).is_ok() {
            let _ = std::fs::rename(&tmp, path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tabs_note(at: i64, tabs: Value) -> Value {
        json!({ "type": NOTE_TYPE, "v": 1, "at": at, "off": false, "tabs": tabs })
    }

    #[test]
    fn the_note_carries_the_session_and_never_a_private_tab() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = BrowserStore::open(dir.path());
        store.set_setting(SETTING, json!(true));
        store.save_session(json!({
            "tabs": [
                { "url": "https://example.com/", "title": "Example" },
                { "url": "https://secret.example/", "title": "Secret", "private": true },
                { "url": "file:///etc/passwd", "title": "File" },
                { "url": "data:text/html,hi", "title": "Data" },
            ],
            "active": 0,
        }));
        let note = note(&store);
        assert_eq!(note["type"], NOTE_TYPE);
        assert_eq!(note["off"], false);
        let tabs = note["tabs"].as_array().unwrap();
        assert_eq!(tabs.len(), 1);
        assert_eq!(tabs[0]["url"], "https://example.com/");
        assert!(note["at"].as_i64().unwrap() > 0);
        assert!(!note.to_string().contains("secret"));
    }

    #[test]
    fn sharing_off_sends_an_empty_off_note() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = BrowserStore::open(dir.path());
        store.save_session(json!({ "tabs": [{ "url": "https://example.com/", "title": "E" }] }));
        store.set_setting(SETTING, json!(false));
        let note = note(&store);
        assert_eq!(note["off"], true);
        assert!(note["tabs"].as_array().unwrap().is_empty());
    }

    #[test]
    fn the_fingerprint_moves_with_the_session_and_the_setting() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = BrowserStore::open(dir.path());
        store.set_setting(SETTING, json!(true));
        let empty = fingerprint(&store);
        store.save_session(json!({ "tabs": [{ "url": "https://example.com/", "title": "E" }] }));
        let one = fingerprint(&store);
        assert_ne!(empty, one);
        assert_eq!(one, fingerprint(&store));
        store.set_setting(SETTING, json!(false));
        assert_ne!(one, fingerprint(&store));
    }

    #[test]
    fn a_peer_describes_itself_under_the_meshs_name() {
        let mut devices = Devices::in_memory();
        let note = tabs_note(10, json!([{ "url": "https://a.example/", "title": "A" }]));
        assert_eq!(devices.take("dev-laptop", "Laptop", &note, 1_000), Ok(true));
        let list = devices.list("dev-phone", 1_000);
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].device, "dev-laptop");
        assert_eq!(list[0].name, "Laptop");
        assert_eq!(list[0].tabs[0].url, "https://a.example/");
        // The same note again changes nothing.
        assert_eq!(
            devices.take("dev-laptop", "Laptop", &note, 1_001),
            Ok(false)
        );
    }

    #[test]
    fn an_older_list_never_replaces_a_newer_one() {
        let mut devices = Devices::in_memory();
        devices
            .take(
                "d",
                "D",
                &tabs_note(20, json!([{ "url": "https://new.example/" }])),
                5,
            )
            .unwrap();
        assert_eq!(
            devices.take(
                "d",
                "D",
                &tabs_note(10, json!([{ "url": "https://old.example/" }])),
                6
            ),
            Ok(false)
        );
        assert_eq!(devices.list("me", 6)[0].tabs[0].url, "https://new.example/");
    }

    #[test]
    fn an_off_note_drops_the_device() {
        let mut devices = Devices::in_memory();
        devices
            .take(
                "d",
                "D",
                &tabs_note(1, json!([{ "url": "https://a.example/" }])),
                5,
            )
            .unwrap();
        let off = json!({ "type": NOTE_TYPE, "v": 1, "at": 2, "off": true, "tabs": [] });
        assert_eq!(devices.take("d", "D", &off, 6), Ok(true));
        assert!(devices.list("me", 6).is_empty());
    }

    #[test]
    fn a_peer_cannot_send_private_rows_files_or_too_many_tabs() {
        let mut devices = Devices::in_memory();
        let note = tabs_note(
            1,
            json!([
                { "url": "https://ok.example/", "title": "x".repeat(MAX_TITLE + 50) },
                { "url": "https://private.example/", "private": true },
                { "url": "file:///sdcard/a" },
                { "url": "intent://x#Intent;end" },
                { "url": format!("https://long.example/{}", "a".repeat(MAX_URL)) },
            ]),
        );
        devices.take("d", "D", &note, 1).unwrap();
        let list = devices.list("me", 1);
        assert_eq!(list[0].tabs.len(), 1);
        assert_eq!(list[0].tabs[0].url, "https://ok.example/");
        assert_eq!(list[0].tabs[0].title.chars().count(), MAX_TITLE);

        let many: Vec<Value> = (0..=MAX_TABS)
            .map(|i| json!({ "url": format!("https://e.example/{i}") }))
            .collect();
        assert_eq!(
            devices.take("e", "E", &tabs_note(1, json!(many)), 1),
            Err(Refused::TooMany)
        );
        assert_eq!(
            devices.take("e", "E", &json!({ "type": "sync.browser" }), 1),
            Err(Refused::Type)
        );
        assert_eq!(
            devices.take(
                "e",
                "E",
                &json!({ "type": NOTE_TYPE, "v": 2, "tabs": [] }),
                1
            ),
            Err(Refused::Version)
        );
    }

    #[test]
    fn this_device_stale_devices_and_empty_ones_are_not_listed() {
        let mut devices = Devices::in_memory();
        let one = tabs_note(1, json!([{ "url": "https://a.example/" }]));
        devices.take("me", "Me", &one, 0).unwrap();
        devices.take("old", "Old", &one, 0).unwrap();
        devices
            .take("bare", "Bare", &tabs_note(1, json!([])), STALE_MS)
            .unwrap();
        devices.take("new", "New", &one, STALE_MS).unwrap();
        let names: Vec<String> = devices
            .list("me", STALE_MS + 1)
            .into_iter()
            .map(|d| d.name)
            .collect();
        assert_eq!(names, vec!["New".to_string()]);
    }

    #[test]
    fn the_list_survives_a_restart_and_stays_bounded() {
        let dir = tempfile::tempdir().unwrap();
        {
            let mut devices = Devices::open(dir.path());
            for i in 0..(MAX_DEVICES + 5) {
                let note = tabs_note(1, json!([{ "url": "https://a.example/" }]));
                devices
                    .take(&format!("d{i}"), "D", &note, i as i64)
                    .unwrap();
            }
        }
        let devices = Devices::open(dir.path());
        let list = devices.list("me", MAX_DEVICES as i64 + 5);
        assert_eq!(list.len(), MAX_DEVICES);
        // The ones heard from longest ago made room.
        assert!(!list.iter().any(|d| d.device == "d0"));
    }
}
