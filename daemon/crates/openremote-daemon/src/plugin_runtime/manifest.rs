//! The OpenRemote part of a plugin's manifest beyond its name, powers and
//! harness block: what it runs (`entry`) and what it adds to OpenRemote
//! (`contributes`: agent tools, composer inputs, views, settings).
//! Everything is checked when the plugin is read for review, so the review
//! shows exactly what the plugin declares and nothing it declares goes
//! unseen: a kind of contribution this OpenRemote doesn't know is refused
//! rather than ignored.
//!
//! Every path a manifest names - its program, its module, a view's page, a
//! composer button's page - is a file inside the plugin's own folder,
//! checked after every link is followed (`confined`). A harness is not a
//! contribution here: main's `harness` block (plugins.rs) stays the one
//! place a plugin declares its driver.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use openremote_core::{
    ComposerInputContribution, PluginContributions, PluginPower, PluginRuntime, SettingField,
    SettingKind, ToolContribution, ViewContribution,
};
use serde::Serialize;
use serde_json::Value;

use crate::plugins::{PluginError, relative_inside};

/// The placeholder a process plugin's command and arguments may use for
/// its own folder.
pub const ROOT_VAR: &str = "${OPENREMOTE_PLUGIN_ROOT}";

/// The MCP server name a chat's agent reaches plugin tools under (the
/// per-chat `openremote` server), and the longest full tool name a
/// harness takes (`mcp__<server>__<tool>`; OpenAI's and Anthropic's
/// function names stop at 64).
const MCP_SERVER: &str = "openremote";
const MAX_FULL_TOOL_NAME: usize = 64;

/// The places a plugin's view can show.
pub const VIEW_PLACES: &[&str] = &["sidebar", "machines", "settings"];

const CONTRIBUTION_KINDS: &[&str] = &["tools", "composer_inputs", "views", "settings"];

/// What a plugin runs: a file in its folder, or a program in its folder
/// with arguments.
#[derive(Clone, Serialize, Debug, PartialEq, Eq)]
#[serde(untagged)]
pub enum Entry {
    /// A WASM module, a page, or a program run as it is - a path inside
    /// the plugin's folder.
    File(String),
    /// A program in the plugin's folder and its arguments, with a
    /// per-system override where the plugin ships one (`windows`, `macos`,
    /// `linux`).
    Command {
        command: String,
        args: Vec<String>,
        #[serde(skip_serializing_if = "BTreeMap::is_empty")]
        per_system: BTreeMap<String, (String, Vec<String>)>,
    },
}

impl Entry {
    /// The command line in a line, as the review shows it.
    pub fn describe(&self) -> String {
        match self {
            Entry::File(path) => path.clone(),
            Entry::Command { .. } => {
                let (command, args) = self.for_this_system();
                std::iter::once(command)
                    .chain(args)
                    .map(|part| {
                        if part.contains(' ') {
                            format!("\"{part}\"")
                        } else {
                            part
                        }
                    })
                    .collect::<Vec<_>>()
                    .join(" ")
            }
        }
    }

    /// The program and arguments this system runs, before the plugin's
    /// folder is put in for `${OPENREMOTE_PLUGIN_ROOT}`.
    pub fn for_this_system(&self) -> (String, Vec<String>) {
        match self {
            Entry::File(path) => (path.clone(), Vec::new()),
            Entry::Command {
                command,
                args,
                per_system,
            } => per_system
                .get(this_system())
                .cloned()
                .unwrap_or_else(|| (command.clone(), args.clone())),
        }
    }

    /// The program to start and its arguments, resolved against the
    /// plugin's folder: the program is a file inside it (checked after
    /// every link is followed), and `${OPENREMOTE_PLUGIN_ROOT}` in an
    /// argument becomes the folder.
    pub fn resolve(&self, root: &Path) -> Result<(PathBuf, Vec<String>), String> {
        let (command, args) = self.for_this_system();
        let program = confined(root, &program_path(&command), "its program")?;
        let root_text = root.to_string_lossy();
        Ok((
            program,
            args.iter()
                .map(|a| a.replace(ROOT_VAR, &root_text))
                .collect(),
        ))
    }
}

/// The program's path inside the folder, as the manifest writes it: with
/// or without `${OPENREMOTE_PLUGIN_ROOT}/` in front.
fn program_path(command: &str) -> String {
    command
        .strip_prefix(ROOT_VAR)
        .map(|rest| rest.trim_start_matches(['/', '\\']))
        .unwrap_or(command)
        .trim_start_matches("./")
        .to_string()
}

/// The word for this operating system in an entry's overrides.
pub fn this_system() -> &'static str {
    if cfg!(windows) {
        "windows"
    } else if cfg!(target_os = "macos") {
        "macos"
    } else {
        "linux"
    }
}

/// A file a plugin names, confined to its folder: a relative path with
/// forward slashes and no `..`, that is a file, and that is still inside
/// the folder after every link is followed - so neither a `..` nor a link
/// that points elsewhere lets a plugin name a file outside itself. Answers
/// the file's real path.
pub fn confined(dir: &Path, path: &str, what: &str) -> Result<PathBuf, String> {
    let trimmed = path.trim_start_matches("./");
    if !relative_inside(trimmed) {
        return Err(format!(
            "{what} '{path}' must be a path inside the plugin's folder (forward slashes, no ..)"
        ));
    }
    let missing = || format!("{what} '{path}' is not in the plugin");
    let root = dir.canonicalize().map_err(|_| missing())?;
    let target = dir.join(trimmed).canonicalize().map_err(|_| missing())?;
    if !target.starts_with(&root) {
        return Err(format!("{what} '{path}' leads outside the plugin's folder"));
    }
    if !target.is_file() {
        return Err(missing());
    }
    Ok(target)
}

/// The checked OpenRemote part of a manifest.
#[derive(Debug, Default)]
pub struct Section {
    pub entry: Option<Entry>,
    /// Tools, composer inputs, views and settings. `harnesses` is left
    /// empty here: the caller fills it from the manifest's harness block.
    pub contributes: PluginContributions,
    /// Powers its contributions need, which it gets without naming them
    /// (and which the review shows like any other).
    pub implied: Vec<PluginPower>,
}

fn invalid(message: impl Into<String>) -> PluginError {
    PluginError::Invalid(message.into())
}

/// Lowercase words and dashes, a letter first.
fn kebab(s: &str, max: usize) -> bool {
    !s.is_empty()
        && s.len() <= max
        && s.starts_with(|c: char| c.is_ascii_lowercase())
        && s.chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

/// Lowercase words and underscores, a letter first.
fn snake(s: &str, max: usize) -> bool {
    !s.is_empty()
        && s.len() <= max
        && s.starts_with(|c: char| c.is_ascii_lowercase())
        && s.chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

fn inside_file(dir: &Path, path: &str, what: &str) -> Result<(), PluginError> {
    confined(dir, path, what).map(|_| ()).map_err(invalid)
}

/// The full name a chat's agent calls a plugin tool by.
pub fn agent_tool_name(plugin: &str, tool: &str) -> String {
    format!("{plugin}__{tool}")
}

/// Read and check `entry` and `contributes` from openremote.json. `name` is
/// the plugin's name.
pub fn read_section(
    raw: &Value,
    dir: &Path,
    name: &str,
    runtime: PluginRuntime,
) -> Result<Section, PluginError> {
    let contributes = read_contributes(raw.get("contributes"), dir, name, runtime)?;
    // What runs a plugin's contributions: a process plugin's program
    // answers its tools and its views' requests; a wasm plugin's module
    // answers its tools. A plugin that contributes none of those runs
    // nothing, and may name no entry (a harness plugin's driver is its
    // harness block's).
    let runs = match runtime {
        PluginRuntime::Process => {
            !(contributes.tools.is_empty()
                && contributes.views.is_empty()
                && contributes.composer_inputs.is_empty())
        }
        PluginRuntime::Wasm => !contributes.tools.is_empty(),
        _ => false,
    };
    let entry = read_entry(raw.get("entry"), dir, runtime, runs)?;
    let mut implied = Vec::new();
    if !contributes.tools.is_empty() {
        implied.push(PluginPower::AgentTools);
    }
    Ok(Section {
        entry,
        contributes,
        implied,
    })
}

fn read_entry(
    raw: Option<&Value>,
    dir: &Path,
    runtime: PluginRuntime,
    runs: bool,
) -> Result<Option<Entry>, PluginError> {
    let Some(raw) = raw.filter(|v| !v.is_null()) else {
        return match runtime {
            PluginRuntime::Process if runs => Err(invalid(
                "openremote.json names no entry: a process plugin names the program that runs what it contributes",
            )),
            PluginRuntime::Wasm if runs => Err(invalid(
                "openremote.json names no entry: a wasm plugin names the .wasm module that runs its tools",
            )),
            _ => Ok(None),
        };
    };
    if runtime == PluginRuntime::Content {
        return Err(invalid(
            "a content plugin runs nothing of its own, so it names no entry",
        ));
    }
    let entry = match raw {
        Value::String(path) => {
            inside_file(dir, path, "its entry")?;
            Entry::File(path.trim_start_matches("./").to_string())
        }
        Value::Object(fields) => {
            if runtime != PluginRuntime::Process {
                return Err(invalid(
                    "only a process plugin's entry is a command; a wasm or js plugin's entry is a file in its folder",
                ));
            }
            let command_of = |v: &Value, at: &str| -> Result<(String, Vec<String>), PluginError> {
                let command = v
                    .get("command")
                    .and_then(Value::as_str)
                    .filter(|c| !c.trim().is_empty())
                    .ok_or_else(|| invalid(format!("{at} names no command")))?
                    .to_string();
                let args = match v.get("args") {
                    None | Some(Value::Null) => Vec::new(),
                    Some(Value::Array(items)) => items
                        .iter()
                        .map(|a| {
                            a.as_str()
                                .map(String::from)
                                .ok_or_else(|| invalid(format!("{at}'s args must be strings")))
                        })
                        .collect::<Result<_, _>>()?,
                    Some(_) => return Err(invalid(format!("{at}'s args must be a list"))),
                };
                Ok((command, args))
            };
            for key in fields.keys() {
                if !matches!(
                    key.as_str(),
                    "command" | "args" | "windows" | "macos" | "linux"
                ) {
                    return Err(invalid(format!(
                        "its entry has '{key}'; an entry takes command, args, and per-system windows, macos or linux"
                    )));
                }
            }
            let (command, args) = command_of(raw, "its entry")?;
            let mut per_system = BTreeMap::new();
            for system in ["windows", "macos", "linux"] {
                if let Some(v) = fields.get(system) {
                    per_system.insert(
                        system.to_string(),
                        command_of(v, &format!("its entry for {system}"))?,
                    );
                }
            }
            let entry = Entry::Command {
                command,
                args,
                per_system,
            };
            // A plugin runs programs from its own folder, as a harness
            // driver does: the program for this system must be a file
            // there. One on PATH (node, python) is not the plugin's.
            let (command, _) = entry.for_this_system();
            confined(dir, &program_path(&command), "its program").map_err(|why| {
                invalid(format!(
                    "{why}; a process plugin runs a program from its own folder"
                ))
            })?;
            entry
        }
        _ => {
            return Err(invalid(
                "its entry is a path in its folder, or {\"command\": ..., \"args\": [...]}",
            ));
        }
    };
    if runtime == PluginRuntime::Wasm
        && !matches!(&entry, Entry::File(path) if path.ends_with(".wasm"))
    {
        return Err(invalid("a wasm plugin's entry is its .wasm module"));
    }
    Ok(Some(entry))
}

/// A list under `contributes`, read item by item so a mistake names its
/// place.
fn list<T: serde::de::DeserializeOwned>(raw: &Value, kind: &str) -> Result<Vec<T>, PluginError> {
    let Some(items) = raw.as_array() else {
        return Err(invalid(format!("contributes.{kind} must be a list")));
    };
    items
        .iter()
        .enumerate()
        .map(|(i, item)| {
            serde_json::from_value(item.clone())
                .map_err(|e| invalid(format!("contributes.{kind}[{i}] is not valid: {e}")))
        })
        .collect()
}

fn read_contributes(
    raw: Option<&Value>,
    dir: &Path,
    name: &str,
    runtime: PluginRuntime,
) -> Result<PluginContributions, PluginError> {
    let Some(raw) = raw.filter(|v| !v.is_null()) else {
        return Ok(PluginContributions::default());
    };
    let Some(fields) = raw.as_object() else {
        return Err(invalid("contributes must be an object"));
    };
    for key in fields.keys() {
        if key == "harnesses" {
            return Err(invalid(
                "contributes.harnesses is not read: a plugin declares its harness in openremote.json's harness block",
            ));
        }
        if !CONTRIBUTION_KINDS.contains(&key.as_str()) {
            return Err(invalid(format!(
                "contributes.{key} is not something this OpenRemote knows; it takes {}",
                CONTRIBUTION_KINDS.join(", ")
            )));
        }
    }
    let mut out = PluginContributions::default();
    if let Some(raw) = fields.get("tools") {
        out.tools = list(raw, "tools")?;
    }
    if let Some(raw) = fields.get("composer_inputs") {
        out.composer_inputs = list(raw, "composer_inputs")?;
    }
    if let Some(raw) = fields.get("views") {
        out.views = list(raw, "views")?;
    }
    if let Some(raw) = fields.get("settings") {
        out.settings = list(&options_as_objects(raw), "settings")?;
    }
    check_tools(&out.tools, name, runtime)?;
    check_composer_inputs(&out.composer_inputs, dir, runtime)?;
    check_views(&out.views, dir)?;
    check_settings(&out.settings)?;
    Ok(out)
}

/// An enum's options may be plain strings: each is its own label.
fn options_as_objects(raw: &Value) -> Value {
    let mut raw = raw.clone();
    if let Some(fields) = raw.as_array_mut() {
        for field in fields {
            if let Some(options) = field.get_mut("options").and_then(Value::as_array_mut) {
                for option in options.iter_mut() {
                    if let Some(word) = option.as_str().map(String::from) {
                        *option = serde_json::json!({"value": word, "label": word});
                    }
                }
            }
        }
    }
    raw
}

fn unique<'a>(ids: impl Iterator<Item = &'a str>, what: &str) -> Result<(), PluginError> {
    let mut seen = BTreeSet::new();
    for id in ids {
        if !seen.insert(id) {
            return Err(invalid(format!("two {what} are named '{id}'")));
        }
    }
    Ok(())
}

fn check_tools(
    tools: &[ToolContribution],
    plugin: &str,
    runtime: PluginRuntime,
) -> Result<(), PluginError> {
    if tools.is_empty() {
        return Ok(());
    }
    if !matches!(runtime, PluginRuntime::Process | PluginRuntime::Wasm) {
        return Err(invalid(
            "agent tools run in a program or a WebAssembly module: only a process or wasm plugin can offer them",
        ));
    }
    unique(tools.iter().map(|t| t.name.as_str()), "tools")?;
    for t in tools {
        if !snake(&t.name, 48) {
            return Err(invalid(format!(
                "the tool name '{}' must be lowercase letters, digits and underscores, a letter first",
                t.name
            )));
        }
        let full = format!("mcp__{MCP_SERVER}__{}", agent_tool_name(plugin, &t.name));
        if full.len() > MAX_FULL_TOOL_NAME {
            return Err(invalid(format!(
                "an agent calls this tool {full}, longer than the {MAX_FULL_TOOL_NAME} characters harnesses take: shorten the plugin's or the tool's name"
            )));
        }
        if t.description.trim().is_empty() {
            return Err(invalid(format!(
                "the tool '{}' needs a description: it is how an agent knows when to call it",
                t.name
            )));
        }
        if t.input_schema.get("type").and_then(Value::as_str) != Some("object") {
            return Err(invalid(format!(
                "the tool '{}''s input_schema must be a JSON Schema of \"type\": \"object\"",
                t.name
            )));
        }
        if t.timeout_secs.is_some_and(|s| !(1..=600).contains(&s)) {
            return Err(invalid(format!(
                "the tool '{}''s timeout_secs must be between 1 and 600",
                t.name
            )));
        }
    }
    Ok(())
}

fn check_icon(icon: &Option<String>, what: &str) -> Result<(), PluginError> {
    if let Some(icon) = icon {
        let ok = icon.starts_with(|c: char| c.is_ascii_alphabetic())
            && icon.len() <= 48
            && icon.chars().all(|c| c.is_ascii_alphanumeric());
        if !ok {
            return Err(invalid(format!(
                "{what}'s icon '{icon}' must be an icon's name, like Microphone"
            )));
        }
    }
    Ok(())
}

fn check_composer_inputs(
    inputs: &[ComposerInputContribution],
    dir: &Path,
    runtime: PluginRuntime,
) -> Result<(), PluginError> {
    unique(inputs.iter().map(|i| i.id.as_str()), "composer inputs")?;
    for input in inputs {
        if !kebab(&input.id, 32) {
            return Err(invalid(format!(
                "the composer input id '{}' must be lowercase letters, digits and dashes",
                input.id
            )));
        }
        if input.label.trim().is_empty() || input.label.len() > 40 {
            return Err(invalid(format!(
                "the composer input '{}' needs a label of up to 40 characters",
                input.id
            )));
        }
        check_icon(&input.icon, &format!("the composer input '{}'", input.id))?;
        match &input.entry {
            Some(entry) => inside_file(dir, entry, &format!("the composer input '{}'", input.id))?,
            // A js plugin's own entry answers its buttons; anyone else's
            // button needs a page to run.
            None if runtime != PluginRuntime::Js => {
                return Err(invalid(format!(
                    "the composer input '{}' names no entry: the page its button runs",
                    input.id
                )));
            }
            None => {}
        }
    }
    Ok(())
}

fn check_views(views: &[ViewContribution], dir: &Path) -> Result<(), PluginError> {
    unique(views.iter().map(|v| v.id.as_str()), "views")?;
    for view in views {
        if !kebab(&view.id, 32) {
            return Err(invalid(format!(
                "the view id '{}' must be lowercase letters, digits and dashes",
                view.id
            )));
        }
        if view.title.trim().is_empty() || view.title.len() > 48 {
            return Err(invalid(format!(
                "the view '{}' needs a title of up to 48 characters",
                view.id
            )));
        }
        if !VIEW_PLACES.contains(&view.place.as_str()) {
            return Err(invalid(format!(
                "the view '{}' shows in '{}'; a view's place is {}",
                view.id,
                view.place,
                VIEW_PLACES.join(", ")
            )));
        }
        check_icon(&view.icon, &format!("the view '{}'", view.id))?;
        inside_file(dir, &view.entry, &format!("the view '{}'", view.id))?;
    }
    Ok(())
}

fn check_settings(fields: &[SettingField]) -> Result<(), PluginError> {
    unique(fields.iter().map(|f| f.key.as_str()), "settings")?;
    for f in fields {
        if !snake(&f.key, 64) {
            return Err(invalid(format!(
                "the setting '{}' must be named in lowercase letters, digits and underscores",
                f.key
            )));
        }
        if f.label.trim().is_empty() {
            return Err(invalid(format!("the setting '{}' needs a label", f.key)));
        }
        if let (Some(min), Some(max)) = (f.min, f.max) {
            if min > max {
                return Err(invalid(format!(
                    "the setting '{}' has a min above its max",
                    f.key
                )));
            }
        }
        if f.kind == SettingKind::Enum {
            if f.options.is_empty() {
                return Err(invalid(format!(
                    "the setting '{}' is a choice with no options",
                    f.key
                )));
            }
            unique(f.options.iter().map(|o| o.value.as_str()), "options")?;
        } else if !f.options.is_empty() {
            return Err(invalid(format!(
                "the setting '{}' has options, which only a choice (enum) takes",
                f.key
            )));
        }
        if let Some(default) = &f.default {
            if f.kind == SettingKind::Secret {
                return Err(invalid(format!(
                    "the setting '{}' is a secret, which has no default",
                    f.key
                )));
            }
            check_value(f, default)
                .map_err(|why| invalid(format!("the setting '{}''s default {why}", f.key)))?;
        }
        // A secret's sendTo is the text setting that holds the one address
        // it may go to: anything else would be a guard that guards nothing.
        if let Some(to) = &f.send_to {
            if f.kind != SettingKind::Secret {
                return Err(invalid(format!(
                    "the setting '{}' has a sendTo, which only a secret takes",
                    f.key
                )));
            }
            if !fields
                .iter()
                .any(|o| &o.key == to && o.kind == SettingKind::String)
            {
                return Err(invalid(format!(
                    "the setting '{}''s sendTo must name one of the plugin's text settings",
                    f.key
                )));
            }
        }
    }
    Ok(())
}

/// Whether a value fits a setting; the error says why not, in words that
/// follow the setting's name.
pub fn check_value(field: &SettingField, value: &Value) -> Result<(), String> {
    match field.kind {
        SettingKind::String | SettingKind::Secret => {
            let text = value.as_str().ok_or("must be text")?;
            if text.len() > 64 * 1024 {
                return Err("is too long".into());
            }
        }
        SettingKind::Boolean => {
            value.as_bool().ok_or("must be true or false")?;
        }
        SettingKind::Number => {
            let n = value.as_f64().ok_or("must be a number")?;
            if field.min.is_some_and(|min| n < min) || field.max.is_some_and(|max| n > max) {
                return Err(format!(
                    "must be between {} and {}",
                    field.min.map_or("any".into(), |m| m.to_string()),
                    field.max.map_or("any".into(), |m| m.to_string())
                ));
            }
        }
        SettingKind::Enum => {
            let word = value.as_str().ok_or("must be one of its options")?;
            if !field.options.iter().any(|o| o.value == word) {
                return Err(format!("'{word}' is not one of its options"));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn folder(files: &[&str]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        for path in files {
            let p = dir.path().join(path);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, "x").unwrap();
        }
        dir
    }

    #[test]
    fn a_process_plugin_with_tools_views_and_settings_reads_whole() {
        let dir = folder(&["bin/plugin", "ui/hosts.html"]);
        let raw = json!({
            "entry": {"command": "${OPENREMOTE_PLUGIN_ROOT}/bin/plugin", "args": ["--root", "${OPENREMOTE_PLUGIN_ROOT}"]},
            "contributes": {
                "tools": [{"name": "run_command", "description": "Run it", "input_schema": {"type": "object"}}],
                "views": [{"id": "hosts", "title": "Hosts", "place": "machines", "entry": "ui/hosts.html"}],
                "settings": [
                    {"key": "token", "type": "secret", "label": "Token", "required": true},
                    {"key": "region", "type": "enum", "label": "Region", "options": ["eu", "us"], "default": "eu"}
                ]
            }
        });
        let section = read_section(&raw, dir.path(), "ssh", PluginRuntime::Process).unwrap();
        assert!(section.contributes.harnesses.is_empty());
        assert_eq!(section.contributes.tools[0].name, "run_command");
        assert_eq!(section.contributes.settings[1].options[1].label, "us");
        assert_eq!(section.implied, vec![PluginPower::AgentTools]);
        let (program, args) = section.entry.unwrap().resolve(dir.path()).unwrap();
        assert_eq!(
            program,
            dir.path().join("bin/plugin").canonicalize().unwrap()
        );
        assert_eq!(args[0], "--root");
        assert!(!args[1].contains("${"));
    }

    #[test]
    fn a_secrets_send_to_names_a_text_setting_and_only_a_secret_has_one() {
        let dir = folder(&["ui/page.html"]);
        let read = |settings: Value| {
            let raw = json!({"entry": "ui/page.html", "contributes": {"settings": settings}});
            read_section(&raw, dir.path(), "dictation", PluginRuntime::Js)
        };
        let address = json!({"key": "address", "type": "string", "label": "Address"});
        let fine = read(json!([
            address,
            {"key": "api_key", "type": "secret", "label": "Key", "sendTo": "address"}
        ]))
        .unwrap();
        assert_eq!(
            fine.contributes.settings[1].send_to.as_deref(),
            Some("address")
        );
        for (settings, words) in [
            (
                json!([{"key": "api_key", "type": "secret", "label": "Key", "sendTo": "nowhere"}]),
                "sendTo",
            ),
            (
                json!([address, {"key": "flag", "type": "boolean", "label": "F", "sendTo": "address"}]),
                "only a secret",
            ),
            (
                json!([
                    {"key": "on", "type": "boolean", "label": "On"},
                    {"key": "api_key", "type": "secret", "label": "Key", "sendTo": "on"}
                ]),
                "sendTo",
            ),
        ] {
            let err = read(settings.clone()).unwrap_err().to_string();
            assert!(err.contains(words), "{settings}: {err}");
        }
    }

    #[test]
    fn a_process_plugins_program_is_a_file_in_its_own_folder() {
        let dir = folder(&["plugin.js"]);
        let tools = json!({"tools": [{"name": "t", "description": "d"}]});
        for command in [
            "node",
            "/usr/bin/node",
            "../outside",
            "C:/Windows/system32/cmd.exe",
        ] {
            let raw =
                json!({"entry": {"command": command, "args": ["plugin.js"]}, "contributes": tools});
            let err = read_section(&raw, dir.path(), "p", PluginRuntime::Process)
                .unwrap_err()
                .to_string();
            assert!(err.contains("its program"), "{command}: {err}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn a_link_that_leads_out_of_the_folder_is_refused() {
        let outside = folder(&["secret.html", "prog"]);
        let dir = folder(&[]);
        std::os::unix::fs::symlink(
            outside.path().join("secret.html"),
            dir.path().join("view.html"),
        )
        .unwrap();
        std::os::unix::fs::symlink(outside.path(), dir.path().join("bin")).unwrap();
        let err = confined(dir.path(), "view.html", "the view").unwrap_err();
        assert!(err.contains("outside"), "{err}");
        let err = confined(dir.path(), "bin/prog", "its program").unwrap_err();
        assert!(err.contains("outside"), "{err}");
    }

    /// Windows: a directory junction (which needs no privilege to make)
    /// that leads out of the folder is refused like a link.
    #[cfg(windows)]
    #[test]
    fn a_junction_that_leads_out_of_the_folder_is_refused() {
        let outside = folder(&["prog.exe"]);
        let dir = folder(&[]);
        let made = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(dir.path().join("bin"))
            .arg(outside.path())
            .output()
            .is_ok_and(|o| o.status.success());
        assert!(made, "mklink /J could not make a junction");
        assert!(dir.path().join("bin/prog.exe").is_file());
        let err = confined(dir.path(), "bin/prog.exe", "its program").unwrap_err();
        assert!(err.contains("outside"), "{err}");
    }

    #[test]
    fn mistakes_are_named() {
        let dir = folder(&["tool.wasm"]);
        let cases = [
            (
                json!({"contributes": {"tools": [{"name": "t", "description": "d"}]}}),
                PluginRuntime::Wasm,
                "names no entry",
            ),
            (
                json!({"entry": "../x.wasm"}),
                PluginRuntime::Wasm,
                "inside the plugin",
            ),
            (
                json!({"entry": "tool.wasm", "contributes": {"widgets": []}}),
                PluginRuntime::Wasm,
                "contributes.widgets",
            ),
            (
                json!({"entry": "tool.wasm", "contributes": {"harnesses": [{"id": "x", "name": "X"}]}}),
                PluginRuntime::Wasm,
                "harness block",
            ),
            (
                json!({"entry": "tool.wasm", "contributes": {"tools": [{"name": "Bad", "description": "d"}]}}),
                PluginRuntime::Wasm,
                "lowercase",
            ),
            (
                json!({"entry": "tool.wasm", "contributes": {"settings": [{"key": "k", "type": "secret", "label": "K", "default": "x"}]}}),
                PluginRuntime::Wasm,
                "no default",
            ),
            (
                json!({"entry": "tool.wasm", "contributes": {"tools": [{"name": "a_tool_with_a_rather_long_name_indeed", "description": "d"}]}}),
                PluginRuntime::Wasm,
                "longer than",
            ),
            (
                json!({"entry": "tool.wasm", "contributes": {"views": [{"id": "v", "title": "V", "place": "sidebar", "entry": "../../etc/passwd"}]}}),
                PluginRuntime::Wasm,
                "inside the plugin",
            ),
            (
                json!({"contributes": {"tools": [{"name": "t", "description": "d"}]}}),
                PluginRuntime::Content,
                "only a process or wasm plugin",
            ),
        ];
        for (raw, runtime, says) in cases {
            let err = read_section(&raw, dir.path(), "a-plugin-with-a-long-name", runtime)
                .unwrap_err()
                .to_string();
            assert!(err.contains(says), "{raw}: {err}");
        }
    }

    #[test]
    fn a_plugin_that_runs_nothing_names_no_entry() {
        let dir = folder(&[]);
        // A process plugin with only a harness (its driver is the harness
        // block's) or only settings runs nothing here.
        let raw =
            json!({"contributes": {"settings": [{"key": "k", "type": "string", "label": "K"}]}});
        let section = read_section(&raw, dir.path(), "p", PluginRuntime::Process).unwrap();
        assert!(section.entry.is_none());
        assert!(section.implied.is_empty());
    }
}
