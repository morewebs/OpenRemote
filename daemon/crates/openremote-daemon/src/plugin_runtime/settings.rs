//! The values a user sets for a plugin, kept on this machine only in
//! `<data dir>/plugin-settings/<plugin id>.json`, a file only this user
//! may read (as the API token is). Secrets live there too and leave it in
//! one direction only: to the plugin, when it starts or runs. The API
//! answers whether a secret is set, never what it is, and nothing here is
//! ever logged.

use std::path::{Path, PathBuf};

use openremote_core::{SettingField, SettingKind};
use serde_json::{Map, Value, json};

use super::manifest::check_value;

pub struct SettingsStore {
    dir: PathBuf,
}

impl SettingsStore {
    pub fn new(data_dir: &Path) -> Self {
        Self {
            dir: data_dir.join("plugin-settings"),
        }
    }

    /// The file a plugin's values live in, always directly inside the
    /// settings folder. A plugin id is `name@marketplace` (`~project` for a
    /// project install): lowercase letters, digits, '-', '.', '_', '@' and
    /// '~'. Anything else - a separator, a `..`, a drive - names no file,
    /// so no id reaches outside the folder.
    fn path(&self, id: &str) -> Option<PathBuf> {
        let safe = !id.is_empty()
            && id.len() <= 200
            && !id.contains("..")
            && !id.starts_with('.')
            && id.chars().all(|c| {
                c.is_ascii_lowercase()
                    || c.is_ascii_uppercase()
                    || c.is_ascii_digit()
                    || matches!(c, '-' | '.' | '_' | '@' | '~')
            });
        safe.then(|| self.dir.join(format!("{id}.json")))
    }

    /// The stored values, secrets included. Empty when nothing was set.
    pub fn load(&self, id: &str) -> Map<String, Value> {
        self.path(id)
            .and_then(|path| std::fs::read(path).ok())
            .and_then(|bytes| serde_json::from_slice::<Map<String, Value>>(&bytes).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, id: &str, values: &Map<String, Value>) -> std::io::Result<()> {
        let path = self.path(id).ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::InvalidInput, "not a plugin id")
        })?;
        openremote_core::fsx::create_private_dir(&self.dir)?;
        let bytes = serde_json::to_vec_pretty(values).expect("settings serialize");
        openremote_core::fsx::write_private(&path, &bytes)
    }

    /// A removed plugin's values go with it - its secrets included.
    pub fn forget(&self, id: &str) {
        if let Some(path) = self.path(id) {
            let _ = std::fs::remove_file(path);
        }
    }
}

/// The settings as the console reads them: every plain value (what was
/// set, else the default, else null) and, for each secret, only whether
/// it is set.
pub fn view(id: &str, schema: &[SettingField], stored: &Map<String, Value>) -> Value {
    let mut values = Map::new();
    let mut secrets = Map::new();
    for field in schema {
        if field.kind == SettingKind::Secret {
            let set = stored
                .get(&field.key)
                .and_then(Value::as_str)
                .is_some_and(|s| !s.is_empty());
            secrets.insert(
                field.key.clone(),
                Value::String(if set { "set" } else { "unset" }.into()),
            );
        } else {
            values.insert(
                field.key.clone(),
                stored
                    .get(&field.key)
                    .cloned()
                    .or_else(|| field.default.clone())
                    .unwrap_or(Value::Null),
            );
        }
    }
    json!({
        "id": id,
        "schema": schema,
        "values": values,
        "secrets": secrets,
        "missing": missing_required(schema, stored),
    })
}

/// Apply a change: each key present is set, a null puts it back to its
/// default (a secret, to unset). Keys the plugin doesn't declare, and
/// values that don't fit, are refused whole - nothing is half-applied.
pub fn apply(
    schema: &[SettingField],
    stored: &mut Map<String, Value>,
    changes: &Map<String, Value>,
) -> Result<(), String> {
    let mut next = stored.clone();
    for (key, value) in changes {
        let Some(field) = schema.iter().find(|f| &f.key == key) else {
            return Err(format!("the plugin has no setting named {key}"));
        };
        if value.is_null() {
            next.remove(key);
            continue;
        }
        check_value(field, value).map_err(|why| format!("{} {why}", field.label))?;
        next.insert(key.clone(), value.clone());
    }
    *stored = next;
    Ok(())
}

/// What the plugin is handed: every declared value - set, else default -
/// secrets included. Undeclared leftovers (from an older version) stay
/// out.
pub fn effective(schema: &[SettingField], stored: &Map<String, Value>) -> Map<String, Value> {
    let mut out = Map::new();
    for field in schema {
        if let Some(value) = stored.get(&field.key).cloned().or(field.default.clone()) {
            out.insert(field.key.clone(), value);
        }
    }
    out
}

/// The labels of required settings still unset: the plugin doesn't run
/// until they are.
pub fn missing_required(schema: &[SettingField], stored: &Map<String, Value>) -> Vec<String> {
    schema
        .iter()
        .filter(|f| f.required)
        .filter(|f| {
            let value = stored.get(&f.key).or(f.default.as_ref());
            match value {
                None | Some(Value::Null) => true,
                Some(Value::String(s)) => s.is_empty(),
                Some(_) => false,
            }
        })
        .map(|f| f.label.clone())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use openremote_core::SettingOption;

    fn field(key: &str, kind: SettingKind) -> SettingField {
        SettingField {
            key: key.into(),
            kind,
            label: key.to_uppercase(),
            help: None,
            default: None,
            options: Vec::new(),
            min: None,
            max: None,
            required: false,
            send_to: None,
        }
    }

    #[test]
    fn a_secret_is_answered_as_set_never_its_value() {
        let mut token = field("token", SettingKind::Secret);
        token.required = true;
        let mut region = field("region", SettingKind::Enum);
        region.options = vec![SettingOption {
            value: "eu".into(),
            label: "EU".into(),
        }];
        region.default = Some(json!("eu"));
        let schema = vec![token, region];
        let mut stored = Map::new();
        assert_eq!(missing_required(&schema, &stored), vec!["TOKEN"]);
        let changes = json!({"token": "s3cret-value"});
        apply(&schema, &mut stored, changes.as_object().unwrap()).unwrap();
        let shown = view("p@local", &schema, &stored).to_string();
        assert!(!shown.contains("s3cret-value"), "{shown}");
        assert!(shown.contains("\"token\":\"set\""), "{shown}");
        assert!(missing_required(&schema, &stored).is_empty());
        // The plugin is handed the secret and the default.
        let handed = effective(&schema, &stored);
        assert_eq!(handed["token"], "s3cret-value");
        assert_eq!(handed["region"], "eu");
        // An undeclared key, or a value that doesn't fit, is refused whole.
        let bad = json!({"token": "other", "nope": 1});
        assert!(apply(&schema, &mut stored, bad.as_object().unwrap()).is_err());
        assert_eq!(stored["token"], "s3cret-value");
        let bad = json!({"region": "mars"});
        assert!(apply(&schema, &mut stored, bad.as_object().unwrap()).is_err());
    }

    #[test]
    fn an_id_never_names_a_file_outside_the_settings_folder() {
        let data = tempfile::tempdir().unwrap();
        let store = SettingsStore::new(data.path());
        let values = json!({"k": "v"}).as_object().unwrap().clone();
        store.save("tool@local", &values).unwrap();
        assert_eq!(store.load("tool@local")["k"], "v");
        for id in [
            "../escape",
            "..\\escape",
            "a/b@local",
            "C:x",
            "",
            ".hidden",
            "x@local/../../y",
        ] {
            assert!(store.path(id).is_none(), "{id}");
            assert!(store.save(id, &values).is_err(), "{id}");
            assert!(store.load(id).is_empty(), "{id}");
        }
        assert!(!data.path().join("escape.json").exists());
        store.forget("tool@local");
        assert!(store.load("tool@local").is_empty());
    }
}
