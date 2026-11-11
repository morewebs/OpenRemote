//! The desktop app's side of the daemon sidecar, for the e2e that kills
//! it: `fixture-agent sidecar-host <daemon>` starts the daemon with its
//! three pipes the way the app does (no console window on Windows), copies
//! the daemon's stdout to its own, reads the daemon's stderr and drops it,
//! and holds the daemon's stdin open. It says `DAEMON_PID <pid>` first.
//! Killing this process is the app's death: every pipe end the daemon
//! talks to goes at once, stderr's reader included.

use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};

pub fn main_sidecar_host(argv: &[String]) {
    let Some(daemon) = argv.get(1) else {
        eprintln!("usage: fixture-agent sidecar-host <daemon>");
        std::process::exit(2);
    };
    let mut command = Command::new(daemon);
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    let mut child = command.spawn().expect("spawn the daemon");
    let mut out = std::io::stdout();
    let _ = writeln!(out, "DAEMON_PID {}", child.id());
    let _ = out.flush();

    // Held, never written: the daemon's stdin closes only when this
    // process goes.
    let _stdin = child.stdin.take();
    let mut stderr = child.stderr.take().expect("stderr piped");
    std::thread::spawn(move || std::io::copy(&mut stderr, &mut std::io::sink()));
    let stdout = child.stdout.take().expect("stdout piped");
    for line in BufReader::new(stdout).lines().map_while(Result::ok) {
        let _ = writeln!(out, "{line}");
        let _ = out.flush();
    }
    // The daemon closed its stdout: it is gone, and so is this host.
    let _ = child.wait();
}
