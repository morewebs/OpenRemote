//! The plugin's program, started and spoken to the way OpenRemote does it
//! (JSON-RPC lines over stdio), with a fake `wt` that writes down every
//! command line it receives. With `WORKTRUNK_TEST_BIN` set to a real
//! Worktrunk program, the last test runs the real thing in a scratch
//! repository too.

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{channel, Receiver};
use std::time::Duration;

use serde_json::{json, Value};

fn fake_wt() -> PathBuf {
    // Cargo builds the examples beside the test binaries for `cargo test`.
    let deps = std::env::current_exe()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf();
    let dir = deps.parent().unwrap().join("examples");
    let name = if cfg!(windows) {
        "fake_wt.exe"
    } else {
        "fake_wt"
    };
    let path = dir.join(name);
    assert!(path.is_file(), "{} is built by cargo test", path.display());
    path
}

struct Plugin {
    child: Child,
    stdin: ChildStdin,
    lines: Receiver<Value>,
    next: u64,
}

impl Plugin {
    fn start(envs: &[(&str, &Path)], extra_env: &[(&str, &str)]) -> Self {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_worktrunk-plugin"));
        cmd.stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit());
        for (k, v) in envs {
            cmd.env(k, v);
        }
        for (k, v) in extra_env {
            cmd.env(k, v);
        }
        let mut child = cmd.spawn().unwrap();
        let stdin = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                if let Ok(v) = serde_json::from_str::<Value>(&line) {
                    let _ = tx.send(v);
                }
            }
        });
        Plugin {
            child,
            stdin,
            lines: rx,
            next: 1,
        }
    }

    fn send(&mut self, v: &Value) {
        writeln!(self.stdin, "{v}").unwrap();
        self.stdin.flush().unwrap();
    }

    /// A call, answering any call the plugin makes meanwhile with `reply`.
    fn call_with(
        &mut self,
        method: &str,
        params: Value,
        reply: &mut dyn FnMut(&Value) -> Value,
    ) -> Value {
        let id = self.next;
        self.next += 1;
        self.send(&json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}));
        loop {
            let msg = self
                .lines
                .recv_timeout(Duration::from_secs(60))
                .expect("the plugin answers");
            if msg.get("method").is_some() {
                if let Some(their) = msg.get("id") {
                    let result = reply(&msg);
                    self.send(&json!({"jsonrpc": "2.0", "id": their, "result": result}));
                }
                continue;
            }
            if msg["id"] == json!(id) {
                assert!(msg.get("error").is_none(), "{msg}");
                return msg["result"].clone();
            }
        }
    }

    fn call(&mut self, method: &str, params: Value) -> Value {
        self.call_with(method, params, &mut |m| {
            panic!("unexpected call from the plugin: {m}")
        })
    }

    fn tool(&mut self, name: &str, arguments: Value, chat: &Value) -> (bool, String) {
        let r = self.call(
            "tools/call",
            json!({"name": name, "arguments": arguments, "chat": chat}),
        );
        (
            r["isError"].as_bool().unwrap(),
            r["content"][0]["text"].as_str().unwrap().to_string(),
        )
    }
}

impl Drop for Plugin {
    fn drop(&mut self) {
        let _ = self.child.kill();
    }
}

struct Scratch {
    root: PathBuf,
    repo: PathBuf,
    data: PathBuf,
    log: PathBuf,
}

fn scratch(name: &str) -> Scratch {
    let root = std::env::temp_dir().join(format!("wtp-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let repo = root.join("repo");
    std::fs::create_dir_all(repo.join(".git")).unwrap();
    let data = root.join("data");
    std::fs::create_dir_all(&data).unwrap();
    Scratch {
        log: root.join("wt.log"),
        root,
        repo,
        data,
    }
}

fn logged(s: &Scratch) -> Vec<String> {
    std::fs::read_to_string(&s.log)
        .unwrap_or_default()
        .lines()
        .map(String::from)
        .collect()
}

fn ran(s: &Scratch, starts: &str) -> Vec<String> {
    logged(s)
        .into_iter()
        .filter(|l| l.starts_with(starts) && !l.contains("--help"))
        .collect()
}

fn init(p: &mut Plugin, s: &Scratch, wt: &Path, settings: Value) {
    let mut settings = settings;
    settings["wt_path"] = json!(wt.to_string_lossy());
    let r = p.call(
        "initialize",
        json!({"protocol": 1, "plugin": {"id": "worktrunk@openremote", "name": "worktrunk", "data": s.data, "scope": "machine"}, "powers": ["agent_tools", "run_programs", "network"], "settings": settings, "platform": "test"}),
    );
    assert_eq!(r, json!({"protocol": 1}));
}

#[test]
fn the_tools_hand_wt_exactly_what_it_needs_and_nothing_more() {
    let s = scratch("tools");
    let mut p = Plugin::start(&[("FAKE_WT_LOG", &s.log)], &[]);
    init(&mut p, &s, &fake_wt(), json!({}));
    let chat = json!({"id": "c1", "workspace": s.repo, "harness": "claude", "model": "opus"});

    let (err, text) = p.tool("list", json!({}), &chat);
    assert!(!err, "{text}");
    assert!(
        text.contains("main") && text.contains("feat") && text.contains("2 ahead"),
        "{text}"
    );
    assert!(
        !text.contains("\u{1b}"),
        "no terminal codes reach the agent"
    );
    assert_eq!(ran(&s, "list"), vec!["list --format json"]);

    let (err, text) = p.tool("create", json!({"branch": "login"}), &chat);
    assert!(!err, "{text}");
    assert!(text.contains(".login"), "{text}");
    assert_eq!(
        ran(&s, "switch"),
        vec!["switch --create --no-cd --format json login"]
    );

    // A branch that would be read as a flag never reaches wt.
    for bad in ["-x", "--execute=calc", "a b"] {
        let (err, text) = p.tool("create", json!({"branch": bad}), &chat);
        assert!(err, "{bad}: {text}");
    }
    let (err, _) = p.tool("create", json!({"branch": "x", "base": "--yes"}), &chat);
    assert!(err);
    assert_eq!(ran(&s, "switch").len(), 1, "nothing more reached wt");

    // The passthrough refuses wt's own ways to run commands.
    for refused in [
        json!(["switch", "-x", "calc", "f"]),
        json!(["merge", "--yes"]),
        json!(["hook", "post-start"]),
        json!(["step", "for-each", "--", "calc"]),
    ] {
        let (err, text) = p.tool("run", json!({"args": refused}), &chat);
        assert!(err, "{refused}: {text}");
    }
    assert!(
        ran(&s, "switch -x").is_empty()
            && ran(&s, "hook").is_empty()
            && ran(&s, "merge").is_empty()
    );
    let (err, text) = p.tool(
        "run",
        json!({"args": ["a-future-command", "--flag"]}),
        &chat,
    );
    assert!(!err, "{text}");
    assert_eq!(ran(&s, "a-future-command"), vec!["a-future-command --flag"]);

    // Hooks wait for the user's approval: the agent is told where to get it.
    let (err, text) = p.tool("create", json!({"branch": "needs-approval"}), &chat);
    assert!(err && text.contains("Worktrunk view"), "{text}");

    // The user's hook setting: skip them.
    p.send(&json!({"jsonrpc": "2.0", "method": "settings/changed", "params": {"settings": {"wt_path": fake_wt().to_string_lossy(), "hooks": "skip"}}}));
    let (err, _) = p.tool("create", json!({"branch": "quiet"}), &chat);
    assert!(!err);
    assert!(
        ran(&s, "switch")
            .contains(&"switch --create --no-cd --format json --no-hooks quiet".to_string()),
        "{:?}",
        ran(&s, "switch")
    );
    assert!(
        logged(&s)
            .iter()
            .all(|l| !l.split(' ').any(|w| w == "--yes" || w == "-y")),
        "never --yes unless the user chose it"
    );

    // Merging from the chat's own worktree keeps it.
    let (err, text) = p.tool("merge", json!({}), &chat);
    assert!(!err, "{text}");
    assert!(
        ran(&s, "merge").last().unwrap().contains("--no-remove"),
        "{:?}",
        ran(&s, "merge")
    );
    assert!(text.contains("kept"), "{text}");

    // Removing the chat's own worktree is refused unless asked for.
    let (err, text) = p.tool("remove", json!({"branches": ["main"]}), &chat);
    assert!(err && text.contains("this chat's own folder"), "{text}");
    let (err, text) = p.tool(
        "remove",
        json!({"branches": ["feat"], "force": true}),
        &chat,
    );
    assert!(!err, "{text}");
    assert_eq!(
        ran(&s, "remove"),
        vec!["remove --foreground --force --format json --no-hooks feat"]
    );

    // A chat in no project names its repository.
    let (err, text) = p.tool("list", json!({}), &json!({"id": "c2", "workspace": null}));
    assert!(err && text.contains("repo"), "{text}");
    let (err, _) = p.tool(
        "list",
        json!({"repo": s.repo}),
        &json!({"id": "c2", "workspace": null}),
    );
    assert!(!err);
    let (err, _) = p.tool(
        "list",
        json!({"repo": "relative/path"}),
        &json!({"id": "c2"}),
    );
    assert!(err);

    let _ = std::fs::remove_dir_all(&s.root);
}

#[test]
fn a_chat_starts_in_a_new_worktree_on_the_asking_chats_harness() {
    let s = scratch("chat");
    let mut p = Plugin::start(&[("FAKE_WT_LOG", &s.log)], &[]);
    init(&mut p, &s, &fake_wt(), json!({}));
    let chat = json!({"id": "c1", "workspace": s.repo, "harness": "codex", "model": "gpt-5"});
    let mut asked = Vec::new();
    let r = p.call_with(
        "tools/call",
        json!({"name": "start_chat", "arguments": {"branch": "review", "prompt": "Review the login page"}, "chat": chat}),
        &mut |m| {
            asked.push(m.clone());
            json!({"chat": "new-chat-id"})
        },
    );
    assert_eq!(r["isError"], json!(false), "{r}");
    assert!(r["content"][0]["text"]
        .as_str()
        .unwrap()
        .contains("new-chat-id"));
    assert_eq!(asked.len(), 1);
    assert_eq!(asked[0]["method"], "chats/start");
    let params = &asked[0]["params"];
    assert_eq!(params["harness"], "codex");
    assert_eq!(params["model"], "gpt-5");
    assert_eq!(params["prompt"], "Review the login page");
    assert!(
        params["workspace"].as_str().unwrap().ends_with(".review"),
        "{params}"
    );
    let _ = std::fs::remove_dir_all(&s.root);
}

#[test]
fn an_older_worktrunk_is_never_handed_flags_it_lacks() {
    let s = scratch("old");
    let mut p = Plugin::start(&[("FAKE_WT_LOG", &s.log)], &[("FAKE_WT_OLD", "1")]);
    init(&mut p, &s, &fake_wt(), json!({}));
    let chat = json!({"id": "c1", "workspace": s.repo});
    let (err, text) = p.tool("list", json!({}), &chat);
    assert!(!err && text.contains("feat"), "schema 1 reads: {text}");
    let (err, _) = p.tool("create", json!({"branch": "x"}), &chat);
    assert!(!err);
    assert_eq!(ran(&s, "switch"), vec!["switch --create --format json x"]);
    let (err, _) = p.tool("remove", json!({"branches": ["feat"]}), &chat);
    assert!(!err);
    assert_eq!(ran(&s, "remove"), vec!["remove --format json feat"]);
    let (err, text) = p.tool("version", json!({"check": false}), &chat);
    assert!(!err && text.contains("0.20.0"), "{text}");
    let _ = std::fs::remove_dir_all(&s.root);
}

#[test]
fn the_view_sees_worktrunk_its_repositories_and_their_worktrees() {
    let s = scratch("view");
    let mut p = Plugin::start(&[("FAKE_WT_LOG", &s.log)], &[]);
    init(&mut p, &s, &fake_wt(), json!({}));
    let added = p.call(
        "ui/request",
        json!({"name": "add_repo", "params": {"path": s.repo}}),
    );
    assert!(added["repo"].is_string(), "{added}");
    let state = p.call("ui/request", json!({"name": "state", "params": {}}));
    assert_eq!(state["version"]["installed"], json!(true), "{state}");
    assert_eq!(state["version"]["version"], json!("9.1.0"));
    assert_eq!(state["repos"].as_array().unwrap().len(), 1);
    assert_eq!(state["list"]["items"].as_array().unwrap().len(), 2);
    assert_eq!(state["approvals"]["state"], json!("approval_required"));
    // Work runs as a job the view watches.
    let started = p.call(
        "ui/request",
        json!({"name": "create", "params": {"repo": s.repo, "branch": "from-view"}}),
    );
    assert!(started["job"]["id"].as_u64().is_some(), "{started}");
    let mut job = Value::Null;
    for _ in 0..200 {
        job = p.call("ui/request", json!({"name": "job", "params": {}}));
        if job["running"] == json!(false) {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    assert_eq!(job["ok"], json!(true), "{job}");
    // Approving hooks is the user's click in the view: only it sends --yes.
    let _ = p.call(
        "ui/request",
        json!({"name": "approve_hooks", "params": {"repo": s.repo}}),
    );
    for _ in 0..200 {
        if p.call("ui/request", json!({"name": "job", "params": {}}))["running"] == json!(false) {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    assert_eq!(
        ran(&s, "config approvals add"),
        vec!["config approvals add --yes"]
    );
    let _ = std::fs::remove_dir_all(&s.root);
}

#[test]
fn without_worktrunk_every_answer_says_how_to_get_it() {
    let s = scratch("none");
    let empty = s.root.join("empty-path");
    std::fs::create_dir_all(&empty).unwrap();
    let mut p = Plugin::start(
        &[
            ("PATH", &empty),
            ("HOME", &empty),
            ("USERPROFILE", &empty),
            ("LOCALAPPDATA", &empty),
            ("CARGO_HOME", &empty),
        ],
        &[],
    );
    init(&mut p, &s, Path::new(""), json!({}));
    let (err, text) = p.tool("list", json!({}), &json!({"id": "c", "workspace": s.repo}));
    assert!(err && text.contains("not installed"), "{text}");
    let state = p.call("ui/request", json!({"name": "state", "params": {}}));
    assert_eq!(state["version"]["installed"], json!(false), "{state}");
    assert!(state["version"]["install_commands"]
        .as_array()
        .is_some_and(|c| !c.is_empty()));
    let _ = std::fs::remove_dir_all(&s.root);
}

/// The real Worktrunk, when the machine has it: create, list, merge and
/// remove a worktree in a scratch repository.
#[test]
fn with_real_worktrunk_when_given_one() {
    let Ok(real) = std::env::var("WORKTRUNK_TEST_BIN") else {
        eprintln!("skipped: set WORKTRUNK_TEST_BIN to a Worktrunk program to run this");
        return;
    };
    let s = scratch("real");
    std::fs::remove_dir_all(s.repo.join(".git")).unwrap();
    let git = |args: &[&str]| {
        let ok = Command::new("git")
            .args(args)
            .current_dir(&s.repo)
            .status()
            .unwrap()
            .success();
        assert!(ok, "git {args:?}");
    };
    git(&["init", "-q", "-b", "main"]);
    git(&[
        "-c",
        "user.name=t",
        "-c",
        "user.email=t@t",
        "commit",
        "-q",
        "--allow-empty",
        "-m",
        "init",
    ]);
    let cfg = s.root.join("wt-config.toml");
    let mut p = Plugin::start(
        &[("WORKTRUNK_CONFIG_PATH", &cfg)],
        &[
            ("GIT_AUTHOR_NAME", "t"),
            ("GIT_AUTHOR_EMAIL", "t@t"),
            ("GIT_COMMITTER_NAME", "t"),
            ("GIT_COMMITTER_EMAIL", "t@t"),
        ],
    );
    init(&mut p, &s, Path::new(&real), json!({}));
    let chat = json!({"id": "c", "workspace": s.repo});
    let (err, text) = p.tool("create", json!({"branch": "feat-real"}), &chat);
    assert!(!err, "{text}");
    let (err, text) = p.tool("list", json!({}), &chat);
    assert!(!err && text.contains("feat-real"), "{text}");
    let (err, text) = p.tool("remove", json!({"branches": ["feat-real"]}), &chat);
    assert!(!err, "{text}");
    let (err, text) = p.tool("list", json!({}), &chat);
    assert!(!err && !text.contains("feat-real"), "{text}");
    let (err, text) = p.tool("version", json!({"check": false}), &chat);
    assert!(!err && text.contains("Worktrunk"), "{text}");
    let (err, text) = p.tool("help", json!({"command": ["switch"]}), &chat);
    assert!(!err && text.contains("--create"), "{text}");
    drop(p);
    let _ = std::fs::remove_dir_all(&s.root);
}
