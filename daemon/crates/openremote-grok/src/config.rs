//! Grok's own model facts, read from its own files - never a process.
//!
//! - Catalog: `~/.grok/models_cache.json` - grok's cache of its models
//!   endpoint (it refetches; we read). Only `hidden: false` entries that
//!   are `supported_in_api` - the same set its own UI lists.
//! - Current default: `~/.grok/config.toml` `[models]` (`default`,
//!   `default_reasoning_effort`) - grok's own words for what a fresh chat
//!   runs. The wire's first-turn `system/init` replaces it with the truth.

use std::path::PathBuf;

use openremote_harness::ModelDescriptor;
use serde_json::Value;

fn grok_home() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
        .and_then(|home| {
            let dir = home.join(".grok");
            dir.is_dir().then_some(dir)
        })
}

/// The catalog grok itself serves from (its cache of its models endpoint,
/// its own names and its own effort words).
pub fn catalog() -> Vec<ModelDescriptor> {
    let Some(home) = grok_home() else {
        return Vec::new();
    };
    let Ok(text) = std::fs::read_to_string(home.join("models_cache.json")) else {
        return Vec::new();
    };
    let Ok(cache) = serde_json::from_str::<Value>(&text) else {
        return Vec::new();
    };
    let Some(models) = cache.get("models").and_then(Value::as_object) else {
        return Vec::new();
    };
    // grok's own id sort (the keys) keeps the order stable.
    let mut ids: Vec<&String> = models.keys().collect();
    ids.sort();
    ids.into_iter()
        .filter_map(|id| {
            let info = models[id].get("info")?;
            if info.get("hidden").and_then(Value::as_bool) == Some(true) {
                return None;
            }
            if info.get("supported_in_api").and_then(Value::as_bool) == Some(false) {
                return None;
            }
            let name = info
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or(id.as_str());
            let efforts = info
                .get("reasoning_efforts")
                .and_then(Value::as_array)
                .map(|list| {
                    list.iter()
                        .filter_map(|e| {
                            e.get("value")
                                .or_else(|| e.get("id"))
                                .and_then(Value::as_str)
                                .map(String::from)
                        })
                        .collect()
                })
                .unwrap_or_default();
            Some(ModelDescriptor {
                model: id.clone(),
                display_name: Some(name.to_string()),
                reasoning_efforts: efforts,
                is_default: false,
            })
        })
        .collect()
}

/// The model (and its own effort word) grok runs on a fresh chat, from its
/// own config. `None` when grok hasn't said.
pub fn default_model() -> Option<(String, Option<String>)> {
    let home = grok_home()?;
    let text = std::fs::read_to_string(home.join("config.toml")).ok()?;
    // A hand-rolled read of the `[models]` table only - no toml dependency
    // for two keys.
    let mut in_models = false;
    let mut default = None;
    let mut effort = None;
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            in_models = trimmed == "[models]";
            continue;
        }
        if !in_models {
            continue;
        }
        if let Some((key, value)) = trimmed.split_once('=') {
            let key = key.trim();
            let value = value.trim().trim_matches('"');
            match key {
                "default" => default = Some(value.to_string()),
                "default_reasoning_effort" => effort = Some(value.to_string()),
                _ => {}
            }
        }
    }
    default.map(|model| (model, effort))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_takes_groks_own_words_only() {
        let models = catalog();
        // This machine's cache: grok-4.7 et al., all visible.
        assert!(!models.is_empty());
        assert!(
            models
                .iter()
                .any(|m| m.model == "grok-4.7" && m.display_name.as_deref() == Some("Grok 4.7"))
        );
        // The effort words ride along - grok's own.
        let grok47 = models.iter().find(|m| m.model == "grok-4.7").unwrap();
        assert!(grok47.reasoning_efforts.contains(&"xhigh".to_string()));
    }

    #[test]
    fn default_model_comes_from_the_models_table() {
        // Machine-dependent; the parse itself is pinned below.
        if let Some((model, effort)) = default_model() {
            assert!(!model.is_empty());
            assert!(effort.is_none() || !effort.unwrap().is_empty());
        }
    }

    #[test]
    fn toml_models_table_parse_pins_the_two_keys() {
        // The [models] table only - a same-named key in another table never
        // leaks in, and an absent table stays silent.
        let text = "[marketplace]\ndefault = \"wrong\"\n\n[models]\ndefault = \"grok-4.7\"\ndefault_reasoning_effort = \"xhigh\"\n";
        let mut in_models = false;
        let mut default = None;
        let mut effort = None;
        for line in text.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with('[') {
                in_models = trimmed == "[models]";
                continue;
            }
            if !in_models {
                continue;
            }
            if let Some((key, value)) = trimmed.split_once('=') {
                let value = value.trim().trim_matches('"');
                match key.trim() {
                    "default" => default = Some(value.to_string()),
                    "default_reasoning_effort" => effort = Some(value.to_string()),
                    _ => {}
                }
            }
        }
        assert_eq!(default.as_deref(), Some("grok-4.7"));
        assert_eq!(effort.as_deref(), Some("xhigh"));
    }

    #[test]
    fn catalog_skips_hidden_and_unsupported_entries() {
        let cache: Value = serde_json::from_str(
            r#"{"models":{
                "a":{"info":{"id":"a","name":"A","hidden":false,"supported_in_api":true}},
                "b":{"info":{"id":"b","name":"B","hidden":true,"supported_in_api":true}},
                "c":{"info":{"id":"c","name":"C","hidden":false,"supported_in_api":false}}
            }}"#,
        )
        .unwrap();
        let models = cache.get("models").unwrap().as_object().unwrap();
        let listed: Vec<&String> = models
            .keys()
            .filter(|id| {
                let info = models[*id].get("info").unwrap();
                info.get("hidden").and_then(Value::as_bool) != Some(true)
                    && info.get("supported_in_api").and_then(Value::as_bool) != Some(false)
            })
            .collect();
        assert_eq!(listed, vec!["a"]);
    }
}
