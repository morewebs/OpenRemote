//! `openremote-daemon mcp-chat`: OpenRemote's MCP server for one chat - its
//! read-a-chat tool (the first of OpenRemote Controls), the built-in
//! browser's tools, and the agent tools of the installed plugins approved to
//! give agents tools (`<plugin>__<tool>`, run by the daemon in the plugin's
//! program or WASM module). A chat's harness starts it over stdio (MCP's
//! newline-delimited JSON-RPC). It reads the API token from the data
//! folder, never from its command line, and asks the daemon on loopback:
//! for a chat the user pointed this chat at with @ (the daemon refuses any
//! other), and for the browser, which the daemon reaches in the app.
//!
//! Every tool listed is sent with every model request, so with `--staged`
//! (given only to Apps whose MCP client re-lists on
//! `notifications/tools/list_changed`) the list grows with the chat: the
//! browser's two entry tools until the chat uses the browser, then all of
//! them; `agents_spawn` until the chat has workers, then the tools that
//! manage them. What is listed only ever grows, so a tool the chat has used
//! is never taken away. Without `--staged` everything is listed at once.

use std::io;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

/// The MCP revision this server speaks when the client names none.
const PROTOCOL: &str = "2025-06-18";

/// How often a staged server asks whether the user opened a gate (turned on
/// the browser for every chat) while some gate is still shut. The chat's own
/// calls are followed at once.
const GATE_POLL: std::time::Duration = std::time::Duration::from_secs(5);

/// What a staged server tells the agent it holds, for an App that loads tool
/// definitions on demand (Claude Code's tool search) and for the tools not
/// yet listed. Sent with every request, so it stays one short line.
const INSTRUCTIONS: &str = "OpenRemote: read_chat reads chats the user @-mentions; browser_* drive the user's built-in browser (all of them are listed once you use one); agents_* start worker agents and manage them.";

/// The daemon's current port, in its data folder. Written by the daemon with
/// `fsx::write_private` (owner-only, atomic) and removed on a clean shutdown, the same
/// treatment the token beside it gets.
pub const PORT_FILE: &str = "mcp-port";

struct Config {
    session: String,
    port: u16,
    data_dir: PathBuf,
    /// List tools as the chat comes to need them, and say when the list grew.
    staged: bool,
}

/// The tools beyond the ones every chat is listed, as the daemon allows them
/// for this chat. Each only ever opens.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Gates {
    /// Every browser tool: the chat has the browser in play.
    browser: bool,
    /// The tools that manage workers: the chat has some.
    team: bool,
}

impl Gates {
    fn with(self, other: Gates) -> Gates {
        Gates {
            browser: self.browser || other.browser,
            team: self.team || other.team,
        }
    }

    fn all_open(self) -> bool {
        self.browser && self.team
    }
}

/// The tools a staged server lists through these gates.
fn staged(tools: Vec<Value>, gates: Gates) -> Vec<Value> {
    tools
        .into_iter()
        .filter(|t| {
            let name = t.get("name").and_then(Value::as_str).unwrap_or("");
            if route(name) == Route::Browser {
                gates.browser || crate::browser::tools::ENTRY.contains(&name)
            } else if route(name) == Route::Agents {
                gates.team || !crate::team::manages(name)
            } else {
                true
            }
        })
        .collect()
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
        staged: args.iter().any(|a| a == "--staged"),
    })
}

pub async fn run(args: &[String]) -> ExitCode {
    let Some(config) = parse(args) else {
        eprintln!(
            "usage: openremote-daemon mcp-chat --session <id> --port <port> --data-dir <dir> [--staged]"
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
    // The gates seen open so far, and the ones the last listing was made
    // through (None: the client has not listed yet, so there is nothing to
    // say has changed).
    let mut open = Gates::default();
    let mut listed: Option<Gates> = None;
    let mut poll = tokio::time::interval(GATE_POLL);
    poll.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        let polling = config.staged && listed.is_some() && !open.all_open();
        let line = tokio::select! {
            line = lines.next_line() => match line {
                Ok(Some(line)) => line,
                _ => break,
            },
            _ = poll.tick(), if polling => {
                if !regate(&config, &token, &mut open, &mut listed, &mut stdout).await {
                    break;
                }
                continue;
            }
        };
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
            "initialize" => {
                let mut answer = json!({
                    "protocolVersion": params
                        .get("protocolVersion")
                        .and_then(Value::as_str)
                        .unwrap_or(PROTOCOL),
                    "capabilities": {"tools": {}},
                    "serverInfo": {"name": "openremote", "version": env!("CARGO_PKG_VERSION")},
                });
                if config.staged {
                    answer["capabilities"]["tools"]["listChanged"] = json!(true);
                    answer["instructions"] = json!(INSTRUCTIONS);
                }
                Ok(answer)
            }
            "ping" => Ok(json!({})),
            "tools/list" => {
                let mut tools = vec![tool()];
                tools.extend(browser_tools(&config, &token).await);
                tools.extend(agent_tools(&config, &token).await);
                tools.extend(plugin_tools(&config, &token).await);
                if config.staged {
                    open = open.with(gates(&config, &token).await);
                    listed = Some(open);
                    tools = staged(tools, open);
                }
                Ok(json!({"tools": tools}))
            }
            "tools/call" => Ok(call(&config, &token, &params).await),
            other => Err(json!({"code": -32601, "message": format!("no method {other}")})),
        };
        let frame = match reply {
            Ok(result) => json!({"jsonrpc": "2.0", "id": id, "result": result}),
            Err(error) => json!({"jsonrpc": "2.0", "id": id, "error": error}),
        };
        if !write_frame(&mut stdout, &frame).await {
            break;
        }
        // A call is how a chat usually opens a gate (its first browser tool,
        // its first worker): say so before the agent's next request.
        if method == "tools/call"
            && config.staged
            && listed.is_some()
            && !regate(&config, &token, &mut open, &mut listed, &mut stdout).await
        {
            break;
        }
    }
    ExitCode::SUCCESS
}

async fn write_frame(stdout: &mut tokio::io::Stdout, frame: &Value) -> bool {
    let mut out = serde_json::to_string(frame).expect("a frame serializes");
    out.push('\n');
    stdout.write_all(out.as_bytes()).await.is_ok() && stdout.flush().await.is_ok()
}

/// Ask the daemon for the gates again; when one opened since the client last
/// listed, tell it the list changed (it lists again). False when stdout is gone.
async fn regate(
    config: &Config,
    token: &str,
    open: &mut Gates,
    listed: &mut Option<Gates>,
    stdout: &mut tokio::io::Stdout,
) -> bool {
    *open = open.with(gates(config, token).await);
    if listed.is_some_and(|was| was != *open) {
        *listed = Some(*open);
        let note = json!({"jsonrpc": "2.0", "method": "notifications/tools/list_changed"});
        return write_frame(stdout, &note).await;
    }
    true
}

/// The gates the daemon has open for this chat; all shut when it can't be
/// reached (what was seen open stays open: see `Gates::with`).
async fn gates(config: &Config, token: &str) -> Gates {
    let url = format!(
        "http://127.0.0.1:{}/sessions/{}/tools/gates",
        config.port,
        encode(&config.session)
    );
    let bearer = format!("Bearer {token}");
    let http = openremote_cloud::https::Http::new();
    let Ok(response) = http
        .request("GET", &url, &[("authorization", bearer.as_str())], None)
        .await
    else {
        return Gates::default();
    };
    let body: Value = serde_json::from_slice(&response.body).unwrap_or(Value::Null);
    Gates {
        browser: body.get("browser").and_then(Value::as_bool) == Some(true),
        team: body.get("team").and_then(Value::as_bool) == Some(true),
    }
}

/// `read_chat` is listed from the start, never staged: a mention arrives in
/// the very message that needs the tool, and a list change sent then would
/// race the model's first request on it.
fn tool() -> Value {
    json!({
        "name": "read_chat",
        "description": "Read an OpenRemote chat the user @-mentioned in this chat (shown as @chat, its title and id): its messages, oldest first.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "chat": {"type": "string", "description": "Chat id, or its first 6+ characters."}
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

/// The installed plugins' agent tools (`<plugin>__<tool>`), as the daemon
/// lists them for this chat.
async fn plugin_tools(config: &Config, token: &str) -> Vec<Value> {
    let url = format!(
        "http://127.0.0.1:{}/sessions/{}/plugins/tools",
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
    // Longer than the daemon's own wait on the browser, so the daemon's
    // answer (even "the browser didn't answer in time") is what arrives;
    // a plugin tool may take up to ten minutes.
    let wait = if kind == "plugins" { 660 } else { 75 };
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
    match http
        .request_within(
            std::time::Duration::from_secs(wait),
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
    match route(name) {
        Route::Plugin => return call_daemon(config, token, "plugins", params).await,
        Route::Browser => return browser_call(config, token, params).await,
        Route::Agents => return agents_call(config, token, params).await,
        Route::ReadChat => {}
        Route::Unknown => return failure(&format!("there is no tool named {name}")),
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

/// Where a tool call goes, by its name.
#[derive(Debug, PartialEq, Eq)]
enum Route {
    Plugin,
    Browser,
    Agents,
    ReadChat,
    Unknown,
}

/// A plugin's tools are named `<plugin>__<tool>`; OpenRemote's own never
/// carry a double underscore, so that is asked first: a plugin named
/// `agents` or `browser` can't reach the team or browser routes.
fn route(name: &str) -> Route {
    if name.contains("__") {
        Route::Plugin
    } else if name.starts_with("browser_") {
        Route::Browser
    } else if name.starts_with("agents_") {
        Route::Agents
    } else if name == "read_chat" {
        Route::ReadChat
    } else {
        Route::Unknown
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
        let mut args: Vec<String> = ["--session", "s1", "--port", "4567", "--data-dir", "/d"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let config = parse(&args).unwrap();
        assert_eq!(config.session, "s1");
        assert_eq!(config.port, 4567);
        assert!(!config.staged);
        assert!(parse(&args[..4]).is_none());
        assert_eq!(encode("ab cd/ef"), "ab%20cd%2Fef");
        args.push("--staged".into());
        assert!(parse(&args).unwrap().staged);
    }

    fn names(tools: &[Value]) -> Vec<&str> {
        tools.iter().filter_map(|t| t["name"].as_str()).collect()
    }

    /// Everything a top-level chat on a hosted daemon can be listed.
    fn everything() -> Vec<Value> {
        let mut tools = vec![tool()];
        tools.extend(crate::browser::tools::TOOLS.iter().map(|t| t.listing()));
        tools.extend(crate::team::listing(crate::supervisor::Offer {
            spawn: true,
            manage: true,
            report: false,
        }));
        tools.push(json!({"name": "notes__find", "inputSchema": {"type": "object"}}));
        tools
    }

    #[test]
    fn a_staged_list_grows_with_the_chat_and_keeps_what_it_starts_with() {
        let shut = staged(everything(), Gates::default());
        let listed = names(&shut);
        // What lets an agent find each feature on its own is always there.
        for kept in [
            "read_chat",
            "browser_tabs",
            "browser_tab_open",
            "agents_harnesses",
            "agents_spawn",
            "notes__find",
        ] {
            assert!(listed.contains(&kept), "{kept} in {listed:?}");
        }
        assert!(!listed.contains(&"browser_click"), "{listed:?}");
        assert!(!listed.contains(&"agents_wait"), "{listed:?}");
        let browsing = staged(
            everything(),
            Gates {
                browser: true,
                team: false,
            },
        );
        assert!(names(&browsing).contains(&"browser_click"));
        assert!(!names(&browsing).contains(&"agents_wait"));
        let lead = staged(
            everything(),
            Gates {
                browser: false,
                team: true,
            },
        );
        assert!(names(&lead).contains(&"agents_wait"));
        assert!(!names(&lead).contains(&"browser_click"));
        let all = staged(
            everything(),
            Gates {
                browser: true,
                team: true,
            },
        );
        assert_eq!(all.len(), everything().len());
        // A gate seen open stays open, whatever a later answer says.
        let open = Gates {
            browser: true,
            team: false,
        };
        assert_eq!(open.with(Gates::default()), open);
    }

    /// What each case costs in tool definitions and server instructions, per
    /// model request, as chars / 4 of the JSON the server sends. The numbers
    /// in docs/prompt-budget.md come from here (`cargo test -p
    /// openremote-daemon --lib what_a_chat_pays -- --nocapture`); the bounds
    /// keep them from creeping back up.
    #[test]
    fn what_a_chat_pays_per_request() {
        let cost = |tools: Vec<Value>, instructions: bool| {
            let listed = serde_json::to_string(&json!({"tools": tools}))
                .unwrap()
                .len();
            (listed + if instructions { INSTRUCTIONS.len() } else { 0 }) / 4
        };
        let mut all = everything();
        all.pop();
        let unstaged = cost(all.clone(), false);
        let plain = cost(staged(all.clone(), Gates::default()), true);
        let browsing = cost(
            staged(
                all.clone(),
                Gates {
                    browser: true,
                    team: false,
                },
            ),
            true,
        );
        let lead = cost(
            staged(
                all.clone(),
                Gates {
                    browser: false,
                    team: true,
                },
            ),
            true,
        );
        println!(
            "MEASURE unstaged={unstaged} plain={plain} browsing={browsing} lead={lead} tools={}",
            all.len()
        );
        assert!(unstaged <= 3_000, "{unstaged}");
        assert!(plain <= 750, "{plain}");
        assert!(browsing <= 2_500, "{browsing}");
        assert!(lead <= 1_300, "{lead}");
    }

    #[test]
    fn a_plugins_tools_are_routed_before_openremotes_own() {
        assert_eq!(route("agents__spawn"), Route::Plugin);
        assert_eq!(route("browser__click"), Route::Plugin);
        assert_eq!(route("agents_spawn"), Route::Agents);
        assert_eq!(route("browser_click"), Route::Browser);
        assert_eq!(route("read_chat"), Route::ReadChat);
        assert_eq!(route("rm_rf"), Route::Unknown);
        // No built-in tool name carries `__`, so none is routed to plugins,
        // and each reaches its own route.
        let builtin = std::iter::once(tool()["name"].as_str().unwrap().to_string())
            .chain(
                crate::browser::tools::TOOLS
                    .iter()
                    .map(|t| t.name.to_string()),
            )
            .chain(crate::team::TOOLS.iter().map(|t| t.name.to_string()));
        for name in builtin {
            assert!(!name.contains("__"), "{name}");
            assert_ne!(route(&name), Route::Plugin, "{name}");
            assert_ne!(route(&name), Route::Unknown, "{name}");
        }
    }
}
