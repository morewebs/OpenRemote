//! A stand-in for Worktrunk's `wt`, for the plugin's tests: it answers the
//! commands the plugin runs with the shapes Worktrunk prints, and writes
//! each command line it was given to `FAKE_WT_LOG`, one per line, so a test
//! sees exactly what reached it.
//!
//! `FAKE_WT_OLD=1` makes it an older Worktrunk: no `--no-cd`, no
//! `--foreground`, and `wt list` prints the bare array of schema 1.

use std::io::Write;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    // OpenRemote clears a plugin's environment, so without FAKE_WT_LOG the
    // log lies beside this program.
    let log = std::env::var("FAKE_WT_LOG").ok().or_else(|| {
        std::env::current_exe().ok().map(|e| {
            e.with_file_name("fake_wt.log")
                .to_string_lossy()
                .into_owned()
        })
    });
    if let Some(log) = log {
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(log)
        {
            let _ = writeln!(f, "{}", args.join(" "));
        }
    }
    let old = std::env::var("FAKE_WT_OLD").is_ok();
    let words: Vec<&str> = args
        .iter()
        .map(String::as_str)
        .filter(|a| !a.starts_with('-'))
        .collect();
    if args.iter().any(|a| a == "--version") {
        println!("wt v{}", if old { "0.20.0" } else { "9.1.0" });
        return;
    }
    if args.iter().any(|a| a == "--help") {
        let flags: &str = match words.first().copied() {
            Some("switch") if old => "  -c, --create\n  -b, --base <BASE>\n  -x, --execute <EXECUTE>\n      --no-hooks\n      --format <FORMAT>\n",
            Some("switch") => "  -c, --create\n  -b, --base <BASE>\n  -x, --execute <EXECUTE>\n      --no-cd\n      --no-hooks\n      --format <FORMAT>\n",
            Some("remove") if old => "  -f, --force\n  -D, --force-delete\n      --no-delete-branch\n      --no-hooks\n      --format <FORMAT>\n",
            Some("remove") => "  -f, --force\n  -D, --force-delete\n      --no-delete-branch\n      --foreground\n      --no-hooks\n      --format <FORMAT>\n",
            Some("merge") => "  -b, --branch <BRANCH>\n      --no-squash\n      --no-commit\n      --no-rebase\n      --no-remove\n      --stage <STAGE>\n      --no-hooks\n      --format <FORMAT>\n",
            Some("list") => "      --branches\n      --remotes\n      --full\n      --format <FORMAT>\n",
            Some("config") => "      --format <FORMAT>\n",
            _ => "Commands:\n  switch  Switch\n  list    List\n\nGlobal Options:\n  -y, --yes\n",
        };
        println!(
            "Usage: wt {} [OPTIONS]\n\nOptions:\n{flags}  -h, --help\n",
            words.join(" ")
        );
        return;
    }
    let cwd = std::env::current_dir()
        .unwrap()
        .to_string_lossy()
        .replace('\\', "/");
    match words.first().copied() {
        Some("list") => {
            let items = format!(
                r#"[{{"branch":"main","head":{{"subject":"init"}},"worktree":{{"path":"{cwd}","main":true,"current":true}},"display":{{"state":"is_main","statusline":"main \u001b[2m^\u001b[22m"}}}},
                   {{"branch":"feat","worktree":{{"path":"{cwd}.feat","main":false,"current":false,"changes":{{"modified":true}}}},"default_branch":{{"ahead":2,"behind":0}},"new_field":1}}]"#
            );
            if old {
                println!("{items}");
            } else {
                println!(r#"{{"schema":2,"repo":{{"default_branch":"main"}},"items":{items}}}"#);
            }
        }
        Some("switch") => {
            let branch = words.last().copied().unwrap_or("");
            if branch == "needs-approval" {
                eprintln!("✗ Cannot prompt for approval in non-interactive environment");
                std::process::exit(1);
            }
            // The worktree's folder, beside the repository, as wt makes it.
            let _ = std::fs::create_dir_all(format!("{cwd}.{branch}"));
            eprintln!("✓ Created branch {branch}");
            if args.iter().any(|a| a == "--format") {
                println!(
                    r#"{{"action":"created","branch":"{branch}","path":"{cwd}.{branch}","created_branch":true}}"#
                );
            }
        }
        Some("remove") => {
            eprintln!("✓ Removed");
            println!(
                r#"[{{"kind":"worktree","branch":"{}","branch_outcome":"deleted"}}]"#,
                words.last().unwrap_or(&"")
            );
        }
        Some("merge") => {
            eprintln!("✓ Merged to main");
            println!(
                r#"{{"branch":"feat","target":"main","squashed":true,"removed":{}}}"#,
                !args.iter().any(|a| a == "--no-remove")
            );
        }
        Some("config") => println!(
            r#"{{"state":"approval_required","commands":[{{"phase":"pre-start","template":"npm install","approved":false}}]}}"#
        ),
        _ => {
            eprintln!("fake wt: {}", args.join(" "));
        }
    }
}
