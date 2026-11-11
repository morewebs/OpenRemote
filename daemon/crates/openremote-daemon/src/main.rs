//! The daemon binary.
//!
//! - `openremote-daemon`: the desktop app's sidecar. Prints
//!   `READY 127.0.0.1:<port>` on stdout when it is listening (all logs go to
//!   stderr) and exits when stdin closes or the app that started it is
//!   gone - the desktop shell owns its lifetime.
//! - `openremote-daemon agent`: a machine's long-running service (systemd
//!   runs it). No stdin watchdog; exits 78 when this computer isn't in
//!   OpenRemote Cloud, so the service manager stops retrying.
//! - `openremote-daemon enroll <code>`: joins this computer to the account
//!   that minted the install code.
//! - `openremote-daemon status`: exits 0 when this computer is in Cloud.
//! - `openremote-daemon unenroll`: leaves Cloud (keeps private chats).
//! - `openremote-daemon mcp-chat --session <id> --port <p> --data-dir <d>`:
//!   the read-a-chat MCP server a chat's harness starts over stdio.
//! - `openremote-daemon --version`.

use std::process::ExitCode;
use std::sync::Arc;

use openremote_cloud::{Cloud, CloudConfig};
use openremote_daemon::app::{self, App, AppOptions};
use openremote_daemon::shutdown;

/// sysexits' EX_CONFIG (78) for a machine that is not in Cloud: a service
/// manager stops retrying, because the next start would say the same thing.
const NOT_ENROLLED: u8 = 78;

/// The same code for the other configuration the machine cannot fix by being
/// restarted - a data folder that cannot take the token, a path that is not
/// writable. Its own name so the code and the sentence in the docs stay about
/// the same thing: 78 means "configuration, do not retry", not always
/// "not enrolled".
const EX_CONFIG: u8 = 78;

/// The one usage line, so a refused argument and an unknown command say the
/// same thing. Exit 2 is a usage error, the same code `mcp-chat` uses.
fn usage_error(why: &str) -> ExitCode {
    eprintln!("openremote-daemon: {why}");
    eprintln!(
        "usage: openremote-daemon [agent | enroll <code> | status | unenroll | mcp-chat <args> | --version]"
    );
    ExitCode::from(2)
}

/// The refusal for a command that takes no arguments, or nothing when the
/// arguments are exactly the command. Ignoring is how
/// `agent --data-dir /srv/openremote` would run against the default folder and
/// make a second identity: the flag a unit file passes is the one thing a
/// person cannot check afterwards.
fn extra_argument(args: &[String], command: &str) -> Option<String> {
    args.get(1)
        .map(|extra| format!("{command} takes no arguments (got '{extra}')"))
}

fn no_arguments(args: &[String], command: &str) -> Option<ExitCode> {
    extra_argument(args, command).map(|why| usage_error(&why))
}

/// The code `enroll` was given, or the sentence saying why it has none. A flag
/// where the code belongs is refused rather than sent: `enroll --data-dir
/// /srv/openremote` would otherwise reach the registry as a code and come back
/// "that code is not valid", which sends a person to mint another code for a
/// flag. The same unit-file mistake the commands above refuse, answered the same
/// way.
fn enroll_code(args: &[String]) -> Result<&str, String> {
    // Any flag in the arguments is refused before the arity is considered:
    // `enroll --data-dir /srv/openremote` is one flag and one value, and the
    // arity alone would call that value an extra code.
    if let Some(flag) = args.iter().skip(1).find(|a| a.starts_with('-')) {
        return Err(format!(
            "enroll takes the code from the machine list, not a flag (got '{flag}')"
        ));
    }
    match (args.get(1), args.get(2)) {
        (None, _) => Err("enroll needs the code from the machine list".to_string()),
        (Some(code), None) => Ok(code),
        (Some(_), Some(extra)) => Err(format!("enroll takes one code (got an extra '{extra}')")),
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("tokio runtime");
    match args.first().map(String::as_str) {
        None => runtime.block_on(serve(false)),
        Some("agent") => match no_arguments(&args, "agent") {
            Some(refused) => refused,
            None => runtime.block_on(serve(true)),
        },
        Some("enroll") => match enroll_code(&args) {
            Ok(code) => runtime.block_on(enroll(code)),
            Err(why) => usage_error(&why),
        },
        // A chat's read-a-chat MCP server (OpenRemote Controls), started by
        // the chat's harness over stdio.
        Some("mcp-chat") => runtime.block_on(openremote_daemon::mcp_chat::run(&args[1..])),
        Some("status") => match no_arguments(&args, "status") {
            Some(refused) => refused,
            None => status(),
        },
        Some("unenroll") => match no_arguments(&args, "unenroll") {
            Some(refused) => refused,
            None => runtime.block_on(unenroll()),
        },
        Some(version @ ("--version" | "-V")) => match no_arguments(&args, version) {
            Some(refused) => refused,
            None => {
                println!("openremote-daemon {}", env!("CARGO_PKG_VERSION"));
                ExitCode::SUCCESS
            }
        },
        // Asking for help is not an error. It printed the usage line and exited 2
        // ("unknown command '--help'"), so a person following the usual reflex, and any
        // script that runs `--help` first to see whether a flag exists, read the one
        // command that always works as a refusal.
        Some(help @ ("--help" | "-h")) => match no_arguments(&args, help) {
            Some(refused) => refused,
            None => {
                println!(
                    "usage: openremote-daemon [agent | enroll <code> | status | unenroll | mcp-chat <args> | --version]"
                );
                ExitCode::SUCCESS
            }
        },
        Some(other) => usage_error(&format!("unknown command '{other}'")),
    }
}

async fn serve(agent: bool) -> ExitCode {
    let data_dir = app::default_data_dir();
    let cloud = CloudConfig::from_env();
    if agent && !Cloud::open(cloud.clone(), &data_dir).enrolled() {
        eprintln!(
            "openremote-daemon: this computer isn't in OpenRemote Cloud - run `openremote-daemon enroll <code>` first"
        );
        return ExitCode::from(NOT_ENROLLED);
    }
    // A data folder that cannot take the token is configuration, not a crash:
    // say which folder and stop, the way the not-enrolled path stops, instead
    // of panicking into a service restart loop that never says why.
    let token = match app::load_or_create_token(&data_dir) {
        Ok(token) => token,
        Err(e) => {
            eprintln!(
                "openremote-daemon: cannot use the data folder {} - {e}. Fix its permissions, or point OPENREMOTE_DATA_DIR at a writable folder",
                data_dir.display()
            );
            return ExitCode::from(EX_CONFIG);
        }
    };
    // Before the first child starts (the registry probe below): every
    // process this daemon starts ends with it, and none of them holds the
    // daemon's own pipes (shutdown.rs says why).
    shutdown::hold_children();
    shutdown::keep_pipes_private();
    // Bind early so the port is known before the app is assembled (the
    // read-a-chat MCP server a chat starts calls back on it), but do not
    // announce it yet. READY is what tells the shell's console that a daemon
    // is there to answer, and the console asks the moment it has an address:
    // the listener's backlog would hold those requests, unanswered, for the
    // whole of App::new below, which on a cold start is the registry probe
    // spawning every harness. The console's own timeout fires first, so a
    // healthy daemon reads as a dead one. The announcement belongs with the
    // accept loop, not with the bind.
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
        .await
        .expect("bind loopback");
    let addr = listener.local_addr().expect("local addr");
    // Publish the port for the read-a-chat server every chat starts: it
    // outlives this daemon, and a restart comes back on a different port, so
    // the value in a chat's argv goes stale. The file is owner-only and
    // atomic, the same treatment the token beside it gets, and it is removed
    // on a clean shutdown so a stale port is never read as a live one.
    if let Err(e) = openremote_daemon::mcp_chat::write_port_file(&data_dir, addr.port()) {
        eprintln!("openremote-daemon: could not publish the port: {e}");
    }
    eprintln!(
        "openremote-daemon: token at {}",
        data_dir.join("token").display()
    );

    let app = App::new(AppOptions {
        data_dir: data_dir.clone(),
        token: token.clone(),
        cloud,
    })
    .await;
    // Chats start OpenRemote's read-a-chat MCP server from this binary,
    // calling back on this port.
    if let Ok(exe) = std::env::current_exe() {
        app.supervisor.set_controls(exe, addr.port());
    }

    if agent {
        // A service is stopped with SIGTERM (or Ctrl-C when run by hand).
        // The sweep before exit(0) is the orphan fix: exit runs no
        // destructors, so without it every harness child outlives the
        // daemon (opencode serve children have null stdin and never see
        // EOF). Bounded - a wedged child never hangs the exit.
        let app = Arc::clone(&app);
        let dir = data_dir.clone();
        tokio::spawn(async move {
            shutdown_signal().await;
            shutdown::leave(app, "stopping", dir).await;
        });
    } else {
        // The app's sidecar: the app's browser talks to us over these same
        // pipes (`BROWSER {json}` lines both ways), so its tools are offered.
        app.browser.set_hosted();
        app.browser.connect(browser_sender());
        // The sidecar watchdog: when the shell dies, our stdin closes, and
        // the daemon must not outlive its owner. Same sweep for the same
        // reason - the children belong to this process, not to the shell.
        // The shell's process is watched too, in case a pipe end ever
        // outlives it.
        shutdown::watch_parent(Arc::clone(&app), data_dir.clone());
        let app = Arc::clone(&app);
        let dir = data_dir.clone();
        tokio::spawn(async move {
            use tokio::io::AsyncBufReadExt;
            let mut lines = tokio::io::BufReader::new(tokio::io::stdin()).lines();
            loop {
                match lines.next_line().await {
                    Ok(Some(line)) => {
                        if let Some(message) = openremote_browser::parse(&line) {
                            app.browser.receive(message);
                        }
                    }
                    // A line that isn't text is skipped; the pipe is still open.
                    Err(e) if e.kind() == std::io::ErrorKind::InvalidData => {}
                    Ok(None) | Err(_) => break,
                }
            }
            shutdown::leave(app, "stdin closed", dir).await;
        });
    }

    // Phone pairing. Its control routes ride on this same loopback listener:
    // the console already knows this address and this token, and a socket
    // bound to 127.0.0.1 is what keeps a phone out of them. The page a phone
    // scans is served by a second, narrow listener that the host opens only
    // while a code is live or a phone is paired.
    let pairing = openremote_daemon::pairing::PairingHost::open(Arc::clone(&app));
    {
        let pairing = Arc::clone(&pairing);
        tokio::spawn(async move { pairing.restore().await });
    }
    let _ = app.port.set(addr.port());
    let router = openremote_daemon::http::router(app).merge(pairing.control_router());
    // The announcement, then the accept loop with nothing awaited between
    // them: that ordering is the whole point of where these two lines sit.
    println!("READY 127.0.0.1:{}", addr.port());
    println!("DATA_DIR {}", data_dir.display());
    axum::serve(listener, router).await.expect("serve");
    ExitCode::SUCCESS
}

/// Calls to the app's browser, one line each on stdout (beside READY and
/// DATA_DIR; every log goes to stderr). Written from a thread of its own,
/// so a slow reader at the other end never holds up the daemon's work.
fn browser_sender() -> openremote_daemon::browser::Sender {
    let (tx, rx) = std::sync::mpsc::channel::<String>();
    std::thread::spawn(move || {
        use std::io::Write;
        let stdout = std::io::stdout();
        for line in rx {
            let mut out = stdout.lock();
            if writeln!(out, "{line}").and_then(|()| out.flush()).is_err() {
                return;
            }
        }
    });
    Arc::new(move |message| {
        let _ = tx.send(openremote_browser::line(&message));
    })
}

async fn shutdown_signal() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};
        if let Ok(mut term) = signal(SignalKind::terminate()) {
            tokio::select! {
                _ = term.recv() => {}
                _ = tokio::signal::ctrl_c() => {}
            }
            return;
        }
    }
    let _ = tokio::signal::ctrl_c().await;
}

async fn enroll(code: &str) -> ExitCode {
    let data_dir = app::default_data_dir();
    let cloud = Cloud::open(CloudConfig::from_env(), &data_dir);
    match cloud.enroll(code.trim()).await {
        Ok(joined) => {
            // The store learns its device id at the next start too; setting
            // it now keeps a crash in between harmless.
            if let Ok(mut store) = openremote_core::Store::open(data_dir) {
                let _ = store.set_this_device(Some(joined.device_id));
            }
            println!(
                "This computer joined {}'s OpenRemote Cloud as {}.",
                joined.account.email,
                cloud.config().device_name
            );
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("openremote-daemon: couldn't join: {e}");
            ExitCode::FAILURE
        }
    }
}

fn status() -> ExitCode {
    let cloud = Cloud::open(CloudConfig::from_env(), &app::default_data_dir());
    let view = cloud.view();
    let (line, code) = status_line(cloud.enrolled(), view["account"]["email"].as_str());
    println!("{line}");
    // A second line for the fact a person on a server cannot otherwise see.
    // Enrolment is not reachability: a proxy that lets the registry through
    // but blocks the relay enrols fine and is never reachable, and the laptop
    // is the only place that would notice. The exit code stays about enrolment
    // - that is the question a service script asks - and the sentence carries
    // the rest.
    if let Some(reach) = reach_line(view["state"].as_str(), view["error"].as_str()) {
        println!("{reach}");
    }
    ExitCode::from(code)
}

/// What a `status` run can honestly say beyond enrolment. The command opens its
/// own Cloud from the two files in the data folder, so the phase it reads is a
/// record left by the last daemon that ran, not the live mesh: printing
/// "reachable" from it would be a claim nobody measured, on a machine whose relay
/// may be blocked right now. One state is a fact the files really hold - a
/// credential the registry refused - and that one is worth saying, because
/// signing in again is the only way back.
fn reach_line(state: Option<&str>, error: Option<&str>) -> Option<String> {
    let why = error
        .map(str::trim)
        .filter(|e| !e.is_empty())
        .map(|e| format!(" ({e})"))
        .unwrap_or_default();
    match state {
        // The credential is present (so the line above says enrolled) but the
        // registry no longer accepts it: signing in again is the only way back.
        Some("relink") => Some(format!(
            "OpenRemote Cloud no longer accepts this computer's credential - sign in again{why}."
        )),
        _ => None,
    }
}

/// What `status` says and the code it exits with. An enrolled machine is in
/// Cloud whether or not the account block carries an email: a service script
/// reads the code and a person reads the sentence, so the two must not
/// disagree about the one fact this command exists to report.
fn status_line(enrolled: bool, email: Option<&str>) -> (String, u8) {
    if !enrolled {
        return ("Not in OpenRemote Cloud.".to_string(), 1);
    }
    match email {
        Some(email) => (format!("In OpenRemote Cloud as {email}."), 0),
        None => (
            "In OpenRemote Cloud, with no account name recorded.".to_string(),
            0,
        ),
    }
}

async fn unenroll() -> ExitCode {
    let data_dir = app::default_data_dir();
    let cloud = Cloud::open(CloudConfig::from_env(), &data_dir);
    // What the machine had before this run, for the sentence: a computer that
    // was never enrolled did not leave anything, and must not be told it did.
    let enrolled = cloud.device_credential().is_some() || cloud.device_id().is_some();
    // The account's side is best effort, but never silent: the registry delete
    // can fail (offline, a credential the registry already rejected), and the
    // sentence below says which half of "left the account" actually happened.
    // A registry that has already removed this device is the outcome this
    // command exists to reach, not a failure.
    let registry_error = match (cloud.device_credential(), cloud.device_id()) {
        (Some(credential), Some(id)) => {
            match cloud.registry().delete(&credential, &id.to_string()).await {
                Ok(()) | Err(openremote_cloud::registry::RegistryError::Revoked) => None,
                Err(e) => Some(e.to_string()),
            }
        }
        _ => None,
    };
    let local_error = cloud.signout().await.err().map(|e| e.to_string());
    let (line, code) = unenroll_answer(enrolled, local_error.as_deref(), registry_error.as_deref());
    if code == 0 {
        println!("{line}");
    } else {
        eprintln!("{line}");
    }
    ExitCode::from(code)
}

/// The sentence and the code after an unenroll attempt: `enrolled` is what the
/// machine had before the attempt, `local_error` is when the sign-out itself
/// failed, `registry_error` when the account still lists this computer. The
/// code follows the account, not just the disk - a machine sold on must not
/// read as gone while the account still lists it - and a machine that was never
/// enrolled gets the same sentence `status` uses, because it did not leave
/// anything.
fn unenroll_answer(
    enrolled: bool,
    local_error: Option<&str>,
    registry_error: Option<&str>,
) -> (String, u8) {
    if let Some(e) = local_error {
        return (format!("openremote-daemon: {e}"), 1);
    }
    if !enrolled {
        return ("Not in OpenRemote Cloud.".to_string(), 0);
    }
    match registry_error {
        Some(e) => (
            format!(
                "This computer left OpenRemote Cloud, but the account still lists it ({e}); it drops off when the registry expires it."
            ),
            1,
        ),
        None => ("This computer left OpenRemote Cloud.".to_string(), 0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn the_unretryable_exit_is_one_code_with_two_names() {
        // AGENTS.md tells a reader "exits 78 when not enrolled", and a service
        // manager keys on the number whatever the constant is called. Both
        // names must stay 78: the enrolment path keeps its own name so that
        // sentence stays true, and a configuration failure uses the general
        // one instead of borrowing the enrolment's.
        assert_eq!(NOT_ENROLLED, 78);
        assert_eq!(EX_CONFIG, 78);
    }

    #[test]
    fn status_agrees_with_itself_about_an_enrolled_machine_without_a_name() {
        let (line, code) = status_line(false, None);
        assert_eq!(code, 1);
        assert!(line.contains("Not in OpenRemote Cloud"));

        // The case D4 found: enrolled, but no name in the account block. It is
        // still enrolled, and both halves of the answer have to say so.
        let (line, code) = status_line(true, None);
        assert_eq!(code, 0);
        assert!(line.contains("In OpenRemote Cloud"));

        let (line, code) = status_line(true, Some("someone@example.com"));
        assert_eq!(code, 0);
        assert!(line.contains("someone@example.com"));
    }

    #[test]
    fn unenroll_says_which_half_happened() {
        let (line, code) = unenroll_answer(true, None, None);
        assert_eq!(code, 0);
        assert_eq!(line, "This computer left OpenRemote Cloud.");

        // A machine that was never enrolled did not leave anything: it gets the
        // sentence `status` uses for that state, not a claim that it left.
        let (line, code) = unenroll_answer(false, None, None);
        assert_eq!(code, 0);
        assert_eq!(line, "Not in OpenRemote Cloud.");

        // The account still lists it: the sentence says so, and the code
        // agrees rather than reporting a clean exit.
        let (line, code) = unenroll_answer(true, None, Some("offline"));
        assert_eq!(code, 1);
        assert!(line.contains("still lists it"));

        // The local sign-out failed: the account was never reached at all.
        let (line, code) = unenroll_answer(true, Some("no credential"), Some("offline"));
        assert_eq!(code, 1);
        assert!(line.contains("no credential"));
    }

    #[test]
    fn status_says_only_what_the_files_can_prove() {
        // A `status` run opens its own Cloud, so the phase it reads is a record
        // from the last daemon that ran rather than the live mesh. Every state
        // but the refused credential is silence, because "reachable" would be a
        // claim this command never measured.
        assert_eq!(reach_line(Some("online"), None), None);
        assert_eq!(reach_line(Some("connecting"), None), None);
        assert_eq!(reach_line(Some("offline"), None), None);
        let relink = reach_line(
            Some("relink"),
            Some("this device's credential was rejected"),
        )
        .unwrap();
        assert!(relink.contains("sign in again"), "{relink}");
        assert!(relink.contains("rejected"), "{relink}");
        // No state to read (an older view, a stubbed cloud): say nothing.
        assert_eq!(reach_line(None, None), None);
        assert_eq!(reach_line(Some("signed_out"), None), None);
        // An empty reason is no reason, not "()".
        assert!(
            !reach_line(Some("relink"), Some("  "))
                .unwrap()
                .contains("()")
        );
    }

    #[test]
    fn a_command_that_takes_no_arguments_refuses_one() {
        // The flag a unit file passes must not run silently against another
        // folder: `agent --data-dir /srv/openremote` used to be ignored.
        assert!(extra_argument(&args(&["agent"]), "agent").is_none());
        let why = extra_argument(&args(&["agent", "--data-dir", "/srv/x"]), "agent")
            .expect("an extra argument is refused");
        assert!(why.contains("agent takes no arguments"), "{why}");
        assert!(why.contains("--data-dir"), "the flag is named back: {why}");
        assert!(extra_argument(&args(&["status", "now"]), "status").is_some());
        assert!(extra_argument(&args(&["--version", "x"]), "--version").is_some());
        // And the refusal is the usage error: 2, with the one usage line.
        assert!(no_arguments(&args(&["status", "now"]), "status").is_some());
    }

    #[test]
    fn enroll_refuses_a_flag_where_the_code_belongs() {
        // The code is the only thing `enroll` takes. A flag sent to the registry
        // as a code comes back "that code is not valid", which sends a person to
        // mint another code for something that was never a code.
        assert_eq!(enroll_code(&args(&["enroll", "abc123"])), Ok("abc123"));
        let flag = enroll_code(&args(&["enroll", "--data-dir", "/srv/x"]))
            .expect_err("a flag is not a code");
        assert!(flag.contains("not a flag"), "{flag}");
        assert!(
            flag.contains("--data-dir"),
            "the flag is named back: {flag}"
        );
        let none = enroll_code(&args(&["enroll"])).expect_err("no code, no enrolment");
        assert!(none.contains("needs the code"), "{none}");
        // Two codes is a typo, not a second account.
        let two = enroll_code(&args(&["enroll", "abc", "def"])).expect_err("one code only");
        assert!(two.contains("one code"), "{two}");
    }
}
