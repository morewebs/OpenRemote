//! What the plugin asks OpenRemote for, read from its own openremote.json:
//! the least it needs, and nothing that could hold a secret.

use std::path::{Path, PathBuf};

use serde_json::Value;

fn folder() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..")
}

fn manifest() -> Value {
    serde_json::from_slice(&std::fs::read(folder().join("openremote.json")).unwrap()).unwrap()
}

#[test]
fn it_asks_for_no_power_beyond_what_a_process_plugin_with_tools_has() {
    let m = manifest();
    // A process plugin runs programs and a plugin with tools has
    // agent_tools; it asks for nothing more: no network (ssh is its own
    // program), no chats.
    assert_eq!(m["powers"], serde_json::json!([]));
    assert_eq!(m["runtime"], "process");
    assert!(m.get("harness").is_none());
}

#[test]
fn its_program_is_its_own_file_never_one_from_path() {
    let m = manifest();
    assert_eq!(m["entry"]["command"], "bin/openremote-ssh");
    assert_eq!(m["entry"]["windows"]["command"], "bin/openremote-ssh.exe");
    let bin = env!("CARGO_BIN_EXE_openremote-ssh");
    assert!(bin.ends_with(if cfg!(windows) {
        "openremote-ssh.exe"
    } else {
        "openremote-ssh"
    }));
}

#[test]
fn no_setting_is_a_secret_or_asks_for_a_key_or_password() {
    let m = manifest();
    let settings = m["contributes"]["settings"].as_array().unwrap();
    let keys: Vec<&str> = settings
        .iter()
        .map(|s| s["key"].as_str().unwrap())
        .collect();
    assert_eq!(keys, ["hosts", "key_file", "agent_socket", "ssh_path"]);
    for s in settings {
        assert_eq!(s["type"], "string", "{s}");
        let key = s["key"].as_str().unwrap();
        assert!(
            !key.contains("password")
                && !key.contains("passphrase")
                && key != "key"
                && key != "private_key",
            "{key}"
        );
    }
}

#[test]
fn an_agent_gets_four_tools_and_none_of_them_trusts_a_key() {
    let m = manifest();
    let names: Vec<&str> = m["contributes"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap())
        .collect();
    assert_eq!(
        names,
        ["ssh_hosts", "ssh_run", "ssh_read_file", "ssh_write_file"]
    );
}

#[test]
fn its_page_is_in_the_folder() {
    let m = manifest();
    let view = &m["contributes"]["views"][0];
    assert_eq!(view["entry"], "ui/ssh.html");
    let page = std::fs::read_to_string(folder().join("ui/ssh.html")).unwrap();
    // The sandboxed frame runs no inline handlers.
    assert!(!page.contains("onclick="));
    for request in [
        "'state'",
        "'verify_start'",
        "'verify_confirm'",
        "'host_test'",
        "'forget'",
    ] {
        assert!(page.contains(request), "{request}");
    }
}

#[test]
fn only_the_list_of_hosts_is_marked_read_only() {
    // ssh_read_file runs a command in the host's shell, so it is not a
    // read-only call as far as the chat's approval is concerned.
    let m = manifest();
    for t in m["contributes"]["tools"].as_array().unwrap() {
        assert_eq!(t["read_only"], t["name"] == "ssh_hosts", "{t}");
    }
}
