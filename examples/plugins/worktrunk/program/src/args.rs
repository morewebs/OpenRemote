//! What an agent may hand to `wt`.
//!
//! No shell ever sits between an agent's words and `wt`: every argument is
//! one argv entry. That still leaves `wt`'s own ways to run commands, which
//! an agent's tool call must not reach without the user:
//!
//! - `-x` / `--execute` runs any program after a switch;
//! - `-y` / `--yes` approves a repository's hook commands unseen;
//! - `--config` and `--config-set` can bring in hook commands of their own;
//! - `wt hook` runs hooks on demand, `wt step for-each` and `wt step tether`
//!   run a command, `wt config shell` edits shell profiles, `wt config
//!   approvals` approves hooks, `wt config alias` runs aliases.
//!
//! Approving a repository's hooks is the user's click in the Worktrunk
//! view, or the plugin's own setting, never an agent's argument.

/// A branch, or one of Worktrunk's shortcuts for one, as a positional
/// argument. It can't start with `-` (it would be read as a flag), and it
/// holds nothing git refuses in a branch name.
pub fn branch(raw: &str) -> Result<String, String> {
    let name = raw.trim();
    if matches!(name, "^" | "-" | "@") {
        return Ok(name.to_string());
    }
    if let Some(n) = name
        .strip_prefix("pr:")
        .or_else(|| name.strip_prefix("mr:"))
    {
        if !n.is_empty() && n.len() <= 12 && n.bytes().all(|b| b.is_ascii_digit()) {
            return Ok(name.to_string());
        }
    }
    if name.starts_with("https://")
        && name.len() <= 300
        && !name.chars().any(|c| c.is_whitespace() || c.is_control())
    {
        return Ok(name.to_string());
    }
    let bad = |why: &str| Err(format!("{name:?} is not a branch name: {why}"));
    if name.is_empty() {
        return bad("it is empty");
    }
    if name.len() > 250 {
        return bad("it is too long");
    }
    if name.starts_with('-') {
        return bad("it starts with a dash");
    }
    if name
        .chars()
        .any(|c| c.is_whitespace() || c.is_control() || "~^:?*[\\".contains(c))
    {
        return bad("it holds a space or one of ~ ^ : ? * [ \\");
    }
    if name.contains("..") || name.contains("@{") || name.contains("//") {
        return bad("it holds .., @{ or //");
    }
    if name.starts_with('/')
        || name.ends_with('/')
        || name.ends_with('.')
        || name.ends_with(".lock")
    {
        return bad("it starts or ends with / or ends with . or .lock");
    }
    if name.split('/').any(|part| part.starts_with('.')) {
        return bad("a part of it starts with a dot");
    }
    Ok(name.to_string())
}

/// A value for a flag (`--base`, `--stage`): anything but another flag.
pub fn value(what: &str, raw: &str) -> Result<String, String> {
    let v = raw.trim();
    if v.is_empty() || v.starts_with('-') || v.chars().any(char::is_control) || v.len() > 300 {
        return Err(format!("{v:?} is not a usable {what}"));
    }
    Ok(v.to_string())
}

const DENIED_LONG: &[&str] = &["--execute", "--yes", "--config", "--config-set"];
const DENIED_SHORT: &[char] = &['x', 'y'];

/// Whether an agent may run `wt <args>` as given (the `run` tool). Answers
/// why not, in words for the agent.
pub fn passthrough(args: &[String]) -> Result<(), String> {
    if args.is_empty() {
        return Err("Name a wt command, for example [\"list\", \"--branches\"].".into());
    }
    for arg in args {
        if arg.chars().any(|c| c == '\0') {
            return Err("An argument holds a NUL character.".into());
        }
        if arg == "--" {
            return Err(
                "Arguments after -- are for a program wt runs, and this tool runs none.".into(),
            );
        }
        if let Some(long) = arg.strip_prefix("--") {
            let name = format!("--{}", long.split('=').next().unwrap_or(""));
            if DENIED_LONG.contains(&name.as_str()) {
                return Err(format!(
                    "{name} is not available to agents: it would run commands or approve hooks without the user. The user can do that in the Worktrunk view."
                ));
            }
        } else if let Some(short) = arg.strip_prefix('-') {
            // A cluster of short flags (-cx): any of them counts. A value
            // glued on (-Cpath) ends the cluster at the first flag that
            // takes one, which for wt is -C, -b, -D's neighbours alike: be
            // strict and look at every letter up to a path-like character.
            let letters: String = short
                .chars()
                .take_while(|c| c.is_ascii_alphabetic())
                .collect();
            if let Some(c) = letters.chars().find(|c| DENIED_SHORT.contains(c)) {
                if !letters.starts_with('C') {
                    return Err(format!(
                        "-{c} is not available to agents: it would run commands or approve hooks without the user."
                    ));
                }
            }
        }
    }
    // The command words: every argument that is not a flag, nor -C's folder.
    let mut words: Vec<&str> = Vec::new();
    let mut skip = false;
    for arg in args {
        if skip {
            skip = false;
            continue;
        }
        if arg == "-C" {
            skip = true;
        } else if !arg.starts_with('-') {
            words.push(arg);
        }
    }
    let first = words.first().copied().unwrap_or("");
    let second = words.get(1).copied().unwrap_or("");
    let refused = match (first, second) {
        ("hook", _) => Some("wt hook runs hook commands"),
        ("step", "for-each") => Some("wt step for-each runs a command in every worktree"),
        ("step", "tether") => Some("wt step tether runs a command"),
        ("config", "show" | "state" | "") => None,
        ("config", _) => {
            Some("wt config changes Worktrunk's setup, approvals, aliases or shell profiles")
        }
        _ => None,
    };
    if let Some(why) = refused {
        return Err(format!(
            "{why}; that is the user's to do, in a terminal or in the Worktrunk view."
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(args: &[&str]) -> Vec<String> {
        args.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn branch_names_are_checked_like_git_does() {
        for ok in [
            "feature",
            "feat/login-page",
            "fix_1.2",
            "pr:123",
            "mr:7",
            "^",
            "-",
            "@",
            "https://github.com/o/r/pull/12",
        ] {
            assert!(branch(ok).is_ok(), "{ok}");
        }
        for bad in [
            "",
            "-rf",
            "--execute=calc",
            "a b",
            "a..b",
            "x:y",
            "a~1",
            "a^",
            "/x",
            "x/",
            "x.lock",
            "x.",
            "a/.hidden",
            "a@{u}",
            "pr:12x",
            "a\nb",
            "a\\b",
        ] {
            assert!(branch(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn flag_values_are_never_flags() {
        assert!(value("base", "main").is_ok());
        assert!(value("base", "--yes").is_err());
        assert!(value("base", "").is_err());
    }

    #[test]
    fn the_ways_wt_runs_commands_stay_with_the_user() {
        for refused in [
            v(&["switch", "-x", "calc", "feat"]),
            v(&["switch", "--execute=calc", "feat"]),
            v(&["switch", "-cx", "calc", "feat"]),
            v(&["switch", "feat", "--yes"]),
            v(&["merge", "-y"]),
            v(&["list", "--config", "/tmp/evil.toml"]),
            v(&["list", "--config-set", "post-start=\"calc\""]),
            v(&["hook", "post-start"]),
            v(&["step", "for-each", "--", "rm", "-rf", "."]),
            v(&["step", "tether", "--", "x"]),
            v(&["config", "shell", "install"]),
            v(&["config", "approvals", "add"]),
            v(&["config", "alias"]),
            v(&["switch", "feat", "--", "arg"]),
            v(&["-C", "/repo", "hook", "pre-merge"]),
            v(&[]),
        ] {
            assert!(passthrough(&refused).is_err(), "{refused:?}");
        }
        for allowed in [
            v(&["list", "--branches", "--format", "json"]),
            v(&["step", "diff"]),
            v(&["step", "commit", "--stage", "tracked"]),
            v(&["step", "prune", "--dry-run"]),
            v(&["config", "show"]),
            v(&["config", "state", "default-branch"]),
            v(&["-C", "/repo", "list"]),
            v(&["remove", "-f", "-D", "old"]),
            v(&["a-future-command", "--a-future-flag"]),
        ] {
            assert!(passthrough(&allowed).is_ok(), "{allowed:?}");
        }
    }
}
