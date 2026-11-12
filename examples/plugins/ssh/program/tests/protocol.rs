//! The plugin's program as OpenRemote runs it: started with a cleared
//! environment, spoken to in JSON-RPC lines, its settings in `initialize`.
//! The system's ssh is played by `examples/fake_ssh.rs`, which records how
//! it was started, so these tests read exactly what reached ssh's command
//! line and environment.

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{channel, Receiver};
use std::time::Duration;

use serde_json::{json, Value};

fn fake_ssh() -> PathBuf {
    let bin = PathBuf::from(env!("CARGO_BIN_EXE_openremote-ssh"));
    let name = if cfg!(windows) {
        "fake_ssh.exe"
    } else {
        "fake_ssh"
    };
    let path = bin.parent().unwrap().join("examples").join(name);
    assert!(
        path.is_file(),
        "{} is built by cargo test with the examples",
        path.display()
    );
    path
}

fn scratch(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "openremote-ssh-test-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

struct Plugin {
    child: Child,
    stdin: ChildStdin,
    lines: Receiver<Value>,
    next: u64,
    data: PathBuf,
    home: PathBuf,
}

impl Plugin {
    /// Start the program the way the daemon does: a cleared environment
    /// with the few variables protocol.md lists, plus two the daemon never
    /// passes, to show the program does not hand them on either.
    fn start(tag: &str, settings: Value) -> Plugin {
        let data = scratch(&format!("{tag}-data"));
        let home = scratch(&format!("{tag}-home"));
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_openremote-ssh"));
        cmd.env_clear()
            .env("HOME", &home)
            .env("USERPROFILE", &home)
            .env("PATH", "/nowhere")
            .env("SSH_AUTH_SOCK", "/from/the/daemon/never")
            .env("SSH_ASKPASS", "/bin/askpass-never")
            .env("OPENREMOTE_PLUGIN_DATA", &data);
        for keep in ["SystemRoot", "SYSTEMROOT", "windir", "TEMP", "TMP"] {
            if let Ok(v) = std::env::var(keep) {
                cmd.env(keep, v);
            }
        }
        let mut child = cmd
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("the plugin starts");
        let stdin = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();
        let (tx, lines) = channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                let value: Value = serde_json::from_str(&line).expect("stdout is JSON lines only");
                if tx.send(value).is_err() {
                    break;
                }
            }
        });
        let mut plugin = Plugin {
            child,
            stdin,
            lines,
            next: 1,
            data: data.clone(),
            home,
        };
        let answer = plugin.call(
            "initialize",
            json!({
                "protocol": 1,
                "openremote": "0.4.0",
                "plugin": {"id": "ssh@local", "name": "ssh", "version": "0.2.0",
                           "root": ".", "data": data, "scope": "machine"},
                "powers": ["agent_tools", "run_programs"],
                "settings": settings,
                "platform": if cfg!(windows) { "windows" } else { "linux" },
            }),
        );
        assert_eq!(answer["result"]["protocol"], 1, "{answer}");
        plugin
    }

    fn call(&mut self, method: &str, params: Value) -> Value {
        let id = self.next;
        self.next += 1;
        let message = json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params});
        writeln!(self.stdin, "{message}").unwrap();
        self.stdin.flush().unwrap();
        loop {
            let got = self
                .lines
                .recv_timeout(Duration::from_secs(60))
                .expect("an answer within a minute");
            if got["id"] == id {
                return got;
            }
        }
    }

    fn notify(&mut self, method: &str, params: Value) {
        let message = json!({"jsonrpc": "2.0", "method": method, "params": params});
        writeln!(self.stdin, "{message}").unwrap();
        self.stdin.flush().unwrap();
    }

    fn tool(&mut self, name: &str, arguments: Value) -> Value {
        let answer = self.call(
            "tools/call",
            json!({"name": name, "arguments": arguments, "chat": {"id": "c", "workspace": null}}),
        );
        answer["result"].clone()
    }

    fn ui(&mut self, name: &str, params: Value) -> Value {
        self.call("ui/request", json!({"name": name, "params": params}))
    }

    /// Every start of ssh so far, in order.
    fn ssh_calls(&self) -> Vec<Value> {
        let file = self.data.join("fake-ssh-calls.jsonl");
        std::fs::read_to_string(file)
            .unwrap_or_default()
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect()
    }
}

impl Drop for Plugin {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self.data);
        let _ = std::fs::remove_dir_all(&self.home);
    }
}

fn text(result: &Value) -> String {
    result["content"][0]["text"]
        .as_str()
        .unwrap_or_default()
        .to_string()
}

fn args_of(call: &Value) -> Vec<String> {
    call["args"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a.as_str().unwrap().to_string())
        .collect()
}

fn options_of(args: &[String]) -> Vec<String> {
    args.windows(2)
        .filter(|w| w[0] == "-o")
        .map(|w| w[1].clone())
        .collect()
}

fn base_settings(extra: Value) -> Value {
    let mut s = json!({
        "hosts": "web-1=deploy@ok.example, db=root@ok.example:2222, moved=ok@changed.example, stuck=slow.example",
        "ssh_path": fake_ssh(),
    });
    for (k, v) in extra.as_object().unwrap() {
        s[k] = v.clone();
    }
    s
}

#[test]
fn a_command_reaches_ssh_as_separate_arguments_with_host_keys_always_checked() {
    let mut p = Plugin::start("run", base_settings(json!({})));
    let result = p.tool(
        "ssh_run",
        json!({"host": "db", "command": "echo $HOME; rm -rf / # ' \" `x`"}),
    );
    assert_eq!(result["isError"], false, "{result}");
    assert!(
        text(&result).contains("ran: echo $HOME; rm -rf / # ' \" `x`"),
        "{result}"
    );

    let calls = p.ssh_calls();
    assert_eq!(calls.len(), 1);
    let args = args_of(&calls[0]);
    // The address, its user and port, and the command: each one argument,
    // the address after `--`, the command after the address.
    let at = args.iter().position(|a| a == "--").unwrap();
    assert_eq!(args[at + 1], "ok.example");
    assert_eq!(args[at + 2], "echo $HOME; rm -rf / # ' \" `x`");
    assert_eq!(args.len(), at + 3);
    assert!(args.windows(2).any(|w| w[0] == "-p" && w[1] == "2222"));
    assert!(args.windows(2).any(|w| w[0] == "-l" && w[1] == "root"));
    // No config file is read, so nothing in one can add a ProxyCommand,
    // a LocalCommand or a forward.
    assert!(args.windows(2).any(|w| w[0] == "-F" && w[1] == "none"));

    let options = options_of(&args);
    assert!(options.contains(&"StrictHostKeyChecking=yes".to_string()));
    assert!(options.contains(&"BatchMode=yes".to_string()));
    assert!(options.contains(&"PasswordAuthentication=no".to_string()));
    assert!(options.contains(&"ProxyCommand=none".to_string()));
    assert!(options.contains(&"PermitLocalCommand=no".to_string()));
    assert!(options.contains(&"ForwardAgent=no".to_string()));
    assert!(options.contains(&"UpdateHostKeys=no".to_string()));
    for o in &options {
        let lower = o.to_ascii_lowercase();
        assert!(
            !lower.starts_with("stricthostkeychecking=no")
                && !lower.starts_with("stricthostkeychecking=accept-new")
                && !lower.contains("/dev/null")
                && !lower.starts_with("localcommand"),
            "{o}"
        );
    }
}

#[test]
fn the_environment_ssh_gets_is_the_few_it_needs_and_the_agent_only_from_the_setting() {
    let socket = if cfg!(windows) {
        r"\\.\pipe\my-agent".to_string()
    } else {
        "/run/user/1000/ssh-agent.socket".to_string()
    };
    let mut p = Plugin::start("env", base_settings(json!({"agent_socket": socket})));
    let result = p.tool("ssh_run", json!({"host": "web-1", "command": "true"}));
    assert_eq!(result["isError"], false, "{result}");
    let env = p.ssh_calls()[0]["env"].as_object().unwrap().clone();
    assert_eq!(
        env.get("SSH_AUTH_SOCK").and_then(Value::as_str),
        Some(socket.as_str())
    );
    assert!(!env.contains_key("SSH_ASKPASS"), "{env:?}");
    assert!(!env.contains_key("OPENREMOTE_PLUGIN_DATA"), "{env:?}");
    assert!(!env.contains_key("PATH"), "{env:?}");
    assert_eq!(
        env.get("HOME").and_then(Value::as_str),
        Some(p.home.to_str().unwrap())
    );

    // With no agent set, ssh gets none: the daemon's own value never passes.
    let mut p = Plugin::start("noagent", base_settings(json!({})));
    p.tool("ssh_run", json!({"host": "web-1", "command": "true"}));
    let env = p.ssh_calls()[0]["env"].as_object().unwrap().clone();
    assert!(!env.contains_key("SSH_AUTH_SOCK"), "{env:?}");
}

#[test]
fn a_changed_host_key_is_refused_and_nothing_is_written_to_known_hosts() {
    let mut p = Plugin::start("changed", base_settings(json!({})));
    let result = p.tool("ssh_run", json!({"host": "moved", "command": "id"}));
    assert_eq!(result["isError"], true, "{result}");
    let said = text(&result);
    assert!(said.contains("host key"), "{said}");
    assert!(said.contains("Verify"), "{said}");
    // One try, no retry with looser options, and no key recorded.
    assert_eq!(p.ssh_calls().len(), 1);
    let known = std::fs::read_to_string(p.data.join("known_hosts")).unwrap_or_default();
    assert!(known.trim().is_empty(), "{known}");
}

#[test]
fn verify_shows_the_key_and_records_it_only_when_the_user_confirms_it() {
    let mut p = Plugin::start("verify", base_settings(json!({})));
    let seen = p.ui("verify_start", json!({"name": "db"}));
    let fingerprint = seen["result"]["fingerprint"].as_str().unwrap().to_string();
    assert!(fingerprint.starts_with("SHA256:"), "{seen}");
    // The look used a scratch file, offered no key, and trusted nothing.
    let scan = args_of(&p.ssh_calls()[0]);
    let options = options_of(&scan);
    assert!(options.contains(&"StrictHostKeyChecking=accept-new".to_string()));
    assert!(options.contains(&"PreferredAuthentications=none".to_string()));
    assert!(options.contains(&"IdentityAgent=none".to_string()));
    let known = std::fs::read_to_string(p.data.join("known_hosts")).unwrap_or_default();
    assert!(known.trim().is_empty(), "{known}");

    // A confirm naming another fingerprint is refused.
    let wrong = p.ui(
        "verify_confirm",
        json!({"name": "db", "fingerprint": "SHA256:not-it"}),
    );
    assert!(wrong["error"].is_object(), "{wrong}");
    // ...and it ends that look: the right fingerprint now needs a new one.
    let late = p.ui(
        "verify_confirm",
        json!({"name": "db", "fingerprint": fingerprint}),
    );
    assert!(late["error"].is_object(), "{late}");
    // A changed setting ends a look too.
    p.ui("verify_start", json!({"name": "db"}));
    p.notify(
        "settings/changed",
        json!({"settings": base_settings(json!({}))}),
    );
    let stale = p.ui(
        "verify_confirm",
        json!({"name": "db", "fingerprint": fingerprint}),
    );
    assert!(stale["error"].is_object(), "{stale}");

    p.ui("verify_start", json!({"name": "db"}));
    let ok = p.ui(
        "verify_confirm",
        json!({"name": "db", "fingerprint": fingerprint}),
    );
    assert!(ok["result"].is_object(), "{ok}");
    let known = std::fs::read_to_string(p.data.join("known_hosts")).unwrap();
    assert!(
        known.starts_with("[ok.example]:2222 ssh-ed25519 "),
        "{known}"
    );

    // Forget takes it back out.
    p.ui("forget", json!({"name": "db"}));
    let known = std::fs::read_to_string(p.data.join("known_hosts")).unwrap();
    assert!(!known.contains("ok.example"), "{known}");
}

#[test]
fn an_agent_cannot_verify_a_host_key() {
    let mut p = Plugin::start("agentverify", base_settings(json!({})));
    let answer = p.call(
        "tools/call",
        json!({"name": "verify_confirm", "arguments": {"name": "db", "fingerprint": "x"}, "chat": {"id": "c"}}),
    );
    assert!(answer["error"].is_object(), "{answer}");
}

#[test]
fn hosts_and_paths_from_settings_are_checked_before_ssh_is_started() {
    let mut p = Plugin::start(
        "badhosts",
        base_settings(json!({"hosts": "evil=-oProxyCommand=calc@x, ok=deploy@ok.example"})),
    );
    let listed = p.tool("ssh_hosts", json!({}));
    let said = text(&listed);
    assert!(said.contains("\"ok\""), "{said}");
    assert!(!said.contains("ProxyCommand=calc"), "{said}");
    assert!(said.contains("left out"), "{said}");
    let refused = p.tool("ssh_run", json!({"host": "evil", "command": "true"}));
    assert_eq!(refused["isError"], true);

    // Settings of other shapes that try to reach ssh's options.
    for hosts in [
        "a=-p@x",
        "a=user@-oProxyCommand=x",
        "a=user@host -oProxyCommand=x",
        "a=user@host:22 -F /tmp/x",
        "a=us er@host",
        "a=user@host:99999",
        "a=user@host\t-oProxyCommand=x",
    ] {
        p.notify(
            "settings/changed",
            json!({"settings": base_settings(json!({"hosts": hosts}))}),
        );
        let refused = p.tool("ssh_run", json!({"host": "a", "command": "true"}));
        assert_eq!(refused["isError"], true, "{hosts}: {refused}");
    }
    assert!(p.ssh_calls().is_empty(), "{:?}", p.ssh_calls());
}

#[test]
fn a_key_or_password_pasted_into_a_setting_is_never_used() {
    let pem = "-----BEGIN OPENSSH PRIVATE KEY-----\nb3BlbnNzaC1rZXktdjEAAAAA\n-----END OPENSSH PRIVATE KEY-----";
    let mut p = Plugin::start("pasted", base_settings(json!({"key_file": pem})));
    let refused = p.tool("ssh_run", json!({"host": "web-1", "command": "true"}));
    assert_eq!(refused["isError"], true, "{refused}");
    let said = text(&refused);
    assert!(said.contains("path"), "{said}");
    assert!(!said.contains("b3BlbnNzaC1rZXktdjEAAAAA"), "{said}");
    assert!(p.ssh_calls().is_empty());

    // A password-looking host entry (user:pass@host) is refused too.
    p.notify(
        "settings/changed",
        json!({"settings": base_settings(json!({"hosts": "web=deploy:hunter2@ok.example"}))}),
    );
    let refused = p.tool("ssh_run", json!({"host": "web", "command": "true"}));
    assert_eq!(refused["isError"], true, "{refused}");
    assert!(!text(&refused).contains("hunter2"), "{refused}");
    assert!(p.ssh_calls().is_empty());
}

#[test]
fn a_key_file_is_a_path_given_to_ssh_as_its_own_argument() {
    let key = scratch("key").join("id_ed25519");
    std::fs::write(&key, "not a real key").unwrap();
    let mut p = Plugin::start(
        "keyfile",
        base_settings(json!({"key_file": key.to_str().unwrap()})),
    );
    let result = p.tool("ssh_run", json!({"host": "web-1", "command": "true"}));
    assert_eq!(result["isError"], false, "{result}");
    let args = args_of(&p.ssh_calls()[0]);
    assert!(args
        .windows(2)
        .any(|w| w[0] == "-i" && Path::new(&w[1]) == key.as_path()));
    assert!(options_of(&args).contains(&"IdentitiesOnly=yes".to_string()));
}

#[test]
fn ssh_is_only_ever_an_absolute_path() {
    let mut p = Plugin::start("relative", base_settings(json!({"ssh_path": "ssh"})));
    let refused = p.tool("ssh_run", json!({"host": "web-1", "command": "true"}));
    assert_eq!(refused["isError"], true, "{refused}");
    assert!(text(&refused).contains("full path"), "{refused}");
}

#[test]
fn files_are_read_and_written_with_the_path_quoted_for_the_remote_shell() {
    let mut p = Plugin::start("files", base_settings(json!({})));
    let read = p.tool(
        "ssh_read_file",
        json!({"host": "web-1", "path": "~/it's here; rm -rf ~"}),
    );
    assert_eq!(read["isError"], false, "{read}");
    let wrote = p.tool(
        "ssh_write_file",
        json!({"host": "web-1", "path": "/tmp/$(id)", "content": "hello"}),
    );
    assert_eq!(wrote["isError"], false, "{wrote}");
    let calls = p.ssh_calls();
    let read_cmd = args_of(&calls[0]).last().unwrap().clone();
    assert_eq!(
        read_cmd,
        "head -c 100001 -- \"$HOME\"/'it'\\''s here; rm -rf ~'"
    );
    let write_cmd = args_of(&calls[1]).last().unwrap().clone();
    assert_eq!(write_cmd, "cat > '/tmp/$(id)'");
    assert_eq!(calls[1]["stdin"], 5);
}

#[test]
fn a_command_that_runs_too_long_is_stopped() {
    let mut p = Plugin::start("slow", base_settings(json!({})));
    let started = std::time::Instant::now();
    let result = p.tool(
        "ssh_run",
        json!({"host": "stuck", "command": "sleep 100", "timeout_s": 1}),
    );
    assert_eq!(result["isError"], true, "{result}");
    assert!(text(&result).contains("did not finish"), "{result}");
    assert!(started.elapsed() < Duration::from_secs(20));
}
