//! Antigravity's own current-model fact: the `model` key in
//! `~/.gemini/antigravity-cli/settings.json` - the same source its own
//! banner shows (observed live, agy 1.2.14: `"Gemini 3.8 Flash (High)"`).
//! The display name carries the effort inside it; it rides verbatim, never
//! decomposed.

use std::path::PathBuf;

/// The model agy runs on a fresh conversation, its own words. `None` when
/// agy hasn't said.
pub fn default_model() -> Option<String> {
    let home = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME"))?;
    let text = std::fs::read_to_string(
        PathBuf::from(home)
            .join(".gemini")
            .join("antigravity-cli")
            .join("settings.json"),
    )
    .ok()?;
    serde_json::from_str::<serde_json::Value>(&text)
        .ok()?
        .get("model")
        .and_then(|v| v.as_str())
        .filter(|m| !m.is_empty())
        .map(String::from)
}

#[cfg(test)]
mod tests {
    #[test]
    fn agy_settings_carry_the_banner_words_or_nothing() {
        // Machine-dependent; the pin is shape-only: a value, or None - never
        // an error, never a guess.
        if let Some(model) = super::default_model() {
            assert!(!model.is_empty());
        }
    }
}
