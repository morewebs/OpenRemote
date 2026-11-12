//! The browser's own data on this device: bookmarks, history, settings,
//! site permissions, the tiles of the new tab page and the tabs to bring
//! back at the next start.
//!
//! Kept as an append-only log of operations under `<data_dir>/browser/`,
//! one file per device, each operation stamped with a hybrid logical clock.
//! The value of a key is the one with the latest stamp (deleting writes an
//! empty value), so logs from several devices can be merged in any order and
//! agree. Today only this device writes; syncing the profile between devices
//! later is a matter of exchanging these operations.

use std::collections::HashMap;
use std::fs::OpenOptions;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

/// History older than this is dropped when the log is compacted (Chrome
/// keeps 90 days too).
pub const HISTORY_DAYS: i64 = 90;
/// The log is rewritten with only what is still true once it holds this
/// many superseded operations.
const COMPACT_AFTER: usize = 20_000;

/// Ceilings on the two collections a peer may write through a sync note. The
/// console writes tens of settings, and a person's bookmark tree is in the
/// hundreds to low thousands, so a real profile stays far below both and the
/// numbers are a trust boundary rather than a product limit. At a ceiling a new
/// key is refused and the rows already held stay (see [`BrowserStore::merge_row`]).
pub const CAP_SETTING: usize = 200;
pub const CAP_BOOKMARK: usize = 5_000;

/// The most bookmarks one import from another browser may add. Bookmarks
/// follow the account, so an import is capped well below [`CAP_BOOKMARK`]:
/// one browser's ten-year pile must not fill every device's tree, and the
/// user keeps room to add their own.
pub const IMPORT_BOOKMARKS: usize = 2_000;
/// An import never takes the bookmark collection above `CAP_BOOKMARK` minus
/// this many keys, whatever is already held, so a second or third import
/// stops short of the sync ceiling too.
pub const IMPORT_ROOM: usize = 500;
/// The most visits one import adds. History stays on this device (the sync
/// note never carries it), and only the last [`HISTORY_DAYS`] are kept.
pub const IMPORT_VISITS: usize = 20_000;
const _: () = assert!(IMPORT_BOOKMARKS + IMPORT_ROOM < CAP_BOOKMARK);

/// What one import added.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct Imported {
    /// Bookmarks new to this store.
    pub bookmarks: usize,
    /// Bookmarks left out because the page is already bookmarked here (or
    /// the import holds it twice).
    pub already: usize,
    /// Bookmarks left out because the import reached its cap.
    pub over_cap: usize,
    /// Visits added to this device's history.
    pub visits: usize,
}

/// A hybrid logical clock stamp: wall-clock milliseconds, a counter for
/// operations within one millisecond, and the device as the tie-break.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Stamp {
    pub ms: i64,
    pub n: u32,
    pub dev: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct Op {
    at: Stamp,
    col: String,
    key: String,
    #[serde(default)]
    val: Value,
}

/// One page in history.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Visit {
    pub id: String,
    pub url: String,
    #[serde(default)]
    pub title: String,
    pub at: i64,
}

/// One bookmark.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Bookmark {
    /// Empty for a new bookmark: the store gives it one.
    #[serde(default)]
    pub id: String,
    pub url: String,
    #[serde(default)]
    pub title: String,
    /// Where it sits: `bar` (the bookmarks bar) or `other`.
    #[serde(default = "bar")]
    pub folder: String,
    #[serde(default)]
    pub position: f64,
    #[serde(default)]
    pub added: i64,
}

fn bar() -> String {
    "bar".into()
}

/// A suggestion for the address bar.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Suggestion {
    pub url: String,
    pub title: String,
    pub bookmarked: bool,
    pub visits: usize,
}

/// A tile on the new tab page.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Tile {
    pub url: String,
    pub title: String,
    /// The user added it themselves (it stays put).
    pub pinned: bool,
}

pub struct BrowserStore {
    dir: PathBuf,
    device: String,
    last: Stamp,
    /// (collection, key) -> the latest operation's stamp and value.
    state: HashMap<(String, String), (Stamp, Value)>,
    ops_written: usize,
}

impl BrowserStore {
    /// Opens (or starts) the browser's data in `<data_dir>/browser/`.
    pub fn open(data_dir: &Path) -> Self {
        let dir = data_dir.join("browser");
        let _ = std::fs::create_dir_all(&dir);
        sweep_import_scratch(&dir);
        let device = device_id(&dir);
        let mut store = Self {
            dir,
            last: Stamp {
                ms: 0,
                n: 0,
                dev: device.clone(),
            },
            device,
            state: HashMap::new(),
            ops_written: 0,
        };
        store.load();
        store
    }

    fn log_path(&self, device: &str) -> PathBuf {
        self.dir.join(format!("ops-{device}.jsonl"))
    }

    fn load(&mut self) {
        let Ok(entries) = std::fs::read_dir(&self.dir) else {
            return;
        };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if !(name.starts_with("ops-") && name.ends_with(".jsonl")) {
                continue;
            }
            let Ok(file) = std::fs::File::open(entry.path()) else {
                continue;
            };
            for line in BufReader::new(file).lines().map_while(Result::ok) {
                // A line cut short by a crash is skipped, not fatal.
                if let Ok(op) = serde_json::from_str::<Op>(&line) {
                    if op.at.dev == self.device {
                        self.ops_written += 1;
                    }
                    self.apply(op);
                }
            }
        }
    }

    fn apply(&mut self, op: Op) {
        if op.at > self.last {
            self.last = Stamp {
                ms: op.at.ms,
                n: op.at.n,
                dev: self.device.clone(),
            };
        }
        let slot = (op.col, op.key);
        match self.state.get(&slot) {
            Some((at, _)) if *at >= op.at => {}
            _ => {
                self.state.insert(slot, (op.at, op.val));
            }
        }
    }

    fn stamp(&mut self, now: i64) -> Stamp {
        let stamp = if now > self.last.ms {
            Stamp {
                ms: now,
                n: 0,
                dev: self.device.clone(),
            }
        } else {
            Stamp {
                ms: self.last.ms,
                n: self.last.n + 1,
                dev: self.device.clone(),
            }
        };
        self.last = stamp.clone();
        stamp
    }

    /// Writes one key (an empty value deletes it).
    pub fn put(&mut self, col: &str, key: &str, val: Value) {
        let op = Op {
            at: self.stamp(openremote_core::now_ms()),
            col: col.into(),
            key: key.into(),
            val,
        };
        let line = serde_json::to_string(&op).expect("an op serializes");
        let path = self.log_path(&self.device);
        if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(&path) {
            let _ = writeln!(file, "{line}");
        }
        self.ops_written += 1;
        self.apply(op);
        if self.ops_written > self.live_count() + COMPACT_AFTER {
            self.compact();
        }
    }

    fn live_count(&self) -> usize {
        self.state.len()
    }

    pub fn get(&self, col: &str, key: &str) -> Option<&Value> {
        self.state
            .get(&(col.to_string(), key.to_string()))
            .map(|(_, v)| v)
            .filter(|v| !v.is_null())
    }

    fn all<'a>(&'a self, col: &'a str) -> impl Iterator<Item = (&'a str, &'a Value)> + 'a {
        self.state
            .iter()
            .filter(move |((c, _), (_, v))| c == col && !v.is_null())
            .map(|((_, k), (_, v))| (k.as_str(), v))
    }

    /// Every row of a collection with the stamp that made it, deletions
    /// included: this is what a note to another device carries (see
    /// [`super::sync`]), so a key removed here stays removed there instead of
    /// being reinvented from the peer's older copy.
    pub fn rows(&self, col: &str) -> Vec<(String, Stamp, Value)> {
        let mut rows: Vec<(String, Stamp, Value)> = self
            .state
            .iter()
            .filter(|((c, _), _)| c == col)
            .map(|((_, k), (at, v))| (k.clone(), at.clone(), v.clone()))
            .collect();
        // Ordered by key: a snapshot that goes over the wire the same way
        // twice is the same bytes, which makes a note easy to compare.
        rows.sort_by(|a, b| a.0.cmp(&b.0));
        rows
    }

    /// Takes one row from another device if it is newer than the local one,
    /// and keeps it in that device's log so it survives this process.
    ///
    /// The row is written where [`Self::load`] already looks for it, under the
    /// device that stamped it, never into this device's own log: compaction
    /// rewrites only its own file, and a foreign stamp in there would be
    /// dropped on the next rewrite. Equal stamps mean the same write, so the
    /// local copy stays and nothing is appended.
    ///
    /// A key this store does not hold may not grow a collection past its
    /// ceiling ([`CAP_SETTING`], [`CAP_BOOKMARK`]): a new key is refused and
    /// the rows already held stay. Dropping the oldest by stamp would let a
    /// peer evict a user's own bookmark by inventing a newer one, and the
    /// collections are small enough that refusing is the honest door. An update
    /// to a key already held always lands; the local writer (`put`) is the
    /// user's own hand and is not capped here.
    pub fn merge_row(&mut self, col: &str, key: &str, at: &Stamp, val: &Value) -> bool {
        let slot = (col.to_string(), key.to_string());
        if self.state.get(&slot).is_some_and(|(local, _)| local >= at) {
            return false;
        }
        if !self.state.contains_key(&slot) && self.at_cap(col) {
            return false;
        }
        let op = Op {
            at: at.clone(),
            col: col.into(),
            key: key.into(),
            val: val.clone(),
        };
        let line = serde_json::to_string(&op).expect("an op serializes");
        let device = at.dev.clone();
        let path = self.log_path(&device);
        if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(&path) {
            let _ = writeln!(file, "{line}");
        }
        self.apply(op);
        true
    }

    /// Whether a collection holds its ceiling. The whole state is checked
    /// first: a collection is never larger than it, so the count only has to be
    /// walked once the store is already big.
    fn at_cap(&self, col: &str) -> bool {
        let cap = if col == "bookmark" {
            CAP_BOOKMARK
        } else {
            CAP_SETTING
        };
        self.state.len() >= cap && self.state.keys().filter(|(c, _)| c == col).count() >= cap
    }

    /// Rewrites this device's log with only what is still true, and drops
    /// history past its keeping time.
    fn compact(&mut self) {
        let horizon = openremote_core::now_ms() - HISTORY_DAYS * 24 * 3600 * 1000;
        self.state.retain(|(col, _), (_, val)| {
            !(col == "visit" && val.get("at").and_then(Value::as_i64).unwrap_or(0) < horizon)
        });
        let mut lines = String::new();
        let mut kept = 0;
        for ((col, key), (at, val)) in &self.state {
            if at.dev != self.device {
                continue;
            }
            let op = Op {
                at: at.clone(),
                col: col.clone(),
                key: key.clone(),
                val: val.clone(),
            };
            lines.push_str(&serde_json::to_string(&op).expect("an op serializes"));
            lines.push('\n');
            kept += 1;
        }
        let path = self.log_path(&self.device);
        let fresh = path.with_extension("jsonl.new");
        if std::fs::write(&fresh, lines).is_ok() && std::fs::rename(&fresh, &path).is_ok() {
            self.ops_written = kept;
        }
    }

    // ---- history ----

    /// Records a visit; answers its id, so a title that arrives later can
    /// land on the same entry.
    pub fn visit(&mut self, url: &str, title: &str, at: i64) -> String {
        let id = uuid::Uuid::new_v4().to_string();
        self.put("visit", &id, json!({"url": url, "title": title, "at": at}));
        id
    }

    pub fn retitle_visit(&mut self, id: &str, title: &str) {
        if let Some(mut visit) = self.get("visit", id).cloned() {
            if visit["title"] != title {
                visit["title"] = json!(title);
                self.put("visit", id, visit);
            }
        }
    }

    fn visits(&self) -> Vec<Visit> {
        self.all("visit")
            .filter_map(|(id, v)| {
                Some(Visit {
                    id: id.to_string(),
                    url: v.get("url")?.as_str()?.to_string(),
                    title: v
                        .get("title")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string(),
                    at: v.get("at").and_then(Value::as_i64).unwrap_or(0),
                })
            })
            .collect()
    }

    /// History, newest first: matching `query` (in the address or title),
    /// older than `before` when given.
    pub fn history(&self, query: &str, before: Option<i64>, limit: usize) -> Vec<Visit> {
        let query = query.trim().to_lowercase();
        let mut visits: Vec<Visit> = self
            .visits()
            .into_iter()
            .filter(|v| before.is_none_or(|b| v.at < b))
            .filter(|v| {
                query.is_empty()
                    || v.url.to_lowercase().contains(&query)
                    || v.title.to_lowercase().contains(&query)
            })
            .collect();
        visits.sort_by(|a, b| b.at.cmp(&a.at).then_with(|| a.id.cmp(&b.id)));
        visits.truncate(limit);
        visits
    }

    pub fn forget_visits(&mut self, ids: &[String]) {
        for id in ids {
            self.put("visit", id, Value::Null);
        }
    }

    /// Clears history from `since` on (all of it when None).
    pub fn clear_history(&mut self, since: Option<i64>) {
        let doomed: Vec<String> = self
            .visits()
            .into_iter()
            .filter(|v| since.is_none_or(|s| v.at >= s))
            .map(|v| v.id)
            .collect();
        self.forget_visits(&doomed);
    }

    /// What the address bar offers for what's typed: pages visited and
    /// bookmarked whose address or title match, the most used first.
    pub fn suggest(&self, typed: &str, limit: usize) -> Vec<Suggestion> {
        let typed = typed.trim().to_lowercase();
        if typed.is_empty() {
            return Vec::new();
        }
        let mut pages: HashMap<String, (String, usize, i64, bool)> = HashMap::new();
        for visit in self.visits() {
            let page = pages
                .entry(visit.url.clone())
                .or_insert((String::new(), 0, 0, false));
            page.1 += 1;
            if visit.at >= page.2 {
                page.2 = visit.at;
                if !visit.title.is_empty() {
                    page.0 = visit.title;
                }
            }
        }
        for mark in self.bookmarks() {
            let page = pages
                .entry(mark.url.clone())
                .or_insert((String::new(), 0, 0, false));
            page.3 = true;
            if page.0.is_empty() {
                page.0 = mark.title;
            }
        }
        let mut hits: Vec<(i64, Suggestion)> = pages
            .into_iter()
            .filter_map(|(url, (title, visits, last, bookmarked))| {
                let bare = bare_address(&url);
                let score = if bare.starts_with(&typed) {
                    3
                } else if bare
                    .split(['/', '.', '?', '-'])
                    .any(|part| part.starts_with(&typed))
                    || title
                        .to_lowercase()
                        .split_whitespace()
                        .any(|w| w.starts_with(&typed))
                {
                    2
                } else if url.to_lowercase().contains(&typed)
                    || title.to_lowercase().contains(&typed)
                {
                    1
                } else {
                    return None;
                };
                let rank = score * 1_000_000
                    + (visits as i64).min(999) * 1000
                    + i64::from(bookmarked) * 500
                    + (last / 86_400_000) % 1000;
                Some((
                    rank,
                    Suggestion {
                        url,
                        title,
                        bookmarked,
                        visits,
                    },
                ))
            })
            .collect();
        hits.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.url.cmp(&b.1.url)));
        hits.into_iter().take(limit).map(|(_, s)| s).collect()
    }

    /// The new tab page's tiles: the user's own first, then the most
    /// visited pages of the last 30 days, without the ones removed.
    pub fn tiles(&self, limit: usize) -> Vec<Tile> {
        let mut tiles: Vec<(f64, Tile)> = self
            .all("tile")
            .filter(|(_, v)| v.get("pinned").and_then(Value::as_bool) == Some(true))
            .map(|(url, v)| {
                (
                    v.get("position").and_then(Value::as_f64).unwrap_or(0.0),
                    Tile {
                        url: url.to_string(),
                        title: v
                            .get("title")
                            .and_then(Value::as_str)
                            .unwrap_or("")
                            .to_string(),
                        pinned: true,
                    },
                )
            })
            .collect();
        tiles.sort_by(|a, b| a.0.total_cmp(&b.0));
        let mut out: Vec<Tile> = tiles.into_iter().map(|(_, t)| t).collect();
        let since = openremote_core::now_ms() - 30 * 86_400_000;
        let mut counts: HashMap<String, (usize, String)> = HashMap::new();
        for visit in self.visits().into_iter().filter(|v| v.at >= since) {
            let site = site_of(&visit.url);
            let entry = counts.entry(site).or_insert((0, String::new()));
            entry.0 += 1;
            if entry.1.is_empty() && !visit.title.is_empty() {
                entry.1 = visit.title;
            }
        }
        let mut frequent: Vec<(String, (usize, String))> = counts.into_iter().collect();
        frequent.sort_by(|a, b| b.1.0.cmp(&a.1.0).then_with(|| a.0.cmp(&b.0)));
        for (url, (_, title)) in frequent {
            if out.len() >= limit {
                break;
            }
            let removed = self
                .get("tile", &url)
                .and_then(|v| v.get("removed"))
                .and_then(Value::as_bool)
                == Some(true);
            if removed || out.iter().any(|t| t.url == url) {
                continue;
            }
            out.push(Tile {
                url,
                title,
                pinned: false,
            });
        }
        out.truncate(limit);
        out
    }

    /// Adds a tile of the user's own, or renames one.
    pub fn pin_tile(&mut self, url: &str, title: &str) {
        let position = self
            .all("tile")
            .filter_map(|(_, v)| v.get("position").and_then(Value::as_f64))
            .fold(0.0, f64::max)
            + 1.0;
        let position = self
            .get("tile", url)
            .and_then(|v| v.get("position"))
            .and_then(Value::as_f64)
            .unwrap_or(position);
        self.put(
            "tile",
            url,
            json!({"pinned": true, "title": title, "position": position}),
        );
    }

    /// Takes a tile off the new tab page (a frequent site stays off).
    pub fn remove_tile(&mut self, url: &str) {
        self.put("tile", url, json!({"removed": true}));
    }

    // ---- bookmarks ----

    pub fn bookmarks(&self) -> Vec<Bookmark> {
        let mut marks: Vec<Bookmark> = self
            .all("bookmark")
            .filter_map(|(id, v)| {
                let mut v = v.clone();
                v["id"] = json!(id);
                serde_json::from_value(v).ok()
            })
            .collect();
        marks.sort_by(|a, b| {
            a.folder
                .cmp(&b.folder)
                .then(a.position.total_cmp(&b.position))
                .then_with(|| a.id.cmp(&b.id))
        });
        marks
    }

    /// Adds or changes a bookmark (a new one goes last in its folder).
    pub fn save_bookmark(&mut self, mut mark: Bookmark) -> Bookmark {
        if mark.id.is_empty() {
            mark.id = uuid::Uuid::new_v4().to_string();
        }
        if mark.added == 0 {
            mark.added = openremote_core::now_ms();
        }
        if self.get("bookmark", &mark.id).is_none() && mark.position == 0.0 {
            mark.position = self
                .bookmarks()
                .iter()
                .filter(|b| b.folder == mark.folder)
                .map(|b| b.position)
                .fold(0.0, f64::max)
                + 1.0;
        }
        let mut val = serde_json::to_value(&mark).expect("a bookmark serializes");
        if let Some(map) = val.as_object_mut() {
            map.remove("id");
        }
        self.put("bookmark", &mark.id, val);
        mark
    }

    pub fn remove_bookmark(&mut self, id: &str) {
        self.put("bookmark", id, Value::Null);
    }

    // ---- import ----

    /// Brings in another browser's bookmarks and visits in one write. Their
    /// ids come from the importer and are the same every time, so importing
    /// the same browser twice adds nothing twice.
    ///
    /// A bookmark is left out when its page is already bookmarked here or its
    /// id is already held, so an import never moves or renames what the user
    /// has. New keys stop at [`IMPORT_BOOKMARKS`], and at `CAP_BOOKMARK -
    /// IMPORT_ROOM` keys in all (deleted rows count: their tombstones are
    /// keys the sync note carries). Imported bookmarks go to the end of
    /// Other bookmarks, never onto the bar.
    pub fn import(&mut self, marks: Vec<Bookmark>, visits: Vec<Visit>) -> Imported {
        let mut done = Imported::default();
        let mut pages: std::collections::HashSet<String> =
            self.bookmarks().into_iter().map(|b| b.url).collect();
        let held = self.state.keys().filter(|(c, _)| c == "bookmark").count();
        let room = (CAP_BOOKMARK - IMPORT_ROOM)
            .saturating_sub(held)
            .min(IMPORT_BOOKMARKS);
        let mut position = self
            .bookmarks()
            .iter()
            .filter(|b| b.folder == "other")
            .map(|b| b.position)
            .fold(0.0, f64::max);
        let mut writes = Vec::new();
        let mut new_keys = 0usize;
        for mut mark in marks {
            if self.get("bookmark", &mark.id).is_some() || pages.contains(&mark.url) {
                done.already += 1;
                continue;
            }
            let slot = ("bookmark".to_string(), mark.id.clone());
            // A key deleted here before comes back without growing the
            // collection; only a key never held counts against the room.
            if !self.state.contains_key(&slot) {
                if new_keys >= room {
                    done.over_cap += 1;
                    continue;
                }
                new_keys += 1;
            }
            done.bookmarks += 1;
            pages.insert(mark.url.clone());
            position += 1.0;
            mark.folder = "other".into();
            mark.position = position;
            let mut val = serde_json::to_value(&mark).expect("a bookmark serializes");
            if let Some(map) = val.as_object_mut() {
                map.remove("id");
            }
            writes.push(("bookmark", mark.id, val));
        }
        for visit in visits.into_iter().take(IMPORT_VISITS) {
            if self.get("visit", &visit.id).is_some() {
                continue;
            }
            done.visits += 1;
            writes.push((
                "visit",
                visit.id,
                json!({"url": visit.url, "title": visit.title, "at": visit.at}),
            ));
        }
        self.put_all(writes);
        done
    }

    /// Writes many keys with one append to the log, not one per key.
    fn put_all(&mut self, writes: Vec<(&str, String, Value)>) {
        if writes.is_empty() {
            return;
        }
        let now = openremote_core::now_ms();
        let mut lines = String::new();
        let mut ops = Vec::with_capacity(writes.len());
        for (col, key, val) in writes {
            let op = Op {
                at: self.stamp(now),
                col: col.into(),
                key,
                val,
            };
            lines.push_str(&serde_json::to_string(&op).expect("an op serializes"));
            lines.push('\n');
            ops.push(op);
        }
        let path = self.log_path(&self.device);
        if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(&path) {
            let _ = file.write_all(lines.as_bytes());
        }
        self.ops_written += ops.len();
        for op in ops {
            self.apply(op);
        }
        if self.ops_written > self.live_count() + COMPACT_AFTER {
            self.compact();
        }
    }

    /// The folder the browser's data lives in (`<data_dir>/browser`): an
    /// import makes its private scratch folder in here.
    pub fn folder(&self) -> &Path {
        &self.dir
    }

    // ---- settings, permissions, the tabs to restore ----

    /// Every setting the user changed (the console knows the defaults).
    pub fn settings(&self) -> serde_json::Map<String, Value> {
        self.all("setting")
            .map(|(k, v)| (k.to_string(), v.clone()))
            .collect()
    }

    pub fn set_setting(&mut self, key: &str, val: Value) {
        self.put("setting", key, val);
    }

    /// Remembered answers to sites' requests: `origin|kind` -> allow/deny.
    pub fn permissions(&self) -> serde_json::Map<String, Value> {
        self.all("permission")
            .map(|(k, v)| (k.to_string(), v.clone()))
            .collect()
    }

    pub fn set_permission(&mut self, origin: &str, kind: &str, state: Option<&str>) {
        let val = state.map_or(Value::Null, |s| json!(s));
        self.put("permission", &format!("{origin}|{kind}"), val);
    }

    /// The tabs this device had open, for the next start.
    pub fn session(&self) -> Value {
        self.get("session", &self.device)
            .cloned()
            .unwrap_or(Value::Null)
    }

    pub fn save_session(&mut self, session: Value) {
        if self.get("session", &self.device) != Some(&session) {
            let device = self.device.clone();
            self.put("session", &device, session);
        }
    }

    // ---- ad blocking ----
    //
    // Two collections of their own, never "setting": the sync note carries
    // only settings and bookmarks, so what this computer blocks, and which
    // lists it downloads, is decided on this computer alone.

    /// Whether the user turned ad blocking on here (off unless they did).
    pub fn adblock_on(&self) -> bool {
        self.get("adblock", "on").and_then(Value::as_bool) == Some(true)
    }

    pub fn set_adblock_on(&mut self, on: bool) {
        // Off is the default: it is stored as no value at all.
        self.put("adblock", "on", if on { json!(true) } else { Value::Null });
    }

    /// The filter lists the user picked (catalog ids), none until they pick.
    pub fn adblock_lists(&self) -> Vec<String> {
        self.get("adblock", "lists")
            .and_then(Value::as_array)
            .map(|ids| {
                ids.iter()
                    .filter_map(Value::as_str)
                    .map(String::from)
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn set_adblock_lists(&mut self, ids: &[String]) {
        let val = if ids.is_empty() {
            Value::Null
        } else {
            json!(ids)
        };
        self.put("adblock", "lists", val);
    }

    /// The sites the user allows ads on (`example.com`), sorted. One key
    /// per site.
    pub fn adblock_sites(&self) -> Vec<String> {
        let mut sites: Vec<String> = self
            .all("adblock_allow")
            .filter(|(_, v)| v.as_bool() == Some(true))
            .map(|(site, _)| site.to_string())
            .collect();
        sites.sort();
        sites
    }

    pub fn set_adblock_site(&mut self, site: &str, allow: bool) {
        let val = if allow { json!(true) } else { Value::Null };
        self.put("adblock_allow", site, val);
    }
}

/// Removes import scratch folders (`import-<id>`, see `super::import::Scratch`)
/// that a daemon stopped mid-import left behind: copies of another browser's
/// history have no business outliving the import that made them. Only real
/// folders by that name go; a link by that name is left alone.
fn sweep_import_scratch(dir: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let named = entry.file_name().to_string_lossy().starts_with("import-");
        if named && entry.file_type().is_ok_and(|t| t.is_dir()) {
            let _ = std::fs::remove_dir_all(entry.path());
        }
    }
}

/// This device's id for the browser's logs: made once, kept in the folder.
fn device_id(dir: &Path) -> String {
    let path = dir.join("device");
    if let Ok(id) = std::fs::read_to_string(&path) {
        let id = id.trim().to_string();
        if !id.is_empty() {
            return id;
        }
    }
    let id = uuid::Uuid::new_v4().simple().to_string();
    let _ = std::fs::write(&path, &id);
    id
}

/// An address without its scheme and `www.`, as people type it.
fn bare_address(url: &str) -> String {
    let lower = url.to_lowercase();
    let rest = lower.split_once("://").map_or(lower.as_str(), |(_, r)| r);
    rest.strip_prefix("www.").unwrap_or(rest).to_string()
}

/// A page's site: scheme and host, so a site's pages count as one tile.
fn site_of(url: &str) -> String {
    let Some((scheme, rest)) = url.split_once("://") else {
        return url.to_string();
    };
    let host = rest.split(['/', '?', '#']).next().unwrap_or(rest);
    format!("{scheme}://{host}/")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fresh() -> (tempfile::TempDir, BrowserStore) {
        let dir = tempfile::tempdir().unwrap();
        let store = BrowserStore::open(dir.path());
        (dir, store)
    }

    #[test]
    fn what_is_written_survives_a_restart_and_the_latest_write_wins() {
        let (dir, mut store) = fresh();
        let mark = store.save_bookmark(Bookmark {
            id: String::new(),
            url: "https://example.com/".into(),
            title: "Example".into(),
            folder: "bar".into(),
            position: 0.0,
            added: 0,
        });
        store.set_setting("search", json!("duckduckgo"));
        store.set_setting("search", json!("google"));
        store.set_permission("https://a.test", "camera", Some("allow"));
        let gone = store.save_bookmark(Bookmark {
            id: String::new(),
            url: "https://gone.test/".into(),
            title: "Gone".into(),
            folder: "bar".into(),
            position: 0.0,
            added: 0,
        });
        store.remove_bookmark(&gone.id);
        drop(store);

        let store = BrowserStore::open(dir.path());
        assert_eq!(store.bookmarks(), vec![mark]);
        assert_eq!(store.settings()["search"], "google");
        assert_eq!(store.permissions()["https://a.test|camera"], "allow");
    }

    #[test]
    fn history_searches_newest_first_and_clears() {
        let (_dir, mut store) = fresh();
        store.visit("https://docs.rs/serde", "serde - Rust", 1_000);
        let id = store.visit("https://example.com/", "", 2_000);
        store.retitle_visit(&id, "Example Domain");
        store.visit("https://docs.rs/tokio", "tokio - Rust", 3_000);
        let all = store.history("", None, 10);
        assert_eq!(
            all.iter().map(|v| v.at).collect::<Vec<_>>(),
            vec![3_000, 2_000, 1_000]
        );
        assert_eq!(all[1].title, "Example Domain");
        assert_eq!(store.history("rust", None, 10).len(), 2);
        assert_eq!(store.history("", Some(3_000), 1)[0].at, 2_000);
        store.clear_history(Some(2_000));
        assert_eq!(store.history("", None, 10).len(), 1);
    }

    #[test]
    fn the_address_bar_suggests_what_was_used_most() {
        let (_dir, mut store) = fresh();
        for at in 0..3 {
            store.visit("https://github.com/tauri-apps/tauri", "Tauri", at);
        }
        store.visit("https://gitlab.com/", "GitLab", 10);
        store.visit("https://example.com/git", "A page about git", 11);
        let hits = store.suggest("git", 5);
        assert_eq!(hits[0].url, "https://github.com/tauri-apps/tauri");
        assert_eq!(hits[1].url, "https://gitlab.com/");
        assert_eq!(hits.len(), 3);
        assert!(store.suggest("zzz", 5).is_empty());
    }

    #[test]
    fn tiles_put_the_users_own_first_and_forget_removed_sites() {
        let (_dir, mut store) = fresh();
        let now = openremote_core::now_ms();
        store.visit("https://news.test/a", "News", now);
        store.visit("https://news.test/b", "News", now);
        store.visit("https://shop.test/", "Shop", now);
        store.pin_tile("https://mine.test/", "Mine");
        let tiles = store.tiles(8);
        assert_eq!(tiles[0].url, "https://mine.test/");
        assert!(tiles[0].pinned);
        assert_eq!(tiles[1].url, "https://news.test/");
        store.remove_tile("https://news.test/");
        let urls: Vec<String> = store.tiles(8).into_iter().map(|t| t.url).collect();
        assert_eq!(urls, vec!["https://mine.test/", "https://shop.test/"]);
    }

    fn imported(n: usize, tag: &str) -> Vec<Bookmark> {
        (0..n)
            .map(|i| Bookmark {
                id: format!("imp-{tag}-{i}"),
                url: format!("https://{tag}.example/{i}"),
                title: format!("Page {i}"),
                folder: "bar".into(),
                position: 0.0,
                added: 1,
            })
            .collect()
    }

    #[test]
    fn an_import_is_capped_below_the_bookmark_sync_ceiling() {
        let (_dir, mut store) = fresh();
        let first = store.import(imported(IMPORT_BOOKMARKS + 500, "a"), Vec::new());
        assert_eq!(first.bookmarks, IMPORT_BOOKMARKS);
        assert_eq!(first.over_cap, 500);
        // Two more browsers: the second fits under the per-import cap, the
        // third only up to the room kept below the sync ceiling.
        let second = store.import(imported(IMPORT_BOOKMARKS, "b"), Vec::new());
        assert_eq!(second.bookmarks, IMPORT_BOOKMARKS);
        let third = store.import(imported(IMPORT_BOOKMARKS, "c"), Vec::new());
        assert_eq!(
            third.bookmarks,
            CAP_BOOKMARK - IMPORT_ROOM - 2 * IMPORT_BOOKMARKS
        );
        assert_eq!(store.rows("bookmark").len(), CAP_BOOKMARK - IMPORT_ROOM);
        assert!(store.rows("bookmark").len() < CAP_BOOKMARK);
        // The user's own hand is not capped by an import.
        store.save_bookmark(Bookmark {
            id: String::new(),
            url: "https://mine.example/".into(),
            title: String::new(),
            folder: "bar".into(),
            position: 0.0,
            added: 0,
        });
        assert_eq!(store.bookmarks().len(), CAP_BOOKMARK - IMPORT_ROOM + 1);
    }

    #[test]
    fn deleted_rows_count_against_the_room() {
        let (_dir, mut store) = fresh();
        store.import(imported(IMPORT_BOOKMARKS, "a"), Vec::new());
        store.import(imported(IMPORT_BOOKMARKS, "b"), Vec::new());
        for mark in store.bookmarks() {
            store.remove_bookmark(&mark.id);
        }
        // 4,000 tombstones are 4,000 keys a sync note carries.
        let again = store.import(imported(IMPORT_BOOKMARKS, "c"), Vec::new());
        assert_eq!(
            again.bookmarks,
            CAP_BOOKMARK - IMPORT_ROOM - 2 * IMPORT_BOOKMARKS
        );
    }

    #[test]
    fn importing_twice_adds_nothing_twice_and_leaves_the_users_own_alone() {
        let (dir, mut store) = fresh();
        let mine = store.save_bookmark(Bookmark {
            id: String::new(),
            url: "https://a.example/1".into(),
            title: "Mine".into(),
            folder: "bar".into(),
            position: 0.0,
            added: 0,
        });
        let visits = vec![Visit {
            id: "imp-a-v1".into(),
            url: "https://a.example/0".into(),
            title: "Zero".into(),
            at: openremote_core::now_ms(),
        }];
        let once = store.import(imported(3, "a"), visits.clone());
        assert_eq!((once.bookmarks, once.already, once.visits), (2, 1, 1));
        let twice = store.import(imported(3, "a"), visits);
        assert_eq!((twice.bookmarks, twice.already, twice.visits), (0, 3, 0));
        drop(store);
        let store = BrowserStore::open(dir.path());
        let marks = store.bookmarks();
        assert_eq!(marks.len(), 3);
        // The user's bookmark is untouched; imported ones go to Other.
        assert!(marks.contains(&mine));
        let other: Vec<&Bookmark> = marks.iter().filter(|m| m.folder == "other").collect();
        assert_eq!(other.len(), 2);
        assert!(other[0].position < other[1].position);
        assert_eq!(store.history("", None, 10).len(), 1);
    }

    /// An import's scratch copies left behind by a daemon that stopped
    /// mid-import (a crash, a kill) are removed at the next start; nothing
    /// else in the folder is touched.
    #[test]
    fn a_left_over_import_scratch_folder_is_removed_at_start() {
        let dir = tempfile::tempdir().unwrap();
        let browser = dir.path().join("browser");
        let left = browser.join("import-0123abcd");
        std::fs::create_dir_all(&left).unwrap();
        std::fs::write(left.join("History"), b"a copy").unwrap();
        std::fs::create_dir_all(browser.join("filters")).unwrap();
        std::fs::write(browser.join("import-note"), b"a file, not a scratch folder").unwrap();
        let _store = BrowserStore::open(dir.path());
        assert!(!left.exists());
        assert!(browser.join("filters").is_dir());
        assert!(browser.join("import-note").is_file());
    }

    #[test]
    fn compaction_keeps_what_is_true() {
        let (dir, mut store) = fresh();
        for n in 0..50 {
            store.set_setting("zoom", json!(n));
        }
        store.compact();
        let lines = std::fs::read_to_string(store.log_path(&store.device)).unwrap();
        assert_eq!(lines.lines().count(), 1);
        drop(store);
        assert_eq!(BrowserStore::open(dir.path()).settings()["zoom"], 49);
    }
}
