//! OpenRemote's official Worktrunk plugin: one program that OpenRemote
//! starts and talks to over stdio (docs/plugins/protocol.md). It answers
//! the agent tools in openremote.json and the Worktrunk view's requests,
//! and runs Worktrunk's `wt` for both, never through a shell.

mod args;
mod links;
mod plugin;
mod rpc;
mod update;
mod wt;

use std::io::BufRead;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_json::{json, Value};

use plugin::{text_result, Plugin};

fn main() {
    let rpc = rpc::Rpc::new(Box::new(std::io::stdout()));
    let plugin = Arc::new(Plugin::new(rpc.clone()));
    let stdin = std::io::stdin();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        let Ok(message) = serde_json::from_str::<Value>(&line) else {
            eprintln!("not JSON: {}", line.chars().take(200).collect::<String>());
            continue;
        };
        if rpc.settle(&message) {
            continue;
        }
        let method = message
            .get("method")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let params = message.get("params").cloned().unwrap_or(Value::Null);
        match message.get("id").cloned() {
            Some(id) => {
                let plugin = plugin.clone();
                std::thread::spawn(move || {
                    let answer = handle(&plugin, &method, params);
                    plugin.rpc.answer(&id, answer);
                });
            }
            None => {
                if method == "settings/changed" {
                    plugin.set_settings(params.get("settings").cloned().unwrap_or(json!({})));
                }
            }
        }
    }
    // stdin closed: OpenRemote is gone.
}

fn handle(plugin: &Arc<Plugin>, method: &str, params: Value) -> Result<Value, String> {
    match method {
        "initialize" => {
            if let Some(data) = params.pointer("/plugin/data").and_then(Value::as_str) {
                *plugin.data.lock().unwrap_or_else(|p| p.into_inner()) = PathBuf::from(data);
            }
            plugin.set_settings(params.get("settings").cloned().unwrap_or(json!({})));
            Ok(json!({"protocol": 1}))
        }
        "tools/call" => {
            let name = params.get("name").and_then(Value::as_str).unwrap_or("");
            let arguments = params.get("arguments").cloned().unwrap_or(json!({}));
            let chat = params.get("chat").cloned().unwrap_or(Value::Null);
            Ok(match tool(plugin, name, &arguments, &chat) {
                Ok(text) => text_result(text, false),
                Err(why) => text_result(why, true),
            })
        }
        "ui/request" => {
            let name = params.get("name").and_then(Value::as_str).unwrap_or("");
            let p = params.get("params").cloned().unwrap_or(json!({}));
            view(plugin, name, &p)
        }
        other => Err(format!("no method {other}")),
    }
}

fn flag(args: &Value, key: &str, default: bool) -> bool {
    args.get(key).and_then(Value::as_bool).unwrap_or(default)
}

fn text<'a>(args: &'a Value, key: &str) -> Option<&'a str> {
    args.get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
}

fn strings(args: &Value, key: &str) -> Vec<String> {
    match args.get(key) {
        Some(Value::Array(items)) => items
            .iter()
            .filter_map(|v| v.as_str().map(String::from))
            .collect(),
        Some(Value::String(s)) if !s.trim().is_empty() => vec![s.clone()],
        _ => vec![],
    }
}

fn pretty(v: &Value) -> String {
    serde_json::to_string_pretty(v).unwrap_or_default()
}

/// The chat's own folder, when it is the worktree an operation would
/// remove: the chat would be left in a folder that no longer exists.
fn is_own(chat: &Value, path: &str) -> bool {
    chat.get("workspace")
        .and_then(Value::as_str)
        .is_some_and(|w| plugin::same_path(Path::new(w), Path::new(path)))
}

fn tool(plugin: &Arc<Plugin>, name: &str, a: &Value, chat: &Value) -> Result<String, String> {
    match name {
        "list" => {
            let repo = plugin.repo(a, chat)?;
            let listed = plugin.list(
                &repo,
                flag(a, "branches", false),
                flag(a, "remotes", false),
                flag(a, "full", false),
            )?;
            if let Some(t) = listed.get("text").and_then(Value::as_str) {
                return Ok(t.to_string());
            }
            let items = listed["items"].as_array().cloned().unwrap_or_default();
            let mut out = format!(
                "{} in {} (default branch {}; @ = this chat's, ^ = main):\n",
                if items.len() == 1 {
                    "1 entry".to_string()
                } else {
                    format!("{} entries", items.len())
                },
                repo.display(),
                listed["default_branch"].as_str().unwrap_or("unknown")
            );
            for item in &items {
                out.push_str(&plugin::item_line(item));
                out.push('\n');
            }
            out.push_str("\nJSON:\n");
            out.push_str(&serde_json::to_string(&items).unwrap_or_default());
            Ok(out)
        }
        "status" => {
            let repo = plugin.repo(a, chat)?;
            let listed = plugin.list(&repo, false, false, false)?;
            let items = listed["items"].as_array().cloned().unwrap_or_default();
            let here = items
                .iter()
                .find(|i| {
                    i.pointer("/worktree/path")
                        .and_then(Value::as_str)
                        .is_some_and(|p| plugin::same_path(Path::new(p), &repo))
                })
                .or_else(|| {
                    items.iter().find(|i| {
                        i.pointer("/worktree/current").and_then(Value::as_bool) == Some(true)
                    })
                });
            match here {
                Some(item) => Ok(format!(
                    "{}\n\nOther worktrees: {}\n\nJSON:\n{}",
                    plugin::item_line(item),
                    items.len().saturating_sub(1),
                    pretty(item)
                )),
                None => Err(format!(
                    "{} is not one of this repository's worktrees",
                    repo.display()
                )),
            }
        }
        "create" | "switch" => {
            let repo = plugin.repo(a, chat)?;
            let branch = text(a, "branch").ok_or("Name the branch.")?;
            let create = name == "create" || flag(a, "create", false);
            let out = plugin.switch(&repo, branch, create, text(a, "base"))?;
            let path = out["path"].as_str().unwrap_or("(unknown)");
            Ok(format!(
                "{} worktree for {} at {path}\nWork there by using that folder (for example `cd {path}` in your shell commands, or full paths); this chat's own folder doesn't change. To hand the work to a new chat in that folder, use start_chat.\n\nJSON:\n{}",
                if out["action"].as_str() == Some("existing") { "Existing" } else if create { "Created" } else { "Ready:" },
                out["branch"].as_str().unwrap_or(branch),
                pretty(&out)
            ))
        }
        "remove" => {
            let repo = plugin.repo(a, chat)?;
            let branches = strings(a, "branches");
            if branches.is_empty() {
                return Err("Name the branches whose worktrees to remove (this tool never removes \"the current one\" by default).".into());
            }
            if !flag(a, "allow_this_chat", false) {
                let listed = plugin.list(&repo, false, false, false)?;
                for item in listed["items"].as_array().cloned().unwrap_or_default() {
                    let (Some(b), Some(p)) = (
                        item["branch"].as_str(),
                        item.pointer("/worktree/path").and_then(Value::as_str),
                    ) else {
                        continue;
                    };
                    if branches.iter().any(|x| x == b) && is_own(chat, p) {
                        return Err(format!("{b}'s worktree is this chat's own folder ({p}); removing it would leave the chat nowhere. Pass allow_this_chat: true if that is really meant."));
                    }
                }
            }
            let out = plugin.remove(
                &repo,
                &branches,
                flag(a, "force", false),
                flag(a, "force_delete", false),
                flag(a, "keep_branch", false),
            )?;
            Ok(format!(
                "{}\n\nJSON:\n{}",
                out["said"].as_str().unwrap_or(""),
                pretty(&out["removed"])
            ))
        }
        "merge" => {
            let repo = plugin.repo(a, chat)?;
            // Merging from the chat's own worktree would remove it: keep it
            // unless the agent asks otherwise.
            let from_here = text(a, "branch").is_none()
                && chat
                    .get("workspace")
                    .and_then(Value::as_str)
                    .is_some_and(|w| plugin::same_path(Path::new(w), &repo));
            let remove = flag(a, "remove", !from_here);
            let out = plugin.merge(
                &repo,
                text(a, "target"),
                text(a, "branch"),
                flag(a, "squash", true),
                flag(a, "commit", true),
                flag(a, "rebase", true),
                remove,
                text(a, "stage"),
            )?;
            let mut said = out["said"].as_str().unwrap_or("").to_string();
            if from_here && !remove {
                said.push_str("\n(This chat's own worktree was kept. Pass remove: true to remove it as well.)");
            }
            Ok(format!("{said}\n\nJSON:\n{}", pretty(&out["merge"])))
        }
        "start_chat" => {
            let repo = plugin.repo(a, chat)?;
            let branch = text(a, "branch").ok_or("Name the branch for the new chat's worktree.")?;
            let made = plugin.switch(&repo, branch, flag(a, "create", true), text(a, "base"))?;
            let path = made["path"]
                .as_str()
                .ok_or("Worktrunk did not say where the worktree is")?
                .to_string();
            let started = plugin.start_chat(
                &path,
                text(a, "prompt"),
                text(a, "harness"),
                text(a, "model"),
                chat,
            )?;
            Ok(format!(
                "Started chat {} in {path} (branch {branch}).\n\nJSON:\n{}",
                started["chat"].as_str().unwrap_or("?"),
                pretty(&json!({"chat": started["chat"], "path": path, "branch": branch}))
            ))
        }
        "version" => {
            // The newest release from the few-hours cache, asked again when
            // stale (and the user's update checks are on).
            let v = plugin.version(false, flag(a, "check", true));
            let mut out = match v["installed"].as_bool() {
                Some(true) => format!(
                    "Worktrunk {} at {} (installed by {}).",
                    v["version"].as_str().unwrap_or("?"),
                    v["path"].as_str().unwrap_or("?"),
                    v["method"].as_str().unwrap_or("?")
                ),
                _ => v["error"]
                    .as_str()
                    .unwrap_or("Worktrunk is not installed.")
                    .to_string(),
            };
            match (v["latest"].as_str(), v["update_available"].as_bool()) {
                (Some(l), Some(true)) => out.push_str(&format!(
                    " Worktrunk {l} is out: the user can update in the Worktrunk view, or with: {}",
                    v["update_command"].as_str().unwrap_or("")
                )),
                (Some(l), _) => out.push_str(&format!(" The newest release is {l}.")),
                _ => {}
            }
            Ok(format!("{out}\n\nJSON:\n{}", pretty(&v)))
        }
        "help" => {
            let loc = plugin.locate()?;
            let mut words = strings(a, "command");
            for w in &words {
                if w.starts_with('-') || w.chars().any(|c| c.is_control()) {
                    return Err(format!("{w:?} is not a command word"));
                }
            }
            words.push("--help".into());
            let ran =
                wt::run(&loc.path, &words, None, wt::PROBE_TIME).map_err(|e| e.to_string())?;
            Ok(format!(
                "Worktrunk {}:\n{}",
                loc.version.map(|v| v.to_string()).unwrap_or_default(),
                wt::strip_ansi(&format!("{}{}", ran.stdout, ran.stderr))
            ))
        }
        "run" => {
            let repo = plugin.repo(a, chat)?;
            let words = strings(a, "args");
            args::passthrough(&words)?;
            let loc = plugin.locate()?;
            let ran = wt::run(
                &loc.path,
                &words,
                Some(&repo),
                std::time::Duration::from_secs(580),
            )
            .map_err(|e| e.to_string())?;
            let body = format!(
                "{}{}",
                wt::strip_ansi(ran.stdout.trim()),
                if ran.stderr.trim().is_empty() {
                    String::new()
                } else {
                    format!("\n{}", wt::strip_ansi(ran.stderr.trim()))
                }
            );
            if ran.ok() {
                Ok(body)
            } else {
                Err(format!("wt exited with {:?}:\n{body}", ran.code))
            }
        }
        other => Err(format!("There is no Worktrunk tool named {other}.")),
    }
}

/// The Worktrunk view's requests (`ui/request`). Quick questions answer
/// at once; work that can take long (hooks, downloads, builds) runs as a
/// job the view watches.
fn view(plugin: &Arc<Plugin>, name: &str, p: &Value) -> Result<Value, String> {
    let repo_of = |p: &Value| -> Result<PathBuf, String> { plugin.repo(p, &Value::Null) };
    match name {
        "state" => {
            let repos = plugin.repos();
            let repo = text(p, "repo")
                .map(String::from)
                .or_else(|| repos.first().cloned());
            let version = plugin.version(false, false);
            let mut out =
                json!({"version": version, "repos": repos, "repo": repo, "job": plugin.job()});
            if let (Some(repo), true) = (&repo, version["installed"].as_bool() == Some(true)) {
                match repo_of(&json!({"repo": repo})).and_then(|r| {
                    plugin
                        .list(&r, flag(p, "branches", false), false, false)
                        .map(|l| (r, l))
                }) {
                    Ok((r, listed)) => {
                        out["list"] = listed;
                        out["approvals"] = plugin.approvals(&r).unwrap_or(Value::Null);
                    }
                    Err(e) => out["list_error"] = json!(e),
                }
            }
            Ok(out)
        }
        // The user's button asks now; the view's own look on opening goes
        // through the cache and the user's update-check setting.
        "check_update" => Ok(if flag(p, "auto", false) {
            plugin.version(false, true)
        } else {
            plugin.version(true, true)
        }),
        "add_repo" => {
            let path = text(p, "path").ok_or("Type the repository's full path.")?;
            let r = repo_of(&json!({"repo": path}))?;
            Ok(json!({"repo": plugin::main_repo(&r).unwrap_or(r)}))
        }
        "forget_repo" => {
            plugin.forget_repo(text(p, "path").unwrap_or(""));
            Ok(json!({}))
        }
        "job" => Ok(plugin.job()),
        "create" | "remove" | "merge" | "start_chat" | "approve_hooks" | "install" | "update" => {
            let p = p.clone();
            let kind = name.to_string();
            Ok(plugin.start_job(name, move |plugin, say| {
                let repo = || plugin.repo(&p, &Value::Null);
                match kind.as_str() {
                    "create" => {
                        let out = plugin.switch(
                            &repo()?,
                            text(&p, "branch").ok_or("Name the branch.")?,
                            true,
                            text(&p, "base"),
                        )?;
                        say(out["said"].as_str().unwrap_or(""));
                        Ok(out)
                    }
                    "remove" => {
                        let out = plugin.remove(
                            &repo()?,
                            &strings(&p, "branches"),
                            flag(&p, "force", false),
                            flag(&p, "force_delete", false),
                            flag(&p, "keep_branch", false),
                        )?;
                        say(out["said"].as_str().unwrap_or(""));
                        Ok(out)
                    }
                    "merge" => {
                        let out = plugin.merge(
                            &repo()?,
                            text(&p, "target"),
                            text(&p, "branch"),
                            flag(&p, "squash", true),
                            true,
                            true,
                            flag(&p, "remove", true),
                            None,
                        )?;
                        say(out["said"].as_str().unwrap_or(""));
                        Ok(out)
                    }
                    "start_chat" => {
                        let branch = text(&p, "branch").ok_or("Name the branch.")?;
                        let made = plugin.switch(
                            &repo()?,
                            branch,
                            flag(&p, "create", true),
                            text(&p, "base"),
                        )?;
                        say(made["said"].as_str().unwrap_or(""));
                        let path = made["path"]
                            .as_str()
                            .ok_or("Worktrunk did not say where the worktree is")?
                            .to_string();
                        let started = plugin.start_chat(
                            &path,
                            text(&p, "prompt"),
                            text(&p, "harness"),
                            text(&p, "model"),
                            &Value::Null,
                        )?;
                        say(&format!("Started a chat in {path}"));
                        Ok(json!({"chat": started["chat"], "path": path}))
                    }
                    "approve_hooks" => {
                        let out = plugin.approve(&repo()?)?;
                        say(out["said"].as_str().unwrap_or("Approved"));
                        Ok(out)
                    }
                    _ => plugin.install_or_update(say),
                }
            }))
        }
        other => Err(format!("no request {other}")),
    }
}
