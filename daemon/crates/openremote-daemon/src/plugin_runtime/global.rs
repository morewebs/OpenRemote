//! Global plugins: installed for every device of the account. Each device
//! keeps the account's records (`GlobalPluginRecord`) and trades them over
//! Cloud, keeping the latest write: when two devices meet they swap the
//! whole list, and a change goes straight to every device online.
//!
//! A record is news, never an approval. A device that learns of a Global
//! plugin it lacks (or a newer copy of one it has) lists it under Plugins;
//! the user reviews it there like any other install - the copy is fetched
//! from a device that has it, over the same end-to-end encrypted mesh, is
//! offered only when it is exactly the bytes the record names, and goes
//! through the same review screen and the same install, where its powers
//! are read from the copy itself and approved here. Nothing a peer sends
//! installs, updates or grants anything.
//!
//! A record marked removed takes the Global copy off this device: a removal
//! only ever takes powers away. Turning a Global plugin off stays on the
//! device that did it, and so do its settings - secrets never leave the
//! device they were typed on.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::extract::{Path as AxumPath, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use base64::Engine as _;
use openremote_cloud::PeerContext;
use openremote_cloud::rpc::Request;
use openremote_core::{DeviceId, GlobalPluginRecord, InstalledPlugin, PluginScope, Store};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::app::App;
use crate::supervisor::lock_recovering;

/// Archive bytes per slice: base64 of this stays well inside the mesh's
/// largest answer.
const SLICE: usize = 4 * 1024 * 1024;
/// A Global plugin's archive is refused past this size.
const MAX_ARCHIVE: usize = 256 * 1024 * 1024;
/// A packed archive is kept this long for the slices after the first.
const PACKED_FOR: Duration = Duration::from_secs(300);
/// One request to a peer.
const PEER_TIME: Duration = Duration::from_secs(120);

/// A Global plugin's folder packed for another device's copy.
pub struct Packed {
    id: String,
    resolved: String,
    bytes: Arc<Vec<u8>>,
    at: Instant,
}

/// Tell the account that this device installed (or removed) a Global
/// plugin: the record is kept here and sent to every device online now.
/// `resolved` is the installed folder's content hash.
pub async fn publish(app: &Arc<App>, plugin: &InstalledPlugin, resolved: String, removed: bool) {
    let record = {
        let mut store = lock_recovering(&app.store);
        let record = GlobalPluginRecord {
            name: plugin.name.clone(),
            marketplace: plugin.marketplace.clone(),
            version: plugin.version.clone(),
            description: plugin.description.clone(),
            runtime: plugin.runtime,
            powers: plugin.powers.clone(),
            source: plugin.source.clone(),
            resolved,
            at: openremote_core::now_ms(),
            by: store.this_device(),
            removed,
        };
        let _ = store.merge_global_plugin(record.clone());
        record
    };
    if let Some(mesh) = app.cloud.mesh() {
        let note = json!({"type": "plugins.global", "record": record});
        for peer in mesh.online() {
            let _ = mesh.note(peer, &note).await;
        }
    }
}

/// Whether a note is one of this module's.
pub fn is_note(note: &Value) -> bool {
    note["type"].as_str() == Some("plugins.global")
}

/// A peer's note: one record changed.
pub fn on_note(app: &Arc<App>, _peer: PeerContext, note: Value) {
    let Ok(record) = serde_json::from_value::<GlobalPluginRecord>(note["record"].clone()) else {
        return;
    };
    let app = Arc::clone(app);
    tokio::spawn(async move {
        take(&app, record).await;
    });
}

/// Two devices met: take the peer's records, keep what is newer, and carry
/// out the removals this device has not yet.
pub async fn sync_with(app: &Arc<App>, peer: DeviceId) {
    let Some(mesh) = app.cloud.mesh() else {
        return;
    };
    if let Some(theirs) = peer_get(&mesh, peer, "/sync/plugins".into()).await {
        for raw in theirs["plugins"].as_array().into_iter().flatten() {
            if let Ok(record) = serde_json::from_value::<GlobalPluginRecord>(raw.clone()) {
                let _ = lock_recovering(&app.store).merge_global_plugin(record);
            }
        }
    }
    let removals: Vec<String> = {
        let store = lock_recovering(&app.store);
        store
            .global_plugins()
            .into_iter()
            .filter_map(|record| removal_here(&store, record))
            .collect()
    };
    for id in removals {
        follow_removal(app, &id).await;
    }
}

/// Keep one record from a peer; a kept removal is carried out here.
async fn take(app: &Arc<App>, record: GlobalPluginRecord) {
    let removal = {
        let mut store = lock_recovering(&app.store);
        match store.merge_global_plugin(record.clone()) {
            Ok(true) => removal_here(&store, &record),
            _ => None,
        }
    };
    if let Some(id) = removal {
        follow_removal(app, &id).await;
    }
}

/// The installed plugin a record takes off this device: only a removal, and
/// only this device's Global copy of it - never a plugin the user installed
/// here for this machine or a project. A live record takes nothing and
/// installs nothing.
pub fn removal_here(store: &Store, record: &GlobalPluginRecord) -> Option<String> {
    if !record.removed {
        return None;
    }
    let id = record.id();
    let here = store.installed_plugin(&id).ok()?;
    (here.scope == PluginScope::Global).then_some(id)
}

async fn follow_removal(app: &Arc<App>, id: &str) {
    match super::install::remove(app, id, false).await {
        Ok(()) => eprintln!("plugin {id}: removed, as another of your devices did"),
        // A chat on its harness holds it here; the next meeting tries again.
        Err((_, why)) => eprintln!("plugin {id}: another of your devices removed it: {why}"),
    }
}

/// A Global plugin of the account's that this device lacks, or has an
/// older copy of: listed for the user to review.
#[derive(Serialize, Debug, PartialEq)]
pub struct Pending {
    pub id: String,
    pub name: String,
    pub marketplace: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// `missing` (not on this device) or `update` (this device's Global
    /// copy is another version).
    pub why: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub by: Option<DeviceId>,
}

/// The account's Global plugins waiting for this device's review. A plugin
/// this device has for itself (Machine) is the user's own choice here, so
/// it is not offered again.
pub fn pending(store: &Store, data_dir: &Path) -> Vec<Pending> {
    store
        .global_plugins()
        .into_iter()
        .filter(|record| !record.removed)
        .filter_map(|record| {
            let id = record.id();
            let why = match store.installed_plugin(&id).ok() {
                None => "missing",
                Some(here) if here.scope == PluginScope::Global => {
                    let dir = super::scope::folder(data_dir, store, here)?;
                    if super::scope::unchanged(&dir, Some(&record.resolved)) {
                        return None;
                    }
                    "update"
                }
                Some(_) => return None,
            };
            Some(Pending {
                id,
                name: record.name.clone(),
                marketplace: record.marketplace.clone(),
                version: record.version.clone(),
                description: record.description.clone(),
                why,
                by: record.by,
            })
        })
        .collect()
}

/// Every record this device holds, for a peer.
pub fn records(store: &Store) -> Value {
    json!({"v": 1, "plugins": store.global_plugins()})
}

async fn peer_get(mesh: &openremote_cloud::Mesh, peer: DeviceId, path: String) -> Option<Value> {
    let response = mesh
        .request(
            peer,
            Request {
                method: "GET".into(),
                path,
                content_type: None,
                body: Vec::new(),
            },
            PEER_TIME,
        )
        .await
        .ok()?;
    (response.status == 200)
        .then(|| serde_json::from_slice(&response.body).ok())
        .flatten()
}

/// Fetch the account's copy of a Global plugin for this device's review:
/// from the device that last wrote its record, else any device online, and
/// only exactly the bytes the record names. What comes back is a folder to
/// stage, which the review reads and the user approves like any other.
pub async fn fetch_for_review(
    app: &Arc<App>,
    id: &str,
) -> Result<crate::plugins::StageRequest, (StatusCode, String)> {
    let record = lock_recovering(&app.store)
        .global_plugin(id)
        .filter(|r| !r.removed)
        .cloned()
        .ok_or((
            StatusCode::NOT_FOUND,
            format!("your account has no plugin {id} for every device"),
        ))?;
    let mesh = app.cloud.mesh().ok_or((
        StatusCode::CONFLICT,
        "this computer is not signed in to Cloud".to_string(),
    ))?;
    let online = mesh.online();
    let mut peers: Vec<DeviceId> = record
        .by
        .into_iter()
        .filter(|by| online.contains(by))
        .collect();
    peers.extend(online.into_iter().filter(|p| Some(*p) != record.by));
    let mut last = "none of your devices that has it is online".to_string();
    let mut bytes = None;
    for peer in peers {
        match fetch_archive(&mesh, peer, id, &record.resolved).await {
            Ok(got) => {
                bytes = Some(got);
                break;
            }
            Err(e) => last = e,
        }
    }
    let bytes = bytes.ok_or((StatusCode::BAD_GATEWAY, last))?;
    let folder = app
        .data_dir
        .join("plugins")
        .join(".staging")
        .join(format!("account-{}", uuid::Uuid::new_v4()));
    let checked = {
        let folder = folder.clone();
        let resolved = record.resolved.clone();
        tokio::task::spawn_blocking(move || unpack_exact(&bytes, &folder, &resolved))
            .await
            .map_err(|e| e.to_string())
            .and_then(|r| r)
    };
    if let Err(why) = checked {
        let _ = std::fs::remove_dir_all(&folder);
        return Err((StatusCode::CONFLICT, why));
    }
    Ok(crate::plugins::StageRequest {
        fetch: crate::plugins::Fetch::Folder(folder),
        marketplace: record.marketplace.clone(),
        entry_name: Some(record.name.clone()),
        entry: None,
        recorded: record.source.clone(),
    })
}

/// Unpack a copy and keep it only when it is exactly the approved bytes.
pub fn unpack_exact(bytes: &[u8], to: &Path, resolved: &str) -> Result<(), String> {
    crate::plugins::unpack(bytes, to).map_err(|e| e.to_string())?;
    let hash = crate::plugins::content_hash(to).map_err(|e| e.to_string())?;
    if hash != resolved {
        return Err("the copy that arrived is not the one your account installed".into());
    }
    Ok(())
}

async fn fetch_archive(
    mesh: &openremote_cloud::Mesh,
    peer: DeviceId,
    id: &str,
    resolved: &str,
) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    loop {
        let path = format!(
            "/sync/plugins/{}/archive?resolved={}&offset={}",
            encode(id),
            encode(resolved),
            bytes.len()
        );
        let slice = peer_get(mesh, peer, path)
            .await
            .ok_or("the device that has it didn't send it")?;
        let total = slice["total"].as_u64().unwrap_or(0) as usize;
        if total > MAX_ARCHIVE {
            return Err("the plugin is too large to copy".into());
        }
        let data = base64::engine::general_purpose::STANDARD
            .decode(slice["data"].as_str().unwrap_or_default())
            .map_err(|_| "the copy arrived damaged".to_string())?;
        if data.is_empty() && bytes.len() < total {
            return Err("the copy stopped short".into());
        }
        bytes.extend_from_slice(&data);
        if bytes.len() > total {
            return Err("the copy arrived damaged".into());
        }
        if bytes.len() == total {
            return Ok(bytes);
        }
    }
}

fn encode(raw: &str) -> String {
    raw.bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
                (b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect()
}

/// A folder as a tar.gz that comes out byte for byte the same every time
/// (sorted, no owners or times), so slices built on different requests fit
/// together.
pub fn pack(dir: &Path) -> std::io::Result<Vec<u8>> {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) -> std::io::Result<()> {
        let mut entries: Vec<_> = std::fs::read_dir(dir)?.flatten().collect();
        entries.sort_by_key(|e| e.file_name());
        for entry in entries {
            if entry.file_name() == ".git" {
                continue;
            }
            let kind = entry.file_type()?;
            if kind.is_dir() {
                walk(&entry.path(), out)?;
            } else if kind.is_file() {
                out.push(entry.path());
            }
        }
        Ok(())
    }
    let mut files = Vec::new();
    walk(dir, &mut files)?;
    let gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    let mut builder = tar::Builder::new(gz);
    builder.mode(tar::HeaderMode::Deterministic);
    for file in files {
        let relative = file.strip_prefix(dir).unwrap_or(&file);
        builder.append_path_with_name(&file, relative)?;
    }
    builder.into_inner()?.finish()
}

/// The routes a peer reaches (Sync tier: any device of the account).
pub fn peer_routes() -> Router<Arc<App>> {
    Router::new()
        .route("/sync/plugins", get(list_records))
        .route("/sync/plugins/{id}/archive", get(archive_slice))
}

async fn list_records(State(app): State<Arc<App>>) -> Response {
    Json(records(&lock_recovering(&app.store))).into_response()
}

#[derive(Deserialize)]
struct SliceQuery {
    resolved: String,
    #[serde(default)]
    offset: usize,
}

/// A slice of this device's copy of a Global plugin, for a device that
/// lacks it - only a Global plugin, and only when the copy here is exactly
/// the one asked for.
async fn archive_slice(
    State(app): State<Arc<App>>,
    AxumPath(id): AxumPath<String>,
    Query(query): Query<SliceQuery>,
) -> Response {
    let refuse =
        |status: StatusCode, why: &str| (status, Json(json!({"error": why}))).into_response();
    let dir = {
        let store = lock_recovering(&app.store);
        let Some(plugin) = store
            .installed_plugin(&id)
            .ok()
            .filter(|p| p.scope == PluginScope::Global)
        else {
            return refuse(StatusCode::NOT_FOUND, "no such Global plugin here");
        };
        crate::plugins::plugin_dir(&app.data_dir, &plugin.marketplace, &plugin.name)
    };
    if !super::scope::unchanged(&dir, Some(&query.resolved)) {
        return refuse(StatusCode::CONFLICT, "this device has another copy");
    }
    let archive = match app.plugin_runtime.packed(&id, &query.resolved, &dir).await {
        Ok(archive) => archive,
        Err(why) => return refuse(StatusCode::INTERNAL_SERVER_ERROR, &why),
    };
    let end = (query.offset.saturating_add(SLICE)).min(archive.len());
    let start = query.offset.min(end);
    Json(json!({
        "total": archive.len(),
        "offset": start,
        "data": base64::engine::general_purpose::STANDARD.encode(&archive[start..end]),
    }))
    .into_response()
}

impl super::PluginRuntime {
    /// A Global plugin's folder as one archive, built once per copy and
    /// kept a few minutes for the slices that follow.
    pub async fn packed(
        &self,
        id: &str,
        resolved: &str,
        dir: &Path,
    ) -> Result<Arc<Vec<u8>>, String> {
        let mut held = self.archive.lock().await;
        if let Some(p) = held.as_ref() {
            if p.id == id && p.resolved == resolved && p.at.elapsed() < PACKED_FOR {
                return Ok(Arc::clone(&p.bytes));
            }
        }
        let dir = dir.to_path_buf();
        let bytes = tokio::task::spawn_blocking(move || pack(&dir))
            .await
            .map_err(|e| e.to_string())?
            .map_err(|e| e.to_string())?;
        if bytes.len() > MAX_ARCHIVE {
            return Err("the plugin is too large to copy".into());
        }
        let bytes = Arc::new(bytes);
        *held = Some(Packed {
            id: id.to_string(),
            resolved: resolved.to_string(),
            bytes: Arc::clone(&bytes),
            at: Instant::now(),
        });
        Ok(bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use openremote_core::{PluginPower, PluginRuntime as Runtime, PluginSource};

    fn record(name: &str, removed: bool) -> GlobalPluginRecord {
        GlobalPluginRecord {
            name: name.into(),
            marketplace: "local".into(),
            version: Some("1.0.0".into()),
            description: None,
            runtime: Runtime::Wasm,
            // Whatever a peer claims, nothing is granted from it.
            powers: vec![
                PluginPower::AgentTools,
                PluginPower::Network,
                PluginPower::RunPrograms,
                PluginPower::ReadChats,
            ],
            source: PluginSource::Folder {
                path: "/elsewhere".into(),
            },
            resolved: "0".repeat(64),
            at: 10,
            by: Some(DeviceId::new()),
            removed,
        }
    }

    #[test]
    fn a_record_from_another_device_installs_nothing_and_waits_for_review() {
        let data = tempfile::tempdir().unwrap();
        let mut store = Store::open(data.path().to_path_buf()).unwrap();
        let live = record("shout", false);
        assert!(store.merge_global_plugin(live.clone()).unwrap());
        // Nothing is installed, so nothing runs and no power is held.
        assert!(store.installed_plugins().is_empty());
        assert_eq!(removal_here(&store, &live), None);
        // It is listed for this device's review instead.
        let waiting = pending(&store, data.path());
        assert_eq!(waiting.len(), 1);
        assert_eq!(
            (waiting[0].id.as_str(), waiting[0].why),
            ("shout@local", "missing")
        );
    }

    #[test]
    fn a_newer_record_for_a_global_plugin_here_is_an_update_to_review_not_an_install() {
        let data = tempfile::tempdir().unwrap();
        let mut store = Store::open(data.path().to_path_buf()).unwrap();
        let dir = crate::plugins::plugin_dir(data.path(), "local", "shout");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("openremote.json"), "{}").unwrap();
        let mut here = super::super::scope::tests::record("shout", PluginScope::Global, None);
        here.powers = vec![PluginPower::AgentTools];
        store.put_installed_plugin(here.clone()).unwrap();
        // The account's record names other bytes, and more powers.
        store.merge_global_plugin(record("shout", false)).unwrap();
        let waiting = pending(&store, data.path());
        assert_eq!(waiting.len(), 1);
        assert_eq!(waiting[0].why, "update");
        // The install here is untouched: its powers are what was approved here.
        let kept = store.installed_plugin("shout@local").unwrap();
        assert_eq!(kept.powers, here.powers);
        assert_eq!((kept.scope, &kept.resolved), (here.scope, &here.resolved));
        // Once the account's record names exactly these bytes, nothing waits.
        let mut same = record("shout", false);
        same.resolved = crate::plugins::content_hash(&dir).unwrap();
        same.at = 20;
        store.merge_global_plugin(same).unwrap();
        assert!(pending(&store, data.path()).is_empty());
    }

    #[test]
    fn a_removal_takes_only_the_global_copy_off_this_device() {
        let data = tempfile::tempdir().unwrap();
        let mut store = Store::open(data.path().to_path_buf()).unwrap();
        let removed = record("shout", true);
        // Not here at all: nothing to do.
        assert_eq!(removal_here(&store, &removed), None);
        // Here for this machine, the user's own choice: kept.
        store
            .put_installed_plugin(super::super::scope::tests::record(
                "shout",
                PluginScope::Machine,
                None,
            ))
            .unwrap();
        assert_eq!(removal_here(&store, &removed), None);
        assert!(
            pending(&store, data.path()).is_empty(),
            "a Machine copy isn't offered"
        );
        // Here as the account's Global copy: it goes.
        store
            .put_installed_plugin(super::super::scope::tests::record(
                "shout",
                PluginScope::Global,
                None,
            ))
            .unwrap();
        assert_eq!(
            removal_here(&store, &removed).as_deref(),
            Some("shout@local")
        );
        // A removed record is never offered for review.
        store.merge_global_plugin(removed).unwrap();
        assert!(pending(&store, data.path()).is_empty());
    }

    #[test]
    fn a_copy_is_kept_only_when_it_is_exactly_the_approved_bytes() {
        let from = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(from.path().join("ui")).unwrap();
        std::fs::write(from.path().join("openremote.json"), r#"{"name":"shout"}"#).unwrap();
        std::fs::write(from.path().join("ui/index.html"), "<p>hi</p>").unwrap();
        let resolved = crate::plugins::content_hash(from.path()).unwrap();
        let packed = pack(from.path()).unwrap();
        // Packing is deterministic, so slices of two packs fit together.
        assert_eq!(packed, pack(from.path()).unwrap());
        let to = tempfile::tempdir().unwrap();
        unpack_exact(&packed, &to.path().join("ok"), &resolved).unwrap();
        assert_eq!(
            crate::plugins::content_hash(&to.path().join("ok")).unwrap(),
            resolved
        );
        let err = unpack_exact(&packed, &to.path().join("bad"), &"f".repeat(64)).unwrap_err();
        assert!(err.contains("not the one"), "{err}");
    }
}
