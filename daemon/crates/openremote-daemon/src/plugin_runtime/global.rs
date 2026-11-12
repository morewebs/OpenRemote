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
//! A record marked removed turns this device's Global copy off - it only
//! ever takes powers away - and lists it as removed on another device. The
//! copy, its settings and its secrets go only when the user here confirms
//! the removal. A removal applies only when it is newer than this device's
//! own install of the plugin, so an old removal can't take off a plugin
//! installed again since. Turning a Global plugin off stays on the device
//! that did it, and so do its settings - secrets never leave the device
//! they were typed on.

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
/// How far ahead of this device's clock a peer's record may be dated: a
/// record dated later would win every meeting from then on.
const SKEW_MS: i64 = 5 * 60 * 1000;

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
    // The archive every device packs of these bytes, so a copy is checked
    // before a byte of it is unpacked.
    let archive = if removed {
        None
    } else {
        let dir = crate::plugins::plugin_dir(&app.data_dir, &plugin.marketplace, &plugin.name);
        let packed = tokio::task::spawn_blocking(move || pack(&dir).map(|bytes| sha256(&bytes)))
            .await
            .map_err(|e| std::io::Error::other(e.to_string()))
            .and_then(|r| r);
        match packed {
            Ok(sha) => Some(sha),
            // A record no device could check a copy against is one no
            // device could install: it is not told to the account.
            Err(e) => {
                eprintln!(
                    "plugin {}: not told to your other devices, its folder can't be packed: {e}",
                    plugin.id()
                );
                return;
            }
        }
    };
    let record = {
        let mut store = lock_recovering(&app.store);
        let held = store.global_plugin(&plugin.id()).map(|r| r.at);
        let record = GlobalPluginRecord {
            name: plugin.name.clone(),
            marketplace: plugin.marketplace.clone(),
            version: plugin.version.clone(),
            description: plugin.description.clone(),
            runtime: plugin.runtime,
            powers: plugin.powers.clone(),
            // A folder or file on this computer is named by its kind only.
            source: super::scope::shareable_source(&plugin.source),
            resolved,
            archive,
            at: local_at(openremote_core::now_ms(), held),
            by: store.this_device(),
            removed,
        };
        // This device's own word replaces what it holds, whatever its date.
        let _ = store.put_global_plugin(record.clone());
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

/// When this device's own write is dated: now, and always after the
/// record it replaces, so it wins every meeting that follows.
pub fn local_at(now: i64, held: Option<i64>) -> i64 {
    held.map_or(now, |held| now.max(held.saturating_add(1)))
}

/// A peer's record dated no later than a little past this device's clock.
pub fn clamp_at(at: i64, now: i64) -> i64 {
    at.min(now.saturating_add(SKEW_MS))
}

pub fn sha256(bytes: &[u8]) -> String {
    use sha2::Digest;
    crate::plugins::hex(&sha2::Sha256::digest(bytes))
}

/// A peer's note: one record changed. It is the sender's own word, so the
/// sender is who wrote it.
pub fn on_note(app: &Arc<App>, peer: PeerContext, note: Value) {
    let Ok(mut record) = serde_json::from_value::<GlobalPluginRecord>(note["record"].clone())
    else {
        return;
    };
    record.by = Some(peer.device_id);
    record.at = clamp_at(record.at, openremote_core::now_ms());
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
            if let Ok(mut record) = serde_json::from_value::<GlobalPluginRecord>(raw.clone()) {
                record.at = clamp_at(record.at, openremote_core::now_ms());
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

/// Whether a removal record is about this device's Global copy, written
/// after this device last installed or changed it.
fn removes(store: &Store, record: &GlobalPluginRecord) -> Option<InstalledPlugin> {
    if !record.removed {
        return None;
    }
    let here = store.installed_plugin(&record.id()).ok()?;
    (here.scope == PluginScope::Global && record.at > here.updated_at).then(|| here.clone())
}

/// The installed plugin a record turns off on this device: only a removal
/// newer than this device's own install, and only this device's Global copy,
/// never a plugin the user installed here for this machine or a project. A
/// live record takes nothing and installs nothing.
pub fn removal_here(store: &Store, record: &GlobalPluginRecord) -> Option<String> {
    removes(store, record)
        .filter(|here| here.enabled)
        .map(|here| here.id())
}

/// Another device removed it: here it is turned off, keeping its files,
/// settings and secrets until this device's user confirms the removal.
async fn follow_removal(app: &Arc<App>, id: &str) {
    let off = lock_recovering(&app.store).turn_off_installed_plugin_keeping_time(id);
    if off.is_ok() {
        app.plugin_runtime.changed(id).await;
        app.supervisor.reprobe_harnesses().await;
        eprintln!("plugin {id}: turned off, as another of your devices removed it");
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
    /// `missing` (not on this device), `update` (this device's Global copy
    /// is another version) or `removed` (another device removed it; it is
    /// off here until this device's user removes it or turns it back on).
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
        .filter_map(|record| {
            let id = record.id();
            if record.removed {
                removes(store, record).filter(|here| !here.enabled)?;
            }
            let why = match store.installed_plugin(&id).ok() {
                _ if record.removed => "removed",
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
    // Nothing is asked of another device for a record no copy could be
    // checked against.
    let archive = record.archive.clone().ok_or((
        StatusCode::CONFLICT,
        "the account's record names no archive to check a copy against; install it again on the device that has it".to_string(),
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
    let bytes = first_matching(&peers, &archive, |peer| {
        let (mesh, resolved) = (Arc::clone(&mesh), record.resolved.clone());
        async move { fetch_archive(&mesh, peer, id, &resolved).await }
    })
    .await
    .map_err(|why| (StatusCode::BAD_GATEWAY, why))?;
    let folder = app
        .data_dir
        .join("plugins")
        .join(".staging")
        .join(format!("account-{}", uuid::Uuid::new_v4()));
    let checked = {
        let folder = folder.clone();
        let resolved = record.resolved.clone();
        tokio::task::spawn_blocking(move || unpack_exact(&bytes, &archive, &folder, &resolved))
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

/// The first device's copy that is exactly the recorded archive (a .tar.gz
/// with its sha256): a device that sends another copy is passed over for
/// the next, and with none left the last reason is said.
pub async fn first_matching<F, Fut>(
    peers: &[DeviceId],
    archive: &str,
    mut fetch: F,
) -> Result<Vec<u8>, String>
where
    F: FnMut(DeviceId) -> Fut,
    Fut: std::future::Future<Output = Result<Vec<u8>, String>>,
{
    let mut last = "none of your devices that has it is online".to_string();
    for peer in peers {
        match fetch(*peer).await {
            Ok(bytes) if !bytes.starts_with(&[0x1f, 0x8b]) => {
                last =
                    "the copy that arrived is not the one your account installed (not a .tar.gz)"
                        .into();
            }
            Ok(bytes) if sha256(&bytes) != archive => {
                last = "the copy that arrived is not the one your account installed".into();
            }
            Ok(bytes) => return Ok(bytes),
            Err(e) => last = e,
        }
    }
    Err(last)
}

/// Unpack a copy and keep it only when it is exactly the approved bytes:
/// the archive is checked against the record's sha256 before anything is
/// unpacked, only a gzip tar is taken, and it unpacks within the size cap.
pub fn unpack_exact(bytes: &[u8], archive: &str, to: &Path, resolved: &str) -> Result<(), String> {
    if sha256(bytes) != archive {
        return Err("the copy that arrived is not the one your account installed".into());
    }
    crate::plugins::unpack_tar_gz(bytes, to, MAX_ARCHIVE as u64).map_err(|e| e.to_string())?;
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
    let programs = programs(dir);
    let gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    let mut builder = tar::Builder::new(gz);
    for file in files {
        let relative = file.strip_prefix(dir).unwrap_or(&file);
        let name = relative.to_string_lossy().replace('\\', "/");
        let bytes = std::fs::read(&file)?;
        // Every header is written the same on every system: no time, no
        // owner, and a mode from the manifest rather than the file system
        // (a copy on Windows has no executable bit to read).
        let mut header = tar::Header::new_gnu();
        header.set_entry_type(tar::EntryType::Regular);
        header.set_size(bytes.len() as u64);
        header.set_mode(if programs.contains(&name) {
            0o755
        } else {
            0o644
        });
        header.set_mtime(0);
        header.set_uid(0);
        header.set_gid(0);
        builder.append_data(&mut header, &name, bytes.as_slice())?;
    }
    builder.into_inner()?.finish()
}

/// The files a plugin's manifest names as programs: its entry (a module or
/// a command) and its harness driver, as paths inside its folder.
fn programs(dir: &Path) -> std::collections::BTreeSet<String> {
    let raw: Value = std::fs::read(dir.join("openremote.json"))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default();
    let mut named = Vec::new();
    match &raw["entry"] {
        Value::String(path) => named.push(path.clone()),
        entry => named.extend(entry["command"].as_str().map(String::from)),
    }
    if let Some(drivers) = raw["harness"]["driver"].as_object() {
        named.extend(drivers.values().filter_map(Value::as_str).map(String::from));
    }
    named
        .into_iter()
        .map(|path| {
            let path = path.replace('\\', "/");
            let path = path
                .strip_prefix("${OPENREMOTE_PLUGIN_ROOT}/")
                .unwrap_or(&path)
                .to_string();
            path.strip_prefix("./").unwrap_or(&path).to_string()
        })
        .collect()
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
            archive: None,
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
    fn another_devices_removal_turns_it_off_and_waits_for_this_devices_word() {
        let data = tempfile::tempdir().unwrap();
        let mut store = Store::open(data.path().to_path_buf()).unwrap();
        let mut here = super::super::scope::tests::record("shout", PluginScope::Global, None);
        here.updated_at = 100;
        store.put_installed_plugin(here).unwrap();
        let mut removed = record("shout", true);
        removed.at = 150;
        store.merge_global_plugin(removed.clone()).unwrap();
        assert!(
            pending(&store, data.path()).is_empty(),
            "on: turned off first"
        );
        store
            .turn_off_installed_plugin_keeping_time("shout@local")
            .unwrap();
        // Off, still installed, and listed for this device's user to confirm.
        assert!(store.installed_plugin("shout@local").is_ok());
        assert_eq!(removal_here(&store, &removed), None, "nothing more to do");
        let waiting = pending(&store, data.path());
        assert_eq!(waiting.len(), 1);
        assert_eq!(waiting[0].why, "removed");
        // Turned back on here (a newer word of this device's): no longer.
        store
            .set_installed_plugin_enabled("shout@local", true)
            .unwrap();
        assert!(pending(&store, data.path()).is_empty());
    }

    #[test]
    fn dates_from_peers_are_held_to_this_clock_and_own_writes_come_after() {
        assert_eq!(clamp_at(1_000, 1_000), 1_000);
        assert_eq!(clamp_at(i64::MAX, 1_000), 1_000 + SKEW_MS);
        assert_eq!(local_at(1_000, None), 1_000);
        assert_eq!(local_at(1_000, Some(500)), 1_000);
        // A record held from a clock ahead of this one: still replaced, later.
        assert_eq!(local_at(1_000, Some(9_000)), 9_001);
    }

    #[tokio::test]
    async fn a_copy_that_does_not_match_is_passed_over_for_the_next_device() {
        let from = tempfile::tempdir().unwrap();
        std::fs::write(from.path().join("openremote.json"), "{}").unwrap();
        let good = pack(from.path()).unwrap();
        let archive = sha256(&good);
        let (bad_peer, good_peer) = (DeviceId::new(), DeviceId::new());
        let asked = std::sync::Mutex::new(Vec::new());
        let got = first_matching(&[bad_peer, good_peer], &archive, |peer| {
            asked.lock().unwrap().push(peer);
            let answer = if peer == bad_peer {
                Ok(b"\x1f\x8b another copy".to_vec())
            } else {
                Ok(good.clone())
            };
            async move { answer }
        })
        .await
        .expect("the second device's copy");
        assert_eq!(got, good);
        assert_eq!(*asked.lock().unwrap(), [bad_peer, good_peer]);
        // None matches: the last reason is said.
        let err = first_matching(&[bad_peer], &archive, |_| async {
            Ok(b"PK\x03\x04".to_vec())
        })
        .await
        .unwrap_err();
        assert!(err.contains("not the one"), "{err}");
    }

    #[test]
    fn a_pack_is_the_same_bytes_on_every_system() {
        let from = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(from.path().join("bin")).unwrap();
        std::fs::write(from.path().join("bin/run"), "program").unwrap();
        std::fs::write(from.path().join("tool.wasm"), "module").unwrap();
        std::fs::write(from.path().join("readme.txt"), "words").unwrap();
        std::fs::write(
            from.path().join("openremote.json"),
            json!({"entry": {"command": "${OPENREMOTE_PLUGIN_ROOT}/bin/run"}}).to_string(),
        )
        .unwrap();
        let modes = |bytes: &[u8]| -> Vec<(String, u32, u64)> {
            let mut archive =
                tar::Archive::new(flate2::read::GzDecoder::new(std::io::Cursor::new(bytes)));
            archive
                .entries()
                .unwrap()
                .map(|e| {
                    let e = e.unwrap();
                    let h = e.header();
                    (
                        e.path().unwrap().to_string_lossy().replace('\\', "/"),
                        h.mode().unwrap(),
                        h.mtime().unwrap(),
                    )
                })
                .collect()
        };
        // Modes follow the manifest, never the file system's own bits (a
        // Windows copy has none), and no time is recorded.
        let packed = pack(from.path()).unwrap();
        assert_eq!(
            modes(&packed),
            [
                ("bin/run".to_string(), 0o755, 0),
                ("openremote.json".to_string(), 0o644, 0),
                ("readme.txt".to_string(), 0o644, 0),
                ("tool.wasm".to_string(), 0o644, 0),
            ]
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let f = from.path().join("readme.txt");
            std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o700)).unwrap();
            assert_eq!(pack(from.path()).unwrap(), packed);
        }
    }

    #[test]
    fn a_stale_removal_does_not_take_off_a_fresh_reinstall() {
        let data = tempfile::tempdir().unwrap();
        let mut store = Store::open(data.path().to_path_buf()).unwrap();
        let mut here = super::super::scope::tests::record("shout", PluginScope::Global, None);
        here.updated_at = 100;
        store.put_installed_plugin(here).unwrap();
        // Removed elsewhere before this device installed it again: kept.
        let mut removed = record("shout", true);
        removed.at = 50;
        assert_eq!(removal_here(&store, &removed), None);
        // Removed after: it applies.
        removed.at = 150;
        assert_eq!(
            removal_here(&store, &removed).as_deref(),
            Some("shout@local")
        );
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
        let archive = sha256(&packed);
        unpack_exact(&packed, &archive, &to.path().join("ok"), &resolved).unwrap();
        assert_eq!(
            crate::plugins::content_hash(&to.path().join("ok")).unwrap(),
            resolved
        );
        let err =
            unpack_exact(&packed, &archive, &to.path().join("bad"), &"f".repeat(64)).unwrap_err();
        assert!(err.contains("not the one"), "{err}");
        // An archive other than the recorded one is refused before a byte of
        // it is unpacked.
        let err = unpack_exact(
            &packed,
            &"0".repeat(64),
            &to.path().join("early"),
            &resolved,
        )
        .unwrap_err();
        assert!(err.contains("not the one"), "{err}");
        assert!(!to.path().join("early").exists());
        // A zip is not how a copy travels, whatever its hash.
        let zip = b"PK\x03\x04 not really a zip".to_vec();
        let err = unpack_exact(&zip, &sha256(&zip), &to.path().join("zip"), &resolved).unwrap_err();
        assert!(err.contains("tar.gz"), "{err}");
        assert!(!to.path().join("zip").exists());
    }
}
