//! A stand-in for the system's ssh, for the plugin's own tests only (it is
//! built by `cargo test`, never by build.sh, and never ships in bin/).
//!
//! It records how it was started - every argument and every environment
//! variable - as one JSON line in `fake-ssh-calls.jsonl`, in the folder of
//! the first known_hosts file it was given (the plugin's data folder), and
//! then plays a host by its address:
//!
//! - `ok.example`: runs nothing, and answers `ran: <command>` and how many
//!   bytes came on stdin. When it is asked to look at a key (Verify's
//!   `accept-new` into a scratch file) it writes a key there, as ssh would,
//!   and refuses the login, as `PreferredAuthentications=none` makes ssh do.
//! - `changed.example`: what ssh prints when the key is not the one on
//!   record, and exit code 255.
//! - `slow.example`: never answers.
//! - `-V`: a version line on stderr.

use std::io::{Read, Write};

/// An ed25519 public key, as ssh files one (any well-formed blob will do).
pub const KEY: &str = "AAAAC3NzaC1lZDI1NTE5AAAAIOMqqnkVzrm0SdG6UOoqKLsabgH5C9okWi0dh2l9GKJl";

fn option(args: &[String], name: &str) -> Option<String> {
    let prefix = format!("{name}=");
    args.windows(2)
        .find(|w| w[0] == "-o" && w[1].starts_with(&prefix))
        .map(|w| w[1][prefix.len()..].to_string())
}

/// The first file of a known_hosts option, unquoted.
fn first_file(value: &str) -> String {
    let value = value.trim();
    if let Some(rest) = value.strip_prefix('"') {
        rest.split('"').next().unwrap_or_default().to_string()
    } else {
        value
            .split_whitespace()
            .next()
            .unwrap_or_default()
            .to_string()
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "-V") {
        eprintln!("OpenSSH_9.9p1 fake, for tests");
        return;
    }
    let known = option(&args, "UserKnownHostsFile")
        .map(|v| first_file(&v))
        .unwrap_or_default();
    let known = std::path::PathBuf::from(known.replace("%%", "%"));
    let scan = option(&args, "StrictHostKeyChecking").as_deref() == Some("accept-new");
    let mut stdin = Vec::new();
    if !args.iter().any(|a| a == "-n") {
        let _ = std::io::stdin().read_to_end(&mut stdin);
    }
    let env: serde_json::Map<String, serde_json::Value> = std::env::vars()
        .map(|(k, v)| (k, serde_json::Value::String(v)))
        .collect();
    let folder = known.parent().map(|p| p.to_path_buf()).unwrap_or_default();
    let record = serde_json::json!({"args": args, "env": env, "stdin": stdin.len()});
    if let Ok(mut log) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(folder.join("fake-ssh-calls.jsonl"))
    {
        let _ = writeln!(log, "{record}");
    }

    let at = args
        .iter()
        .position(|a| a == "--")
        .expect("an address after --");
    let host = args[at + 1].clone();
    let command = args.get(at + 2).cloned().unwrap_or_default();
    match host.as_str() {
        "changed.example" => {
            eprintln!("@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@");
            eprintln!("@    WARNING: REMOTE HOST IDENTIFICATION HAS CHANGED!     @");
            eprintln!("@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@");
            eprintln!("Host key verification failed.");
            std::process::exit(255);
        }
        "slow.example" => {
            std::thread::sleep(std::time::Duration::from_secs(60));
        }
        _ if scan => {
            let port = args
                .windows(2)
                .find(|w| w[0] == "-p")
                .map(|w| w[1].clone())
                .unwrap_or_else(|| "22".into());
            let field = if port == "22" {
                host.clone()
            } else {
                format!("[{host}]:{port}")
            };
            std::fs::write(&known, format!("{field} ssh-ed25519 {KEY}\n")).unwrap();
            eprintln!("{host}: Permission denied (publickey).");
            std::process::exit(255);
        }
        _ => {
            println!("ran: {command}");
            println!("stdin: {} bytes", stdin.len());
        }
    }
}
