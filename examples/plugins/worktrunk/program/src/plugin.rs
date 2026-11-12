//! The plugin's work: Worktrunk's operations, shared by the agent tools and
//! the view, and the answers each one gives.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{json, Value};

use crate::args;
use crate::rpc::Rpc;
use crate::update::{self, Method};
use crate::wt::{self, Located, Ran, Wt};

/// A wt run on an agent's behalf stays inside the tool's own time limit
/// (600 s for the tools that may run hooks).
const WT_TIME: Duration = Duration::from_secs(580);
const QUICK_TIME: Duration = Duration::from_secs(50);

pub struct Plugin {
    pub rpc: Arc<Rpc>,
    pub wt: Wt,
    pub settings: Mutex<Value>,
    pub data: Mutex<PathBuf>,
    env: HashMap<String, String>,
    job: Mutex<Job>,
}

#[derive(Clone, Default)]
struct Job {
    id: u64,
    kind: String,
    running: bool,
    ok: Option<bool>,
    log: Vec<String>,
    result: Value,
}

impl Job {
    fn json(&self) -> Value {
        json!({"id": self.id, "kind": self.kind, "running": self.running, "ok": self.ok, "log": self.log, "result": self.result})
    }
}

pub fn text_result(text: impl Into<String>, is_error: bool) -> Value {
    json!({"content": [{"type": "text", "text": text.into()}], "isError": is_error})
}

fn string<'a>(args: &'a Value, key: &str) -> Option<&'a str> {
    args.get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
}

/// Two paths name the same folder (case aside on Windows and macOS).
pub fn same_path(a: &Path, b: &Path) -> bool {
    let norm = |p: &Path| {
        let p = std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
        let s = p.to_string_lossy().replace('\\', "/");
        let s = s
            .trim_start_matches("//?/")
            .trim_end_matches('/')
            .to_string();
        if cfg!(any(windows, target_os = "macos")) {
            s.to_lowercase()
        } else {
            s
        }
    };
    norm(a) == norm(b)
}

/// The repository a path belongs to: its main worktree's folder, read from
/// git's own files (a linked worktree's `.git` file points back at it).
pub fn main_repo(path: &Path) -> Option<PathBuf> {
    let mut dir = Some(path);
    while let Some(d) = dir {
        let dotgit = d.join(".git");
        if dotgit.is_dir() {
            return Some(d.to_path_buf());
        }
        if dotgit.is_file() {
            let text = std::fs::read_to_string(&dotgit).ok()?;
            let gitdir = text.lines().find_map(|l| l.strip_prefix("gitdir:"))?.trim();
            let gitdir = if Path::new(gitdir).is_absolute() {
                PathBuf::from(gitdir)
            } else {
                d.join(gitdir)
            };
            // <main>/.git/worktrees/<name>
            let common = gitdir.parent()?.parent()?;
            if common.file_name()? == ".git" {
                return common.parent().map(Path::to_path_buf);
            }
            return Some(d.to_path_buf());
        }
        dir = d.parent();
    }
    None
}

/// The worktree list's items, whichever shape this Worktrunk prints:
/// schema 2's envelope, or the older bare array.
pub fn list_items(parsed: &Value) -> Vec<Value> {
    let items = match parsed {
        Value::Array(items) => items.clone(),
        other => other
            .get("items")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default(),
    };
    items
        .into_iter()
        .map(|mut item| {
            // The terminal rendering, colors and all, helps no one here.
            if let Some(display) = item.get_mut("display").and_then(Value::as_object_mut) {
                display.remove("statusline");
            }
            item
        })
        .collect()
}

/// One line per worktree, for an agent or a person to read at a glance.
pub fn item_line(item: &Value) -> String {
    let wt = item.get("worktree");
    let mut marks = String::new();
    if wt.and_then(|w| w.get("current")).and_then(Value::as_bool) == Some(true) {
        marks.push('@');
    }
    if wt.and_then(|w| w.get("main")).and_then(Value::as_bool) == Some(true) {
        marks.push('^');
    }
    let branch = item
        .get("branch")
        .and_then(Value::as_str)
        .unwrap_or("(detached)");
    let mut line = format!("{:<2} {branch}", marks);
    match wt.and_then(|w| w.get("path")).and_then(Value::as_str) {
        Some(path) => line.push_str(&format!("  {path}")),
        None => line.push_str("  (no worktree)"),
    }
    let mut facts = Vec::new();
    if let Some(state) = item.pointer("/display/state").and_then(Value::as_str) {
        facts.push(state.replace('_', " "));
    }
    let n = |p: &str| item.pointer(p).and_then(Value::as_u64).unwrap_or(0);
    let (ahead, behind) = (n("/default_branch/ahead"), n("/default_branch/behind"));
    if ahead > 0 || behind > 0 {
        facts.push(format!("{ahead} ahead, {behind} behind the default branch"));
    }
    if let Some(changes) = wt.and_then(|w| w.get("changes")).and_then(Value::as_object) {
        let dirty: Vec<&str> = [
            "staged",
            "modified",
            "untracked",
            "deleted",
            "renamed",
            "conflicted",
        ]
        .into_iter()
        .filter(|k| changes.get(*k).and_then(Value::as_bool) == Some(true))
        .collect();
        if !dirty.is_empty() {
            facts.push(dirty.join("+"));
        }
    }
    if let Some(up) = item.get("upstream") {
        let (a, b) = (
            up.get("ahead").and_then(Value::as_u64).unwrap_or(0),
            up.get("behind").and_then(Value::as_u64).unwrap_or(0),
        );
        if a > 0 || b > 0 {
            facts.push(format!("upstream {a} ahead, {b} behind"));
        }
    }
    if !facts.is_empty() {
        line.push_str(&format!("  [{}]", facts.join("; ")));
    }
    if let Some(subject) = item.pointer("/head/subject").and_then(Value::as_str) {
        line.push_str(&format!(
            "  \"{}\"",
            subject.chars().take(80).collect::<String>()
        ));
    }
    line
}

impl Plugin {
    pub fn new(rpc: Arc<Rpc>) -> Self {
        let env = std::env::vars().collect();
        Self {
            rpc,
            wt: Wt::new(),
            settings: Mutex::new(json!({})),
            data: Mutex::new(PathBuf::new()),
            env,
            job: Mutex::new(Job::default()),
        }
    }

    fn env(&self, key: &str) -> Option<String> {
        self.env
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(key))
            .map(|(_, v)| v.clone())
            .filter(|v| !v.is_empty())
    }

    fn setting(&self, key: &str) -> Option<String> {
        let settings = self.settings.lock().unwrap_or_else(|p| p.into_inner());
        settings
            .get(key)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(String::from)
    }

    fn setting_bool(&self, key: &str, default: bool) -> bool {
        let settings = self.settings.lock().unwrap_or_else(|p| p.into_inner());
        settings
            .get(key)
            .and_then(Value::as_bool)
            .unwrap_or(default)
    }

    pub fn set_settings(&self, values: Value) {
        *self.settings.lock().unwrap_or_else(|p| p.into_inner()) = values;
        self.wt.forget();
    }

    pub fn data_dir(&self) -> PathBuf {
        let data = self.data.lock().unwrap_or_else(|p| p.into_inner()).clone();
        if data.as_os_str().is_empty() {
            self.env("OPENREMOTE_PLUGIN_DATA")
                .map(PathBuf::from)
                .unwrap_or_else(std::env::temp_dir)
        } else {
            data
        }
    }

    fn program(&self, name: &str) -> Option<PathBuf> {
        let env = |k: &str| self.env(k);
        wt::find_program(name, &env)
    }

    pub fn locate(&self) -> Result<Located, String> {
        let env = |k: &str| self.env(k);
        let setting = self.setting("wt_path");
        self.wt.locate(
            setting.as_deref(),
            update::managed_program(&self.data_dir()),
            &env,
        )
    }

    fn supports(&self, loc: &Located, command: &[&str], flag: &str) -> bool {
        self.wt.flags(loc, command).contains(flag)
    }

    /// The hook flags the user's setting asks for, as far as this version
    /// knows them.
    fn hook_args(&self, loc: &Located, command: &[&str]) -> Vec<String> {
        match self.setting("hooks").as_deref() {
            Some("skip") if self.supports(loc, command, "--no-hooks") => vec!["--no-hooks".into()],
            Some("all") if self.supports(loc, &[], "--yes") => vec!["--yes".into()],
            _ => vec![],
        }
    }

    fn json_args(&self, loc: &Located, command: &[&str]) -> Vec<String> {
        if self.supports(loc, command, "--format") {
            vec!["--format".into(), "json".into()]
        } else {
            vec![]
        }
    }

    fn run(
        &self,
        loc: &Located,
        cwd: &Path,
        args: Vec<String>,
        limit: Duration,
    ) -> Result<Ran, String> {
        wt::run(&loc.path, &args, Some(cwd), limit)
            .map_err(|e| format!("Worktrunk could not run: {e}"))
    }

    /// The repository a call is about: the one it names, else the chat's.
    pub fn repo(&self, args: &Value, chat: &Value) -> Result<PathBuf, String> {
        let named = string(args, "repo").map(PathBuf::from);
        let path = match named {
            Some(p) => p,
            None => chat
                .get("workspace")
                .and_then(Value::as_str)
                .map(PathBuf::from)
                .ok_or(
                    "This chat is in no project: pass `repo`, the full path of a git repository.",
                )?,
        };
        if !path.is_absolute() {
            return Err(format!("{} is not a full path", path.display()));
        }
        if !path.is_dir() {
            return Err(format!(
                "{} is not a folder on this computer",
                path.display()
            ));
        }
        if main_repo(&path).is_none() {
            return Err(format!("{} is not inside a git repository", path.display()));
        }
        self.remember_repo(&path);
        Ok(path)
    }

    // ---- repositories the view offers ----

    fn repos_file(&self) -> PathBuf {
        self.data_dir().join("repos.json")
    }

    pub fn repos(&self) -> Vec<String> {
        std::fs::read_to_string(self.repos_file())
            .ok()
            .and_then(|t| serde_json::from_str::<Vec<String>>(&t).ok())
            .unwrap_or_default()
    }

    pub fn remember_repo(&self, path: &Path) {
        let Some(main) = main_repo(path) else { return };
        let main = main.to_string_lossy().into_owned();
        let mut repos = self.repos();
        repos.retain(|r| !same_path(Path::new(r), Path::new(&main)));
        repos.insert(0, main);
        repos.truncate(50);
        let _ = std::fs::create_dir_all(self.data_dir());
        let _ = std::fs::write(
            self.repos_file(),
            serde_json::to_string(&repos).unwrap_or_default(),
        );
    }

    pub fn forget_repo(&self, path: &str) {
        let mut repos = self.repos();
        repos.retain(|r| !same_path(Path::new(r), Path::new(path)));
        let _ = std::fs::write(
            self.repos_file(),
            serde_json::to_string(&repos).unwrap_or_default(),
        );
    }

    // ---- operations ----

    pub fn list(
        &self,
        repo: &Path,
        branches: bool,
        remotes: bool,
        full: bool,
    ) -> Result<Value, String> {
        let loc = self.locate()?;
        let mut a = vec!["list".to_string()];
        a.extend(self.json_args(&loc, &["list"]));
        let json = a.len() > 1;
        for (on, f) in [
            (branches, "--branches"),
            (remotes, "--remotes"),
            (full, "--full"),
        ] {
            if on && self.supports(&loc, &["list"], f) {
                a.push(f.into());
            }
        }
        let ran = self.run(
            &loc,
            repo,
            a,
            QUICK_TIME.max(if full { WT_TIME } else { QUICK_TIME }),
        )?;
        if !ran.ok() {
            return Err(ran.said());
        }
        if !json {
            return Ok(json!({"items": [], "text": wt::strip_ansi(&ran.stdout)}));
        }
        let parsed: Value = serde_json::from_str(&ran.stdout)
            .map_err(|e| format!("wt list printed something that isn't JSON: {e}"))?;
        Ok(json!({
            "items": list_items(&parsed),
            "default_branch": parsed.pointer("/repo/default_branch").cloned().unwrap_or(Value::Null),
            "schema": parsed.get("schema").cloned().unwrap_or(json!(1)),
        }))
    }

    /// Make sure `branch` has a worktree (creating the branch with
    /// `create`), as `wt switch` does, without changing anyone's folder.
    pub fn switch(
        &self,
        repo: &Path,
        branch: &str,
        create: bool,
        base: Option<&str>,
    ) -> Result<Value, String> {
        let branch = args::branch(branch)?;
        let loc = self.locate()?;
        let cmd = ["switch"];
        let mut a = vec!["switch".to_string()];
        if create {
            a.push("--create".into());
        }
        if let Some(base) = base {
            a.push("--base".into());
            a.push(args::value("base branch", base)?);
        }
        if self.supports(&loc, &cmd, "--no-cd") {
            a.push("--no-cd".into());
        }
        a.extend(self.json_args(&loc, &cmd));
        a.extend(self.hook_args(&loc, &cmd));
        a.push(branch.clone());
        let ran = self.run(&loc, repo, a, WT_TIME)?;
        if !ran.ok() {
            return Err(self.explain(&ran));
        }
        let mut out =
            serde_json::from_str::<Value>(ran.stdout.trim()).unwrap_or_else(|_| json!({}));
        if out.get("path").and_then(Value::as_str).is_none() {
            // An older Worktrunk without JSON here: find the path in the list.
            let listed = self.list(repo, false, false, false)?;
            let path = listed["items"]
                .as_array()
                .and_then(|items| {
                    items
                        .iter()
                        .find(|i| i["branch"].as_str() == Some(branch.as_str()))
                })
                .and_then(|i| i.pointer("/worktree/path").cloned());
            out["path"] = path.unwrap_or(Value::Null);
            out["branch"] = json!(branch);
        }
        out["said"] = json!(ran.said());
        Ok(out)
    }

    pub fn remove(
        &self,
        repo: &Path,
        branches: &[String],
        force: bool,
        force_delete: bool,
        keep_branch: bool,
    ) -> Result<Value, String> {
        let loc = self.locate()?;
        let cmd = ["remove"];
        let mut a = vec!["remove".to_string()];
        if self.supports(&loc, &cmd, "--foreground") {
            a.push("--foreground".into());
        }
        if force {
            a.push("--force".into());
        }
        if force_delete {
            a.push("--force-delete".into());
        }
        if keep_branch {
            a.push("--no-delete-branch".into());
        }
        a.extend(self.json_args(&loc, &cmd));
        a.extend(self.hook_args(&loc, &cmd));
        for b in branches {
            a.push(args::branch(b)?);
        }
        let ran = self.run(&loc, repo, a, WT_TIME)?;
        if !ran.ok() {
            return Err(self.explain(&ran));
        }
        let parsed = serde_json::from_str::<Value>(ran.stdout.trim()).unwrap_or(Value::Null);
        Ok(json!({"removed": parsed, "said": ran.said()}))
    }

    #[allow(clippy::too_many_arguments)]
    pub fn merge(
        &self,
        repo: &Path,
        target: Option<&str>,
        branch: Option<&str>,
        squash: bool,
        commit: bool,
        rebase: bool,
        remove: bool,
        stage: Option<&str>,
    ) -> Result<Value, String> {
        let loc = self.locate()?;
        let cmd = ["merge"];
        let mut a = vec!["merge".to_string()];
        if let Some(b) = branch {
            a.push("--branch".into());
            a.push(args::branch(b)?);
        }
        for (off, f) in [
            (!squash, "--no-squash"),
            (!commit, "--no-commit"),
            (!rebase, "--no-rebase"),
            (!remove, "--no-remove"),
        ] {
            if off {
                if !self.supports(&loc, &cmd, f) {
                    return Err(format!(
                        "This Worktrunk ({}) has no {f}",
                        loc.version.map(|v| v.to_string()).unwrap_or_default()
                    ));
                }
                a.push(f.into());
            }
        }
        if let Some(stage) = stage {
            a.push("--stage".into());
            a.push(args::value("stage", stage)?);
        }
        a.extend(self.json_args(&loc, &cmd));
        a.extend(self.hook_args(&loc, &cmd));
        if let Some(t) = target {
            a.push(args::branch(t)?);
        }
        let ran = self.run(&loc, repo, a, WT_TIME)?;
        if !ran.ok() {
            return Err(self.explain(&ran));
        }
        let parsed = serde_json::from_str::<Value>(ran.stdout.trim()).unwrap_or(Value::Null);
        Ok(json!({"merge": parsed, "said": ran.said()}))
    }

    /// A failed run, in words, with what to do about hooks that wait for
    /// the user's approval.
    fn explain(&self, ran: &Ran) -> String {
        let said = ran.said();
        if said.to_lowercase().contains("approval") {
            format!(
                "{said}\n\nThis repository's Worktrunk hooks need the user's approval first. Ask the user to approve them in the Worktrunk view (Plugins in the sidebar), or to run `wt config approvals add` in the repository."
            )
        } else {
            said
        }
    }

    pub fn approvals(&self, repo: &Path) -> Result<Value, String> {
        let loc = self.locate()?;
        if !self.supports(&loc, &["config", "approvals", "list"], "--format") {
            return Ok(Value::Null);
        }
        let ran = self.run(
            &loc,
            repo,
            vec![
                "config".into(),
                "approvals".into(),
                "list".into(),
                "--format".into(),
                "json".into(),
            ],
            QUICK_TIME,
        )?;
        if !ran.ok() {
            return Err(ran.said());
        }
        serde_json::from_str(ran.stdout.trim()).map_err(|e| e.to_string())
    }

    /// The user approved this repository's hook commands, in the view.
    pub fn approve(&self, repo: &Path) -> Result<Value, String> {
        let loc = self.locate()?;
        let ran = self.run(
            &loc,
            repo,
            vec![
                "config".into(),
                "approvals".into(),
                "add".into(),
                "--yes".into(),
            ],
            QUICK_TIME,
        )?;
        if !ran.ok() {
            return Err(ran.said());
        }
        Ok(json!({"said": ran.said()}))
    }

    /// The installed Worktrunk, the newest release, and how to update.
    pub fn version(&self, force: bool, may_ask: bool) -> Value {
        let data = self.data_dir();
        let git = self.program("git");
        let latest = if may_ask && self.setting_bool("check_updates", true) || force {
            update::latest(&data, git.as_deref(), force)
        } else {
            std::fs::read_to_string(data.join("update-check.json"))
                .ok()
                .and_then(|t| serde_json::from_str::<Value>(&t).ok())
                .and_then(|c| {
                    Some((
                        wt::Version::release_tag(c["latest"].as_str()?)?,
                        c["checked_at"].as_u64()?,
                    ))
                })
                .ok_or_else(|| "not checked yet".to_string())
        };
        let (latest_v, checked_at, latest_err) = match &latest {
            Ok((v, at)) => (Some(*v), Some(*at), None),
            Err(e) => (None, None, Some(e.clone())),
        };
        let can_self_install = update::this_asset().is_some()
            && self.program("curl").is_some()
            && self.program("tar").is_some();
        match self.locate() {
            Ok(loc) => {
                let crates = self.env("CARGO_HOME").map(PathBuf::from).or_else(|| {
                    self.env("HOME")
                        .or_else(|| self.env("USERPROFILE"))
                        .map(|h| PathBuf::from(h).join(".cargo"))
                });
                let crates_toml =
                    crates.and_then(|c| std::fs::read_to_string(c.join(".crates.toml")).ok());
                let method = update::method_for(&loc, crates_toml.as_deref());
                let plan = update::plan(method);
                let runnable = match method {
                    Method::Plugin => can_self_install,
                    _ => plan.program.and_then(|p| self.program(p)).is_some(),
                };
                let available = match (loc.version, latest_v) {
                    (Some(have), Some(newest)) => newest > have,
                    _ => false,
                };
                // What this version can do, from its own help: an agent
                // learns new commands the day they ship.
                let commands = wt::run(&loc.path, &["--help".into()], None, wt::PROBE_TIME)
                    .map(|r| wt::subcommands(&format!("{}\n{}", r.stdout, r.stderr)))
                    .unwrap_or_default();
                json!({
                    "installed": true,
                    "commands": commands,
                    "path": loc.path,
                    "version": loc.version.map(|v| v.to_string()),
                    "origin": format!("{:?}", loc.origin).to_lowercase(),
                    "method": method.name(),
                    "latest": latest_v.map(|v| v.to_string()),
                    "checked_at": checked_at,
                    "check_error": latest_err,
                    "update_available": available,
                    "update_command": plan.shown,
                    "can_update_here": runnable,
                    "can_install_own_copy": can_self_install,
                })
            }
            Err(why) => json!({
                "installed": false,
                "error": why,
                "latest": latest_v.map(|v| v.to_string()),
                "check_error": latest_err,
                "can_install_own_copy": can_self_install,
                "install_commands": update::install_choices(),
            }),
        }
    }

    /// Install the newest release as the plugin's own copy, or update the
    /// copy there is the way it was installed.
    pub fn install_or_update(&self, say: &mut dyn FnMut(&str)) -> Result<Value, String> {
        let data = self.data_dir();
        let current = self.locate().ok();
        let crates = self
            .env("HOME")
            .or_else(|| self.env("USERPROFILE"))
            .map(|h| PathBuf::from(h).join(".cargo").join(".crates.toml"));
        let crates_toml = crates.and_then(|c| std::fs::read_to_string(c).ok());
        let method = current
            .as_ref()
            .map(|l| update::method_for(l, crates_toml.as_deref()))
            .unwrap_or(Method::Plugin);
        let result = if method == Method::Plugin {
            let (newest, _) = update::latest(&data, self.program("git").as_deref(), true)?;
            if current.as_ref().and_then(|l| l.version) == Some(newest) {
                say(&format!("Worktrunk {newest} is already the newest release"));
                return Ok(json!({"version": newest.to_string()}));
            }
            let curl = self
                .program("curl")
                .ok_or("curl is not installed, so Worktrunk can't be downloaded")?;
            let tar = self
                .program("tar")
                .ok_or("tar is not installed, so Worktrunk can't be unpacked")?;
            let path = update::install_release(&data, newest, &curl, &tar, say)?;
            json!({"path": path, "version": newest.to_string()})
        } else {
            let plan = update::plan(method);
            let program = plan
                .program
                .and_then(|p| self.program(p))
                .ok_or_else(|| format!("Update it with: {}", plan.shown))?;
            say(&format!("Running {}", plan.shown));
            let said = update::run_manager(&program, &plan.args)?;
            say(&said);
            json!({"said": said})
        };
        self.wt.forget();
        self.rpc
            .log("info", &format!("Worktrunk installed or updated: {result}"));
        Ok(result)
    }

    /// Start a chat in `path`. The harness: the one named, else the user's
    /// setting, else the asking chat's own.
    pub fn start_chat(
        &self,
        path: &str,
        prompt: Option<&str>,
        harness: Option<&str>,
        model: Option<&str>,
        chat: &Value,
    ) -> Result<Value, String> {
        let harness = harness
            .map(String::from)
            .or_else(|| self.setting("chat_harness"))
            .or_else(|| chat.get("harness").and_then(Value::as_str).map(String::from))
            .ok_or("Name the harness for the new chat (for example claude or codex), or set one under Settings > Plugins > Worktrunk.")?;
        let model = model.map(String::from).or_else(|| {
            if chat.get("harness").and_then(Value::as_str) == Some(harness.as_str()) {
                chat.get("model").and_then(Value::as_str).map(String::from)
            } else {
                None
            }
        });
        let mut params = json!({"harness": harness, "workspace": path});
        if let Some(m) = model {
            params["model"] = json!(m);
        }
        if let Some(p) = prompt {
            params["prompt"] = json!(p);
        }
        self.rpc
            .call("chats/start", params, Duration::from_secs(60))
    }

    // ---- the view's long work, one at a time ----

    pub fn job(&self) -> Value {
        self.job.lock().unwrap_or_else(|p| p.into_inner()).json()
    }

    pub fn start_job(
        self: &Arc<Self>,
        kind: &str,
        work: impl FnOnce(&Plugin, &mut dyn FnMut(&str)) -> Result<Value, String> + Send + 'static,
    ) -> Value {
        let id = {
            let mut job = self.job.lock().unwrap_or_else(|p| p.into_inner());
            if job.running {
                return json!({"error": format!("Still busy with {}", job.kind), "job": job.json()});
            }
            *job = Job {
                id: job.id + 1,
                kind: kind.into(),
                running: true,
                ok: None,
                log: vec![],
                result: Value::Null,
            };
            job.id
        };
        let me = self.clone();
        std::thread::spawn(move || {
            let log_to = me.clone();
            let mut say = move |line: &str| {
                let mut job = log_to.job.lock().unwrap_or_else(|p| p.into_inner());
                if job.id == id {
                    for l in line.lines().filter(|l| !l.trim().is_empty()) {
                        job.log.push(l.chars().take(400).collect());
                    }
                    let n = job.log.len();
                    if n > 200 {
                        job.log.drain(..n - 200);
                    }
                }
            };
            let outcome = work(&me, &mut say);
            let mut job = me.job.lock().unwrap_or_else(|p| p.into_inner());
            job.running = false;
            match outcome {
                Ok(v) => {
                    job.ok = Some(true);
                    job.result = v;
                }
                Err(e) => {
                    job.ok = Some(false);
                    for l in e.lines().filter(|l| !l.trim().is_empty()) {
                        job.log.push(l.chars().take(400).collect());
                    }
                }
            }
        });
        json!({"job": self.job()})
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_list_schemas_read() {
        let v2 = json!({"schema": 2, "items": [{"branch": "main", "display": {"state": "is_main", "statusline": "x"}}]});
        let v1 = json!([{"branch": "main"}, {"branch": "f"}]);
        let items = list_items(&v2);
        assert_eq!(items.len(), 1);
        assert!(items[0]["display"].get("statusline").is_none());
        assert_eq!(list_items(&v1).len(), 2);
        assert!(list_items(&json!({"something": "new"})).is_empty());
    }

    #[test]
    fn a_line_says_what_matters_and_tolerates_missing_fields() {
        let item = json!({
            "branch": "feature",
            "head": {"subject": "Add login page"},
            "worktree": {"path": "/r.feature", "current": true, "main": false, "changes": {"modified": true, "untracked": true}},
            "default_branch": {"ahead": 3, "behind": 1},
            "upstream": {"ahead": 0, "behind": 2},
            "display": {"state": "diverged"},
            "a_field_from_the_future": {"x": 1}
        });
        let line = item_line(&item);
        for part in [
            "@",
            "feature",
            "/r.feature",
            "diverged",
            "3 ahead, 1 behind",
            "modified+untracked",
            "upstream 0 ahead, 2 behind",
            "Add login page",
        ] {
            assert!(line.contains(part), "{part} in {line}");
        }
        assert!(item_line(&json!({})).contains("(detached)"));
        assert!(item_line(&json!({"branch": "x"})).contains("(no worktree)"));
    }

    #[test]
    fn a_linked_worktree_leads_back_to_its_repository() {
        let root = std::env::temp_dir().join(format!("wtp-main-{}", std::process::id()));
        let main = root.join("repo");
        let linked = root.join("repo.feat");
        std::fs::create_dir_all(main.join(".git").join("worktrees").join("repo.feat")).unwrap();
        std::fs::create_dir_all(linked.join("src")).unwrap();
        std::fs::write(
            linked.join(".git"),
            format!(
                "gitdir: {}\n",
                main.join(".git")
                    .join("worktrees")
                    .join("repo.feat")
                    .display()
            ),
        )
        .unwrap();
        assert!(same_path(&main_repo(&linked.join("src")).unwrap(), &main));
        assert!(same_path(&main_repo(&main).unwrap(), &main));
        assert!(main_repo(Path::new("/")).is_none() || cfg!(windows));
        std::fs::remove_dir_all(&root).unwrap();
    }
}
