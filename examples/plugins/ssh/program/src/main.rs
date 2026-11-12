//! openremote-ssh: an OpenRemote process plugin. It gives a chat's agent
//! four tools to work on the user's own SSH hosts (servers that run no
//! OpenRemote): `ssh_hosts`, `ssh_run`, `ssh_read_file` and
//! `ssh_write_file`. The hosts are the user's, set in the plugin's settings;
//! the connections are the system's own ssh, started by its full path.
//!
//! OpenRemote starts this program the first time something needs it and
//! talks to it over stdin and stdout in newline-delimited JSON-RPC 2.0
//! (docs/plugins/protocol.md). stdout carries protocol messages only.
//! When stdin closes, OpenRemote is gone: every ssh this program started is
//! stopped, and it exits.
//!
//! What it will not do, by construction (docs/plugins/ssh.md):
//! - sign in with a password, or keep a key: a host is an address, a key is
//!   a file's path, and a setting that holds a key or a password is refused;
//! - trust a host key nobody checked, or a changed one: ssh checks every key
//!   strictly, and only the user's Verify (from the plugin's page, never an
//!   agent's tool) records one;
//! - read an ssh config, or take an ssh option from anyone: ssh's options
//!   are this program's own constants;
//! - wait for a prompt: every connection is batch mode, with a time limit.

mod config;
mod digest;
mod ssh;

use std::collections::HashMap;
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use config::{Config, Host};

const OUTPUT_CAP: usize = 200_000;
const MAX_COMMAND: usize = 12_000;
const MAX_PATH: usize = 4_096;
const MAX_WRITE_BYTES: usize = 2_000_000;
const READ_DEFAULT: u64 = 100_000;
const READ_MAX: u64 = 200_000;
const RUN_DEFAULT_S: u64 = 60;
const RUN_MAX_S: u64 = 300;
const FILE_TIMEOUT: Duration = Duration::from_secs(60);
const WRITE_TIMEOUT: Duration = Duration::from_secs(120);
/// A look at a key or a test connection answers within the 60 s a page's
/// request waits.
const UI_TIMEOUT: Duration = Duration::from_secs(25);
/// A Verify the user has not confirmed stops counting after this long.
const PENDING_FOR: Duration = Duration::from_secs(300);

struct Pending {
    host: Host,
    lines: Vec<String>,
    fingerprint: String,
    at: Instant,
}

struct State {
    config: Config,
    data: PathBuf,
    pending: HashMap<String, Pending>,
}

#[derive(Clone)]
struct Shared {
    state: Arc<Mutex<State>>,
    out: Arc<Mutex<std::io::Stdout>>,
    live: ssh::Live,
    next: Arc<AtomicU64>,
}

impl Shared {
    fn send(&self, message: Value) {
        let mut out = self.out.lock().unwrap();
        let _ = writeln!(out, "{message}");
        let _ = out.flush();
    }

    fn log(&self, level: &str, message: &str) {
        self.send(json!({"jsonrpc": "2.0", "method": "log", "params": {"level": level, "message": message}}));
    }

    fn config(&self) -> Config {
        self.state.lock().unwrap().config.clone()
    }

    fn data(&self) -> PathBuf {
        self.state.lock().unwrap().data.clone()
    }

    fn known_hosts(&self) -> PathBuf {
        self.data().join("known_hosts")
    }
}

fn main() {
    let shared = Shared {
        state: Arc::new(Mutex::new(State {
            config: Config::from_settings(&Value::Null),
            data: std::env::var_os("OPENREMOTE_PLUGIN_DATA")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from(".data")),
            pending: HashMap::new(),
        })),
        out: Arc::new(Mutex::new(std::io::stdout())),
        live: Arc::default(),
        next: Arc::new(AtomicU64::new(1)),
    };
    let stdin = std::io::stdin();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        let Ok(message) = serde_json::from_str::<Value>(&line) else {
            eprintln!("ssh: a line that is not JSON was ignored");
            continue;
        };
        let method = message["method"].as_str().unwrap_or_default().to_string();
        let params = message.get("params").cloned().unwrap_or(Value::Null);
        let Some(id) = message.get("id").cloned() else {
            if method == "settings/changed" {
                apply_settings(&shared, &params["settings"]);
            }
            continue;
        };
        if method == "initialize" {
            initialize(&shared, &params);
            shared.send(json!({"jsonrpc": "2.0", "id": id, "result": {"protocol": 1}}));
            continue;
        }
        // Each call on its own thread: a long command does not hold up the
        // page's requests or another chat's call.
        let shared = shared.clone();
        std::thread::spawn(move || {
            let answer = match method.as_str() {
                "tools/call" => call_tool(&shared, &params),
                "ui/request" => ui_request(&shared, &params),
                other => Err((-32601, format!("no method {other}"))),
            };
            shared.send(match answer {
                Ok(result) => json!({"jsonrpc": "2.0", "id": id, "result": result}),
                Err((code, message)) => {
                    json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}})
                }
            });
        });
    }
    // OpenRemote is gone: nothing may be left running on the user's account.
    for (_, mut child) in shared.live.lock().unwrap().drain() {
        let _ = child.kill();
        let _ = child.wait();
    }
    std::process::exit(0);
}

fn initialize(shared: &Shared, params: &Value) {
    if let Some(data) = params["plugin"]["data"].as_str().filter(|d| !d.is_empty()) {
        shared.state.lock().unwrap().data = PathBuf::from(data);
    }
    let data = shared.data();
    if let Err(e) = std::fs::create_dir_all(&data) {
        eprintln!(
            "ssh: the data folder {} could not be made: {e}",
            data.display()
        );
    }
    // A scan file left by a Verify that was under way when the program
    // stopped holds a key nobody confirmed.
    if let Ok(entries) = std::fs::read_dir(&data) {
        for entry in entries.flatten() {
            if entry.file_name().to_string_lossy().starts_with("scan-") {
                let _ = std::fs::remove_file(entry.path());
            }
        }
    }
    apply_settings(shared, &params["settings"]);
    shared.log("info", "ready");
}

fn apply_settings(shared: &Shared, settings: &Value) {
    let config = Config::from_settings(settings);
    for note in &config.left_out {
        shared.log("warn", note);
    }
    let mut state = shared.state.lock().unwrap();
    state.config = config;
    // A look at a key is for the host as it was; a changed setting starts it over.
    state.pending.clear();
}

// ---- answers ----

fn reply(text: impl Into<String>, is_error: bool) -> Value {
    json!({"content": [{"type": "text", "text": text.into()}], "isError": is_error})
}

type Answer = Result<Value, (i64, String)>;

/// What a call needs to start ssh for a host, or the words that say why it
/// can't. Everything here is checked before ssh is started.
struct Ready {
    host: Host,
    program: PathBuf,
    args: Vec<String>,
    env: Vec<(String, String)>,
}

fn ready(
    shared: &Shared,
    name: &str,
    mode: ssh::Mode,
    known: Vec<PathBuf>,
) -> Result<Ready, String> {
    let config = shared.config();
    let Some(host) = config.host(name.trim()).cloned() else {
        let names: Vec<&str> = config.hosts.iter().map(|h| h.name.as_str()).collect();
        let shown: String = name.chars().take(60).collect();
        return Err(if names.is_empty() {
            "The user has not set up any SSH hosts yet. They add them under Settings > Plugins > SSH, in Hosts.".into()
        } else {
            format!(
                "There is no SSH host named \"{shown}\". The hosts are: {}.",
                names.join(", ")
            )
        });
    };
    let program = config.ssh.clone()?;
    let key = match &config.key_file {
        Some(Err(why)) => return Err(why.clone()),
        Some(Ok(path)) => Some(path.clone()),
        None => None,
    };
    let agent = match &config.agent_socket {
        Some(Err(why)) => return Err(why.clone()),
        Some(Ok(socket)) => Some(socket.clone()),
        None => None,
    };
    let args = ssh::args(&ssh::Target {
        host: &host,
        known_hosts: known,
        key_file: key.as_deref(),
        mode,
    })?;
    // A scan offers nothing to a host nobody has checked: no agent either.
    let env = ssh::environment(if mode == ssh::Mode::Scan {
        None
    } else {
        agent.as_deref()
    });
    Ok(Ready {
        host,
        program,
        args,
        env,
    })
}

/// The known_hosts files a real call trusts: the keys verified in this
/// plugin, then the user's own (filled when they connect from a terminal
/// and accept a key there). Neither is ever written by a connection.
fn trusted_files(shared: &Shared) -> Vec<PathBuf> {
    let mut files = vec![shared.known_hosts()];
    if let Some(home) = config::home() {
        files.push(home.join(".ssh").join("known_hosts"));
    }
    files
}

fn go(
    shared: &Shared,
    ready: &Ready,
    command: &str,
    input: Option<Vec<u8>>,
    timeout: Duration,
) -> Result<ssh::Outcome, String> {
    let id = shared.next.fetch_add(1, Ordering::Relaxed);
    ssh::run(
        &ready.program,
        ready.args.clone(),
        command,
        input,
        &ready.env,
        timeout,
        &shared.live,
        id,
    )
}

enum Verdict {
    TimedOut,
    /// ssh itself could not get the command run (255 and a complaint).
    Failed(&'static str, String),
    /// The command ran; its own exit code.
    Ran(Option<i32>),
}

fn verdict(host: &Host, outcome: &ssh::Outcome) -> Verdict {
    if outcome.timed_out {
        return Verdict::TimedOut;
    }
    let said = String::from_utf8_lossy(&outcome.stderr.bytes);
    if outcome.code == Some(255) && !said.trim().is_empty() {
        let (kind, words) = ssh::explain(&said, &host.name);
        return Verdict::Failed(kind, words);
    }
    Verdict::Ran(outcome.code)
}

fn last_chars(text: &str, count: usize) -> &str {
    let n = text.chars().count();
    if n <= count {
        return text;
    }
    let skip = text
        .char_indices()
        .nth(n - count)
        .map(|(i, _)| i)
        .unwrap_or(0);
    &text[skip..]
}

fn section(label: &str, tail: &ssh::Tail, keep: usize) -> String {
    let full = String::from_utf8_lossy(&tail.bytes);
    let text = last_chars(&full, keep);
    if text.is_empty() && tail.total == 0 {
        return format!("--- {label} ---\n(empty)");
    }
    let cut = tail.total.saturating_sub(text.len());
    let note = if cut > 0 {
        format!("[{label} was longer than the limit: its first {cut} bytes are left out, the end is shown]\n")
    } else {
        String::new()
    };
    format!("--- {label} ---\n{note}{text}")
}

/// Both streams inside one budget, each keeping its end; stderr keeps at
/// least a quarter of it.
fn render(outcome: &ssh::Outcome) -> String {
    let out_len = String::from_utf8_lossy(&outcome.stdout.bytes)
        .chars()
        .count();
    let err_len = String::from_utf8_lossy(&outcome.stderr.bytes)
        .chars()
        .count();
    let (mut out_keep, mut err_keep) = (out_len, err_len);
    if out_keep + err_keep > OUTPUT_CAP {
        err_keep = err_keep.min(OUTPUT_CAP.saturating_sub(out_keep).max(OUTPUT_CAP / 4));
        out_keep = out_keep.min(OUTPUT_CAP - err_keep);
    }
    format!(
        "{}\n\n{}",
        section("stdout", &outcome.stdout, out_keep),
        section("stderr", &outcome.stderr, err_keep)
    )
}

// ---- the agent's words, on their way to the remote shell ----

/// One word for a POSIX shell: everything inside single quotes is literal,
/// and a single quote is written as '\'' (close, escaped quote, open).
fn sh_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

/// A remote path as a shell word. A leading ~ becomes $HOME, spelled by
/// this code and not by the agent; everything else is quoted literally.
fn remote_path(value: &str) -> String {
    if value == "~" {
        return "\"$HOME\"".into();
    }
    if let Some(rest) = value.strip_prefix("~/") {
        return format!("\"$HOME\"/{}", sh_quote(rest));
    }
    sh_quote(value)
}

fn text_arg(
    args: &Value,
    key: &str,
    what: &str,
    max: usize,
    allow_newlines: bool,
) -> Result<String, String> {
    let value = args[key].as_str().unwrap_or_default();
    if value.is_empty() {
        return Err(format!("{what} is needed."));
    }
    if value.contains('\0') || (!allow_newlines && value.chars().any(char::is_control)) {
        return Err(format!("{what} cannot contain control characters."));
    }
    if value.chars().count() > max {
        return Err(format!("{what} is longer than {max} characters."));
    }
    Ok(value.to_string())
}

fn whole(args: &Value, key: &str, min: u64, max: u64, default: u64) -> u64 {
    args[key]
        .as_f64()
        .filter(|n| n.is_finite())
        .map(|n| (n.floor().max(0.0) as u64).clamp(min, max))
        .unwrap_or(default)
}

// ---- the tools ----

fn call_tool(shared: &Shared, params: &Value) -> Answer {
    let name = params["name"].as_str().unwrap_or_default();
    let args = if params["arguments"].is_object() {
        params["arguments"].clone()
    } else {
        json!({})
    };
    let result = match name {
        "ssh_hosts" => Ok(tool_hosts(shared)),
        "ssh_run" => tool_run(shared, &args),
        "ssh_read_file" => tool_read(shared, &args),
        "ssh_write_file" => tool_write(shared, &args),
        // Verify, Test and Forget are the user's, from the plugin's page:
        // an agent can't trust a key.
        other => return Err((-32602, format!("no tool named {other}"))),
    };
    Ok(result.unwrap_or_else(|why| reply(why, true)))
}

fn host_name(args: &Value) -> String {
    args["host"].as_str().unwrap_or_default().to_string()
}

fn tool_hosts(shared: &Shared) -> Value {
    let config = shared.config();
    if config.hosts.is_empty() && config.left_out.is_empty() {
        return reply(
            "No SSH hosts are set up yet. The user adds them under Settings > Plugins > SSH, in Hosts.",
            false,
        );
    }
    let hosts: Vec<Value> = config
        .hosts
        .iter()
        .map(|h| json!({"name": h.name, "address": h.shown(), "host_key": known_state(shared, h)}))
        .collect();
    let mut text = serde_json::to_string_pretty(&hosts).unwrap_or_default();
    if !config.left_out.is_empty() {
        text.push_str(&format!(
            "\n\nSome entries of the Hosts setting were left out: {}",
            config.left_out.join(" ")
        ));
    }
    text.push_str(
        "\n\nA host whose key is not verified is refused by ssh before anything is sent; the user verifies it, never an agent.",
    );
    reply(text, false)
}

fn tool_run(shared: &Shared, args: &Value) -> Result<Value, String> {
    let command = text_arg(args, "command", "The command", MAX_COMMAND, true)?;
    let seconds = whole(args, "timeout_s", 1, RUN_MAX_S, RUN_DEFAULT_S);
    let ready = ready(
        shared,
        &host_name(args),
        ssh::Mode::Run,
        trusted_files(shared),
    )?;
    let outcome = go(shared, &ready, &command, None, Duration::from_secs(seconds))?;
    let name = &ready.host.name;
    Ok(match verdict(&ready.host, &outcome) {
        Verdict::TimedOut => reply(
            format!(
                "The command did not finish within {seconds} s, so the connection to {name} was closed. It may still be running there. What it printed so far:\n\n{}",
                render(&outcome)
            ),
            true,
        ),
        Verdict::Failed(_, words) => reply(format!("The command was not run on {name}. {words}"), true),
        Verdict::Ran(code) => {
            let head = match code {
                Some(c) => format!("exit code {c} on {name} ({:.1} s)", outcome.elapsed.as_secs_f64()),
                None => format!("ssh was stopped before the command ended on {name}"),
            };
            reply(format!("{head}\n\n{}", render(&outcome)), code != Some(0))
        }
    })
}

fn tool_read(shared: &Shared, args: &Value) -> Result<Value, String> {
    let file = text_arg(args, "path", "The path", MAX_PATH, false)?;
    let max = whole(args, "max_bytes", 1, READ_MAX, READ_DEFAULT);
    let ready = ready(
        shared,
        &host_name(args),
        ssh::Mode::Run,
        trusted_files(shared),
    )?;
    // One byte more than asked, to know whether the file goes on.
    let command = format!("head -c {} -- {}", max + 1, remote_path(&file));
    let outcome = go(shared, &ready, &command, None, FILE_TIMEOUT)?;
    let name = &ready.host.name;
    match verdict(&ready.host, &outcome) {
        Verdict::TimedOut => Err(format!(
            "{file} was not read within {} s on {name}.",
            FILE_TIMEOUT.as_secs()
        )),
        Verdict::Failed(_, words) => Err(words),
        Verdict::Ran(code) if code != Some(0) => {
            let said = String::from_utf8_lossy(&outcome.stderr.bytes)
                .trim()
                .to_string();
            Err(format!(
                "{file} could not be read on {name}: {}",
                if said.is_empty() {
                    format!("exit code {code:?}")
                } else {
                    said
                }
            ))
        }
        Verdict::Ran(_) => {
            let bytes = &outcome.stdout.bytes;
            if bytes.contains(&0) {
                return Err(format!(
                    "{file} on {name} looks like a binary file, and ssh_read_file reads text. Look inside it with ssh_run (file, strings, xxd)."
                ));
            }
            let cut = bytes.len() as u64 > max;
            let body = String::from_utf8_lossy(&bytes[..bytes.len().min(max as usize)]);
            let body = if cut {
                body.trim_end_matches('\u{FFFD}')
            } else {
                &body
            };
            if body.is_empty() && !cut {
                return Ok(reply(format!("({file} is empty)"), false));
            }
            let note = if cut {
                format!("\n\n[{file} is longer than {max} bytes: only the first {max} are shown. Ask for more with max_bytes (up to {READ_MAX}), or read a part with ssh_run.]")
            } else {
                String::new()
            };
            Ok(reply(format!("{body}{note}"), false))
        }
    }
}

fn tool_write(shared: &Shared, args: &Value) -> Result<Value, String> {
    let file = text_arg(args, "path", "The path", MAX_PATH, false)?;
    let Some(content) = args["content"].as_str() else {
        return Err("content is needed (it may be empty).".into());
    };
    let bytes = content.as_bytes().to_vec();
    if bytes.len() > MAX_WRITE_BYTES {
        return Err(format!(
            "That is {} bytes; ssh_write_file takes up to {MAX_WRITE_BYTES}. Write it in parts with append.",
            bytes.len()
        ));
    }
    let append = args["append"].as_bool() == Some(true);
    let ready = ready(
        shared,
        &host_name(args),
        ssh::Mode::Run,
        trusted_files(shared),
    )?;
    let command = format!(
        "cat {} {}",
        if append { ">>" } else { ">" },
        remote_path(&file)
    );
    let size = bytes.len();
    let outcome = go(shared, &ready, &command, Some(bytes), WRITE_TIMEOUT)?;
    let name = &ready.host.name;
    match verdict(&ready.host, &outcome) {
        Verdict::TimedOut => Err(format!(
            "Writing {file} on {name} did not finish within {} s. The file may be incomplete.",
            WRITE_TIMEOUT.as_secs()
        )),
        Verdict::Failed(_, words) => Err(words),
        Verdict::Ran(code) if code != Some(0) => {
            let said = String::from_utf8_lossy(&outcome.stderr.bytes)
                .trim()
                .to_string();
            Err(format!(
                "{file} was not written on {name}: {}",
                if said.is_empty() {
                    format!("exit code {code:?}")
                } else {
                    said
                }
            ))
        }
        Verdict::Ran(_) => Ok(reply(
            format!(
                "{} {size} bytes to {file} on {name}.",
                if append { "Appended" } else { "Wrote" }
            ),
            false,
        )),
    }
}

// ---- host keys ----

struct KnownKey {
    field: String,
    kind: String,
    blob: String,
    line: String,
}

/// The keys in a known_hosts text. Comments, blank lines and @-marked
/// lines (@cert-authority, @revoked) are not keys a host presented.
fn parse_known_hosts(text: &str) -> Vec<KnownKey> {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#') && !l.starts_with('@'))
        .filter_map(|line| {
            let mut parts = line.split_whitespace();
            let (field, kind, blob) = (parts.next()?, parts.next()?, parts.next()?);
            digest::base64_decode(blob)?;
            Some(KnownKey {
                field: field.into(),
                kind: kind.into(),
                blob: blob.into(),
                line: line.into(),
            })
        })
        .collect()
}

fn is_for(field: &str, host: &Host) -> bool {
    let wanted = host.known_hosts_field();
    field.split(',').any(|f| f == wanted)
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_default()
}

/// The key this plugin verified for a host, if any.
fn verified_key(shared: &Shared, host: &Host) -> Option<KnownKey> {
    parse_known_hosts(&read(&shared.known_hosts()))
        .into_iter()
        .find(|k| is_for(&k.field, host))
}

/// Whether a host's key is known: verified here, in the user's own
/// known_hosts, or perhaps there (a hashed file can't be read for names).
fn known_state(shared: &Shared, host: &Host) -> &'static str {
    if verified_key(shared, host).is_some() {
        return "verified in OpenRemote";
    }
    let yours = config::home()
        .map(|h| read(&h.join(".ssh").join("known_hosts")))
        .unwrap_or_default();
    if parse_known_hosts(&yours)
        .iter()
        .any(|k| is_for(&k.field, host))
    {
        return "in your known_hosts";
    }
    if yours.lines().any(|l| l.trim_start().starts_with("|1|")) {
        return "not verified in OpenRemote (your known_hosts is hashed, so it may be there)";
    }
    "not verified"
}

/// Write the plugin's known_hosts without the lines for one host, then
/// with `add` after them. A new file is written beside the old and moved
/// over it, so a crash leaves one or the other.
fn rewrite_known_hosts(shared: &Shared, host: &Host, add: &[String]) -> Result<(), String> {
    let file = shared.known_hosts();
    let kept: Vec<String> = read(&file)
        .lines()
        .filter(|line| {
            let field = line.split_whitespace().next().unwrap_or("");
            line.trim().is_empty() || !is_for(field, host)
        })
        .filter(|l| !l.trim().is_empty())
        .map(String::from)
        .chain(add.iter().cloned())
        .collect();
    let body = if kept.is_empty() {
        String::new()
    } else {
        format!("{}\n", kept.join("\n"))
    };
    let temp = file.with_extension(format!("tmp-{}", std::process::id()));
    std::fs::write(&temp, body).map_err(|e| format!("known_hosts could not be written: {e}"))?;
    std::fs::rename(&temp, &file).map_err(|e| {
        let _ = std::fs::remove_file(&temp);
        format!("known_hosts could not be written: {e}")
    })
}

fn key_label(kind: &str) -> &str {
    match kind {
        "ssh-ed25519" => "ED25519",
        "ssh-rsa" => "RSA",
        "ecdsa-sha2-nistp256" | "ecdsa-sha2-nistp384" | "ecdsa-sha2-nistp521" => "ECDSA",
        "sk-ssh-ed25519@openssh.com" => "ED25519-SK",
        "sk-ecdsa-sha2-nistp256@openssh.com" => "ECDSA-SK",
        other => other,
    }
}

// ---- the page's requests ----

fn ui_request(shared: &Shared, params: &Value) -> Answer {
    let name = params["name"].as_str().unwrap_or_default();
    let p = if params["params"].is_object() {
        params["params"].clone()
    } else {
        json!({})
    };
    let host = || p["name"].as_str().unwrap_or_default().to_string();
    let result = match name {
        "state" => Ok(page_state(shared)),
        "verify_start" => verify_start(shared, &host()),
        "verify_confirm" => verify_confirm(
            shared,
            &host(),
            p["fingerprint"].as_str().unwrap_or_default(),
        ),
        "forget" => forget(shared, &host()),
        "host_test" => host_test(shared, &host()),
        other => return Err((-32601, format!("no request named {other}"))),
    };
    result.map_err(|why| (-32000, why))
}

fn page_state(shared: &Shared) -> Value {
    let config = shared.config();
    let hosts: Vec<Value> = config
        .hosts
        .iter()
        .map(|h| {
            let key = verified_key(shared, h);
            json!({
                "name": h.name,
                "address": h.shown(),
                "host_key": known_state(shared, h),
                "verified": key.is_some(),
                "fingerprint": key.as_ref().and_then(|k| digest::fingerprint(&k.blob)),
                "key_type": key.as_ref().map(|k| key_label(&k.kind).to_string()),
            })
        })
        .collect();
    fn problem<T>(r: &Option<Result<T, String>>) -> Value {
        match r {
            Some(Err(why)) => Value::String(why.clone()),
            _ => Value::Null,
        }
    }
    json!({
        "ssh": match &config.ssh {
            Ok(path) => json!({"path": path, "problem": null}),
            Err(why) => json!({"path": null, "problem": why}),
        },
        "key_file": {"set": config.key_file.is_some(), "problem": problem(&config.key_file)},
        "agent": {"set": config.agent_socket.is_some(), "problem": problem(&config.agent_socket)},
        "hosts": hosts,
        "left_out": config.left_out,
        "connections": ssh::CONNECTIONS,
    })
}

/// Verify, first half: connect once and show the key. Nothing is trusted
/// here; what was seen is held for the user's confirm.
fn verify_start(shared: &Shared, name: &str) -> Result<Value, String> {
    let scan = shared.data().join(format!(
        "scan-{}-{}",
        std::process::id(),
        shared.next.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::write(&scan, "").map_err(|e| format!("A scratch file could not be made: {e}"))?;
    let result = (|| {
        let ready = ready(shared, name, ssh::Mode::Scan, vec![scan.clone()])?;
        let outcome = go(shared, &ready, "true", None, UI_TIMEOUT)?;
        let keys = parse_known_hosts(&read(&scan));
        let Some(first) = keys.first() else {
            if outcome.timed_out {
                return Err(format!(
                    "{} did not answer in time, so no host key could be read.",
                    ready.host.name
                ));
            }
            let said = String::from_utf8_lossy(&outcome.stderr.bytes);
            return Err(ssh::explain(&said, &ready.host.name).1);
        };
        let fingerprint =
            digest::fingerprint(&first.blob).ok_or("The host's key could not be read.")?;
        let lines: Vec<String> = keys
            .iter()
            .filter(|k| k.blob == first.blob)
            .map(|k| k.line.clone())
            .collect();
        let previous = verified_key(shared, &ready.host)
            .filter(|k| k.blob != first.blob)
            .map(|k| json!({"type": key_label(&k.kind), "fingerprint": digest::fingerprint(&k.blob)}));
        let same = verified_key(shared, &ready.host).is_some_and(|k| k.blob == first.blob);
        let answer = json!({
            "name": ready.host.name,
            "address": ready.host.shown(),
            "type": key_label(&first.kind),
            "fingerprint": fingerprint,
            "same": same,
            "previous": previous,
        });
        shared.state.lock().unwrap().pending.insert(
            ready.host.name.clone(),
            Pending {
                host: ready.host.clone(),
                lines,
                fingerprint,
                at: Instant::now(),
            },
        );
        Ok(answer)
    })();
    let _ = std::fs::remove_file(&scan);
    result
}

/// Verify, second half: the user compared the fingerprint the page showed
/// with the host's own, and trusts it. The page sends that fingerprint
/// back; only the key it names, seen by this program within five minutes
/// for the host as it is now, is recorded.
fn verify_confirm(shared: &Shared, name: &str, fingerprint: &str) -> Result<Value, String> {
    let pending = shared.state.lock().unwrap().pending.remove(name);
    let Some(pending) = pending else {
        return Err("That check ran out, or the settings changed. Press Verify again.".into());
    };
    if pending.at.elapsed() > PENDING_FOR {
        return Err("That check ran out. Press Verify again.".into());
    }
    if pending.fingerprint != fingerprint {
        return Err("That is not the key that was shown. Press Verify again.".into());
    }
    let config = shared.config();
    if config.host(name) != Some(&pending.host) {
        return Err(format!(
            "{name} was changed while it was being checked. Press Verify again."
        ));
    }
    rewrite_known_hosts(shared, &pending.host, &pending.lines)?;
    Ok(json!({"name": name, "verified": true, "fingerprint": pending.fingerprint}))
}

fn forget(shared: &Shared, name: &str) -> Result<Value, String> {
    let config = shared.config();
    let host = config
        .host(name)
        .ok_or_else(|| format!("There is no host named {name}."))?;
    rewrite_known_hosts(shared, host, &[])?;
    Ok(json!({"name": name, "verified": false}))
}

fn host_test(shared: &Shared, name: &str) -> Result<Value, String> {
    let ready = ready(shared, name, ssh::Mode::Run, trusted_files(shared))?;
    let outcome = go(shared, &ready, "true", None, UI_TIMEOUT)?;
    Ok(match verdict(&ready.host, &outcome) {
        Verdict::TimedOut => {
            json!({"ok": false, "kind": "timeout", "message": format!("{name} did not answer within {} s.", UI_TIMEOUT.as_secs())})
        }
        Verdict::Failed(kind, words) => json!({"ok": false, "kind": kind, "message": words}),
        Verdict::Ran(Some(0)) => {
            json!({"ok": true, "kind": "ok", "ms": outcome.elapsed.as_millis() as u64})
        }
        Verdict::Ran(code) => {
            json!({"ok": false, "kind": "other", "message": format!("The test command ended with {code:?}.")})
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_remote_path_is_one_literal_shell_word() {
        assert_eq!(sh_quote("a b"), "'a b'");
        assert_eq!(sh_quote("it's"), "'it'\\''s'");
        assert_eq!(sh_quote("$(id) `x` ;|&"), "'$(id) `x` ;|&'");
        assert_eq!(remote_path("~"), "\"$HOME\"");
        assert_eq!(remote_path("~/a'b"), "\"$HOME\"/'a'\\''b'");
        assert_eq!(remote_path("~user/x"), "'~user/x'");
        assert_eq!(remote_path("-rf"), "'-rf'");
    }

    #[test]
    fn known_hosts_lines_are_read_by_host_and_port() {
        let text = "# c\n@revoked * ssh-rsa AAAA\nweb.example,1.2.3.4 ssh-ed25519 YWJj\n[db.example]:2222 ssh-rsa YWJj\nbad line\n";
        let keys = parse_known_hosts(text);
        assert_eq!(keys.len(), 2);
        let web = Host {
            name: "w".into(),
            user: None,
            address: "web.example".into(),
            port: 22,
        };
        let db = Host {
            name: "d".into(),
            user: None,
            address: "db.example".into(),
            port: 2222,
        };
        let db22 = Host {
            port: 22,
            ..db.clone()
        };
        assert!(is_for(&keys[0].field, &web));
        assert!(is_for(&keys[1].field, &db));
        assert!(!is_for(&keys[1].field, &db22));
    }

    #[test]
    fn output_keeps_its_end_within_the_budget() {
        let out = ssh::Tail {
            bytes: vec![b'a'; OUTPUT_CAP * 2],
            total: OUTPUT_CAP * 2,
        };
        let outcome = ssh::Outcome {
            code: Some(0),
            timed_out: false,
            stdout: out,
            stderr: ssh::Tail::default(),
            elapsed: Duration::ZERO,
        };
        let text = render(&outcome);
        assert!(text.contains("left out"));
        assert!(text.len() < OUTPUT_CAP + 500);
    }
}
