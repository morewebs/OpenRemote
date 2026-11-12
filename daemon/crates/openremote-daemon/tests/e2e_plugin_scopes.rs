//! Who a plugin is for, end to end (plan step 5): this machine, one
//! project (outside or inside its repo), or every device of the account.
//! Three promises are held here:
//!
//! - who a plugin is for never widens what it may do;
//! - a project's plugin runs only for that project's chats;
//! - a Global plugin arriving from another device installs nothing until
//!   the user reviews and approves it here, as any install.

mod common;

use std::path::Path;

use common::cloud::{device, until};
use common::*;
use fake_cloud::FakeCloud;
use serde_json::{Value, json};

/// A WASM tool plugin (no program, so nothing to build): its one tool
/// answers "wasm". It asks for no power; reading it implies `agent_tools`.
fn wasm_plugin() -> tempfile::TempDir {
    let folder = tempfile::tempdir().expect("plugin folder");
    write_module(folder.path(), "wasm");
    std::fs::create_dir_all(folder.path().join("ui")).unwrap();
    std::fs::write(folder.path().join("ui/page.html"), "reviewed").unwrap();
    std::fs::write(
        folder.path().join("openremote.json"),
        json!({
            "api": 1,
            "name": "wasm-tools",
            "runtime": "wasm",
            "entry": "tool.wasm",
            "contributes": {"tools": [{"name": "hello", "description": "Say hello"}]}
        })
        .to_string(),
    )
    .unwrap();
    folder
}

/// `tool.wasm` in `folder`, its one tool answering `word`. Words of one
/// length make modules of one size.
fn write_module(folder: &Path, word: &str) {
    let answer = format!(r#"{{"content":[{{"type":"text","text":"{word}"}}]}}"#);
    let wat = format!(
        r#"(module
            (memory (export "memory") 1)
            (data (i32.const 1024) "{}")
            (func (export "or_alloc") (param i32) (result i32) (i32.const 4096))
            (func (export "or_call") (param i32 i32) (result i64)
                (i64.or (i64.shl (i64.const 1024) (i64.const 32)) (i64.const {}))))"#,
        answer.replace('"', "\\\""),
        answer.len()
    );
    std::fs::write(
        folder.join("tool.wasm"),
        wat::parse_str(&wat).expect("assemble"),
    )
    .unwrap();
}

/// A directory link at `link` to `target`: a junction on Windows (which
/// needs no privilege), a symlink elsewhere.
fn link_dir(link: &Path, target: &Path) {
    #[cfg(windows)]
    {
        let made = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(link)
            .arg(target)
            .output()
            .is_ok_and(|o| o.status.success());
        assert!(made, "mklink /J could not make a junction");
    }
    #[cfg(unix)]
    std::os::unix::fs::symlink(target, link).unwrap();
}

/// Every file under `dir`, with its bytes, to see that nothing changed.
fn tree(dir: &Path) -> Vec<(std::path::PathBuf, Vec<u8>)> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(dir).unwrap().flatten() {
        let path = entry.path();
        if path.is_dir() {
            out.extend(tree(&path));
        } else {
            out.push((path.clone(), std::fs::read(&path).unwrap()));
        }
    }
    out.sort();
    out
}

async fn stage_source(daemon: &TestDaemon, source: Value) -> Reply {
    call(
        daemon,
        "POST",
        "/plugins/stage",
        Some(json!({"request_id": uuid::Uuid::new_v4().to_string(), "source": source})),
    )
    .await
}

async fn stage_folder(daemon: &TestDaemon, folder: &Path) -> String {
    let staged = stage_source(daemon, json!({"kind": "folder", "path": folder})).await;
    assert_eq!(staged.status, 200, "{}", staged.raw);
    staged.body["result"]["stage_id"]
        .as_str()
        .expect("stage id")
        .to_string()
}

/// Approve a staged plugin with an install body of `extra` fields.
async fn approve(daemon: &TestDaemon, stage_id: &str, extra: Value) -> Reply {
    let mut body = json!({"request_id": uuid::Uuid::new_v4().to_string()});
    for (k, v) in extra.as_object().unwrap() {
        body[k] = v.clone();
    }
    call(
        daemon,
        "POST",
        &format!("/plugins/stage/{stage_id}/install"),
        Some(body),
    )
    .await
}

async fn remove(daemon: &TestDaemon, id: &str) {
    let removed = call(
        daemon,
        "POST",
        &format!("/plugins/{}/remove", id.replace('~', "%7E")),
        Some(json!({"request_id": uuid::Uuid::new_v4().to_string()})),
    )
    .await;
    assert_eq!(removed.status, 200, "{}", removed.raw);
}

async fn project(daemon: &TestDaemon, folder: &Path) -> String {
    let saved = call(
        daemon,
        "POST",
        "/projects",
        Some(json!({"request_id": uuid::Uuid::new_v4().to_string(), "folders": [folder]})),
    )
    .await;
    assert_eq!(saved.status, 201, "{}", saved.raw);
    saved.body["id"].as_str().expect("project id").to_string()
}

async fn chat_in(daemon: &TestDaemon, folder: &Path) -> String {
    let created = call(
        daemon,
        "POST",
        "/sessions",
        Some(json!({
            "request_id": uuid::Uuid::new_v4().to_string(),
            "harness": "claude",
            "workspace": folder,
        })),
    )
    .await;
    assert_eq!(created.status, 201, "{}", created.raw);
    created.body["id"].as_str().expect("chat id").to_string()
}

async fn tool_names(daemon: &TestDaemon, chat: &str) -> Vec<String> {
    let listed = call(
        daemon,
        "GET",
        &format!("/sessions/{chat}/plugins/tools"),
        None,
    )
    .await;
    assert_eq!(listed.status, 200, "{}", listed.raw);
    listed.body["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap().to_string())
        .collect()
}

async fn call_hello(daemon: &TestDaemon, chat: &str) -> Value {
    let reply = call(
        daemon,
        "POST",
        &format!("/sessions/{chat}/plugins/call"),
        Some(json!({"name": "wasm-tools__hello", "arguments": {}})),
    )
    .await;
    assert_eq!(reply.status, 200, "{}", reply.raw);
    reply.body
}

#[tokio::test]
async fn who_a_plugin_is_for_never_widens_what_it_may_do() {
    let daemon = start_daemon(&[]).await;
    let folder = wasm_plugin();
    let repo = workspace(None);
    let p = project(&daemon, repo.path()).await;
    // Every scope, each asked for with powers slipped into the request: the
    // record holds what the review read from the plugin, and nothing more.
    let greedy = json!(["run_programs", "network", "read_chats", "microphone"]);
    for (scope, project) in [
        ("machine", None),
        ("global", None),
        ("project_local", Some(p.clone())),
        ("project", Some(p.clone())),
    ] {
        let stage_id = stage_folder(&daemon, folder.path()).await;
        let mut extra = json!({"scope": scope, "powers": greedy, "not_sandboxed": false});
        if let Some(project) = &project {
            extra["project"] = json!(project);
        }
        let installed = approve(&daemon, &stage_id, extra).await;
        assert_eq!(installed.status, 200, "{scope}: {}", installed.raw);
        let record = &installed.body["result"];
        assert_eq!(record["scope"], scope);
        assert_eq!(record["powers"], json!(["agent_tools"]), "{scope}");
        let id = record["id"].as_str().unwrap().to_string();
        let listed = call(&daemon, "GET", "/plugins", None).await;
        let here = listed.body["installed"]
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p["id"] == id)
            .cloned()
            .expect("listed");
        assert_eq!(here["powers"], json!(["agent_tools"]), "{scope}");
        remove(&daemon, &id).await;
    }
    // The Global install told the account, and its removal did too.
    let records = call(&daemon, "GET", "/sync/plugins", None).await;
    assert_eq!(records.status, 200, "{}", records.raw);
    assert_eq!(records.body["plugins"][0]["name"], "wasm-tools");
    // A folder on this computer is named to the account by its kind only.
    let folder_name = folder.path().file_name().unwrap().to_string_lossy();
    assert!(!records.raw.contains(&*folder_name), "{}", records.raw);
    assert_eq!(records.body["plugins"][0]["removed"], true);
}

#[tokio::test]
async fn a_project_plugin_runs_only_for_its_own_projects_chats() {
    let daemon = start_daemon(&[]).await;
    let folder = wasm_plugin();
    let (repo_a, repo_b, loose) = (workspace(None), workspace(None), workspace(None));
    let a = project(&daemon, repo_a.path()).await;
    let _b = project(&daemon, repo_b.path()).await;

    let stage_id = stage_folder(&daemon, folder.path()).await;
    let installed = approve(
        &daemon,
        &stage_id,
        json!({"scope": "project", "project": a}),
    )
    .await;
    assert_eq!(installed.status, 200, "{}", installed.raw);
    let id = installed.body["result"]["id"].as_str().unwrap().to_string();
    assert_eq!(id, format!("wasm-tools@local~{a}"));
    // Its files went into the repo, with a note of where it came from.
    let in_repo = repo_a.path().join(".openremote/plugins/wasm-tools");
    assert!(in_repo.join("tool.wasm").is_file());
    let index = std::fs::read_to_string(repo_a.path().join(".openremote/plugins.json")).unwrap();
    assert!(index.contains("wasm-tools") && !index.contains(&*folder.path().to_string_lossy()));

    let (in_a, in_b, elsewhere) = (
        chat_in(&daemon, repo_a.path()).await,
        chat_in(&daemon, repo_b.path()).await,
        chat_in(&daemon, loose.path()).await,
    );
    assert_eq!(tool_names(&daemon, &in_a).await, ["wasm-tools__hello"]);
    let result = call_hello(&daemon, &in_a).await;
    assert_eq!(result["content"][0]["text"], "wasm", "{result}");
    for chat in [&in_b, &elsewhere] {
        assert!(tool_names(&daemon, chat).await.is_empty());
        let refused = call_hello(&daemon, chat).await;
        assert_eq!(refused["isError"], true, "{refused}");
    }

    // Someone changes the plugin in the repo: it stops running until this
    // machine reviews the new files.
    let files = format!("/plugins/{id}/files/openremote.json");
    assert_eq!(call(&daemon, "GET", &files, None).await.status, 200);
    std::fs::write(in_repo.join("added.txt"), "changed upstream").unwrap();
    // Its views are not served from the changed files either.
    assert_eq!(call(&daemon, "GET", &files, None).await.status, 409);
    assert!(tool_names(&daemon, &in_a).await.is_empty());
    assert_eq!(call_hello(&daemon, &in_a).await["isError"], true);
    let listed = call(&daemon, "GET", "/plugins", None).await;
    assert_eq!(listed.body["found"][0]["why"], "changed", "{}", listed.raw);
    assert_eq!(listed.body["found"][0]["project"], a);
    // Reviewed and approved again, for its project: it runs again.
    let staged = stage_source(
        &daemon,
        json!({"kind": "project", "project": a, "name": "wasm-tools"}),
    )
    .await;
    assert_eq!(staged.status, 200, "{}", staged.raw);
    assert_eq!(staged.body["result"]["for_project"], a);
    assert_eq!(staged.body["result"]["id"], id);
    let stage_id = staged.body["result"]["stage_id"].as_str().unwrap();
    let again = approve(&daemon, stage_id, json!({"scope": "project", "project": a})).await;
    assert_eq!(again.status, 200, "{}", again.raw);
    assert_eq!(tool_names(&daemon, &in_a).await, ["wasm-tools__hello"]);
    let listed = call(&daemon, "GET", "/plugins", None).await;
    assert_eq!(listed.body["found"], json!([]));

    // Removed: its files leave the repo too.
    remove(&daemon, &id).await;
    assert!(!in_repo.exists());
}

#[tokio::test]
async fn a_project_install_names_its_project_and_clashes_only_where_chats_meet() {
    let daemon = start_daemon(&[]).await;
    let folder = wasm_plugin();
    let repo = workspace(None);
    let p = project(&daemon, repo.path()).await;
    for (scope, project, status) in [
        ("project", None, 400),
        ("project_local", None, 400),
        ("project", Some(uuid::Uuid::new_v4().to_string()), 404),
    ] {
        let stage_id = stage_folder(&daemon, folder.path()).await;
        let mut extra = json!({"scope": scope});
        if let Some(project) = project {
            extra["project"] = json!(project);
        }
        let refused = approve(&daemon, &stage_id, extra).await;
        assert_eq!(refused.status, status, "{scope}: {}", refused.raw);
    }
    let listed = call(&daemon, "GET", "/plugins", None).await;
    assert_eq!(listed.body["installed"], json!([]), "nothing was installed");

    // Installed for this machine, the same plugin for one project would
    // ride that project's chats twice: refused, in its own words.
    let stage_id = stage_folder(&daemon, folder.path()).await;
    let machine = approve(&daemon, &stage_id, json!({})).await;
    assert_eq!(machine.status, 200, "{}", machine.raw);
    let stage_id = stage_folder(&daemon, folder.path()).await;
    let clash = approve(
        &daemon,
        &stage_id,
        json!({"scope": "project_local", "project": p}),
    )
    .await;
    assert_eq!(clash.status, 409, "{}", clash.raw);
    assert!(
        clash.raw.contains("already installed for this machine"),
        "{}",
        clash.raw
    );
    assert!(!clash.raw.contains("from local"), "{}", clash.raw);
}

/// Two of an account's devices: a Global plugin installed on one reaches
/// the other as news to review. Nothing installs, and no power is held,
/// until the user approves it there - and then its powers are the ones read
/// from the copy on that device.
#[tokio::test]
async fn a_global_plugin_from_another_device_waits_for_this_devices_approval() {
    let cloud = FakeCloud::start().await;
    let (a, _id_a) = device(&cloud, "desk-a").await;
    let (b, id_b) = device(&cloud, "desk-b").await;
    until("a sees b online", || {
        common::cloud::sees(&a, &id_b, "online", json!(true))
    })
    .await;

    let folder = wasm_plugin();
    let stage_id = stage_folder(&a, folder.path()).await;
    let installed = approve(&a, &stage_id, json!({"scope": "global"})).await;
    assert_eq!(installed.status, 200, "{}", installed.raw);

    // b hears of it, and lists it for review - installed, it is not.
    let pending = until("b lists the account's plugin", || async {
        let listed = call(&b, "GET", "/plugins", None).await;
        let pending = listed.body["global_pending"].as_array()?.first()?.clone();
        Some((pending, listed.body["installed"].clone()))
    })
    .await;
    assert_eq!(pending.0["id"], "wasm-tools@local");
    assert_eq!(pending.0["why"], "missing");
    assert_eq!(pending.1, json!([]), "a record installs nothing");
    let chat = chat_in(&b, workspace(None).path()).await;
    assert!(tool_names(&b, &chat).await.is_empty());
    assert_eq!(call_hello(&b, &chat).await["isError"], true);

    // The user reviews it on b: the copy comes from a, exactly as approved,
    // through the same review as any install.
    let staged = stage_source(&b, json!({"kind": "account", "id": "wasm-tools@local"})).await;
    assert_eq!(staged.status, 200, "{}", staged.raw);
    let review = &staged.body["result"];
    assert_eq!(review["for_account"], true);
    assert_eq!(review["manifest"]["powers"], json!(["agent_tools"]));
    assert!(
        review["scopes"]
            .as_array()
            .unwrap()
            .contains(&json!("global"))
    );
    // Closing the review installs nothing.
    let cancelled = call(
        &b,
        "POST",
        &format!(
            "/plugins/stage/{}/cancel",
            review["stage_id"].as_str().unwrap()
        ),
        Some(json!({"request_id": uuid::Uuid::new_v4().to_string()})),
    )
    .await;
    assert_eq!(cancelled.status, 200, "{}", cancelled.raw);
    assert_eq!(
        call(&b, "GET", "/plugins", None).await.body["installed"],
        json!([])
    );

    // Approved: now it is b's, with the powers b's review read.
    let staged = stage_source(&b, json!({"kind": "account", "id": "wasm-tools@local"})).await;
    let stage_id = staged.body["result"]["stage_id"].as_str().unwrap();
    let approved = approve(&b, stage_id, json!({"scope": "global"})).await;
    assert_eq!(approved.status, 200, "{}", approved.raw);
    assert_eq!(approved.body["result"]["powers"], json!(["agent_tools"]));
    assert_eq!(tool_names(&b, &chat).await, ["wasm-tools__hello"]);
    let listed = call(&b, "GET", "/plugins", None).await;
    assert_eq!(listed.body["global_pending"], json!([]), "{}", listed.raw);

    // Removed from every device on a: b's copy is turned off, not deleted,
    // and b's user is asked to confirm the removal there.
    remove(&a, "wasm-tools@local").await;
    let listed = until("b turns it off", || async {
        let listed = call(&b, "GET", "/plugins", None).await;
        (listed.body["installed"][0]["enabled"] == json!(false)).then_some(listed)
    })
    .await;
    assert_eq!(listed.body["installed"][0]["id"], "wasm-tools@local");
    assert_eq!(
        listed.body["global_pending"][0]["why"], "removed",
        "{}",
        listed.raw
    );
    assert!(tool_names(&b, &chat).await.is_empty());
    // b's user confirms: now it goes, here.
    remove(&b, "wasm-tools@local").await;
    let listed = call(&b, "GET", "/plugins", None).await;
    assert_eq!(listed.body["installed"], json!([]));
    assert_eq!(listed.body["global_pending"], json!([]));
}

/// The account's routes are open to every device of the account, and
/// answer only with what a review on that device then checks.
#[tokio::test]
async fn a_peer_reaches_the_records_and_only_a_global_copy() {
    use std::sync::Arc;
    use tower::ServiceExt;
    let (daemon, app) = start_embedded(dead_cloud()).await;
    let folder = wasm_plugin();
    let stage_id = stage_folder(&daemon, folder.path()).await;
    let machine = approve(&daemon, &stage_id, json!({})).await;
    assert_eq!(machine.status, 200, "{}", machine.raw);
    let peer = openremote_daemon::http::peer_router(Arc::clone(&app));
    let get = |uri: String| {
        let peer = peer.clone();
        async move {
            let request = axum::http::Request::builder()
                .uri(uri)
                .body(axum::body::Body::empty())
                .unwrap();
            peer.oneshot(request).await.unwrap().status().as_u16()
        }
    };
    assert_eq!(get("/sync/plugins".into()).await, 200);
    // A plugin this device has for itself is not the account's to copy.
    let hash = "0".repeat(64);
    assert_eq!(
        get(format!(
            "/sync/plugins/wasm-tools@local/archive?resolved={hash}"
        ))
        .await,
        404
    );
}

/// The source text between `from` and the next `until`.
fn between<'a>(source: &'a str, from: &str, until: &str) -> &'a str {
    let start = source.find(from).expect("the start");
    let end = source[start..].find(until).expect("the end");
    &source[start..start + end]
}

/// Every install path asks the tool-name question twice: before it touches
/// anything, and again under the store's lock right before the record is
/// written, so a second install that slipped in between can't take the
/// same tool names. Two installs can't be raced on demand, so the order is
/// pinned in the source.
#[test]
fn an_install_asks_about_tool_names_again_under_the_lock_before_the_record() {
    let source = include_str!("../src/plugin_runtime/install.rs");
    let body = between(source, "pub async fn install(", "pub async fn remove(");
    let first = body.find("tool_name_clash").expect("asked first");
    let lock = body
        .rfind("let mut store = lock_recovering")
        .expect("the lock");
    let second = body.rfind("tool_name_clash").expect("asked again");
    let record = body.find("put_installed_plugin").expect("the record");
    assert!(first < lock && lock < second && second < record);
    // The route and the account's copy both go through this one install.
    let http = include_str!("../src/http.rs");
    let route = between(http, "async fn install_staged(", "async fn cancel_staged(");
    assert!(route.contains("plugin_runtime::install::install("));
    assert!(
        !route.contains("put_installed_plugin"),
        "no second way to write a record"
    );
}

/// A command plugin made again before a chat replaces its folder: the
/// program running the old copy stops before the swap (Windows refuses to
/// move a running program's folder), and again after. Pinned: running a
/// command plugin needs a marketplace and a harness chat.
#[test]
fn a_command_plugin_made_again_stops_its_old_program() {
    let plugins = include_str!("../src/plugins.rs");
    let rerun = between(plugins, "pub async fn rerun_command<", "Ok(Some(hash))");
    let stop = rerun
        .find("before_swap.await")
        .expect("a stop before the swap");
    let swap = rerun.find("swap_in(").expect("the swap");
    assert!(stop < swap);
    let supervisor = include_str!("../src/supervisor.rs");
    let caller = between(
        supervisor,
        "async fn rerun_plugin_commands(",
        "pub fn remove_mcp_server(",
    );
    let handed = caller.find("plugin_changed").expect("the stop handed in");
    let call = caller
        .find("rerun_command(&data_dir, &plugin, stop)")
        .expect("the call");
    let after = caller.rfind("plugin_changed").expect("the stop after");
    assert!(handed < call && call < after);
    let app = include_str!("../src/app.rs");
    let wired = between(app, "set_plugin_changed(", "});");
    assert!(wired.contains("runtime.changed(&id)"), "{wired}");
}

/// A project plugin runs the bytes the user reviewed, from OpenRemote's own
/// copy: a repo edit that keeps every file's size and time cannot slip
/// other code in.
#[tokio::test]
async fn a_project_plugin_runs_the_reviewed_bytes_not_the_repos_live_files() {
    let daemon = start_daemon(&[]).await;
    let folder = wasm_plugin();
    let repo = workspace(None);
    let p = project(&daemon, repo.path()).await;
    let stage_id = stage_folder(&daemon, folder.path()).await;
    let installed = approve(
        &daemon,
        &stage_id,
        json!({"scope": "project", "project": p}),
    )
    .await;
    assert_eq!(installed.status, 200, "{}", installed.raw);
    let chat = chat_in(&daemon, repo.path()).await;
    assert_eq!(
        call_hello(&daemon, &chat).await["content"][0]["text"],
        "wasm"
    );

    // Swap a repo file for other bytes of the same size, and put its time
    // back: a look at counts, sizes and times sees nothing.
    let page = repo
        .path()
        .join(".openremote/plugins/wasm-tools/ui/page.html");
    let when = std::fs::metadata(&page).unwrap().modified().unwrap();
    std::fs::write(&page, "SWAPPED!").unwrap();
    std::fs::File::options()
        .write(true)
        .open(&page)
        .unwrap()
        .set_modified(when)
        .unwrap();
    let id = installed.body["result"]["id"]
        .as_str()
        .unwrap()
        .replace('~', "%7E");
    let served = call(
        &daemon,
        "GET",
        &format!("/plugins/{id}/files/ui/page.html"),
        None,
    )
    .await;
    assert!(!served.raw.contains("SWAPPED!"), "{}", served.raw);
    assert_eq!(
        call_hello(&daemon, &chat).await["content"][0]["text"],
        "wasm"
    );
}

/// A repo whose `.openremote`, `.openremote/plugins` or plugin folder is a
/// link leading out of the project: OpenRemote writes and deletes nothing
/// through it.
#[tokio::test]
async fn repo_paths_that_are_links_are_refused_and_nothing_outside_is_touched() {
    let daemon = start_daemon(&[]).await;
    let folder = wasm_plugin();

    // `.openremote/plugins` leads outside, where a folder of the plugin's
    // name already sits.
    let repo = workspace(None);
    let p = project(&daemon, repo.path()).await;
    let outside = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(outside.path().join("wasm-tools")).unwrap();
    std::fs::write(outside.path().join("wasm-tools/marker.txt"), "keep me").unwrap();
    std::fs::create_dir_all(repo.path().join(".openremote")).unwrap();
    link_dir(
        &repo.path().join(".openremote").join("plugins"),
        outside.path(),
    );
    let before = tree(outside.path());
    let stage_id = stage_folder(&daemon, folder.path()).await;
    let refused = approve(
        &daemon,
        &stage_id,
        json!({"scope": "project", "project": p}),
    )
    .await;
    assert_eq!(refused.status, 409, "{}", refused.raw);
    let staged = stage_source(
        &daemon,
        json!({"kind": "project", "project": p, "name": "wasm-tools"}),
    )
    .await;
    assert_ne!(staged.status, 200, "{}", staged.raw);
    assert_eq!(tree(outside.path()), before, "nothing outside changed");

    // `.openremote` itself leads outside: no index is written there.
    let repo2 = workspace(None);
    let p2 = project(&daemon, repo2.path()).await;
    let outside2 = tempfile::tempdir().unwrap();
    link_dir(&repo2.path().join(".openremote"), outside2.path());
    let stage_id = stage_folder(&daemon, folder.path()).await;
    let refused = approve(
        &daemon,
        &stage_id,
        json!({"scope": "project", "project": p2}),
    )
    .await;
    assert_eq!(refused.status, 409, "{}", refused.raw);
    assert!(tree(outside2.path()).is_empty(), "nothing written outside");

    // Installed while the repo was plain, then the plugins folder is made a
    // link to the outside: removing it deletes nothing out there.
    let repo3 = workspace(None);
    let p3 = project(&daemon, repo3.path()).await;
    let stage_id = stage_folder(&daemon, folder.path()).await;
    let ok = approve(
        &daemon,
        &stage_id,
        json!({"scope": "project", "project": p3}),
    )
    .await;
    assert_eq!(ok.status, 200, "{}", ok.raw);
    let id = ok.body["result"]["id"].as_str().unwrap().to_string();
    std::fs::remove_dir_all(repo3.path().join(".openremote/plugins")).unwrap();
    link_dir(
        &repo3.path().join(".openremote").join("plugins"),
        outside.path(),
    );
    let removed = call(
        &daemon,
        "POST",
        &format!("/plugins/{}/remove", id.replace('~', "%7E")),
        Some(json!({"request_id": uuid::Uuid::new_v4().to_string()})),
    )
    .await;
    assert_eq!(removed.status, 409, "{}", removed.raw);
    assert_eq!(tree(outside.path()), before, "nothing outside deleted");
}

/// A record that names no archive is refused before anything is asked of
/// another device, and a Global install whose files can't be packed is not
/// told to the account (no device could fetch it).
#[tokio::test]
async fn a_record_no_device_could_check_is_neither_fetched_nor_published() {
    let (daemon, app) = start_embedded(dead_cloud()).await;
    let record: openremote_core::GlobalPluginRecord = serde_json::from_value(json!({
        "name": "shout", "marketplace": "local", "runtime": "wasm",
        "source": {"kind": "folder", "path": ""},
        "resolved": "0".repeat(64), "at": 1
    }))
    .unwrap();
    app.store
        .lock()
        .unwrap()
        .merge_global_plugin(record)
        .unwrap();
    let staged = stage_source(&daemon, json!({"kind": "account", "id": "shout@local"})).await;
    assert_eq!(staged.status, 409, "{}", staged.raw);
    assert!(staged.raw.contains("no archive"), "{}", staged.raw);

    let plugin: openremote_core::InstalledPlugin = serde_json::from_value(json!({
        "name": "gone", "marketplace": "local", "runtime": "wasm",
        "source": {"kind": "folder", "path": ""}, "scope": "global",
        "installed_at": 1, "updated_at": 1
    }))
    .unwrap();
    openremote_daemon::plugin_runtime::global::publish(&app, &plugin, "0".repeat(64), false).await;
    assert!(
        app.store
            .lock()
            .unwrap()
            .global_plugin("gone@local")
            .is_none()
    );
}
