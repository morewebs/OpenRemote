//! What the plugin asks OpenRemote for, read from its own openremote.json:
//! the least it needs, every tool its program answers, and every page it
//! names in its own folder.

use std::path::{Path, PathBuf};

use serde_json::{json, Value};

fn folder() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..")
}

fn manifest() -> Value {
    serde_json::from_slice(&std::fs::read(folder().join("openremote.json")).unwrap()).unwrap()
}

#[test]
fn it_asks_only_for_the_network_beyond_what_a_process_plugin_with_tools_has() {
    let m = manifest();
    // Running programs and agent tools come with a process plugin that has
    // tools; the network is for looking up and downloading Worktrunk
    // releases. No chats are read.
    assert_eq!(m["powers"], json!(["network"]));
    assert_eq!(m["runtime"], "process");
    assert!(m.get("harness").is_none());
}

#[test]
fn its_program_is_its_own_file_never_one_from_path() {
    let m = manifest();
    assert_eq!(m["entry"]["command"], "bin/worktrunk-plugin");
    assert_eq!(m["entry"]["windows"]["command"], "bin/worktrunk-plugin.exe");
    assert!(
        env!("CARGO_BIN_EXE_worktrunk-plugin").ends_with(if cfg!(windows) {
            "worktrunk-plugin.exe"
        } else {
            "worktrunk-plugin"
        })
    );
}

#[test]
fn every_tool_fits_its_name_and_the_program_knows_it() {
    let m = manifest();
    let tools = m["contributes"]["tools"].as_array().unwrap();
    let names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
    assert_eq!(
        names,
        [
            "list",
            "status",
            "create",
            "switch",
            "remove",
            "merge",
            "start_chat",
            "version",
            "help",
            "run"
        ]
    );
    let main = std::fs::read_to_string(folder().join("program/src/main.rs")).unwrap();
    for t in tools {
        let name = t["name"].as_str().unwrap();
        assert!(
            format!("mcp__openremote__worktrunk__{name}").len() <= 64,
            "{name}"
        );
        assert!(
            main.contains(&format!("\"{name}\"")),
            "the program answers {name}"
        );
        assert_eq!(t["input_schema"]["type"], "object", "{name}");
        let secs = t["timeout_secs"].as_u64().unwrap_or(120);
        assert!((1..=600).contains(&secs), "{name}");
        assert!(
            t["description"].as_str().unwrap().len() > 40,
            "{name} says what it does"
        );
    }
    // Only the tools that change nothing say they only read.
    let read_only: Vec<&str> = tools
        .iter()
        .filter(|t| t["read_only"] == json!(true))
        .map(|t| t["name"].as_str().unwrap())
        .collect();
    assert_eq!(read_only, ["list", "status", "version", "help"]);
}

#[test]
fn its_pages_are_its_own_files_and_no_setting_is_a_secret() {
    let m = manifest();
    for page in [
        m["contributes"]["views"][0]["entry"].as_str().unwrap(),
        m["contributes"]["composer_inputs"][0]["entry"]
            .as_str()
            .unwrap(),
    ] {
        assert!(folder().join(page).is_file(), "{page}");
    }
    let keys: Vec<&str> = m["contributes"]["settings"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| {
            assert_ne!(s["type"], "secret", "{s}");
            s["key"].as_str().unwrap()
        })
        .collect();
    assert_eq!(keys, ["wt_path", "hooks", "chat_harness", "check_updates"]);
}

#[test]
fn the_catalog_lists_it_with_what_it_asks_for() {
    let catalog: Value = serde_json::from_slice(
        &std::fs::read(folder().join("../../../.claude-plugin/marketplace.json")).unwrap(),
    )
    .unwrap();
    let entry = catalog["plugins"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["name"] == "worktrunk")
        .expect("in OpenRemote's catalog");
    assert_eq!(entry["source"], "./examples/plugins/worktrunk");
    assert_eq!(entry["metadata"]["openremote"]["runtime"], "process");
    let plugin: Value = serde_json::from_slice(
        &std::fs::read(folder().join(".claude-plugin/plugin.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(entry["version"], manifest()["version"]);
    assert_eq!(plugin["version"], manifest()["version"]);
}
