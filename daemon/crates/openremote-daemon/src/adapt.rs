//! Plugins written in a sibling plugin layout, rewritten at install so
//! Claude Code runs them as they are. That layout was built from Claude
//! Code's own - the same skills, commands, agents, hooks and MCP servers -
//! and differs where Claude Code can't read it as written: the manifest
//! sits in a folder of its own, the path variables have other names, a
//! hook can be a program and its arguments with a time limit in
//! milliseconds, and an MCP server can sign in through an account at the
//! plugin's publisher. The rewrite happens in the staged copy, never in
//! the source, and says what it changed and what it left out, so the
//! install review tells the truth about what will run. A plugin with no
//! manifest of that layout is never touched: Claude Code reads it as it is.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use openremote_core::LOCAL_MARKETPLACE;
use serde::Serialize;
use serde_json::{Map, Value, json};

use crate::plugins::{PluginError, read_json, relative_inside};

/// The layout's manifest folder, beside Claude Code's `.claude-plugin`.
pub const MANIFEST_DIR: &str = ".zcode-plugin";

/// The layout's path variables, and Claude Code's names for the same
/// folders.
const PATH_VARIABLES: &[(&str, &str)] = &[
    ("ZCODE_PLUGIN_ROOT", "CLAUDE_PLUGIN_ROOT"),
    ("ZCODE_PLUGIN_DATA", "CLAUDE_PLUGIN_DATA"),
    ("ZCODE_PROJECT_DIR", "CLAUDE_PROJECT_DIR"),
];

/// The variables that name the plugin itself, written in as text.
const NAME_VARIABLE: &str = "ZCODE_PLUGIN_NAME";
const ID_VARIABLE: &str = "ZCODE_PLUGIN_ID";

/// Every variable the layout's own app sets starts so. One with no Claude
/// Code equivalent - the address of that app's service - leaves out what
/// uses it.
const HOST_PREFIX: &str = "ZCODE_";

/// An MCP server that signs in with an account at the plugin's publisher.
const PUBLISHER_ACCOUNT: &str = "zcode_official";

/// The layout's time limit for a hook that names none. Claude Code's own
/// is ten times longer, so it is written in.
const DEFAULT_HOOK_SECS: u64 = 60;

/// The manifest fields Claude Code reads too: filled from the layout's
/// manifest where Claude Code's says nothing.
const SHARED_FIELDS: &[&str] = &[
    "name",
    "displayName",
    "version",
    "description",
    "author",
    "homepage",
    "repository",
    "license",
    "keywords",
    "userConfig",
    "skills",
    "commands",
    "agents",
    "hooks",
    "mcpServers",
];

/// The fields of a Claude Code setting (a `userConfig` option). It refuses
/// to load a plugin whose setting has any other.
const SETTING_FIELDS: &[&str] = &[
    "type",
    "title",
    "description",
    "required",
    "default",
    "options",
    "multiple",
    "sensitive",
    "min",
    "max",
];
const SETTING_TYPES: &[&str] = &["string", "number", "boolean", "directory", "file"];

/// What adapting a plugin changed and what it left out, in plain words
/// for the install review.
#[derive(Clone, Default, Debug, PartialEq, Eq, Serialize)]
pub struct Notes {
    pub adapted: Vec<String>,
    pub left_out: Vec<String>,
}

/// What adapting a folder would do, worked out without writing anything:
/// the files to write or remove, what to say about it, and whether any of
/// the MCP servers and hooks it declares survive.
pub struct Plan {
    writes: Vec<(PathBuf, Option<Value>)>,
    pub notes: Notes,
    /// None: it declares no MCP server at all.
    pub mcp_kept: Option<bool>,
    /// None: it declares no hook at all.
    pub hooks_kept: Option<bool>,
}

impl Plan {
    /// Write the plan into the folder it was made for.
    pub fn apply(&self, dir: &Path) -> Result<(), PluginError> {
        for (relative, content) in &self.writes {
            let path = dir.join(relative);
            match content {
                Some(value) => {
                    if let Some(parent) = path.parent() {
                        std::fs::create_dir_all(parent)?;
                    }
                    std::fs::write(
                        &path,
                        serde_json::to_vec_pretty(value).expect("JSON serializes"),
                    )?;
                }
                None => match std::fs::remove_file(&path) {
                    Ok(()) => {}
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                    Err(e) => return Err(e.into()),
                },
            }
        }
        Ok(())
    }
}

/// Rewrite a staged plugin folder for Claude Code, in place. None when it
/// isn't in the sibling layout. `name` is the name it installs under (its
/// marketplace entry's), when there is one.
pub fn adapt(
    dir: &Path,
    name: Option<&str>,
    marketplace: &str,
) -> Result<Option<Notes>, PluginError> {
    let Some(plan) = plan(dir, name, Some(marketplace))? else {
        return Ok(None);
    };
    plan.apply(dir)?;
    Ok(Some(plan.notes))
}

/// The plugin as the rewrite sees it.
struct Ctx<'a> {
    dir: &'a Path,
    name: String,
    id: String,
}

/// What the rewrite met on its way, for the notes.
#[derive(Default)]
struct Tally {
    hooks_declared: usize,
    hooks_kept: usize,
    hooks_rewritten: usize,
    servers_declared: usize,
    servers_kept: usize,
    servers_rewritten: bool,
    variables: bool,
    account: Vec<String>,
    off: Vec<String>,
    host_only: Vec<String>,
    no_program: Vec<String>,
    missing: Vec<(String, String)>,
    elsewhere: Vec<String>,
    settings_read: BTreeSet<String>,
}

/// Work out the rewrite of a plugin folder in the sibling layout; None
/// when the folder has no manifest of that layout.
pub fn plan(
    dir: &Path,
    name: Option<&str>,
    marketplace: Option<&str>,
) -> Result<Option<Plan>, PluginError> {
    let Some(alternate) = read_object(&dir.join(MANIFEST_DIR).join("plugin.json"))? else {
        return Ok(None);
    };
    let manifest_path = Path::new(".claude-plugin").join("plugin.json");
    let claude = read_object(&dir.join(&manifest_path))?;
    let text = |m: Option<&Map<String, Value>>| {
        m.and_then(|m| m.get("name"))
            .and_then(Value::as_str)
            .map(String::from)
    };
    let plugin = name
        .map(String::from)
        .or_else(|| text(claude.as_ref()))
        .or_else(|| text(Some(&alternate)))
        .unwrap_or_else(|| "plugin".into());
    let ctx = Ctx {
        dir,
        id: format!("{plugin}@{}", marketplace.unwrap_or(LOCAL_MARKETPLACE)),
        name: plugin,
    };
    let mut tally = Tally::default();
    let mut writes = Vec::new();

    // Claude Code's manifest: its own where there is one (it wins), every
    // gap filled from the layout's, in Claude Code's shapes.
    let mut manifest = claude.clone().unwrap_or_default();
    for key in SHARED_FIELDS {
        if manifest.contains_key(*key) {
            continue;
        }
        if let Some(value) = alternate.get(*key).and_then(|v| claude_field(key, v)) {
            manifest.insert((*key).to_string(), value);
        }
    }
    for key in ["skills", "commands", "agents"] {
        if let Some(value) = manifest.remove(key) {
            if let Some(value) = claude_paths(key, value, dir) {
                manifest.insert(key.to_string(), value);
            }
        }
    }

    // Hooks: the standard file, then whatever the manifest names.
    let standard_hooks = Path::new("hooks").join("hooks.json");
    if let Some(file) = read_object(&dir.join(&standard_hooks))? {
        match adapt_hooks_file(file.clone(), &ctx, &mut tally) {
            Some(new) if new != file => writes.push((standard_hooks, Some(Value::Object(new)))),
            Some(_) => {}
            None => writes.push((standard_hooks, None)),
        }
    }
    if let Some(hooks) = manifest.remove("hooks") {
        if let Some(hooks) = manifest_hooks(hooks, &ctx, &mut tally, &mut writes) {
            manifest.insert("hooks".into(), hooks);
        }
    }

    // MCP servers: the standard file, then whatever the manifest names.
    if let Some(file) = read_object(&dir.join(".mcp.json"))? {
        match adapt_servers_file(file.clone(), &ctx, &mut tally) {
            Some(new) if new != file => {
                writes.push((PathBuf::from(".mcp.json"), Some(Value::Object(new))))
            }
            Some(_) => {}
            None => writes.push((PathBuf::from(".mcp.json"), None)),
        }
    }
    if let Some(servers) = manifest.remove("mcpServers") {
        if let Some(servers) = manifest_servers(servers, &ctx, &mut tally, &mut writes) {
            manifest.insert("mcpServers".into(), servers);
        }
    }

    // Settings: in the shape Claude Code accepts, and one the plugin reads
    // without declaring it gets declared, empty, so Claude Code keeps the
    // server that reads it.
    let mut settings = manifest
        .remove("userConfig")
        .and_then(|v| v.as_object().map(claude_settings))
        .unwrap_or_default();
    let undeclared: Vec<String> = tally
        .settings_read
        .iter()
        .filter(|key| !settings.contains_key(key.as_str()))
        .cloned()
        .collect();
    for key in &undeclared {
        settings.insert(
            key.clone(),
            json!({
                "type": "string",
                "title": key,
                "description": "Read by the plugin, which declares no setting for it.",
                "default": "",
            }),
        );
    }
    if !settings.is_empty() {
        manifest.insert("userConfig".into(), Value::Object(settings));
    }

    let manifest_written = claude.as_ref() != Some(&manifest);
    if manifest_written {
        writes.push((manifest_path, Some(Value::Object(manifest))));
    }

    let panels = panel_titles(&alternate);
    let notes = tally.notes(manifest_written, &undeclared, &panels);
    Ok(Some(Plan {
        writes,
        notes,
        mcp_kept: (tally.servers_declared > 0).then_some(tally.servers_kept > 0),
        hooks_kept: (tally.hooks_declared > 0).then_some(tally.hooks_kept > 0),
    }))
}

/// A JSON file that must hold an object; None when it isn't there.
fn read_object(path: &Path) -> Result<Option<Map<String, Value>>, PluginError> {
    match read_json::<Value>(path)? {
        None => Ok(None),
        Some(Value::Object(map)) => Ok(Some(map)),
        Some(_) => Err(PluginError::Invalid(format!(
            "{} is not a JSON object",
            path.file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default()
        ))),
    }
}

/// A field of the layout's manifest in Claude Code's shape; None for a
/// value Claude Code would refuse (it won't load a plugin over a bad
/// author or homepage).
fn claude_field(key: &str, value: &Value) -> Option<Value> {
    match key {
        "author" => match value {
            Value::String(name) if !name.is_empty() => Some(json!({ "name": name })),
            Value::Object(o) if o.get("name").is_some_and(Value::is_string) => Some(value.clone()),
            _ => None,
        },
        "homepage" => value
            .as_str()
            .filter(|url| url.starts_with("https://") || url.starts_with("http://"))
            .map(|_| value.clone()),
        "repository" => match value {
            Value::String(_) => Some(value.clone()),
            Value::Object(o) => o.get("url").filter(|url| url.is_string()).cloned(),
            _ => None,
        },
        "keywords" => value
            .as_array()
            .map(|words| Value::Array(words.iter().filter(|w| w.is_string()).cloned().collect())),
        "name" | "displayName" | "version" | "description" | "license" => {
            value.is_string().then(|| value.clone())
        }
        _ => Some(value.clone()),
    }
}

/// A component path field in Claude Code's form: every path starts with
/// `./` (it refuses one that doesn't), paths to nothing are dropped (it
/// refuses those too), `skills` leaves out the default folder it scans
/// anyway, and `agents` lists files, since Claude Code takes no folder
/// there. None when nothing is left.
fn claude_paths(key: &str, value: Value, dir: &Path) -> Option<Value> {
    let paths: Vec<Value> = match value {
        Value::String(s) => vec![Value::String(s)],
        Value::Array(items) => items,
        // An inline map (commands by name) is Claude Code's own shape too.
        other => return Some(other),
    };
    let was_empty = paths.is_empty();
    let mut out = Vec::new();
    for item in paths {
        let Some(raw) = item.as_str() else {
            out.push(item);
            continue;
        };
        let path = if raw == "." || raw == "./" {
            raw.to_string()
        } else {
            format!("./{}", raw.trim_start_matches("./"))
        };
        let relative = path.trim_start_matches("./").trim_end_matches('/');
        if !(relative.is_empty() || relative == "." || relative_inside(relative)) {
            out.push(Value::String(path));
            continue;
        }
        let on_disk = dir.join(relative);
        if !on_disk.exists() {
            continue;
        }
        if key == "skills" && relative == "skills" {
            continue;
        }
        if key == "agents" && on_disk.is_dir() {
            let mut files = Vec::new();
            markdown_files(&on_disk, &mut files);
            for file in files {
                if let Ok(inner) = file.strip_prefix(dir) {
                    let inner = inner.to_string_lossy().replace('\\', "/");
                    out.push(Value::String(format!("./{inner}")));
                }
            }
            continue;
        }
        out.push(Value::String(path));
    }
    match out.len() {
        0 if !was_empty => None,
        1 => out.pop(),
        _ => Some(Value::Array(out)),
    }
}

fn markdown_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut paths: Vec<PathBuf> = entries.flatten().map(|e| e.path()).collect();
    paths.sort();
    for path in paths {
        if path.is_dir() {
            markdown_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "md") {
            out.push(path);
        }
    }
}

/// One string with the layout's variables in Claude Code's names. The
/// error names a variable of the layout's app that has no equivalent.
fn rewrite(text: &str, ctx: &Ctx) -> Result<String, String> {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find('$') {
        out.push_str(&rest[..at]);
        let after = &rest[at + 1..];
        let (name, span) = variable_at(after);
        if span == 0 || !name.starts_with(HOST_PREFIX) {
            out.push('$');
            rest = after;
            continue;
        }
        let token = &after[..span];
        if let Some((_, claude)) = PATH_VARIABLES.iter().find(|(ours, _)| *ours == name) {
            out.push('$');
            out.push_str(&token.replacen(name, claude, 1));
        } else if name == NAME_VARIABLE {
            out.push_str(&ctx.name);
        } else if name == ID_VARIABLE {
            out.push_str(&ctx.id);
        } else {
            return Err(name.to_string());
        }
        rest = &after[span..];
    }
    out.push_str(rest);
    Ok(out)
}

/// The variable a `$` starts - `{NAME}`, `{NAME:-default}` or a bare
/// `NAME` - as its name and how far past the `$` it reaches (0 for none).
fn variable_at(after: &str) -> (&str, usize) {
    if let Some(inner) = after.strip_prefix('{') {
        return match inner.find('}') {
            Some(end) => {
                let body = &inner[..end];
                (body.split(":-").next().unwrap_or(body), end + 2)
            }
            None => ("", 0),
        };
    }
    let end = after
        .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        .unwrap_or(after.len());
    (&after[..end], end)
}

/// Every string in a hook or server rewritten (keys stay: they are the
/// names its own program reads). Answers whether anything changed.
fn rewrite_value(value: &mut Value, ctx: &Ctx) -> Result<bool, String> {
    match value {
        Value::String(s) => {
            let new = rewrite(s, ctx)?;
            let changed = new != *s;
            *s = new;
            Ok(changed)
        }
        Value::Array(items) => {
            let mut changed = false;
            for item in items {
                changed |= rewrite_value(item, ctx)?;
            }
            Ok(changed)
        }
        Value::Object(map) => {
            let mut changed = false;
            for item in map.values_mut() {
                changed |= rewrite_value(item, ctx)?;
            }
            Ok(changed)
        }
        _ => Ok(false),
    }
}

/// The settings a hook or server reads (`${user_config.KEY}`).
fn settings_read(value: &Value, out: &mut BTreeSet<String>) {
    match value {
        Value::String(s) => {
            let mut rest = s.as_str();
            while let Some(at) = rest.find("${user_config.") {
                let after = &rest[at + "${user_config.".len()..];
                let Some(end) = after.find('}') else {
                    break;
                };
                out.insert(after[..end].to_string());
                rest = &after[end..];
            }
        }
        Value::Array(items) => items.iter().for_each(|v| settings_read(v, out)),
        Value::Object(map) => map.values().for_each(|v| settings_read(v, out)),
        _ => {}
    }
}

/// The first script inside the plugin that a hook or server runs and the
/// folder doesn't have - a build output its source never carries.
fn missing_script(value: &Value, dir: &Path) -> Option<String> {
    const ROOT: &str = "${CLAUDE_PLUGIN_ROOT}/";
    const SCRIPTS: &[&str] = &[
        "js", "mjs", "cjs", "ts", "py", "sh", "ps1", "rb", "exe", "bat", "cmd",
    ];
    let mut run: Vec<&str> = Vec::new();
    if let Some(command) = value.get("command").and_then(Value::as_str) {
        // A program inside the plugin named on its own, whatever its name.
        let program = command.trim_matches(['"', '\'']);
        if let Some(path) = program.strip_prefix(ROOT) {
            if !path.contains(char::is_whitespace)
                && relative_inside(path)
                && !dir.join(path).is_file()
            {
                return Some(path.to_string());
            }
        }
        run.push(command);
    }
    if let Some(args) = value.get("args").and_then(Value::as_array) {
        run.extend(args.iter().filter_map(Value::as_str));
    }
    for text in run {
        let mut rest = text;
        while let Some(at) = rest.find(ROOT) {
            let after = &rest[at + ROOT.len()..];
            let end = after
                .find(|c: char| c.is_whitespace() || "\"'`;&|<>()".contains(c))
                .unwrap_or(after.len());
            let path = &after[..end];
            let script = Path::new(path)
                .extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| SCRIPTS.contains(&e.to_ascii_lowercase().as_str()));
            if script && relative_inside(path) && !dir.join(path).is_file() {
                return Some(path.to_string());
            }
            rest = &after[end..];
        }
    }
    None
}

fn count_handlers(events: &Map<String, Value>) -> usize {
    events
        .values()
        .filter_map(Value::as_array)
        .flatten()
        .filter_map(|group| group.get("hooks").and_then(Value::as_array))
        .map(Vec::len)
        .sum()
}

/// A hooks file in Claude Code's shape (the event map under `hooks`; a
/// bare event map is wrapped). None when every hook in it was left out.
fn adapt_hooks_file(
    mut file: Map<String, Value>,
    ctx: &Ctx,
    tally: &mut Tally,
) -> Option<Map<String, Value>> {
    let bare =
        !file.contains_key("hooks") && !file.is_empty() && file.values().all(Value::is_array);
    if bare {
        file = Map::from_iter([("hooks".to_string(), Value::Object(file))]);
    }
    let Some(Value::Object(events)) = file.get_mut("hooks") else {
        return Some(file);
    };
    let before = count_handlers(events);
    adapt_hook_events(events, ctx, tally);
    (before == 0 || !events.is_empty()).then_some(file)
}

/// The manifest's `hooks`: an inline event map rewritten in place, a hooks
/// file rewritten where it is, the standard file dropped (Claude Code
/// loads it anyway, and twice would run each hook twice).
fn manifest_hooks(
    value: Value,
    ctx: &Ctx,
    tally: &mut Tally,
    writes: &mut Vec<(PathBuf, Option<Value>)>,
) -> Option<Value> {
    match value {
        Value::String(raw) => {
            let relative = raw.trim_start_matches("./").to_string();
            if relative == "hooks/hooks.json" {
                return None;
            }
            let path = format!("./{relative}");
            if !relative_inside(&relative) {
                return Some(Value::String(path));
            }
            match read_object(&ctx.dir.join(&relative)) {
                Ok(Some(file)) => match adapt_hooks_file(file.clone(), ctx, tally) {
                    Some(new) => {
                        if new != file {
                            writes.push((PathBuf::from(&relative), Some(Value::Object(new))));
                        }
                        Some(Value::String(path))
                    }
                    None => {
                        writes.push((PathBuf::from(&relative), None));
                        None
                    }
                },
                // Missing or unreadable: as written; Claude Code says so.
                _ => Some(Value::String(path)),
            }
        }
        Value::Object(mut events) => {
            let before = count_handlers(&events);
            adapt_hook_events(&mut events, ctx, tally);
            (before == 0 || !events.is_empty()).then_some(Value::Object(events))
        }
        Value::Array(items) => {
            let kept: Vec<Value> = items
                .into_iter()
                .filter_map(|item| manifest_hooks(item, ctx, tally, writes))
                .collect();
            (!kept.is_empty()).then_some(Value::Array(kept))
        }
        other => Some(other),
    }
}

/// An event map's hooks rewritten in place; a group or event left with no
/// hook goes too.
fn adapt_hook_events(events: &mut Map<String, Value>, ctx: &Ctx, tally: &mut Tally) {
    for (event, groups) in events.iter_mut() {
        let Some(groups) = groups.as_array_mut() else {
            continue;
        };
        for group in groups.iter_mut() {
            if let Some(handlers) = group.get_mut("hooks").and_then(Value::as_array_mut) {
                handlers.retain_mut(|handler| adapt_hook(handler, event, ctx, tally));
            }
        }
        groups.retain(|group| {
            group
                .get("hooks")
                .and_then(Value::as_array)
                .is_none_or(|handlers| !handlers.is_empty())
        });
    }
    events.retain(|_, groups| groups.as_array().is_none_or(|g| !g.is_empty()));
}

/// The layout's hook time limit, in Claude Code's seconds: `timeoutMs`
/// wins over `timeout` (already seconds), and none means the layout's
/// default.
fn hook_secs(fields: &Map<String, Value>) -> u64 {
    if let Some(ms) = fields.get("timeoutMs").and_then(Value::as_u64) {
        return ms.div_ceil(1000).max(1);
    }
    fields
        .get("timeout")
        .and_then(Value::as_u64)
        .unwrap_or(DEFAULT_HOOK_SECS)
}

/// One hook handler for Claude Code; false leaves it out. A program hook
/// becomes Claude Code's exec form - the same program and arguments, no
/// shell in between - with its time limit in seconds.
fn adapt_hook(handler: &mut Value, event: &str, ctx: &Ctx, tally: &mut Tally) -> bool {
    if !handler.is_object() {
        return true;
    }
    tally.hooks_declared += 1;
    let what = format!("A {event} hook");
    if handler.get("enabled") == Some(&Value::Bool(false)) {
        tally.off.push(what);
        return false;
    }
    // What changed counts toward the notes only for a hook that is kept.
    let renamed = match rewrite_value(handler, ctx) {
        Ok(renamed) => renamed,
        Err(_) => {
            tally.host_only.push(what);
            return false;
        }
    };
    let mut changed = renamed;
    let Some(fields) = handler.as_object_mut() else {
        return true;
    };
    let kind = fields
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or("command")
        .to_string();
    match kind.as_str() {
        "process" => {
            let Some(command) = fields.get("command").filter(|c| c.is_string()).cloned() else {
                tally.no_program.push(what);
                return false;
            };
            let args = fields
                .get("args")
                .filter(|a| a.is_array())
                .cloned()
                .unwrap_or_else(|| json!([]));
            let mut rewritten = Map::new();
            rewritten.insert("type".into(), json!("command"));
            rewritten.insert("command".into(), command);
            rewritten.insert("args".into(), args);
            rewritten.insert("timeout".into(), json!(hook_secs(fields)));
            if let Some(status) = fields.get("statusMessage").filter(|s| s.is_string()) {
                rewritten.insert("statusMessage".into(), status.clone());
            }
            *fields = rewritten;
            changed = true;
        }
        "command" => {
            if fields.contains_key("timeoutMs") || !fields.contains_key("timeout") {
                let secs = hook_secs(fields);
                fields.remove("timeoutMs");
                fields.insert("timeout".into(), json!(secs));
                changed = true;
            }
            // `shell: true` means "through the shell", which Claude Code's
            // shell form already is; it takes only a shell's name there.
            if fields.get("shell").is_some_and(Value::is_boolean) {
                fields.remove("shell");
                changed = true;
            }
            changed |= fields.remove("enabled").is_some();
        }
        _ => {}
    }
    if let Some(file) = missing_script(handler, ctx.dir) {
        tally.missing.push((what, file));
        return false;
    }
    settings_read(handler, &mut tally.settings_read);
    tally.hooks_kept += 1;
    tally.variables |= renamed;
    if changed {
        tally.hooks_rewritten += 1;
    }
    true
}

/// A servers file (`{"mcpServers": {...}}`, or the map itself) rewritten.
/// None when every server in it was left out.
fn adapt_servers_file(
    mut file: Map<String, Value>,
    ctx: &Ctx,
    tally: &mut Tally,
) -> Option<Map<String, Value>> {
    let wrapped = file.get("mcpServers").is_some_and(Value::is_object);
    let servers = if wrapped {
        file.get_mut("mcpServers")
            .and_then(Value::as_object_mut)
            .expect("checked just above")
    } else {
        &mut file
    };
    let before = servers.len();
    servers.retain(|name, server| adapt_server(name, server, ctx, tally));
    (before == 0 || !servers.is_empty()).then_some(file)
}

/// The manifest's `mcpServers`: an inline map rewritten in place, a JSON
/// file rewritten where it is, the standard `.mcp.json` dropped (Claude
/// Code loads it anyway), a bundle left as it is.
fn manifest_servers(
    value: Value,
    ctx: &Ctx,
    tally: &mut Tally,
    writes: &mut Vec<(PathBuf, Option<Value>)>,
) -> Option<Value> {
    match value {
        Value::String(raw) => {
            let lower = raw.to_ascii_lowercase();
            if lower.ends_with(".mcpb") || lower.ends_with(".dxt") || lower.contains("://") {
                return Some(Value::String(raw));
            }
            let relative = raw.trim_start_matches("./").to_string();
            if relative == ".mcp.json" {
                return None;
            }
            let path = format!("./{relative}");
            if !relative_inside(&relative) {
                return Some(Value::String(path));
            }
            match read_object(&ctx.dir.join(&relative)) {
                Ok(Some(file)) => match adapt_servers_file(file.clone(), ctx, tally) {
                    Some(new) => {
                        if new != file {
                            writes.push((PathBuf::from(&relative), Some(Value::Object(new))));
                        }
                        Some(Value::String(path))
                    }
                    None => {
                        writes.push((PathBuf::from(&relative), None));
                        None
                    }
                },
                _ => Some(Value::String(path)),
            }
        }
        Value::Object(mut servers) => {
            let before = servers.len();
            servers.retain(|name, server| adapt_server(name, server, ctx, tally));
            (before == 0 || !servers.is_empty()).then_some(Value::Object(servers))
        }
        Value::Array(items) => {
            let kept: Vec<Value> = items
                .into_iter()
                .filter_map(|item| manifest_servers(item, ctx, tally, writes))
                .collect();
            (!kept.is_empty()).then_some(Value::Array(kept))
        }
        other => Some(other),
    }
}

/// One MCP server for Claude Code; false leaves it out.
fn adapt_server(name: &str, server: &mut Value, ctx: &Ctx, tally: &mut Tally) -> bool {
    if !server.is_object() {
        return true;
    }
    tally.servers_declared += 1;
    let what = format!("The MCP server {name}");
    let account = server
        .get("auth")
        .and_then(|auth| auth.get("type"))
        .and_then(Value::as_str)
        == Some(PUBLISHER_ACCOUNT);
    if account {
        tally.account.push(name.to_string());
        return false;
    }
    if server.get("enabled") == Some(&Value::Bool(false)) {
        tally.off.push(what);
        return false;
    }
    // What changed counts toward the notes only for a server that is kept.
    let renamed = match rewrite_value(server, ctx) {
        Ok(renamed) => renamed,
        Err(_) => {
            tally.host_only.push(what);
            return false;
        }
    };
    let Some(fields) = server.as_object_mut() else {
        return true;
    };
    let mut rewritten = false;
    // Both count milliseconds; Claude Code calls it `timeout`.
    if let Some(ms) = fields.remove("timeoutMs") {
        fields.entry("timeout").or_insert(ms);
        rewritten = true;
    }
    rewritten |= fields.remove("enabled").is_some();
    // Claude Code takes no start folder for a server: it starts in the
    // chat's folder, which is the one nearly every server asks for.
    let mut elsewhere = false;
    if let Some(cwd) = fields.remove("cwd") {
        rewritten = true;
        elsewhere = cwd.as_str() != Some("${CLAUDE_PROJECT_DIR}");
    }
    // The layout reads a server with a `url` and no type as HTTP; Claude
    // Code would read it as a program to start, and skip it.
    if !fields.contains_key("type") && fields.contains_key("url") {
        fields.insert("type".into(), json!("http"));
        rewritten = true;
    }
    if let Some(file) = missing_script(server, ctx.dir) {
        tally.missing.push((what, file));
        return false;
    }
    settings_read(server, &mut tally.settings_read);
    tally.servers_kept += 1;
    tally.variables |= renamed;
    tally.servers_rewritten |= rewritten;
    if elsewhere {
        tally.elsewhere.push(name.to_string());
    }
    true
}

/// Settings in the shape Claude Code accepts: only its fields, and the
/// title and description it requires (the key and the title stand in).
fn claude_settings(options: &Map<String, Value>) -> Map<String, Value> {
    let mut out = Map::new();
    for (key, option) in options {
        let Some(option) = option.as_object() else {
            continue;
        };
        let mut setting: Map<String, Value> = SETTING_FIELDS
            .iter()
            .filter_map(|f| option.get(*f).map(|v| ((*f).to_string(), v.clone())))
            .collect();
        let kind = option
            .get("type")
            .and_then(Value::as_str)
            .filter(|t| SETTING_TYPES.contains(t))
            .unwrap_or("string");
        setting.insert("type".into(), json!(kind));
        let title = option
            .get("title")
            .and_then(plain_text)
            .unwrap_or_else(|| key.clone());
        let description = option
            .get("description")
            .and_then(plain_text)
            .unwrap_or_else(|| title.clone());
        setting.insert("title".into(), json!(title));
        setting.insert("description".into(), json!(description));
        out.insert(key.clone(), Value::Object(setting));
    }
    out
}

/// A text the layout may give per language: English where there is one.
fn plain_text(value: &Value) -> Option<String> {
    match value {
        Value::String(s) if !s.trim().is_empty() => Some(s.clone()),
        Value::Object(by_language) => ["en-US", "en"]
            .iter()
            .find_map(|l| by_language.get(*l).and_then(Value::as_str))
            .or_else(|| by_language.values().find_map(Value::as_str))
            .map(String::from),
        _ => None,
    }
}

/// The panels the plugin shows in its own app (MCP App pages). OpenRemote
/// has no host for them yet.
fn panel_titles(alternate: &Map<String, Value>) -> Vec<String> {
    alternate
        .get("ui")
        .and_then(|ui| ui.get("surfaces"))
        .and_then(Value::as_array)
        .map(|surfaces| {
            surfaces
                .iter()
                .filter_map(|s| {
                    s.get("title")
                        .and_then(plain_text)
                        .or_else(|| s.get("id").and_then(Value::as_str).map(String::from))
                })
                .collect()
        })
        .unwrap_or_default()
}

/// "a", "a and b", "a, b and c".
fn listed(names: &[String]) -> String {
    match names {
        [] => String::new(),
        [one] => one.clone(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    }
}

impl Tally {
    fn notes(&self, manifest_written: bool, undeclared: &[String], panels: &[String]) -> Notes {
        let mut adapted = Vec::new();
        if manifest_written {
            adapted.push("Its manifest is rewritten in Claude Code's layout".to_string());
        }
        match self.hooks_rewritten {
            0 => {}
            1 => adapted.push(
                "Its hook runs as a Claude Code hook, with the same program, arguments and time limit"
                    .into(),
            ),
            n => adapted.push(format!(
                "Its {n} hooks run as Claude Code hooks, with the same programs, arguments and time limits"
            )),
        }
        if self.servers_rewritten {
            adapted.push("Its MCP server settings are rewritten for Claude Code".into());
        }
        if self.variables {
            adapted.push("The paths it names use Claude Code's names for the same folders".into());
        }
        for name in &self.elsewhere {
            adapted.push(format!(
                "The MCP server {name} starts in the chat's folder, not the one it names"
            ));
        }
        if !undeclared.is_empty() {
            adapted.push(format!(
                "Settings it reads but never declares start empty: {}",
                undeclared.join(", ")
            ));
        }

        let mut left_out = Vec::new();
        match self.account.as_slice() {
            [] => {}
            [one] => left_out.push(format!(
                "The MCP server {one} needs an account with this plugin's publisher"
            )),
            many => left_out.push(format!(
                "The MCP servers {} need an account with this plugin's publisher",
                listed(many)
            )),
        }
        for what in &self.off {
            left_out.push(format!("{what} is turned off in the plugin itself"));
        }
        for what in &self.host_only {
            left_out.push(format!(
                "{what} uses an address only its publisher's app provides"
            ));
        }
        for what in &self.no_program {
            left_out.push(format!("{what} names no program to run"));
        }
        for (what, file) in &self.missing {
            left_out.push(format!(
                "{what} needs {file}, which this copy of the plugin doesn't include"
            ));
        }
        for title in panels {
            left_out.push(format!(
                "The {title} panel needs a host for plugin panels, which OpenRemote doesn't have yet"
            ));
        }
        Notes { adapted, left_out }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx(dir: &Path) -> Ctx<'_> {
        Ctx {
            dir,
            name: "toy".into(),
            id: "toy@market".into(),
        }
    }

    fn folder(files: &[(&str, &str)]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        for (path, body) in files {
            let p = dir.path().join(path);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, body).unwrap();
        }
        dir
    }

    fn json_at(dir: &Path, path: &str) -> Value {
        serde_json::from_slice(&std::fs::read(dir.join(path)).unwrap()).unwrap()
    }

    #[test]
    fn the_layouts_variables_take_claude_codes_names() {
        let dir = tempfile::tempdir().unwrap();
        let c = ctx(dir.path());
        assert_eq!(
            rewrite("${ZCODE_PLUGIN_ROOT}/hooks/run.cjs", &c).unwrap(),
            "${CLAUDE_PLUGIN_ROOT}/hooks/run.cjs"
        );
        assert_eq!(
            rewrite(
                "cd \"$ZCODE_PROJECT_DIR\" && x ${ZCODE_PLUGIN_DATA:-/tmp}",
                &c
            )
            .unwrap(),
            "cd \"$CLAUDE_PROJECT_DIR\" && x ${CLAUDE_PLUGIN_DATA:-/tmp}"
        );
        assert_eq!(
            rewrite("${ZCODE_PLUGIN_NAME} ${ZCODE_PLUGIN_ID}", &c).unwrap(),
            "toy toy@market"
        );
        // Claude's own, a setting, an ordinary variable and a lone $ stay.
        let kept = "${CLAUDE_PLUGIN_ROOT} ${user_config.key} $HOME costs $5";
        assert_eq!(rewrite(kept, &c).unwrap(), kept);
        // The layout's app's own service address has no equivalent.
        assert_eq!(
            rewrite("${ZCODE_BASE_URL}/api", &c).unwrap_err(),
            "ZCODE_BASE_URL"
        );
    }

    #[test]
    fn a_program_hook_becomes_claude_codes_exec_form_in_seconds() {
        let dir = folder(&[("hooks/check.mjs", "")]);
        let mut tally = Tally::default();
        let mut events = json!({
            "PreToolUse": [{"matcher": "Bash", "hooks": [
                {"type": "process", "command": "node", "args": ["${ZCODE_PLUGIN_ROOT}/hooks/check.mjs"],
                 "timeoutMs": 4500, "statusMessage": "Checking"},
                {"type": "command", "command": "echo off", "enabled": false},
            ]}],
            "Stop": [{"hooks": [{"type": "command", "command": "echo done", "shell": true}]}],
            "SessionStart": [{"hooks": [{"type": "process", "command": "node",
                 "args": ["${ZCODE_PLUGIN_ROOT}/dist/missing.mjs"]}]}],
        });
        adapt_hook_events(
            events.as_object_mut().unwrap(),
            &ctx(dir.path()),
            &mut tally,
        );
        assert_eq!(
            events,
            json!({
                "PreToolUse": [{"matcher": "Bash", "hooks": [
                    {"type": "command", "command": "node", "args": ["${CLAUDE_PLUGIN_ROOT}/hooks/check.mjs"],
                     "timeout": 5, "statusMessage": "Checking"},
                ]}],
                "Stop": [{"hooks": [{"type": "command", "command": "echo done", "timeout": 60}]}],
            })
        );
        assert_eq!(tally.off, vec!["A PreToolUse hook"]);
        assert_eq!(
            tally.missing,
            vec![(
                "A SessionStart hook".to_string(),
                "dist/missing.mjs".to_string()
            )]
        );
    }

    #[test]
    fn servers_needing_the_publishers_account_are_left_out_and_the_rest_rewritten() {
        let dir = folder(&[("server.mjs", "")]);
        let mut tally = Tally::default();
        let file = json!({"mcpServers": {
            "data": {"type": "http", "url": "${ZCODE_BASE_URL}/mcp", "auth": {"type": "zcode_official"}},
            "local": {"command": "node", "args": ["${ZCODE_PLUGIN_ROOT}/server.mjs"],
                      "cwd": "${ZCODE_PROJECT_DIR}", "timeoutMs": 120000, "enabled": true},
            "remote": {"url": "https://example.com/mcp"},
            "off": {"command": "node", "enabled": false},
        }});
        let out = adapt_servers_file(
            file.as_object().unwrap().clone(),
            &ctx(dir.path()),
            &mut tally,
        )
        .unwrap();
        assert_eq!(
            Value::Object(out),
            json!({"mcpServers": {
                "local": {"command": "node", "args": ["${CLAUDE_PLUGIN_ROOT}/server.mjs"], "timeout": 120000},
                "remote": {"type": "http", "url": "https://example.com/mcp"},
            }})
        );
        let notes = tally.notes(false, &[], &[]);
        assert_eq!(
            notes.left_out,
            vec![
                "The MCP server data needs an account with this plugin's publisher",
                "The MCP server off is turned off in the plugin itself",
            ]
        );
    }

    #[test]
    fn a_plugin_with_only_the_layouts_manifest_gets_claude_codes() {
        let dir = folder(&[
            (
                ".zcode-plugin/plugin.json",
                r#"{"name": "toy", "version": "0.2.0", "author": "Someone", "homepage": "not a url",
                   "description_i18n": {"en": "x"}, "skills": "skills", "agents": "agents",
                   "mcpServers": {"toy": {"command": "node", "args": ["${ZCODE_PLUGIN_ROOT}/s.mjs"],
                                          "env": {"KEY": "${user_config.key}", "OTHER": "${user_config.other}"}}},
                   "userConfig": {"key": {"type": "string", "default": "a", "extra": 1}},
                   "ui": {"surfaces": [{"id": "toy", "title": {"zh-CN": "x", "en-US": "Toy"}}]}}"#,
            ),
            ("skills/greet/SKILL.md", "# greet"),
            ("agents/helper.md", "# helper"),
            ("s.mjs", ""),
        ]);
        let notes = adapt(dir.path(), Some("toy"), "market").unwrap().unwrap();
        let written = json_at(dir.path(), ".claude-plugin/plugin.json");
        assert_eq!(written["name"], "toy");
        assert_eq!(written["author"], json!({"name": "Someone"}));
        assert!(written.get("homepage").is_none());
        assert!(written.get("description_i18n").is_none());
        // The default skills folder is scanned anyway; agents are files.
        assert!(written.get("skills").is_none());
        assert_eq!(written["agents"], "./agents/helper.md");
        assert_eq!(
            written["mcpServers"]["toy"]["args"],
            json!(["${CLAUDE_PLUGIN_ROOT}/s.mjs"])
        );
        assert_eq!(
            written["userConfig"]["key"],
            json!({"type": "string", "default": "a", "title": "key", "description": "key"})
        );
        assert_eq!(written["userConfig"]["other"]["default"], "");
        assert!(
            notes.left_out.iter().any(|n| n.contains("The Toy panel")),
            "{notes:?}"
        );
        assert!(
            notes.adapted.iter().any(|n| n.ends_with(": other")),
            "{notes:?}"
        );
        // A plugin in Claude Code's layout alone is never touched.
        let claude = folder(&[(".claude-plugin/plugin.json", r#"{"name": "c"}"#)]);
        assert!(plan(claude.path(), None, None).unwrap().is_none());
    }
}
