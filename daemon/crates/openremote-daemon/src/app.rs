//! App assembly: data dir, token, store, supervisor, and the serving loop.

use std::path::PathBuf;
use std::sync::{Arc, Mutex as StdMutex, OnceLock};
use std::time::Duration;

use futures_util::stream::Stream;
use openremote_core::{SessionId, Store};
use tokio::sync::mpsc;

use crate::http;
use crate::registry::{self, HarnessRegistry};
use crate::supervisor::Supervisor;

pub struct App {
    pub token: String,
    pub data_dir: PathBuf,
    pub store: Arc<StdMutex<Store>>,
    pub supervisor: Arc<Supervisor>,
    pub cloud: Arc<openremote_cloud::Cloud>,
    /// The loopback port the API listens on: the sign-in callback's address.
    pub port: OnceLock<u16>,
    pub sync: crate::sync::SyncState,
    /// The plugin folders on this machine and the installs awaiting an
    /// answer (Plugins).
    pub plugins: crate::plugins::PluginHost,
    /// The marketplaces' cached catalogs (Plugins > Discover).
    pub marketplaces: crate::marketplace::MarketplaceHost,
    /// @ file suggestions, a few seconds' listing per workspace.
    pub files: crate::files::FileIndex,
    /// The built-in browser: the app's tabs, the agents' tools, and the
    /// browser's own data (bookmarks, history, settings).
    pub browser: crate::browser::BrowserService,
}

pub struct AppOptions {
    pub data_dir: PathBuf,
    pub token: String,
    pub cloud: openremote_cloud::CloudConfig,
}

impl App {
    pub async fn new(options: AppOptions) -> Arc<Self> {
        let store = Store::open(options.data_dir.clone()).expect("store opens");
        let store = Arc::new(StdMutex::new(store));
        // The harnesses installed plugins declare are probed beside the six
        // built-ins. The store is the source: a plugin installed in an
        // earlier run is here before anything else starts.
        let plugins = {
            let store = crate::supervisor::lock_recovering(&store);
            crate::plugins::installed_harnesses(&options.data_dir, &store.installed_plugins())
        };
        let supervisor = Supervisor::new(
            store.clone(),
            HarnessRegistry::probe(&registry::env_overrides(), &plugins).await,
        );
        Self::assemble(options, store, supervisor)
    }

    /// The parts, already built (the e2e suites bring their own registry).
    pub fn assemble(
        options: AppOptions,
        store: Arc<StdMutex<Store>>,
        supervisor: Arc<Supervisor>,
    ) -> Arc<Self> {
        let cloud = openremote_cloud::Cloud::open(options.cloud, &options.data_dir);
        // The store learns this device's id from Cloud's own files, so the
        // two agree even when `enroll` ran in another process.
        {
            let mut store = crate::supervisor::lock_recovering(&store);
            let id = cloud.device_id();
            if id.is_some() && store.this_device() != id {
                let _ = store.set_this_device(id);
            }
            // OpenRemote's own marketplace and Anthropic's directory, each
            // added once: fetched the first time Discover opens.
            for preset in crate::marketplace::presets(openremote_core::now_ms()) {
                let _ = store.seed_marketplace(preset);
            }
            // Migrate an existing official marketplace record pointing at the
            // older separate morewebs/openremote-plugins repository, resetting its
            // source to the project's own repository and clearing any fetch error.
            if let Ok(record) = store.marketplace(crate::marketplace::OFFICIAL) {
                let stale = match &record.source {
                    openremote_core::MarketplaceSource::Github { repo, .. } => {
                        !repo.eq_ignore_ascii_case(crate::marketplace::OFFICIAL_REPO)
                    }
                    openremote_core::MarketplaceSource::Git { url, .. } => {
                        url.to_ascii_lowercase().contains("openremote-plugins")
                    }
                    _ => true,
                };
                if stale {
                    let mut updated = record.clone();
                    updated.source = openremote_core::MarketplaceSource::Github {
                        repo: crate::marketplace::OFFICIAL_REPO.to_string(),
                        reference: None,
                    };
                    updated.error = None;
                    let _ = store.put_marketplace(updated);
                }
            }
        }
        let plugins = crate::plugins::PluginHost::new(&options.data_dir);
        let marketplaces = crate::marketplace::MarketplaceHost::new(&options.data_dir);
        let browser = crate::browser::BrowserService::new(&options.data_dir);
        let app = Arc::new(Self {
            browser,
            token: options.token,
            data_dir: options.data_dir,
            plugins,
            marketplaces,
            files: crate::files::FileIndex::default(),
            store,
            supervisor,
            cloud,
            port: OnceLock::new(),
            sync: crate::sync::SyncState::default(),
        });
        crate::sync::start_pusher(&app);
        crate::browser::start_sweeper(&app);
        app.start_cloud();
        app
    }

    /// Brings the mesh up once this device has joined Cloud.
    pub fn start_cloud(self: &Arc<Self>) {
        self.cloud.start(Arc::new(crate::host::DaemonHost {
            app: Arc::downgrade(self),
        }));
    }
}

/// Default data dir: `~/.openremote` (where the desktop shell reads the
/// token from). `OPENREMOTE_DATA_DIR` overrides - the e2e suite and any
/// parallel daemon use it.
pub fn default_data_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("OPENREMOTE_DATA_DIR") {
        return PathBuf::from(dir);
    }
    let home = std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    home.join(".openremote")
}

/// The bearer token: read from `<data>/token` or generated on first run.
/// Whoever reads it controls every harness here, so the file and its
/// directory are this user's alone (older builds left them group-readable).
///
/// A token that exists but can't be READ (an antivirus lock on Windows,
/// a permissions blip) is never silently regenerated: the shell holds
/// the old one and would get 401 on everything while the daemon looks
/// "up". Read errors get a brief retry window and then a loud failure;
/// only a missing or empty file means "no token yet" and mints one.
pub fn load_or_create_token(data_dir: &std::path::Path) -> std::io::Result<String> {
    let path = data_dir.join("token");
    let mut unreadable = None;
    for attempt in 1..=5 {
        match std::fs::read_to_string(&path) {
            Ok(raw) => {
                let token = raw.trim().to_string();
                if !token.is_empty() {
                    openremote_core::fsx::restrict(&path)?;
                    openremote_core::fsx::create_private_dir(data_dir)?;
                    return Ok(token);
                }
                // An empty file is a truncated write, not a read
                // failure: mint again.
                unreadable = None;
                break;
            }
            // No file is the first run (or the token was never written).
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => break,
            Err(e) => {
                eprintln!("warning: can't read the token yet (attempt {attempt}/5): {e}");
                unreadable = Some(e);
                if attempt < 5 {
                    std::thread::sleep(Duration::from_millis(100));
                }
            }
        }
    }
    if let Some(e) = unreadable {
        return Err(std::io::Error::new(
            e.kind(),
            format!(
                "can't read the token at {} and won't mint a new one over it: {e}",
                path.display()
            ),
        ));
    }
    openremote_core::fsx::create_private_dir(data_dir)?;
    let token = uuid::Uuid::new_v4().to_string();
    openremote_core::fsx::write_private(&path, token.as_bytes())?;
    Ok(token)
}

/// Serve the API on `listener`; runs until the process ends.
pub async fn serve(app: Arc<App>, listener: tokio::net::TcpListener) -> std::io::Result<()> {
    if let Ok(addr) = listener.local_addr() {
        let _ = app.port.set(addr.port());
    }
    let router = http::router(Arc::clone(&app));
    axum::serve(listener, router).await
}

/// The SSE stream for one session: replay from the store after `after`,
/// then live from the broadcast, deduped by seq, with keep-alive comments.
///
/// Ordering that avoids both gaps and duplicates: subscribe to the
/// broadcast *first*, then replay from the store, then forward only
/// broadcast events whose seq is past the replayed tail.
pub fn event_stream(
    app: Arc<App>,
    session_id: SessionId,
    after: Option<u64>,
) -> impl Stream<Item = Result<Vec<u8>, std::convert::Infallible>> + Send + 'static {
    let (tx, rx) = mpsc::channel::<Vec<u8>>(64);
    tokio::spawn(async move {
        let mut live = app.supervisor.subscribe();
        // A copy of a chat running on another device: that device sends the
        // chat's events live while it is open here (the watch ends when
        // this stream does).
        let watch: Arc<StdMutex<Option<crate::sync::Watch>>> = Arc::default();
        {
            let (app, watch) = (Arc::clone(&app), Arc::clone(&watch));
            tokio::spawn(async move {
                let started = crate::sync::watch(&app, session_id).await;
                if let Ok(mut slot) = watch.lock() {
                    *slot = started;
                }
            });
        }
        let _watch = watch;
        // The cursor: the highest seq the client already holds. `None` is a
        // fresh connect - replay everything, then accept any live seq.
        let mut last: Option<u64> = after;
        let replay = {
            let store = crate::supervisor::lock_recovering(&app.store);
            store.events_after(&session_id, after).unwrap_or_default()
        };
        for event in replay {
            last = Some(last.map_or(event.seq, |l| l.max(event.seq)));
            if tx.send(sse_frame(&event)).await.is_err() {
                return;
            }
        }
        // Live phase.
        loop {
            let keep_alive = async {
                tokio::time::sleep(Duration::from_secs(15)).await;
            };
            let incoming = async {
                match live.recv().await {
                    Ok(raw) => {
                        if let Ok(event) = serde_json::from_str::<openremote_core::Event>(&raw) {
                            if event.session_id == session_id {
                                return Some(event);
                            }
                        }
                        None
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => None,
                    Err(_) => None,
                }
            };
            tokio::select! {
                biased;
                _ = keep_alive => {
                    if tx.send(b": keep-alive\n\n".to_vec()).await.is_err() {
                        return;
                    }
                }
                event = incoming => {
                    match event {
                        Some(event) => {
                            if last.is_none_or(|l| event.seq > l) {
                                last = Some(event.seq);
                                if tx.send(sse_frame(&event)).await.is_err() {
                                    return;
                                }
                            }
                        }
                        None => {
                            // A lag or a foreign session's event: catch up
                            // from the store so nothing is missed.
                            let missed = {
                                let store = crate::supervisor::lock_recovering(&app.store);
                                store
                                    .events_after(&session_id, last)
                                    .unwrap_or_default()
                            };
                            for event in missed {
                                if last.is_none_or(|l| event.seq > l) {
                                    last = Some(event.seq);
                                    if tx.send(sse_frame(&event)).await.is_err() {
                                        return;
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    });
    tokio_stream_from_channel(rx)
}

fn sse_frame(event: &openremote_core::Event) -> Vec<u8> {
    let json = serde_json::to_string(event).unwrap_or_default();
    format!("id: {}\ndata: {}\n\n", event.seq, json).into_bytes()
}

fn tokio_stream_from_channel(
    mut rx: mpsc::Receiver<Vec<u8>>,
) -> impl Stream<Item = Result<Vec<u8>, std::convert::Infallible>> + Send + 'static {
    futures_util::stream::poll_fn(move |cx| rx.poll_recv(cx).map(|item| item.map(Ok)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_round_trips_and_regenerates_only_when_missing() {
        let dir = std::env::temp_dir().join(format!("or-token-{}", uuid::Uuid::new_v4()));
        let first = load_or_create_token(&dir).unwrap();
        assert_eq!(load_or_create_token(&dir).unwrap(), first);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn an_empty_token_file_mints_and_the_new_one_sticks() {
        let dir = std::env::temp_dir().join(format!("or-token-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        // A zero-length token: a truncated write, not a secret worth keeping.
        std::fs::write(dir.join("token"), b"").unwrap();
        let minted = load_or_create_token(&dir).unwrap();
        assert!(!minted.is_empty());
        assert_eq!(load_or_create_token(&dir).unwrap(), minted);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_token_that_exists_but_can_t_be_read_fails_loudly() {
        let dir = std::env::temp_dir().join(format!("or-token-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        // A directory where the token should be: every platform turns that
        // into a read error that is not NotFound (Windows denies the
        // open, Unix refuses the read) - the same shape as an antivirus
        // lock or a permissions blip on the real file.
        std::fs::create_dir(dir.join("token")).unwrap();
        let err = load_or_create_token(&dir).unwrap_err();
        assert!(
            err.to_string().contains("won't mint a new one"),
            "the failure names the stakes: {err}"
        );
        assert!(
            dir.join("token").is_dir(),
            "no token was minted over the unreadable path"
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn stale_official_marketplace_record_migrates_on_startup() {
        let dir = std::env::temp_dir().join(format!("or-mig-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        {
            let mut store = openremote_core::Store::open(dir.clone()).unwrap();
            let stale = openremote_core::MarketplaceRecord {
                name: crate::marketplace::OFFICIAL.to_string(),
                source: openremote_core::MarketplaceSource::Github {
                    repo: "morewebs/OpenRemote".into(),
                    reference: None,
                },
                enabled: true,
                removable: false,
                owner: None,
                description: None,
                plugin_count: 0,
                added_at: 100,
                refreshed_at: None,
                error: Some(
                    "git could not fetch https://github.com/morewebs/OpenRemote.git".into(),
                ),
                heads: Default::default(),
            };
            store.seed_marketplace(stale).unwrap();
            let custom = openremote_core::MarketplaceRecord {
                name: "community".to_string(),
                source: openremote_core::MarketplaceSource::Github {
                    repo: "other/community-plugins".into(),
                    reference: None,
                },
                enabled: true,
                removable: true,
                owner: None,
                description: None,
                plugin_count: 0,
                added_at: 100,
                refreshed_at: None,
                error: Some("some error".into()),
                heads: Default::default(),
            };
            store.seed_marketplace(custom).unwrap();
        }
        let app = App::new(AppOptions {
            token: "test-token".into(),
            data_dir: dir.clone(),
            cloud: openremote_cloud::CloudConfig::from_env(),
        })
        .await;
        let store = crate::supervisor::lock_recovering(&app.store);
        let migrated = store.marketplace(crate::marketplace::OFFICIAL).unwrap();
        assert!(matches!(
            &migrated.source,
            openremote_core::MarketplaceSource::Github { repo, .. } if repo == crate::marketplace::OFFICIAL_REPO
        ));
        assert!(migrated.error.is_none());
        let custom = store.marketplace("community").unwrap();
        assert!(custom.error.is_some());
        drop(store);
        drop(app);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
