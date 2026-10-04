//! Harness sign-in on this machine - the harness's own login command,
//! relayed. The CLI keeps its own flow: it opens the browser itself (or
//! prints a device URL and code) and asks for a pasted code on stdin;
//! the daemon runs it with piped stdio, streams its words to the console
//! line by line, and feeds the human's answers through. When the CLI
//! exits, the supervisor re-asks the harness's own status words.
//!
//! The catalog is the harnesses whose login runs without a terminal:
//! claude (`auth login` - browser + pasted code), codex (`login` -
//! browser), grok (`login` - printed device URL and code). OpenCode's
//! `auth login` is an interactive TUI the console cannot relay, and Pi
//! signs in through a provider of its own; Antigravity has no login
//! command. No row, nothing honest to run.

use std::time::Duration;

use openremote_harness::Resolution;

/// (harness id, login argv) - the harness's own words, verbatim.
const LOGINS: &[(&str, &[&str])] = &[
    ("claude", &["auth", "login"]),
    ("codex", &["login"]),
    ("grok", &["login"]),
];

/// Whether this harness's own login command can be run at all - the
/// console only offers sign-in where the daemon can honestly drive it.
pub fn login_supported(harness_id: &str) -> bool {
    LOGINS.iter().any(|(id, _)| *id == harness_id)
}

/// The login argv for a harness, in its own words.
pub fn login_argv(harness_id: &str) -> Option<&'static [&'static str]> {
    LOGINS
        .iter()
        .find(|(id, _)| *id == harness_id)
        .map(|(_, argv)| *argv)
}

/// How a sign-in run surfaces to the console.
#[derive(Clone, Debug, Default, serde::Serialize)]
pub struct SignInView {
    pub running: bool,
    /// The harness's own words so far, ANSI escapes stripped.
    #[serde(default)]
    pub lines: Vec<String>,
    /// Set when the run settled: `(ok, the CLI's own closing words)`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub done: Option<SignInDone>,
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct SignInDone {
    pub ok: bool,
    pub text: String,
}

/// Ten minutes, then the run is failed - the install contract: the
/// human is present, not walked away.
const TIMEOUT: Duration = Duration::from_secs(600);

/// One piped login run. The supervisor owns the child; the console polls
/// `SignInView` and feeds lines through the stdin sender.
pub struct SignInRun {
    /// The transcript plus the settled state, appended by the reader
    /// tasks, read by the state view.
    pub state: std::sync::Mutex<SignInView>,
    /// Feeds a line to the CLI's stdin (claude's pasted code). `None`
    /// once the CLI closed its side or the run settled.
    pub stdin: tokio::sync::mpsc::Sender<String>,
    /// Stop signal - the reaper kills the login child when it fires. A
    /// permit is stored, so a stop that lands before the reaper reaches
    /// its wait still takes effect.
    stop: tokio::sync::Notify,
}

impl SignInRun {
    pub fn view(&self) -> SignInView {
        self.state.lock().expect("signin state lock").clone()
    }

    pub fn feed(&self, line: &str) -> Result<(), String> {
        if self.state.lock().expect("signin state lock").done.is_some() {
            return Err("this sign-in already finished".to_string());
        }
        self.stdin
            .try_send(format!("{line}\n"))
            .map_err(|_| "the harness's login prompt is no longer reading".to_string())
    }

    /// Ask the reaper to kill the login child. A human who abandoned the
    /// browser flow should not wait out the timeout to try again.
    pub fn stop(&self) {
        self.stop.notify_one();
    }

    /// Resolves when a stop was asked - the reaper races it against the
    /// child's own exit. (Notify::notified borrows the Notify, so the
    /// future rides inside the reaper's select; the guard is dropped
    /// before it.)
    pub fn stopped(&self) -> impl std::future::Future<Output = ()> + '_ {
        self.stop.notified()
    }
}

/// Spawn the harness's own login command with piped stdio. The returned
/// run relays its words; the caller keeps the child's reaper - exit
/// status lands through `on_exit`, after which the registry is re-probed.
pub async fn spawn_login(
    harness_id: &str,
    resolution: &Resolution,
) -> Result<(std::sync::Arc<SignInRun>, tokio::process::Child), String> {
    let Some(argv) = login_argv(harness_id) else {
        return Err(format!(
            "'{harness_id}' signs in through its own setup - no login command to relay"
        ));
    };
    let (program, mut full): (std::path::PathBuf, Vec<std::ffi::OsString>) = match resolution {
        Resolution::Executable(path) => (path.clone(), Vec::new()),
        Resolution::NodeScript { node, script } => {
            (node.clone(), vec![script.as_os_str().to_os_string()])
        }
        Resolution::Unavailable => {
            return Err(format!("'{harness_id}' is not installed on this machine"));
        }
    };
    full.extend(argv.iter().map(|arg| (*arg).into()));

    let mut child = tokio::process::Command::new(&program)
        .args(&full)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("spawn failed: {e}"))?;

    let stdout = child.stdout.take().expect("stdout piped");
    let stderr = child.stderr.take().expect("stderr piped");
    let (stdin_tx, mut stdin_rx) = tokio::sync::mpsc::channel::<String>(4);
    let run = std::sync::Arc::new(SignInRun {
        state: std::sync::Mutex::new(SignInView {
            running: true,
            lines: Vec::new(),
            done: None,
        }),
        stdin: stdin_tx,
        stop: tokio::sync::Notify::new(),
    });

    // The CLI's own words, line by line, from both pipes. ANSI escapes
    // are stripped - the console renders the words, not the terminal
    // dressing the CLI assumed it had.
    let readers: Vec<Box<dyn tokio::io::AsyncRead + Send + Unpin>> =
        vec![Box::new(stdout), Box::new(stderr)];
    for pipe in readers {
        let run = std::sync::Arc::clone(&run);
        tokio::spawn(async move {
            use tokio::io::AsyncBufReadExt;
            let mut lines = tokio::io::BufReader::new(pipe).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                let clean = strip_ansi(&line);
                if clean.trim().is_empty() {
                    continue;
                }
                let mut state = run.state.lock().expect("signin state lock");
                // The transcript is bounded - a login that chatters
                // cannot grow the daemon's memory.
                if state.lines.len() < 200 {
                    state.lines.push(clean);
                }
            }
        });
    }

    // The human's answers ride back through stdin until the CLI closes.
    let mut stdin = child.stdin.take().expect("stdin piped");
    let writer = tokio::spawn(async move {
        use tokio::io::AsyncWriteExt;
        while let Some(line) = stdin_rx.recv().await {
            if stdin.write_all(line.as_bytes()).await.is_err() {
                break;
            }
            let _ = stdin.flush().await;
        }
        // Dropping closes the pipe - a CLI that reads stdin to the end
        // (an abandoned prompt) sees EOF, not a hang.
        stdin_rx.close();
    });

    // The writer rides with the child: when the reaper kills a timed-out
    // child, the writer's stdin handle drops too.
    std::mem::forget(writer);

    Ok((run, child))
}

/// The exit status settles the run: the CLI's own closing words (its
/// combined pipes, trimmed) become the done fact the console shows.
pub fn settle(run: &SignInRun, status: &std::process::ExitStatus) -> SignInDone {
    let done = SignInDone {
        ok: status.success(),
        text: if status.success() {
            "signed in".to_string()
        } else {
            format!("exit {}", status.code().unwrap_or(-1))
        },
    };
    run.state.lock().expect("signin state lock").done = Some(done.clone());
    run.state.lock().expect("signin state lock").running = false;
    done
}

/// Mark a run failed without an exit status (the timeout killed it).
pub fn fail(run: &SignInRun, text: &str) -> SignInDone {
    let done = SignInDone {
        ok: false,
        text: text.to_string(),
    };
    let mut state = run.state.lock().expect("signin state lock");
    state.done = Some(done.clone());
    state.running = false;
    done
}

/// Strip ANSI escape sequences and OSC commands - the words stay, the
/// terminal dressing goes.
fn strip_ansi(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\u{1b}' {
            out.push(c);
            continue;
        }
        match chars.peek() {
            Some('[') => {
                chars.next();
                for c in chars.by_ref() {
                    if c.is_ascii_alphabetic() {
                        break;
                    }
                }
            }
            Some(']') => {
                chars.next();
                for c in chars.by_ref() {
                    if c == '\u{7}' || c == '\u{1b}' {
                        break;
                    }
                }
            }
            _ => {
                chars.next();
            }
        }
    }
    out
}

/// How long a login may run before the daemon gives up on it.
pub fn timeout() -> Duration {
    TIMEOUT
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_catalog_signs_harnesses_in_in_their_own_words() {
        assert_eq!(login_argv("claude"), Some(&["auth", "login"][..]));
        assert_eq!(login_argv("codex"), Some(&["login"][..]));
        assert_eq!(login_argv("grok"), Some(&["login"][..]));
        // OpenCode's login is a TUI the console cannot relay; Pi signs in
        // through a provider of its own; Antigravity has no login command.
        // No guessed rows.
        assert!(!login_supported("opencode"));
        assert!(!login_supported("pi"));
        assert!(!login_supported("agy"));
    }

    #[tokio::test]
    async fn an_unavailable_harness_refuses_instead_of_running() {
        let err = spawn_login("codex", &Resolution::Unavailable)
            .await
            .err()
            .expect("refusal");
        assert!(err.contains("not installed"), "got: {err}");
    }

    #[tokio::test]
    async fn a_harness_without_a_login_command_says_so() {
        let err = spawn_login("pi", &Resolution::Unavailable)
            .await
            .err()
            .expect("refusal");
        assert!(err.contains("through its own setup"), "got: {err}");
    }

    #[test]
    fn ansi_dressing_is_stripped_but_words_stay() {
        assert_eq!(strip_ansi("\u{1b}[90mgrey words\u{1b}[0m"), "grey words");
        assert_eq!(strip_ansi("plain"), "plain");
        assert_eq!(
            strip_ansi("\u{1b}[?25lhidden cursor\u{1b}[?25h"),
            "hidden cursor"
        );
    }
}
