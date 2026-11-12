//! Running the system's ssh. Every connection the plugin makes is built by
//! `args` and started by `run`: its options, its environment, its timeout
//! and its share of the four connection slots.
//!
//! What makes it safe to hand to an agent:
//! - ssh is started directly (no shell on this computer), by its full path,
//!   with the host's user, port and address each one argument, the address
//!   after `--`, and the remote command one argument after it;
//! - `-F none`: no ssh config file is read, so nothing in one adds a
//!   ProxyCommand, a LocalCommand, a forward or a control socket; every
//!   option is this file's own constant, apart from the known_hosts files
//!   and the key file, which are paths;
//! - host keys are always checked (`StrictHostKeyChecking=yes`) against
//!   the keys the user verified, and never written by a connection;
//! - no prompt, no password, no forwarding;
//! - ssh's environment is a handful of variables it needs to find the
//!   user's home folder, plus the agent's socket when the user set one.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use crate::config::{option_path, Host};

/// At most this many ssh connections live at once; more calls wait.
pub const CONNECTIONS: usize = 4;
/// The longest a run takes after its time is up: ssh is stopped, and its
/// output is collected for at most this long. A call's whole life is its
/// wait for a connection (the caller's `queue_wait`), its run and this,
/// which together stay inside the time the daemon gives the call.
pub const STOP_GRACE: Duration = Duration::from_secs(5);
/// How much of each stream is held: a command that prints gigabytes costs
/// this program a few megabytes, not the lot.
pub const KEEP_BYTES: usize = 1_000_000;

/// The variables ssh is given from this program's own environment (the
/// cleared one the daemon passes): what it needs to find the user's home
/// folder and, on Windows, to start at all. Nothing else is handed on: not
/// PATH (ssh is started by its full path, and with no config it runs no
/// other program), not SSH_ASKPASS or DISPLAY (nothing may prompt).
/// Windows' ssh exits at once, saying nothing, without ProgramData.
pub const PASSED_ENV: &[&str] = &[
    "HOME",
    "USER",
    "LOGNAME",
    "TMPDIR",
    "LANG",
    "LC_ALL",
    "LC_CTYPE",
    "USERPROFILE",
    "USERNAME",
    "HOMEDRIVE",
    "HOMEPATH",
    "SystemRoot",
    "SYSTEMDRIVE",
    "windir",
    "TEMP",
    "TMP",
    "ProgramData",
];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    /// Every real call: the host's key must already be known and match.
    Run,
    /// Verify's one look at a key: an empty scratch file, a new key
    /// accepted into it (never into the real file), and no login tried,
    /// so nothing is offered to a host whose key nobody has checked.
    Scan,
}

pub struct Target<'a> {
    pub host: &'a Host,
    /// The known_hosts files, the plugin's own first.
    pub known_hosts: Vec<PathBuf>,
    pub key_file: Option<&'a Path>,
    pub mode: Mode,
    /// Offer no agent's keys: set on Windows when the user named no agent,
    /// since Windows' ssh finds its agent service without being told.
    pub no_agent: bool,
}

/// ssh's arguments for one connection, up to and including the address;
/// the caller adds the remote command.
pub fn args(target: &Target) -> Result<Vec<String>, String> {
    let scan = target.mode == Mode::Scan;
    let known = target
        .known_hosts
        .iter()
        .map(|p| option_path(p))
        .collect::<Result<Vec<_>, _>>()?
        .join(" ");
    let mut args: Vec<String> = vec!["-F".into(), "none".into(), "-T".into()];
    let mut option = |o: &str| {
        args.push("-o".into());
        args.push(o.into());
    };
    // No prompts, ever: a passphrase, a password or a yes/no question is a
    // failure with a reason, not a wait.
    option("BatchMode=yes");
    option("ConnectTimeout=10");
    option("ServerAliveInterval=15");
    option("ServerAliveCountMax=3");
    option(if scan {
        "StrictHostKeyChecking=accept-new"
    } else {
        "StrictHostKeyChecking=yes"
    });
    option(&format!("UserKnownHostsFile={known}"));
    option(&format!("GlobalKnownHostsFile={known}"));
    option("UpdateHostKeys=no");
    option("CheckHostIP=no");
    option("HashKnownHosts=no");
    option("PasswordAuthentication=no");
    option("KbdInteractiveAuthentication=no");
    if scan {
        option("PreferredAuthentications=none");
        option("PubkeyAuthentication=no");
    } else {
        option("PreferredAuthentications=publickey");
    }
    if scan || target.no_agent {
        option("IdentityAgent=none");
    }
    // What a config could set for convenience in a terminal and must not do
    // for an agent. With -F none none of it is read anyway; these say so on
    // the command line too.
    option("ProxyCommand=none");
    option("PermitLocalCommand=no");
    option("ForwardAgent=no");
    option("ForwardX11=no");
    option("ClearAllForwardings=yes");
    option("ControlMaster=no");
    option("ControlPath=none");
    // ssh's own log level is left at its default: at ERROR, Windows' ssh
    // says nothing at all when a connection with a ConnectTimeout is refused.
    if let (Some(key), false) = (target.key_file, scan) {
        args.push("-i".into());
        args.push(key.to_string_lossy().into_owned());
        args.push("-o".into());
        args.push("IdentitiesOnly=yes".into());
    }
    args.push("-p".into());
    args.push(target.host.port.to_string());
    if let Some(user) = &target.host.user {
        args.push("-l".into());
        args.push(user.clone());
    }
    // After --, the address is an address even if it began with a dash.
    args.push("--".into());
    args.push(target.host.address.clone());
    Ok(args)
}

/// The environment ssh starts with.
pub fn environment(agent_socket: Option<&str>) -> Vec<(String, String)> {
    let mut env: Vec<(String, String)> = std::env::vars()
        .filter(|(k, _)| PASSED_ENV.iter().any(|p| p.eq_ignore_ascii_case(k)))
        .collect();
    if let Some(socket) = agent_socket {
        env.push(("SSH_AUTH_SOCK".into(), socket.into()));
    }
    env
}

/// The last `keep` bytes of a stream, and how many came in all.
#[derive(Default)]
pub struct Tail {
    pub bytes: Vec<u8>,
    pub total: usize,
}

impl Tail {
    fn push(&mut self, chunk: &[u8]) {
        self.total += chunk.len();
        self.bytes.extend_from_slice(chunk);
        if self.bytes.len() > 2 * KEEP_BYTES {
            let cut = self.bytes.len() - KEEP_BYTES;
            self.bytes.drain(..cut);
        }
    }

    fn finish(&mut self) {
        if self.bytes.len() > KEEP_BYTES {
            let cut = self.bytes.len() - KEEP_BYTES;
            self.bytes.drain(..cut);
        }
    }
}

pub struct Outcome {
    pub code: Option<i32>,
    pub timed_out: bool,
    pub stdout: Tail,
    pub stderr: Tail,
    pub elapsed: Duration,
}

/// A counting gate: `acquire` waits up to a limit for a free slot.
pub struct Gate {
    used: Mutex<usize>,
    freed: Condvar,
}

pub struct Slot<'a>(&'a Gate);

impl Drop for Slot<'_> {
    fn drop(&mut self) {
        *self.0.used.lock().unwrap() -= 1;
        self.0.freed.notify_one();
    }
}

impl Gate {
    pub const fn new() -> Gate {
        Gate {
            used: Mutex::new(0),
            freed: Condvar::new(),
        }
    }

    pub fn acquire(&self, wait: Duration) -> Option<Slot<'_>> {
        let deadline = Instant::now() + wait;
        let mut used = self.used.lock().unwrap();
        while *used >= CONNECTIONS {
            let left = deadline.checked_duration_since(Instant::now())?;
            used = self.freed.wait_timeout(used, left).unwrap().0;
        }
        *used += 1;
        Some(Slot(self))
    }
}

pub static GATE: Gate = Gate::new();

/// Every ssh this program started that is still running, so they all go
/// when the program does.
pub type Live = Arc<Mutex<HashMap<u64, Child>>>;

fn drain(mut from: impl Read + Send + 'static) -> std::sync::mpsc::Receiver<Tail> {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut tail = Tail::default();
        let mut buf = [0u8; 16 * 1024];
        while let Ok(n) = from.read(&mut buf) {
            if n == 0 {
                break;
            }
            tail.push(&buf[..n]);
        }
        tail.finish();
        let _ = tx.send(tail);
    });
    rx
}

/// One ssh run, however it ends. `input` is the command's stdin (None: ssh
/// gets none, `-n`).
#[allow(clippy::too_many_arguments)]
pub fn run(
    program: &Path,
    mut args: Vec<String>,
    command: &str,
    input: Option<Vec<u8>>,
    env: &[(String, String)],
    queue_wait: Duration,
    timeout: Duration,
    live: &Live,
    id: u64,
) -> Result<Outcome, String> {
    // A call that finds no free connection in its wait gives up there, and
    // never starts later: the agent has its answer.
    let _slot = GATE.acquire(queue_wait).ok_or_else(|| {
        format!(
            "All {CONNECTIONS} ssh connections stayed busy for {} s, so this call did not start and will not. Try again in a moment.",
            queue_wait.as_secs()
        )
    })?;
    if input.is_none() {
        args.insert(0, "-n".into());
    }
    args.push(command.into());
    let started = Instant::now();
    let mut cmd = Command::new(program);
    cmd.args(&args)
        .env_clear()
        .envs(env.iter().map(|(k, v)| (k, v)))
        .stdin(if input.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // No console window flashes up for each connection.
        cmd.creation_flags(0x0800_0000);
    }
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("ssh could not be started from {}: {e}", program.display()))?;
    let out = drain(child.stdout.take().expect("piped"));
    let err = drain(child.stderr.take().expect("piped"));
    if let (Some(bytes), Some(mut stdin)) = (input, child.stdin.take()) {
        std::thread::spawn(move || {
            // A host that hangs up early closes this pipe under the write;
            // that is the host's answer, not a failure here.
            let _ = stdin.write_all(&bytes);
        });
    }
    live.lock().unwrap().insert(id, child);
    let mut timed_out = false;
    let code = loop {
        let status = {
            let mut map = live.lock().unwrap();
            match map.get_mut(&id) {
                Some(child) => child.try_wait().ok().flatten(),
                // The program is leaving and stopped it.
                None => break None,
            }
        };
        if let Some(status) = status {
            break status.code();
        }
        if started.elapsed() >= timeout {
            timed_out = true;
            if let Some(child) = live.lock().unwrap().get_mut(&id) {
                let _ = child.kill();
                let _ = child.wait();
            }
            break None;
        }
        std::thread::sleep(Duration::from_millis(25));
    };
    live.lock().unwrap().remove(&id);
    // Output still on its way is waited for briefly, never past the grace.
    let collect_by = Instant::now() + STOP_GRACE;
    let collect = |rx: std::sync::mpsc::Receiver<Tail>| {
        rx.recv_timeout(collect_by.saturating_duration_since(Instant::now()))
            .unwrap_or_default()
    };
    Ok(Outcome {
        code,
        timed_out,
        stdout: collect(out),
        stderr: collect(err),
        elapsed: started.elapsed(),
    })
}

/// What went wrong with a connection, from what ssh said: a kind for the
/// page, and words for the person or the agent.
pub fn explain(stderr: &str, name: &str) -> (&'static str, String) {
    let said = stderr.to_ascii_lowercase();
    let has = |needles: &[&str]| needles.iter().any(|n| said.contains(n));
    if has(&[
        "remote host identification has changed",
        "has changed and you have requested strict checking",
        "offending",
        "host key mismatch",
    ]) {
        return ("hostkey_changed", format!(
            "{name}'s host key is not the one that was verified, so ssh refused to connect and nothing was sent. That happens when a server is reinstalled, and it is also what someone in the way looks like. Ask the user to compare the host's new fingerprint with the host's own and, only if it is right, press Verify on {name} again, or connect from their own terminal and replace the old key in ~/.ssh/known_hosts there."
        ));
    }
    if has(&[
        "host key verification failed",
        "no ed25519 host key is known",
        "host key is known",
    ]) {
        return ("hostkey_unknown", format!(
            "{name}'s host key is not verified yet, so ssh refused to connect and nothing was sent. Ask the user to verify it: press Verify on {name} in the plugin's page, or connect once from their own terminal and check the fingerprint there."
        ));
    }
    if has(&[
        "unprotected private key file",
        "bad permissions",
        "are too open",
    ]) {
        return ("key_perms", format!(
            "ssh will not use the key file set for {name}: other users can read it. Restrict it to the user's account and try again."
        ));
    }
    if has(&["too many authentication failures"]) {
        return (
            "auth_many",
            format!(
            "ssh offered more keys than {name} allows. Set a Key file, so only that key is offered."
        ),
        );
    }
    if has(&["permission denied"]) {
        return ("auth", format!(
            "{name} refused the login. Only keys are used here, never passwords: the key must be in the host's authorized_keys and either loaded in the ssh agent (a key with a passphrase needs ssh-add, since nothing here can ask for it) or set as the Key file."
        ));
    }
    if has(&["connection refused"]) {
        return (
            "refused",
            format!("Nothing accepts ssh connections at {name}'s address and port."),
        );
    }
    if has(&["timed out"]) {
        return (
            "timeout",
            format!("{name} did not answer in time (connection timed out)."),
        );
    }
    if has(&[
        "could not resolve hostname",
        "name or service not known",
        "no such host is known",
        "nodename nor servname",
    ]) {
        return ("dns", format!("{name}'s address could not be found."));
    }
    if has(&["network is unreachable", "no route to host"]) {
        return (
            "unreachable",
            format!("This computer has no route to {name}."),
        );
    }
    if has(&[
        "bad configuration option",
        "unsupported option",
        "unknown option",
        "command-line line",
    ]) {
        return ("ssh_too_old", "This computer's ssh does not understand an option this plugin sends; it needs OpenSSH 8.0 or newer.".into());
    }
    let last = stderr
        .lines()
        .map(str::trim)
        .rfind(|l| !l.is_empty())
        .unwrap_or("");
    let last: String = last.chars().take(300).collect();
    (
        "other",
        if last.is_empty() {
            "ssh could not finish.".into()
        } else {
            format!("ssh could not finish: {last}")
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn host() -> Host {
        Host {
            name: "web".into(),
            user: Some("deploy".into()),
            address: "web.example".into(),
            port: 2200,
        }
    }

    fn options(args: &[String]) -> Vec<&str> {
        args.windows(2)
            .filter(|w| w[0] == "-o")
            .map(|w| w[1].as_str())
            .collect()
    }

    /// Every option the plugin may ever hand ssh, by name. A new one has to
    /// be added here on purpose.
    const ALLOWED: &[&str] = &[
        "BatchMode=yes",
        "ConnectTimeout=10",
        "ServerAliveInterval=15",
        "ServerAliveCountMax=3",
        "StrictHostKeyChecking=yes",
        "StrictHostKeyChecking=accept-new",
        "UpdateHostKeys=no",
        "CheckHostIP=no",
        "HashKnownHosts=no",
        "PasswordAuthentication=no",
        "KbdInteractiveAuthentication=no",
        "PreferredAuthentications=publickey",
        "PreferredAuthentications=none",
        "PubkeyAuthentication=no",
        "IdentityAgent=none",
        "ProxyCommand=none",
        "PermitLocalCommand=no",
        "ForwardAgent=no",
        "ForwardX11=no",
        "ClearAllForwardings=yes",
        "ControlMaster=no",
        "ControlPath=none",
        "IdentitiesOnly=yes",
    ];

    #[test]
    fn every_option_is_a_constant_of_this_file_or_a_known_hosts_path() {
        let key = PathBuf::from("/k/id");
        for mode in [Mode::Run, Mode::Scan] {
            let t = Target {
                host: &host(),
                known_hosts: vec![
                    PathBuf::from("/d/known_hosts"),
                    PathBuf::from("/h/.ssh/known_hosts"),
                ],
                key_file: Some(&key),
                mode,
                no_agent: false,
            };
            let args = args(&t).unwrap();
            for o in options(&args) {
                let ok = ALLOWED.contains(&o)
                    || o == "UserKnownHostsFile=/d/known_hosts /h/.ssh/known_hosts"
                    || o == "GlobalKnownHostsFile=/d/known_hosts /h/.ssh/known_hosts";
                assert!(ok, "{o}");
            }
        }
    }

    #[test]
    fn a_real_call_checks_host_keys_strictly_and_signs_in_by_key_only() {
        let key = PathBuf::from("/k/id");
        let t = Target {
            host: &host(),
            known_hosts: vec![PathBuf::from("/d/kh")],
            key_file: Some(&key),
            mode: Mode::Run,
            no_agent: false,
        };
        let args = args(&t).unwrap();
        let o = options(&args);
        assert!(o.contains(&"StrictHostKeyChecking=yes"));
        assert!(!o
            .iter()
            .any(|x| x.contains("accept-new") || *x == "StrictHostKeyChecking=no"));
        assert!(o.contains(&"PreferredAuthentications=publickey"));
        assert_eq!(&args[..2], ["-F", "none"]);
        let tail: Vec<&str> = args[args.len() - 9..].iter().map(String::as_str).collect();
        assert_eq!(
            tail,
            [
                "/k/id",
                "-o",
                "IdentitiesOnly=yes",
                "-p",
                "2200",
                "-l",
                "deploy",
                "--",
                "web.example"
            ]
        );
    }

    #[test]
    fn a_scan_offers_no_key_and_trusts_only_a_scratch_file() {
        let key = PathBuf::from("/k/id");
        let t = Target {
            host: &host(),
            known_hosts: vec![PathBuf::from("/d/scan-1")],
            key_file: Some(&key),
            mode: Mode::Scan,
            no_agent: false,
        };
        let args = args(&t).unwrap();
        let o = options(&args);
        assert!(o.contains(&"StrictHostKeyChecking=accept-new"));
        assert!(o.contains(&"PreferredAuthentications=none"));
        assert!(o.contains(&"IdentityAgent=none"));
        assert!(o.contains(&"UserKnownHostsFile=/d/scan-1"));
        assert!(!args.iter().any(|a| a == "-i"));
    }

    #[test]
    fn ssh_is_given_only_the_listed_variables_and_the_agent_from_the_setting() {
        let env = environment(Some("/run/agent.sock"));
        for (k, _) in &env {
            assert!(
                k == "SSH_AUTH_SOCK" || PASSED_ENV.iter().any(|p| p.eq_ignore_ascii_case(k)),
                "{k}"
            );
        }
        assert!(env
            .iter()
            .any(|(k, v)| k == "SSH_AUTH_SOCK" && v == "/run/agent.sock"));
        assert!(!environment(None).iter().any(|(k, _)| k == "SSH_AUTH_SOCK"));
        for never in [
            "PATH",
            "SSH_ASKPASS",
            "SSH_ASKPASS_REQUIRE",
            "DISPLAY",
            "SSH_AUTH_SOCK",
        ] {
            assert!(
                !PASSED_ENV.iter().any(|p| p.eq_ignore_ascii_case(never)),
                "{never}"
            );
        }
    }

    #[test]
    fn a_changed_key_is_told_apart_from_an_unknown_one() {
        let changed = "@@@\n@ WARNING: REMOTE HOST IDENTIFICATION HAS CHANGED! @\nHost key verification failed.\n";
        assert_eq!(explain(changed, "web").0, "hostkey_changed");
        assert_eq!(
            explain("Host key verification failed.\n", "web").0,
            "hostkey_unknown"
        );
        assert_eq!(
            explain("deploy@x: Permission denied (publickey).", "web").0,
            "auth"
        );
        assert_eq!(
            explain("ssh: connect to host x port 22: Connection refused", "web").0,
            "refused"
        );
        assert_eq!(
            explain("something odd\n", "web").1,
            "ssh could not finish: something odd"
        );
    }

    #[test]
    fn no_agent_turns_the_agent_off_for_a_real_call() {
        let t = Target {
            host: &host(),
            known_hosts: vec![PathBuf::from("/d/kh")],
            key_file: None,
            mode: Mode::Run,
            no_agent: true,
        };
        assert!(options(&args(&t).unwrap()).contains(&"IdentityAgent=none"));
        let t = Target {
            no_agent: false,
            ..t
        };
        assert!(!options(&args(&t).unwrap()).contains(&"IdentityAgent=none"));
    }
}
