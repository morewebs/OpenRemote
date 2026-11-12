//! Ad and tracker blocking, from the daemon's side. The blocking itself
//! happens in the app's browser, which holds the filter engine and answers
//! blocked requests on the spot; the daemon keeps what outlives it: whether
//! blocking is on, which filter lists the user picked, the sites the user
//! allows ads on, and the downloaded lists.
//!
//! Nothing is on, and nothing is picked, until the user says so. No list
//! ships with the app and none is downloaded until the user picks it with
//! blocking on: while blocking is off, or on with no list picked, this
//! module makes no network request at all. A picked list is downloaded over
//! HTTPS only (every redirect too), within a size cap and a time limit, and
//! refreshed weekly while blocking stays on. Its text is someone else's, so
//! it is checked and trimmed before it is kept (`sanitize`).
//!
//! All of it is this computer's alone: kept in the store's own `adblock`
//! collections, which the sync note never carries, and served only on this
//! device's routes (none is a peer route). Whenever any of it changes, and
//! whenever a browser that can block says hello, the browser hears it all
//! in one [`Call::Adblock`].

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use openremote_browser::{Call, ToHost};
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::sync::Notify;

use super::store::BrowserStore;
use super::{BrowserService, lock};
use crate::app::App;

/// One filter list the user may pick, from its publisher.
pub struct FilterList {
    pub id: &'static str,
    pub title: &'static str,
    /// What it blocks, in a few words.
    pub about: &'static str,
    /// Where it is downloaded from: always `https://`.
    pub url: &'static str,
    /// Its licence, named beside it wherever the list is offered.
    pub licence: &'static str,
    /// The publisher's page.
    pub home: &'static str,
}

/// The lists on offer. None is picked by default; which (if any) should be
/// is an open decision. EasyList and EasyPrivacy are dual licensed
/// GPL-3.0-or-later or CC BY-SA 3.0 or later, and each copy is downloaded by
/// the user's own app from the publisher, never redistributed with it.
pub const CATALOG: &[FilterList] = &[
    FilterList {
        id: "easylist",
        title: "EasyList",
        about: "Adverts",
        url: "https://easylist.to/easylist/easylist.txt",
        licence: "GPL-3.0-or-later or CC BY-SA 3.0+",
        home: "https://easylist.to/",
    },
    FilterList {
        id: "easyprivacy",
        title: "EasyPrivacy",
        about: "Trackers",
        url: "https://easylist.to/easylist/easyprivacy.txt",
        licence: "GPL-3.0-or-later or CC BY-SA 3.0+",
        home: "https://easylist.to/",
    },
];

/// Lists older than this are downloaded again (while blocking is on).
const REFRESH_AFTER: Duration = Duration::from_secs(7 * 24 * 3600);
/// A failed download is tried again after this long (or at once when the
/// user turns blocking on or picks a list).
const RETRY_AFTER: Duration = Duration::from_secs(3600);
/// How often the keeper looks at the lists' age on its own.
const TICK: Duration = Duration::from_secs(3600);
/// More than any list on offer weighs (EasyList is about 2 MB).
pub const MAX_LIST_BYTES: u64 = 16 << 20;
/// How long one download (one hop of it) may take.
const DOWNLOAD_TIME: Duration = Duration::from_secs(60);
/// Redirects followed before a download gives up.
const MAX_HOPS: usize = 5;
/// A rule longer than this is dropped: no real one comes near it, and the
/// engine should not be asked to compile it.
pub const MAX_LINE: usize = 4096;
/// Rules kept from one list (EasyList has about 80,000 lines).
pub const MAX_RULES: usize = 300_000;

/// Keeps the picked lists fresh and the browser told.
pub struct Keeper {
    dir: PathBuf,
    wake: Arc<Notify>,
    /// The browser said hello: it hears everything again.
    resend: AtomicBool,
    /// The user just turned blocking on or picked a list: a failed download
    /// is retried now rather than in an hour.
    urgent: AtomicBool,
    state: Mutex<KeeperState>,
}

#[derive(Default)]
struct KeeperState {
    fetching: bool,
    error: Option<String>,
    last_try: Option<Instant>,
    /// What the browser was told last.
    told: Option<Call>,
}

/// The picked ids that are in the catalog, in the catalog's order.
fn picked_lists(picked: &[String]) -> Vec<&'static FilterList> {
    CATALOG
        .iter()
        .filter(|list| picked.iter().any(|id| id == list.id))
        .collect()
}

/// The lists to keep: none while blocking is off, else the picked ones.
pub fn wanted(on: bool, picked: &[String]) -> Vec<&'static FilterList> {
    if on { picked_lists(picked) } else { Vec::new() }
}

impl Keeper {
    pub fn new(data_dir: &Path) -> Self {
        Self {
            dir: data_dir.join("browser").join("filters"),
            wake: Arc::new(Notify::new()),
            resend: AtomicBool::new(false),
            urgent: AtomicBool::new(false),
            state: Mutex::new(KeeperState::default()),
        }
    }

    fn path(&self, list: &FilterList) -> PathBuf {
        self.dir.join(format!("{}.txt", list.id))
    }

    /// When a list on disk was downloaded, and its size.
    fn on_disk(&self, list: &FilterList) -> Option<(SystemTime, u64)> {
        let meta = std::fs::metadata(self.path(list)).ok()?;
        Some((meta.modified().ok()?, meta.len()))
    }

    /// The wanted lists that are missing or past their week: what a
    /// download would fetch now. Empty while blocking is off, and while no
    /// browser on this daemon can block (`supported`): a list nothing here
    /// could use is never downloaded.
    pub fn due(&self, on: bool, picked: &[String], supported: bool) -> Vec<&'static FilterList> {
        if !supported {
            return Vec::new();
        }
        wanted(on, picked)
            .into_iter()
            .filter(|list| match self.on_disk(list) {
                Some((at, _)) => at.elapsed().map_or(true, |age| age > REFRESH_AFTER),
                None => true,
            })
            .collect()
    }

    /// The browser said hello: tell it everything again.
    pub fn hello(&self) {
        self.resend.store(true, Ordering::SeqCst);
        self.wake.notify_one();
    }

    /// The setting, the picked lists or the allowed sites changed; `urgent`
    /// when the user just turned blocking on or picked a list.
    pub fn changed(&self, urgent: bool) {
        if urgent {
            self.urgent.store(true, Ordering::SeqCst);
        }
        self.wake.notify_one();
    }

    /// What the browser is told. It blocks only when blocking is on and a
    /// picked list is on disk; anything else is plain off, with nothing
    /// else said.
    pub fn call(&self, on: bool, picked: &[String], allow: Vec<String>) -> Call {
        let mut lists = Vec::new();
        let mut updated = 0i64;
        for list in wanted(on, picked) {
            if let Some((at, _)) = self.on_disk(list) {
                lists.push(self.path(list).to_string_lossy().into_owned());
                updated = updated.max(ms(at));
            }
        }
        if lists.is_empty() {
            return Call::Adblock {
                on: false,
                lists: Vec::new(),
                updated: 0,
                allow: Vec::new(),
            };
        }
        Call::Adblock {
            on: true,
            lists,
            updated,
            allow,
        }
    }

    /// Downloads these lists, keeping the old copy of any that fails.
    async fn fetch(&self, lists: &[&FilterList]) {
        {
            let mut state = lock(&self.state);
            state.fetching = true;
            state.last_try = Some(Instant::now());
        }
        let mut error = None;
        if let Err(e) = std::fs::create_dir_all(&self.dir) {
            error = Some(e.to_string());
        } else {
            for list in lists {
                if let Err(e) = self.fetch_one(list).await {
                    error = Some(e);
                }
            }
        }
        let mut state = lock(&self.state);
        state.fetching = false;
        state.error = error;
    }

    async fn fetch_one(&self, list: &FilterList) -> Result<(), String> {
        let body = download(list.url).await?;
        let text = String::from_utf8(body)
            .map_err(|_| format!("{} came back as something other than text", list.title))?;
        let text = sanitize(&text).ok_or_else(|| {
            format!(
                "{} came back as something other than a filter list (a network sign-in page?)",
                list.title
            )
        })?;
        let path = self.path(list);
        let part = path.with_extension("txt.part");
        std::fs::write(&part, text.as_bytes()).map_err(|e| e.to_string())?;
        std::fs::rename(&part, &path).map_err(|e| e.to_string())
    }

    /// Whether a download may start now: none is running, and the last
    /// failure is an hour old (or the user just asked).
    fn may_fetch(&self) -> bool {
        let state = lock(&self.state);
        // A download is running: the user's ask stays, so the retry runs
        // once this one ends.
        if state.fetching {
            return false;
        }
        let urgent = self.urgent.swap(false, Ordering::SeqCst);
        urgent
            || state.error.is_none()
            || state.last_try.is_none_or(|at| at.elapsed() > RETRY_AFTER)
    }

    /// What Settings shows. `supported` is whether the browser here can
    /// block at all (the desktop app on Windows, for now).
    pub fn status(&self, store: &BrowserStore, supported: bool) -> Value {
        let on = store.adblock_on();
        let picked = store.adblock_lists();
        let state = lock(&self.state);
        let mut newest: Option<i64> = None;
        let catalog: Vec<Value> = CATALOG
            .iter()
            .map(|list| {
                let chosen = picked.iter().any(|id| id == list.id);
                let disk = self.on_disk(list).filter(|_| chosen);
                if let Some((at, _)) = disk {
                    newest = newest.max(Some(ms(at)));
                }
                json!({
                    "id": list.id,
                    "title": list.title,
                    "about": list.about,
                    "url": list.url,
                    "licence": list.licence,
                    "home": list.home,
                    "picked": chosen,
                    "updated": disk.map(|(at, _)| ms(at)),
                    "bytes": disk.map(|(_, size)| size),
                })
            })
            .collect();
        let wanted = wanted(on, &picked);
        let due = !self.due(on, &picked, supported).is_empty();
        let word = if !on {
            "off"
        } else if !supported {
            // On, but no browser here can block, so nothing is downloaded.
            "unsupported"
        } else if wanted.is_empty() {
            // On, but nothing to block with until the user picks a list.
            "none"
        } else if state.fetching || (due && state.error.is_none()) {
            "updating"
        } else if due {
            "error"
        } else {
            "on"
        };
        json!({
            "on": on,
            "supported": supported,
            "state": word,
            "error": if on { state.error.clone() } else { None },
            "updated": newest,
            "lists": catalog,
            "sites": store.adblock_sites(),
        })
    }
}

/// A filter list starts with its format line (`[Adblock Plus 2.0]`), which
/// a captive portal's page or an error page never does.
fn looks_like_a_list(text: &str) -> bool {
    text.trim_start_matches('\u{feff}')
        .trim_start()
        .starts_with("[Adblock")
}

/// A downloaded list as it is kept: `None` when it is not a filter list at
/// all. Lines no real rule looks like (overlong, or holding control
/// characters) are dropped, and a list is cut at [`MAX_RULES`] lines, so
/// what the browser's engine reads is bounded whatever the server sent.
pub fn sanitize(text: &str) -> Option<String> {
    if !looks_like_a_list(text) {
        return None;
    }
    let mut out = String::with_capacity(text.len().min(MAX_LIST_BYTES as usize));
    let mut kept = 0usize;
    for line in text.trim_start_matches('\u{feff}').lines() {
        if kept >= MAX_RULES {
            break;
        }
        let line = line.trim_end_matches('\r');
        if line.len() > MAX_LINE || line.chars().any(|c| c.is_control() && c != '\t') {
            continue;
        }
        out.push_str(line);
        out.push('\n');
        kept += 1;
    }
    Some(out)
}

/// The next address a download goes to after a redirect: HTTPS only, and a
/// path alone stays on the same server.
fn next_hop(current: &str, location: &str) -> Result<String, String> {
    let location = location.trim();
    let next = if location.starts_with('/') && !location.starts_with("//") {
        let origin_end = current
            .strip_prefix("https://")
            .and_then(|rest| rest.find('/').map(|i| "https://".len() + i))
            .unwrap_or(current.len());
        format!("{}{location}", &current[..origin_end])
    } else {
        location.to_string()
    };
    if is_https(&next) {
        Ok(next)
    } else {
        Err("a filter list redirected away from HTTPS; it was not downloaded".into())
    }
}

fn is_https(url: &str) -> bool {
    url.get(..8)
        .is_some_and(|scheme| scheme.eq_ignore_ascii_case("https://"))
        && url.len() > 8
}

/// Downloads one list: HTTPS on every hop, within [`MAX_LIST_BYTES`] and
/// [`DOWNLOAD_TIME`] a hop.
async fn download(location: &str) -> Result<Vec<u8>, String> {
    if !is_https(location) {
        return Err("filter lists are downloaded over HTTPS only".into());
    }
    let http = openremote_cloud::https::Http::new();
    let max_body = usize::try_from(MAX_LIST_BYTES + 1).unwrap_or(usize::MAX);
    let mut url = location.to_string();
    for _ in 0..MAX_HOPS {
        let response = http
            .download(
                DOWNLOAD_TIME,
                max_body,
                &url,
                // The client names itself (User-Agent: OpenRemote/<version>).
                &[("accept", "text/plain")],
            )
            .await
            .map_err(|e| format!("could not download {url}: {e}"))?;
        match response.status {
            200 => {
                if response.body.len() as u64 > MAX_LIST_BYTES {
                    return Err(format!("{url} is too large to be a filter list"));
                }
                return Ok(response.body);
            }
            301 | 302 | 303 | 307 | 308 => {
                let location = response
                    .headers
                    .get("location")
                    .and_then(|v| v.to_str().ok())
                    .ok_or_else(|| format!("{url} redirects nowhere"))?;
                url = next_hop(&url, location)?;
            }
            status => return Err(format!("{url} answered {status}")),
        }
    }
    Err(format!("{location} redirects too many times"))
}

fn ms(at: SystemTime) -> i64 {
    at.duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Whether the browser here can block ads and trackers.
fn supported(service: &BrowserService) -> bool {
    lock(&service.link)
        .as_ref()
        .is_some_and(|link| link.caps.adblock)
}

/// Tells the browser, without waiting: whether there was a browser able
/// to hear it.
fn tell(service: &BrowserService, call: Call) -> bool {
    let send = lock(&service.link)
        .as_ref()
        .filter(|link| link.caps.adblock)
        .map(|link| link.send.clone());
    let Some(send) = send else {
        return false;
    };
    let id = service.next.fetch_add(1, Ordering::SeqCst) + 1;
    send(ToHost { id, call });
    true
}

/// Tells the browser what blocking should be doing, if that changed (or
/// the browser asked to hear it all again).
fn inform(service: &BrowserService) {
    let keeper = &service.adblock;
    let call = {
        let store = lock(&service.store);
        keeper.call(
            store.adblock_on(),
            &store.adblock_lists(),
            store.adblock_sites(),
        )
    };
    let resend = keeper.resend.swap(false, Ordering::SeqCst);
    let changed = lock(&keeper.state).told.as_ref() != Some(&call);
    if (resend || changed) && tell(service, call.clone()) {
        lock(&keeper.state).told = Some(call);
    } else if resend {
        // Nobody could hear it yet: ask again when a browser says hello.
        keeper.resend.store(true, Ordering::SeqCst);
    }
}

/// One look: tell the browser, download what is due (only while blocking
/// is on and a list is picked), and tell it again if that changed things.
async fn keep(service: &BrowserService) {
    let keeper = &service.adblock;
    inform(service);
    let due = {
        let store = lock(&service.store);
        keeper.due(
            store.adblock_on(),
            &store.adblock_lists(),
            supported(service),
        )
    };
    if !due.is_empty() && keeper.may_fetch() {
        keeper.fetch(&due).await;
        inform(service);
    }
}

/// Starts the keeper: it looks when woken and once an hour.
pub fn start(app: &Arc<App>) {
    let weak = Arc::downgrade(app);
    let wake = Arc::clone(&app.browser.adblock.wake);
    tokio::spawn(async move {
        loop {
            {
                let Some(app) = weak.upgrade() else {
                    return;
                };
                keep(&app.browser).await;
            }
            tokio::select! {
                () = wake.notified() => {}
                () = tokio::time::sleep(TICK) => {}
            }
        }
    });
}

// ---- routes ----

pub fn routes() -> Router<Arc<App>> {
    Router::new()
        .route("/browser/adblock", get(status).post(set))
        .route("/browser/adblock/site", post(set_site))
}

fn bad(message: &str) -> Response {
    (StatusCode::BAD_REQUEST, Json(json!({"error": message}))).into_response()
}

/// Whether blocking is on, the catalog with what is picked, the lists'
/// state and the allowed sites.
async fn status(State(app): State<Arc<App>>) -> Response {
    let store = lock(&app.browser.store);
    Json(app.browser.adblock.status(&store, supported(&app.browser))).into_response()
}

#[derive(Deserialize)]
struct SetBody {
    #[serde(default)]
    on: Option<bool>,
    /// The whole set of picked lists (catalog ids).
    #[serde(default)]
    lists: Option<Vec<String>>,
}

/// Turns blocking on or off, and picks the lists. Downloads start only for
/// picked lists while blocking is on; the answer says "updating" until
/// they're in.
async fn set(State(app): State<Arc<App>>, Json(body): Json<SetBody>) -> Response {
    if let Some(ids) = &body.lists {
        if let Some(unknown) = ids.iter().find(|id| !CATALOG.iter().any(|l| l.id == *id)) {
            return bad(&format!("{unknown} is not a filter list this app offers"));
        }
    }
    let keeper = &app.browser.adblock;
    {
        let mut store = lock(&app.browser.store);
        if let Some(ids) = &body.lists {
            let mut ids: Vec<String> = picked_lists(ids)
                .into_iter()
                .map(|l| l.id.to_string())
                .collect();
            ids.dedup();
            store.set_adblock_lists(&ids);
        }
        if let Some(on) = body.on {
            store.set_adblock_on(on);
        }
    }
    let urgent = body.on == Some(true) || body.lists.as_ref().is_some_and(|l| !l.is_empty());
    keeper.changed(urgent);
    let able = supported(&app.browser);
    let store = lock(&app.browser.store);
    let mut status = keeper.status(&store, able);
    if !keeper
        .due(store.adblock_on(), &store.adblock_lists(), able)
        .is_empty()
    {
        // The keeper is waking to download them: said now, so the switch
        // doesn't show an old error for a moment.
        status["state"] = json!("updating");
        status["error"] = Value::Null;
    }
    Json(status).into_response()
}

#[derive(Deserialize)]
struct SiteBody {
    site: String,
    allow: bool,
}

/// Allows ads on a site (`example.com`), or blocks them there again.
async fn set_site(State(app): State<Arc<App>>, Json(body): Json<SiteBody>) -> Response {
    let site = body.site.trim().trim_end_matches('.').to_ascii_lowercase();
    if !is_site(&site) {
        return bad("a site is a host name, like example.com");
    }
    {
        let mut store = lock(&app.browser.store);
        store.set_adblock_site(&site, body.allow);
    }
    app.browser.adblock.changed(false);
    let store = lock(&app.browser.store);
    Json(app.browser.adblock.status(&store, supported(&app.browser))).into_response()
}

/// A host name or address as the browser names a site: no scheme, path,
/// port or spaces.
fn is_site(site: &str) -> bool {
    !site.is_empty()
        && site.len() <= 253
        && site
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_' | '[' | ']' | ':'))
        && !site.starts_with('.')
        && !site.contains("..")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    fn put_lists(keeper: &Keeper, which: &[&str]) {
        std::fs::create_dir_all(&keeper.dir).unwrap();
        for list in CATALOG.iter().filter(|l| which.contains(&l.id)) {
            std::fs::write(keeper.path(list), "[Adblock Plus 2.0]\n||ads.test^\n").unwrap();
        }
    }

    #[test]
    fn every_list_on_offer_is_https_and_names_its_licence() {
        assert!(!CATALOG.is_empty());
        for list in CATALOG {
            assert!(list.url.starts_with("https://"), "{}", list.id);
            assert!(list.home.starts_with("https://"), "{}", list.id);
            assert!(!list.licence.is_empty(), "{}", list.id);
        }
    }

    #[test]
    fn nothing_is_downloaded_while_off_or_before_a_list_is_picked() {
        let dir = tempfile::tempdir().unwrap();
        let keeper = Keeper::new(dir.path());
        // Off, whatever is picked.
        assert!(
            keeper
                .due(false, &ids(&["easylist", "easyprivacy"]), true)
                .is_empty()
        );
        // On, nothing picked.
        assert!(keeper.due(true, &[], true).is_empty());
        // On, a list the catalog doesn't offer.
        assert!(
            keeper
                .due(true, &ids(&["https://evil.test/list"]), true)
                .is_empty()
        );
        // On with one picked: that one, and only that one.
        let due: Vec<&str> = keeper
            .due(true, &ids(&["easyprivacy"]), true)
            .iter()
            .map(|l| l.id)
            .collect();
        assert_eq!(due, vec!["easyprivacy"]);
        put_lists(&keeper, &["easyprivacy"]);
        assert!(keeper.due(true, &ids(&["easyprivacy"]), true).is_empty());
    }

    #[test]
    fn off_tells_the_browser_nothing_but_off() {
        let dir = tempfile::tempdir().unwrap();
        let keeper = Keeper::new(dir.path());
        let off = Call::Adblock {
            on: false,
            lists: vec![],
            updated: 0,
            allow: vec![],
        };
        put_lists(&keeper, &["easylist", "easyprivacy"]);
        assert_eq!(
            keeper.call(false, &ids(&["easylist"]), vec!["example.com".into()]),
            off
        );
        // On with nothing picked hooks nothing either.
        assert_eq!(keeper.call(true, &[], vec![]), off);
        let Call::Adblock {
            on, lists, allow, ..
        } = keeper.call(true, &ids(&["easylist"]), vec!["a.test".into()])
        else {
            unreachable!()
        };
        assert!(on);
        assert_eq!(lists.len(), 1);
        assert!(lists[0].ends_with("easylist.txt"));
        assert_eq!(allow, vec!["a.test".to_string()]);
    }

    #[test]
    fn on_with_a_picked_list_not_yet_here_is_still_off_to_the_browser() {
        let dir = tempfile::tempdir().unwrap();
        let keeper = Keeper::new(dir.path());
        assert!(matches!(
            keeper.call(true, &ids(&["easylist"]), vec![]),
            Call::Adblock { on: false, .. }
        ));
    }

    #[test]
    fn status_reads_off_none_updating_and_on() {
        let dir = tempfile::tempdir().unwrap();
        let keeper = Keeper::new(dir.path());
        let mut store = BrowserStore::open(dir.path());
        let s = keeper.status(&store, true);
        assert_eq!(s["state"], "off");
        assert_eq!(s["on"], false);
        assert!(
            s["lists"]
                .as_array()
                .unwrap()
                .iter()
                .all(|l| l["picked"] == false && l["licence"].as_str().is_some())
        );
        store.set_adblock_on(true);
        assert_eq!(keeper.status(&store, true)["state"], "none");
        store.set_adblock_lists(&ids(&["easylist"]));
        assert_eq!(keeper.status(&store, true)["state"], "updating");
        put_lists(&keeper, &["easylist"]);
        assert_eq!(keeper.status(&store, true)["state"], "on");
    }

    #[test]
    fn what_this_computer_blocks_is_its_own_and_never_synced() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = BrowserStore::open(dir.path());
        store.set_adblock_on(true);
        store.set_adblock_lists(&ids(&["easylist"]));
        store.set_adblock_site("example.com", true);
        assert!(store.settings().is_empty());
        let note = super::super::sync::note(&store, 1).to_string();
        assert!(!note.contains("easylist"), "{note}");
        assert!(!note.contains("example.com"), "{note}");
        // And it survives a restart.
        drop(store);
        let store = BrowserStore::open(dir.path());
        assert!(store.adblock_on());
        assert_eq!(store.adblock_lists(), ids(&["easylist"]));
        assert_eq!(store.adblock_sites(), ids(&["example.com"]));
    }

    #[test]
    fn only_a_filter_list_counts_as_one() {
        assert!(sanitize("[Adblock Plus 2.0]\n! Title: EasyList\n||ads.example^\n").is_some());
        assert!(sanitize("\u{feff}[Adblock Plus 1.1]\n").is_some());
        assert!(sanitize("<!doctype html><title>Sign in</title>").is_none());
        assert!(sanitize("").is_none());
    }

    #[test]
    fn a_hostile_list_is_trimmed_to_what_a_rule_looks_like() {
        let long = "a".repeat(MAX_LINE + 1);
        let nasty = format!(
            "[Adblock Plus 2.0]\r\n||ok.test^\r\n{long}\n||nul\0.test^\n/(a+)+$/\n##.ad\u{7}\n\n\u{202e}||rtl.test^\n"
        );
        let kept = sanitize(&nasty).unwrap();
        assert!(kept.contains("||ok.test^\n"));
        assert!(!kept.contains('\r'));
        assert!(!kept.contains(&long));
        assert!(!kept.contains('\0'));
        assert!(!kept.contains('\u{7}'));
        // A regex rule passes through: the browser's engine compiles it with
        // a linear-time regex crate, which the shell's own tests exercise.
        assert!(kept.contains("/(a+)+$/"));
        assert!(kept.lines().all(|l| l.len() <= MAX_LINE));

        // Millions of lines are cut at the cap.
        let mut many = String::from("[Adblock Plus 2.0]\n");
        for i in 0..(MAX_RULES + 10) {
            many.push_str(&format!("||{i}.test^\n"));
        }
        assert_eq!(sanitize(&many).unwrap().lines().count(), MAX_RULES);

        // Odd bytes and odd shapes neither panic nor pass for a list.
        for junk in [
            "\u{feff}",
            "[",
            "[Adblock",
            "\n\n\n[Adblock Plus]",
            "\u{feff}\u{feff}[Adblock Plus 2.0]",
            "[Adblock Plus 2.0]\n\u{0}\u{0}\u{0}",
        ] {
            let _ = sanitize(junk);
        }
    }

    #[test]
    fn a_download_never_leaves_https() {
        assert!(is_https("https://easylist.to/easylist/easylist.txt"));
        assert!(is_https("HTTPS://easylist.to/"));
        assert!(!is_https("http://easylist.to/easylist/easylist.txt"));
        assert!(!is_https("https://"));
        assert!(!is_https("file:///etc/passwd"));
        assert_eq!(
            next_hop(
                "https://easylist.to/easylist/easylist.txt",
                "/v2/easylist.txt"
            )
            .unwrap(),
            "https://easylist.to/v2/easylist.txt"
        );
        assert_eq!(
            next_hop("https://a.test/x", "https://cdn.test/list.txt").unwrap(),
            "https://cdn.test/list.txt"
        );
        assert!(next_hop("https://a.test/x", "http://a.test/x").is_err());
        assert!(next_hop("https://a.test/x", "//cdn.test/x").is_err());
        assert!(next_hop("https://a.test/x", "ftp://a.test/x").is_err());
        assert!(next_hop("https://a.test/x", "javascript:alert(1)").is_err());
    }

    #[tokio::test]
    async fn a_plain_http_list_is_refused_before_any_request() {
        let err = download("http://easylist.to/easylist/easylist.txt")
            .await
            .unwrap_err();
        assert!(err.contains("HTTPS"), "{err}");
    }

    #[test]
    fn only_a_browser_that_can_block_hears_about_blocking() {
        use openremote_browser::{Caps, FromHost};
        for can in [false, true] {
            let dir = tempfile::tempdir().unwrap();
            let service = BrowserService::new(dir.path());
            let sent: Arc<Mutex<Vec<ToHost>>> = Arc::default();
            let log = Arc::clone(&sent);
            service.connect(Arc::new(move |m| log.lock().unwrap().push(m)));
            service.receive(FromHost::Hello {
                engine: "test".into(),
                caps: Caps {
                    adblock: can,
                    ..Caps::default()
                },
            });
            inform(&service);
            let told: Vec<Call> = sent
                .lock()
                .unwrap()
                .iter()
                .map(|m| m.call.clone())
                .collect();
            if can {
                assert_eq!(
                    told,
                    vec![Call::Adblock {
                        on: false,
                        lists: vec![],
                        updated: 0,
                        allow: vec![]
                    }]
                );
                // Nothing changed: nothing more is said.
                inform(&service);
                assert_eq!(sent.lock().unwrap().len(), 1);
            } else {
                assert!(told.is_empty(), "{told:?}");
            }
        }
    }

    #[test]
    fn nothing_is_downloaded_where_no_browser_can_block() {
        let dir = tempfile::tempdir().unwrap();
        let keeper = Keeper::new(dir.path());
        assert!(keeper.due(true, &ids(&["easylist"]), false).is_empty());
        assert_eq!(keeper.due(true, &ids(&["easylist"]), true).len(), 1);
        let mut store = BrowserStore::open(dir.path());
        store.set_adblock_on(true);
        store.set_adblock_lists(&ids(&["easylist"]));
        assert_eq!(keeper.status(&store, false)["state"], "unsupported");
        assert_eq!(keeper.status(&store, true)["state"], "updating");
    }

    #[test]
    fn an_ask_during_a_download_runs_once_it_ends() {
        let dir = tempfile::tempdir().unwrap();
        let keeper = Keeper::new(dir.path());
        {
            let mut state = lock(&keeper.state);
            state.fetching = true;
            state.error = Some("easylist.to answered 503".into());
            state.last_try = Some(Instant::now());
        }
        keeper.changed(true);
        assert!(!keeper.may_fetch(), "never two downloads at once");
        // The download ends, failed, a moment ago: without the kept ask it
        // would wait an hour.
        lock(&keeper.state).fetching = false;
        assert!(keeper.may_fetch(), "the user's ask is still there");
        // Used once: the next look waits for the retry hour again.
        assert!(!keeper.may_fetch());
    }

    #[test]
    fn sites_are_host_names() {
        assert!(is_site("example.com"));
        assert!(is_site("192.168.1.20"));
        assert!(is_site("localhost"));
        assert!(!is_site("https://example.com/"));
        assert!(!is_site("exa mple.com"));
        assert!(!is_site("a..b"));
        assert!(!is_site(""));
    }
}
