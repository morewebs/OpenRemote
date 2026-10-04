//! Claude's own model resolution, read the way claude itself reads it -
//! claude's documented priority (its headless docs): the `--model` we pass
//! → `ANTHROPIC_MODEL` → the `model` key in `~/.claude/settings.json` →
//! `ANTHROPIC_DEFAULT_MODEL`. The pick is the caller's rung; this reads
//! the rest. The first turn's `system/init` replaces whatever this says
//! with the wire's truth.

use std::path::PathBuf;

/// The configured model, claude's own priority order. `None` when every
/// rung is silent - claude's built-in default then governs, and nothing
/// honest can be shown pre-send.
pub fn configured_model(picked: Option<&str>) -> Option<String> {
    if let Some(model) = picked {
        return Some(model.to_string());
    }
    if let Some(model) = std::env::var_os("ANTHROPIC_MODEL") {
        if !model.is_empty() {
            return Some(model.to_string_lossy().into_owned());
        }
    }
    if let Some(model) = settings_model() {
        return Some(model);
    }
    if let Some(model) = std::env::var_os("ANTHROPIC_DEFAULT_MODEL") {
        if !model.is_empty() {
            return Some(model.to_string_lossy().into_owned());
        }
    }
    None
}

/// The top-level `model` key in `~/.claude/settings.json` - one key, never
/// the whole file.
fn settings_model() -> Option<String> {
    let home = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME"))?;
    let text = std::fs::read_to_string(PathBuf::from(home).join(".claude").join("settings.json"))
        .ok()?;
    let value = serde_json::from_str::<serde_json::Value>(&text).ok()?;
    value
        .get("model")
        .and_then(|v| v.as_str())
        .filter(|m| !m.is_empty())
        .map(String::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_pick_wins_over_every_rung_below() {
        // The caller's own pick is the first rung - config never overrides it.
        assert_eq!(
            configured_model(Some("opus")),
            Some("opus".to_string())
        );
    }

    #[test]
    fn settings_json_model_is_read_when_no_pick() {
        // Machine-dependent; the pin is that a present key resolves and an
        // absent one doesn't error.
        let model = settings_model();
        if let Some(m) = model {
            assert!(!m.is_empty());
        }
    }
}
