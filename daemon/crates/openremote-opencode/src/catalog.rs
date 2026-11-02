//! OpenCode's model catalog: a short-lived `opencode serve`, then
//! `GET /api/model`. v2 wraps the list as `{data: Model.Info[]}`.
//! Each pick word is `providerID/id`, the shape the session body takes.

use std::process::Stdio;
use std::time::Duration;

use openremote_harness::{DriverError, ModelDescriptor, Resolution};
use serde_json::Value;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use uuid::Uuid;

use crate::listen::parse_listen_line;

pub async fn models(resolution: &Resolution) -> Result<Vec<ModelDescriptor>, DriverError> {
    let mut command = match resolution {
        Resolution::Executable(path) => Command::new(path),
        Resolution::NodeScript { node, script } => {
            let mut c = Command::new(node);
            c.arg(script);
            c
        }
        Resolution::Unavailable => {
            return Err(DriverError::Spawn("opencode CLI not found".to_string()));
        }
    };
    let password = Uuid::new_v4().to_string();
    command
        .args(["serve", "--port", "0", "--hostname", "127.0.0.1"])
        .env("OPENCODE_SERVER_PASSWORD", &password)
        .current_dir(std::env::temp_dir())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    #[cfg(windows)]
    command.creation_flags(openremote_harness::creation_flags());
    let mut child = command
        .spawn()
        .map_err(|e| DriverError::Spawn(format!("opencode models: {e}")))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| DriverError::Spawn("stdout not piped".into()))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| DriverError::Spawn("stderr not piped".into()))?;
    let ask = async {
        let mut lines = BufReader::new(stdout).lines();
        let port = loop {
            let Some(line) = lines.next_line().await? else {
                let detail = crate::driver::stderr_excerpt(stderr).await;
                return Err(if detail.is_empty() {
                    DriverError::Protocol("opencode exited before listening".into())
                } else {
                    DriverError::Protocol(format!("opencode exited before listening: {detail}"))
                });
            };
            if let Some((_, port)) = parse_listen_line(&line) {
                break port;
            }
        };
        let client = crate::http::HttpClient::new(port, &password);
        // v2 is `/api/model`. v1 served the same list at `/model`.
        let listed = match client.get("/api/model").await {
            Ok(value) => value,
            Err(_) => client.get("/model").await?,
        };
        Ok::<_, DriverError>(descriptors(&listed))
    };
    let answer = tokio::time::timeout(Duration::from_secs(30), ask).await;
    let _ = child.start_kill();
    match answer {
        Ok(result) => result,
        Err(_) => Err(DriverError::Protocol(
            "opencode did not list models in time".into(),
        )),
    }
}

/// `providerID/id` is the pick word. The session reports `id` back, so
/// that spelling is `resolved_model` and the chip still finds the row.
pub(crate) fn descriptors(value: &Value) -> Vec<ModelDescriptor> {
    let rows = value
        .get("data")
        .and_then(Value::as_array)
        .or_else(|| value.as_array());
    let Some(rows) = rows else {
        return Vec::new();
    };
    rows.iter()
        .filter(|row| row.get("enabled").and_then(Value::as_bool) != Some(false))
        .filter_map(|row| {
            let id = row.get("id").and_then(Value::as_str)?.trim();
            if id.is_empty() {
                return None;
            }
            let provider = row
                .get("providerID")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
                .unwrap_or("opencode");
            let model = if id.contains('/') {
                id.to_string()
            } else {
                format!("{provider}/{id}")
            };
            let reasoning_efforts = row
                .get("variants")
                .and_then(Value::as_array)
                .map(|variants| {
                    variants
                        .iter()
                        .filter_map(|variant| {
                            variant
                                .get("id")
                                .and_then(Value::as_str)
                                .filter(|s| !s.is_empty())
                                .map(str::to_string)
                        })
                        .collect()
                })
                .unwrap_or_default();
            Some(ModelDescriptor {
                model,
                display_name: row
                    .get("name")
                    .and_then(Value::as_str)
                    .filter(|s| !s.is_empty())
                    .map(str::to_string),
                resolved_model: Some(id.to_string()),
                reasoning_efforts,
                is_default: false,
                // OpenCode's catalog has no second line or notice to show.
                detail: None,
                note: None,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn v2_catalog_uses_provider_and_id() {
        let listed = json!({
            "data": [
                {"id": "flint-40b-free", "providerID": "opencode", "name": "Flint 40B Free", "enabled": true},
                {"id": "hidden", "providerID": "opencode", "name": "Hidden", "enabled": false}
            ]
        });
        let rows = descriptors(&listed);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].model, "opencode/flint-40b-free");
        assert_eq!(rows[0].resolved_model.as_deref(), Some("flint-40b-free"));
        assert_eq!(rows[0].display_name.as_deref(), Some("Flint 40B Free"));
    }
}
