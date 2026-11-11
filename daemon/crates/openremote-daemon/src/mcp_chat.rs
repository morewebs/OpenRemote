//! `openremote-daemon mcp-chat`: OpenRemote's MCP server for one chat - its
//! read-a-chat tool (the first of OpenRemote Controls) and the built-in
//! browser's tools. A chat's harness starts it over stdio (MCP's
//! newline-delimited JSON-RPC). It reads the API token from the data
//! folder, never from its command line, and asks the daemon on loopback:
//! for a chat the user pointed this chat at with @ (the daemon refuses any
//! other), and for the browser, which the daemon reaches in the app.

use std::io;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

/// The MCP revision this server speaks when the client names none.
const PROTOCOL: &str = "2025-06-18";

/// The daemon's current port, in its data folder. Written by the daemon with
/// `fsx::write_private` (owner-only, atomic) and removed on a clean shutdown, the same
/// treatment the token beside it gets.
pub const PORT_FILE: &str = "mcp-port";

struct Config {
    session: String,
    port: u16,
    data_dir: PathBuf,
}

/// Records the port the daemon is serving on, so every running chat's server can find it
/// after a restart moved it.
pub fn write_port_file(data_dir: &Path, port: u16) -> io::Result<()> {
    openremote_core::fsx::write_private(&data_dir.join(PORT_FILE), port.to_string().as_bytes())
}

/// Removes it on a clean shutdown: the next start writes the next port.
pub fn remove_port_file(data_dir: &Path) {
    let _ = std::fs::remove_file(data_dir.join(PORT_FILE));
}

fn parse(args: &[String]) -> Option<Config> {
    let value = |flag: &str| {
        args.iter()
            .position(|a| a == flag)
            .and_then(|i| args.get(i + 1))
            .cloned()
    };
    Some(Config {
        session: value("--session")?,
        port: value("--port")?.parse().ok()?,
        data_dir: PathBuf::from(value("--data-dir")?),
    })
}

pub async fn run(args: &[String]) -> ExitCode {
    let Some(config) = parse(args) else {
        eprintln!(
            "usage: openremote-daemon mcp-chat --session <id> --port <port> --data-dir <dir>"
        );
        return ExitCode::from(2);
    };
    let token = match std::fs::read_to_string(config.data_dir.join("token")) {
        Ok(token) => token.trim().to_string(),
        Err(e) => {
            eprintln!("openremote-daemon mcp-chat: no token: {e}");
            return ExitCode::FAILURE;
        }
    };
    let mut lines = BufReader::new(tokio::io::stdin()).lines();
    let mut stdout = tokio::io::stdout();
    while let Ok(Some(line)) = lines.next_line().await {
        let Ok(message) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        // A notification has no id and wants no answer.
        let Some(id) = message.get("id").cloned() else {
            continue;
        };
        let method = message.get("method").and_then(Value::as_str).unwrap_or("");
        let params = message.get("params").cloned().unwrap_or(Value::Null);
        let reply = match method {
            "initialize" => Ok(json!({
                "protocolVersion": params
                    .get("protocolVersion")
                    .and_then(Value::as_str)
                    .unwrap_or(PROTOCOL),
                "capabilities": {"tools": {}},
                "serverInfo": {"name": "openremote", "version": env!("CARGO_PKG_VERSION")},
            })),
            "ping" => Ok(json!({})),
            "tools/list" => {
                let mut tools = vec![tool()];
                tools.extend(browser_tools(&config, &token).await);
                tools.extend(agent_tools(&config, &token).await);
                Ok(json!({"tools": tools}))
            }
            "tools/call" => Ok(call(&config, &token, &params).await),
            other => Err(json!({"code": -32601, "message": format!("no method {other}")})),
        };
        let frame = match reply {
            Ok(result) => json!({"jsonrpc": "2.0", "id": id, "result": result}),
            Err(error) => json!({"jsonrpc": "2.0", "id": id, "error": error}),
        };
        let mut out = serde_json::to_string(&frame).expect("a frame serializes");
        out.push('\n');
        if stdout.write_all(out.as_bytes()).await.is_err() || stdout.flush().await.is_err() {
            break;
        }
    }
    ExitCode::SUCCESS
}

fn tool() -> Value {
    json!({
        "name": "read_chat",
        "description": "Read an OpenRemote chat the user pointed you at with @ (they appear in their message as @chat followed by the chat's title and id). Answers the chat's messages, oldest first. Only chats the user mentioned in this chat can be read.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "chat": {"type": "string", "description": "The chat's id, or at least its first 6 characters."}
            },
            "required": ["chat"],
        },
    })
}

/// The browser's tools, as the daemon offers them: none where no app hosts
/// a browser.
async fn browser_tools(config: &Config, token: &str) -> Vec<Value> {
    let url = format!("http://127.0.0.1:{}/browser/tools", config.port);
    let bearer = format!("Bearer {token}");
    let http = openremote_cloud::https::Http::new();
    let Ok(response) = http
        .request("GET", &url, &[("authorization", bearer.as_str())], None)
        .await
    else {
        return Vec::new();
    };
    serde_json::from_slice::<Value>(&response.body)
        .ok()
        .and_then(|v| v.get("tools").and_then(Value::as_array).cloned())
        .unwrap_or_default()
}

/// The team tools this chat is offered: none where the user turned them
/// off, and only the ones this chat can use (a chat at the bottom of a team
/// can only report up).
async fn agent_tools(config: &Config, token: &str) -> Vec<Value> {
    let url = format!(
        "http://127.0.0.1:{}/sessions/{}/agents/tools",
        config.port,
        encode(&config.session)
    );
    let bearer = format!("Bearer {token}");
    let http = openremote_cloud::https::Http::new();
    let Ok(response) = http
        .request("GET", &url, &[("authorization", bearer.as_str())], None)
        .await
    else {
        return Vec::new();
    };
    serde_json::from_slice::<Value>(&response.body)
        .ok()
        .and_then(|v| v.get("tools").and_then(Value::as_array).cloned())
        .unwrap_or_default()
}

/// A browser tool: the daemon runs it in the app's browser and answers the
/// tool's result as MCP shapes it.
async fn browser_call(config: &Config, token: &str, params: &Value) -> Value {
    call_daemon(config, token, "browser", params).await
}

/// A team tool: the daemon does it and answers the tool's result.
async fn agents_call(config: &Config, token: &str, params: &Value) -> Value {
    call_daemon(config, token, "agents", params).await
}

/// A tool the daemon runs for this chat (`/sessions/{id}/<kind>/call`),
/// answered as the MCP result it came back as.
async fn call_daemon(config: &Config, token: &str, kind: &str, params: &Value) -> Value {
    let url = format!(
        "http://127.0.0.1:{}/sessions/{}/{kind}/call",
        config.port,
        encode(&config.session)
    );
    let bearer = format!("Bearer {token}");
    let body = json!({
        "name": params.get("name").cloned().unwrap_or(Value::Null),
        "arguments": params.get("arguments").cloned().unwrap_or_else(|| json!({})),
    });
    let body = serde_json::to_vec(&body).expect("a call serializes");
    let http = openremote_cloud::https::Http::new();
    // Longer than the daemon's own wait on the browser, so the daemon's
    // answer (even "the browser didn't answer in time") is what arrives.
    match http
        .request_within(
            std::time::Duration::from_secs(75),
            "POST",
            &url,
            &[("authorization", bearer.as_str())],
            Some(("application/json", body)),
        )
        .await
    {
        Ok(response) if response.status == 200 => serde_json::from_slice(&response.body)
            .unwrap_or_else(|_| failure("OpenRemote answered oddly")),
        Ok(response) => {
            let body: Value = serde_json::from_slice(&response.body).unwrap_or(Value::Null);
            failure(
                body.get("error")
                    .and_then(Value::as_str)
                    .unwrap_or("OpenRemote refused the call"),
            )
        }
        Err(e) => failure(&format!("OpenRemote is not reachable: {e}")),
    }
}

/// The tool's answer: the daemon's condensed chat, or its refusal as a
/// tool error the agent can read.
async fn call(config: &Config, token: &str, params: &Value) -> Value {
    let name = params.get("name").and_then(Value::as_str).unwrap_or("");
    if name.starts_with("browser_") {
        return browser_call(config, token, params).await;
    }
    if name.starts_with("agents_") {
        return agents_call(config, token, params).await;
    }
    if name != "read_chat" {
        return failure(&format!("there is no tool named {name}"));
    }
    let Some(chat) = params.pointer("/arguments/chat").and_then(Value::as_str) else {
        return failure("read_chat needs the chat's id");
    };
    let url = format!(
        "http://127.0.0.1:{}/sessions/{}/mentions/{}",
        config.port,
        config.session,
        encode(chat)
    );
    let bearer = format!("Bearer {token}");
    let http = openremote_cloud::https::Http::new();
    match http
        .request("GET", &url, &[("authorization", bearer.as_str())], None)
        .await
    {
        Ok(response) => {
            let body: Value = serde_json::from_slice(&response.body).unwrap_or(Value::Null);
            if response.status == 200 {
                let text = body.get("text").and_then(Value::as_str).unwrap_or("");
                json!({"content": [{"type": "text", "text": text}]})
            } else {
                failure(
                    body.get("error")
                        .and_then(Value::as_str)
                        .unwrap_or("OpenRemote refused the read"),
                )
            }
        }
        Err(e) => failure(&format!("OpenRemote is not reachable: {e}")),
    }
}

fn failure(text: &str) -> Value {
    json!({"content": [{"type": "text", "text": text}], "isError": true})
}

/// A path segment, percent-encoded past the unreserved characters.
fn encode(raw: &str) -> String {
    raw.bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
                (b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_its_flags_and_encodes_a_chat_name() {
        let args: Vec<String> = ["--session", "s1", "--port", "4567", "--data-dir", "/d"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let config = parse(&args).unwrap();
        assert_eq!(config.session, "s1");
        assert_eq!(config.port, 4567);
        assert!(parse(&args[..4]).is_none());
        assert_eq!(encode("ab cd/ef"), "ab%20cd%2Fef");
    }
}
