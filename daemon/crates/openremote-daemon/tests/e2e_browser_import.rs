//! Importing another browser's bookmarks and history, through the real
//! daemon binary. Its environment points at a made-up home folder holding the
//! Chrome fixture profile where Chrome keeps it on this operating system, so
//! the daemon finds it by itself; nothing on the machine running the test is
//! read.

mod common;

use std::path::{Path, PathBuf};

use common::*;
use serde_json::json;

fn fixture(path: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/browser-import")
        .join(path)
}

fn copy_tree(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap().flatten() {
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), &target).unwrap();
        }
    }
}

/// Where Chrome keeps its data, under the made-up home.
fn chrome_root(home: &Path) -> PathBuf {
    if cfg!(windows) {
        home.join("local/Google/Chrome/User Data")
    } else if cfg!(target_os = "macos") {
        home.join("home/Library/Application Support/Google/Chrome")
    } else {
        home.join("home/.config/google-chrome")
    }
}

#[tokio::test]
async fn a_browser_found_here_is_imported_once_into_other_bookmarks() {
    let home = tempfile::tempdir().unwrap();
    copy_tree(&fixture("chromium"), &chrome_root(home.path()));
    let env = [
        ("LOCALAPPDATA", home.path().join("local")),
        ("APPDATA", home.path().join("roaming")),
        ("HOME", home.path().join("home")),
        ("USERPROFILE", home.path().join("home")),
    ];
    let env: Vec<(&str, String)> = env
        .iter()
        .map(|(k, v)| (*k, v.to_string_lossy().into_owned()))
        .collect();
    let daemon = spawn_daemon_process(&env).await;

    let listed = call(&daemon, "GET", "/browser/import", None).await;
    assert_eq!(listed.status, 200, "{}", listed.raw);
    let sources = listed.body["sources"].as_array().unwrap();
    assert_eq!(sources.len(), 1, "{}", listed.raw);
    assert_eq!(sources[0]["id"], "chrome-default");
    assert_eq!(sources[0]["browser"], "Chrome");
    assert_eq!(sources[0]["bookmarks"], true);
    // A listing names no folder on this computer.
    assert!(!listed.raw.contains("User Data"), "{}", listed.raw);

    let body = json!({"source": "chrome-default", "history": false});
    let first = call(&daemon, "POST", "/browser/import", Some(body.clone())).await;
    assert_eq!(first.status, 200, "{}", first.raw);
    // Five web bookmarks, one of them twice; three were not web pages.
    assert_eq!(first.body["bookmarks"], 4, "{}", first.raw);
    assert_eq!(first.body["already"], 1);
    assert_eq!(first.body["refused"], 3);
    assert_eq!(first.body["visits"], 0);

    let marks = call(&daemon, "GET", "/browser/bookmarks", None).await;
    let marks = marks.body["bookmarks"].as_array().unwrap();
    assert_eq!(marks.len(), 4);
    assert!(marks.iter().all(|m| m["folder"] == "other"));
    assert!(
        marks
            .iter()
            .all(|m| m["url"].as_str().unwrap().starts_with("https://"))
    );

    // Again: nothing is added twice.
    let again = call(&daemon, "POST", "/browser/import", Some(body)).await;
    assert_eq!(again.body["bookmarks"], 0, "{}", again.raw);
    assert_eq!(again.body["already"], 5);

    // A folder the user picked works too, and anything else is refused.
    let picked = json!({"path": fixture("safari").to_string_lossy(), "history": false});
    let picked = call(&daemon, "POST", "/browser/import", Some(picked)).await;
    assert_eq!(picked.status, 200, "{}", picked.raw);
    assert_eq!(picked.body["browser"], "Safari");
    assert_eq!(picked.body["bookmarks"], 2);
    for refused in [
        json!({"path": "relative/folder"}),
        json!({"path": home.path().to_string_lossy()}),
        json!({"source": "no-such-browser"}),
        json!({}),
        json!({"source": "chrome-default", "bookmarks": false, "history": false}),
    ] {
        let reply = call(&daemon, "POST", "/browser/import", Some(refused.clone())).await;
        assert!(reply.status >= 400, "{refused}: {}", reply.raw);
        assert!(reply.body["error"].is_string(), "{refused}: {}", reply.raw);
    }
}
