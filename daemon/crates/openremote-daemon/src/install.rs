//! Harness installs on this machine. The catalog is the harnesses whose
//! install is a plain npm package — the layouts each crate's resolver
//! verified (`%APPDATA%\npm\node_modules\@anthropic-ai\claude-code`, …).
//! Grok and Antigravity install through their own roots (`~/.grok/bin`,
//! `~/.local/bin/agy`), so they carry no install row: nothing honest to
//! show, nothing honest to run.

use std::time::Duration;

use openremote_core::InstallSpec;

/// (harness id, display name, npm package) — the harness's own names.
const SPECS: &[(&str, &str, &str)] = &[
    ("claude", "Claude Code", "@anthropic-ai/claude-code"),
    ("codex", "Codex", "@openai/codex"),
    ("pi", "Pi Agent", "@earendil-works/pi-coding-agent"),
    ("opencode", "OpenCode", "opencode-ai"),
];

/// The command shown and run, verbatim.
pub fn command_for(harness_id: &str) -> Option<String> {
    SPECS
        .iter()
        .find(|(id, _, _)| *id == harness_id)
        .map(|(_, _, package)| format!("npm install -g {package}"))
}

/// The install rows for a machine: every catalog harness that isn't
/// already installed there.
pub fn installable(installed: &[String]) -> Vec<InstallSpec> {
    SPECS
        .iter()
        .filter(|(id, _, _)| !installed.iter().any(|i| i == id))
        .map(|(id, name, package)| InstallSpec {
            harness_id: id.to_string(),
            name: name.to_string(),
            command: format!("npm install -g {package}"),
        })
        .collect()
}

/// Run the install for one harness. `OPENREMOTE_INSTALL_NPM_CMD` replaces
/// the whole command (e2e points it at a script that materializes the
/// harness binary); production runs npm itself. Ten minutes, then it's
/// failed — npm hangs, not us.
pub async fn install(harness_id: &str) -> Result<String, String> {
    let custom = std::env::var("OPENREMOTE_INSTALL_NPM_CMD").ok();
    install_with(harness_id, custom.as_deref()).await
}

/// The install runner itself — the command override rides in as a
/// parameter so tests never mutate process env.
pub async fn install_with(harness_id: &str, command: Option<&str>) -> Result<String, String> {
    let Some(package) = SPECS
        .iter()
        .find(|(id, _, _)| *id == harness_id)
        .map(|(_, _, package)| *package)
    else {
        return Err(format!("'{harness_id}' has no install command"));
    };

    let run: std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<String, String>> + Send + '_>,
    > = if let Some(command) = command {
        Box::pin(run_custom(command))
    } else {
        Box::pin(run_npm(package))
    };
    let output = match tokio::time::timeout(Duration::from_secs(600), run).await {
        Ok(output) => output,
        Err(_) => return Err("npm install timed out after ten minutes".to_string()),
    };
    match output {
        Ok(text) => Ok(text),
        Err(failed) => Err(format!("npm install failed: {failed}")),
    }
}

async fn run_npm(package: &str) -> Result<String, String> {
    let output = if cfg!(windows) {
        // npm is a `npm.cmd` shim on Windows — CreateProcess can't run
        // shims, so route through cmd. No stdin rides with it.
        tokio::process::Command::new("cmd")
            .args(["/c", "npm", "install", "-g", package])
            .stdin(std::process::Stdio::null())
            .output()
            .await
    } else {
        tokio::process::Command::new("npm")
            .args(["install", "-g", package])
            .stdin(std::process::Stdio::null())
            .output()
            .await
    };
    finish(output)
}

async fn run_custom(command: &str) -> Result<String, String> {
    let mut parts = command.split_whitespace();
    let program = parts.next().ok_or("empty install command")?;
    let output = tokio::process::Command::new(program)
        .args(parts)
        .stdin(std::process::Stdio::null())
        .output()
        .await;
    finish(output)
}

fn finish(output: std::io::Result<std::process::Output>) -> Result<String, String> {
    let output = output.map_err(|e| e.to_string())?;
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
    .trim()
    .to_string();
    if output.status.success() {
        Ok(if text.is_empty() {
            "done".to_string()
        } else {
            text
        })
    } else {
        Err(if text.is_empty() {
            format!("exit {}", output.status)
        } else {
            text
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_catalog_names_harnesses_in_their_own_words() {
        assert_eq!(
            command_for("claude").as_deref(),
            Some("npm install -g @anthropic-ai/claude-code")
        );
        assert_eq!(
            command_for("opencode").as_deref(),
            Some("npm install -g opencode-ai")
        );
        // Grok and Antigravity install through their own roots — no row.
        assert_eq!(command_for("grok"), None);
        assert_eq!(command_for("agy"), None);
    }

    #[test]
    fn install_rows_skip_what_is_already_installed() {
        let rows = installable(&["claude".to_string(), "grok".to_string()]);
        let ids: Vec<&str> = rows.iter().map(|r| r.harness_id.as_str()).collect();
        assert_eq!(ids, ["codex", "pi", "opencode"]);
    }

    #[tokio::test]
    async fn the_runner_runs_a_command_override_and_reports_failure() {
        // The honest custom command: echo is a cmd builtin on Windows.
        let echo = if cfg!(windows) {
            "cmd /c echo install"
        } else {
            "echo install"
        };
        let out = install_with("codex", Some(echo)).await.unwrap();
        assert!(out.contains("install"), "got: {out}");

        // A failing command surfaces its output, not a generic error.
        let fail = if cfg!(windows) {
            "cmd /c exit 3"
        } else {
            "false"
        };
        let err = install_with("codex", Some(fail)).await.unwrap_err();
        assert!(err.starts_with("npm install failed:"), "got: {err}");

        // No harness in the catalog: an explicit refusal, not a run.
        assert!(install_with("grok", Some(echo)).await.is_err());
    }
}
