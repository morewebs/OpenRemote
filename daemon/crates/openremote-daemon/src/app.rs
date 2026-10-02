//! App assembly: data dir, token, store, supervisor, and the serving loop.

use std::path::PathBuf;
use std::sync::{Arc, Mutex as StdMutex};
use std::time::Duration;

use futures_util::stream::Stream;
use openremote_core::{SessionId, Store};
use tokio::sync::mpsc;

use crate::http;
use crate::registry::HarnessRegistry;
use crate::supervisor::Supervisor;

pub struct App {
    pub token: String,
    pub data_dir: PathBuf,
    pub store: Arc<StdMutex<Store>>,
    pub supervisor: Arc<Supervisor>,
}

pub struct AppOptions {
    pub data_dir: PathBuf,
    pub token: String,
}

impl App {
    pub async fn new(options: AppOptions) -> Arc<Self> {
        let store = Store::open(options.data_dir.clone()).expect("store opens");
        let store = Arc::new(StdMutex::new(store));
        let supervisor = Supervisor::new(
            store.clone(),
            HarnessRegistry::probe(&env_overrides()).await,
        );
        Arc::new(Self {
            token: options.token,
            data_dir: options.data_dir,
            store,
            supervisor,
        })
    }
}

/// Debug/testing escape hatch: `OPENREMOTE_<HARNESS>_PATH` forces a
/// harness's binary (e.g. a fixture agent) without touching PATH.
fn env_overrides() -> std::collections::HashMap<String, PathBuf> {
    let mut map = std::collections::HashMap::new();
    for (key, value) in std::env::vars_os() {
        let Some(key) = key.to_str() else { continue };
        let Some(rest) = key.strip_prefix("OPENREMOTE_") else {
            continue;
        };
        let Some(harness) = rest.strip_suffix("_PATH") else {
            continue;
        };
        if !harness.is_empty() {
            map.insert(harness.to_ascii_lowercase(), PathBuf::from(value));
        }
    }
    map
}

/// Default data dir: `~/.openremote` (where the desktop shell reads the
/// token from). `OPENREMOTE_DATA_DIR` overrides — the e2e suite and any
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
pub fn load_or_create_token(data_dir: &PathBuf) -> std::io::Result<String> {
    let path = data_dir.join("token");
    if let Ok(token) = std::fs::read_to_string(&path) {
        let token = token.trim().to_string();
        if !token.is_empty() {
            return Ok(token);
        }
    }
    std::fs::create_dir_all(data_dir)?;
    let token = uuid::Uuid::new_v4().to_string();
    std::fs::write(&path, &token)?;
    Ok(token)
}

/// Serve the API on `listener`; runs until the process ends.
pub async fn serve(app: Arc<App>, listener: tokio::net::TcpListener) -> std::io::Result<()> {
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
        // The cursor: the highest seq the client already holds. `None` is a
        // fresh connect — replay everything, then accept any live seq.
        let mut last: Option<u64> = after;
        let replay = {
            let store = app.store.lock().expect("store lock");
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
                                let store = app.store.lock().expect("store lock");
                                store.events_after(&session_id, last).unwrap_or_default()
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
}
