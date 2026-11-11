//! The team tools agents get through OpenRemote's MCP server (`agents_*`):
//! what each is called and takes, which only read (those never ask the
//! user), and the routes the chat's MCP server and this device's console
//! use. None of the routes answers the user's other devices: a team's tools
//! run where its lead runs.

use std::sync::Arc;

use axum::extract::{Path as AxumPath, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use openremote_core::SessionId;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::app::App;
use crate::supervisor::{Offer, SpawnRequest, Supervisor};

/// Which chats a tool is for.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Group {
    /// A chat that may start workers.
    Spawn,
    /// A chat that has workers, or may have.
    Manage,
    /// A chat that has a lead.
    Report,
}

/// One tool as MCP lists it.
pub struct Tool {
    pub name: &'static str,
    pub description: &'static str,
    /// Reading never asks; starting, messaging and stopping do, unless the
    /// chat allows them.
    pub reads: bool,
    group: Group,
    schema: fn() -> Value,
}

impl Tool {
    fn listing(&self) -> Value {
        json!({
            "name": self.name,
            "description": self.description,
            "inputSchema": (self.schema)(),
            "annotations": {"readOnlyHint": self.reads},
        })
    }

    fn offered(&self, offer: Offer) -> bool {
        match self.group {
            Group::Spawn => offer.spawn,
            Group::Manage => offer.manage,
            Group::Report => offer.report,
        }
    }
}

fn object(props: Value, required: &[&str]) -> Value {
    json!({"type": "object", "properties": props, "required": required})
}

fn agent_prop() -> Value {
    json!({"type": "string", "description": "The agent's id (from agents_list or agents_spawn), or at least its first 6 characters."})
}

pub const TOOLS: &[Tool] = &[
    Tool {
        name: "agents_harnesses",
        description: "List the agent tools installed on this computer that you can start a worker on: each one's id and name, the models and effort levels it offers, and the permission its workers run with. Read it before agents_spawn.",
        reads: true,
        group: Group::Spawn,
        schema: || object(json!({}), &[]),
    },
    Tool {
        name: "agents_spawn",
        description: "Start a worker: another chat, on the harness you pick (any installed one; a different one from yours is fine), that works on the brief you write while you carry on. It returns at once with the worker's id. The worker's final message comes back to you as its report - use agents_wait to wait for reports, or carry on: a report that arrives while you are busy waits for the end of your turn. Start workers only when the user asked for help in parallel, or the task splits into parts that can run side by side and touch different files; never for small things you can do yourself. A worker knows nothing you do not tell it: write a self-contained brief - the goal, the files and limits, and what its final message should contain. Workers share your folder.",
        reads: false,
        group: Group::Spawn,
        schema: || {
            object(
                json!({
                    "harness": {"type": "string", "description": "The harness's id, from agents_harnesses (for example claude, codex, opencode, pi)."},
                    "brief": {"type": "string", "description": "What the worker should do, and what to put in its final message."},
                    "title": {"type": "string", "description": "A short name for the worker's chat (up to 60 characters). Leave out to take it from the brief."},
                    "model": {"type": "string", "description": "A model the harness offers. Leave out for its default."},
                    "effort": {"type": "string", "description": "An effort level the model offers. Leave out for its default."},
                    "permission": {"type": "string", "description": "Leave out. Workers run with the permission the user set for the harness; naming it changes nothing."},
                    "workspace": {"type": "string", "description": "Only \"inherit\" - the worker shares your folder - is available for now."},
                }),
                &["harness", "brief"],
            )
        },
    },
    Tool {
        name: "agents_list",
        description: "List your team: the chat that started you (if one did), the workers you started and theirs - each with its id, harness, title, state (working, waiting for you, idle, failed or stopped) and when it last reported.",
        reads: true,
        group: Group::Manage,
        schema: || object(json!({}), &[]),
    },
    Tool {
        name: "agents_read",
        description: "Read what one of your workers did: its messages in order, each tool call as one line (long ones keep their end). With tail, only its last few messages: tail 1 is its latest message, which is its report.",
        reads: true,
        group: Group::Manage,
        schema: || {
            object(
                json!({
                    "agent": agent_prop(),
                    "tail": {"type": "integer", "description": "Only the last this-many messages."},
                }),
                &["agent"],
            )
        },
    },
    Tool {
        name: "agents_wait",
        description: "Wait for your workers. Answers as soon as one finishes or sends a note - with its report - or after timeout_s seconds (30 unless you say, 40 at most) with where each stands. Call it again to keep waiting. A report also reaches you by itself when you are not waiting, so wait only when you have nothing else to do.",
        reads: true,
        group: Group::Manage,
        schema: || {
            object(
                json!({"timeout_s": {"type": "integer", "description": "How long to wait: 1 to 40 seconds. Default 30."}}),
                &[],
            )
        },
    },
    Tool {
        name: "agents_report",
        description: "Send the chat that started you a note without ending your turn: progress it needs to know, a question you cannot settle yourself, or that you are blocked. Your final message is already your report to it - do not repeat that here. Notes less than 15 seconds apart are refused.",
        reads: true,
        group: Group::Report,
        schema: || {
            object(
                json!({
                    "text": {"type": "string", "description": "The note."},
                    "status": {"type": "string", "description": "A few words on where you stand, like \"blocked\" or \"half done\"."},
                }),
                &["text"],
            )
        },
    },
    Tool {
        name: "agents_message",
        description: "Send a message to one of your workers. By default it waits for the worker's current turn to end (or starts one at once if the worker is idle); with mode steer it goes into the turn that is running now. The worker reads it as coming from you, not from the user.",
        reads: false,
        group: Group::Manage,
        schema: || {
            object(
                json!({
                    "agent": agent_prop(),
                    "text": {"type": "string", "description": "The message."},
                    "mode": {"type": "string", "enum": ["queue", "steer"], "description": "queue (default) or steer."},
                }),
                &["agent", "text"],
            )
        },
    },
    Tool {
        name: "agents_stop",
        description: "Stop one of your workers, and any workers it started. Use it when a worker is no longer needed or is going the wrong way.",
        reads: false,
        group: Group::Manage,
        schema: || object(json!({"agent": agent_prop()}), &["agent"]),
    },
];

/// The tools that never ask, in Claude Code's own names for an MCP tool
/// (`mcp__<server>__<tool>`), for its allowed-tools list.
pub fn reading_tool_names(server: &str) -> Vec<String> {
    TOOLS
        .iter()
        .filter(|t| t.reads)
        .map(|t| format!("mcp__{server}__{}", t.name))
        .collect()
}

/// The tools a chat is offered, as MCP lists them.
pub fn listing(offer: Offer) -> Vec<Value> {
    TOOLS
        .iter()
        .filter(|t| t.offered(offer))
        .map(Tool::listing)
        .collect()
}

fn text_result(text: String) -> Value {
    json!({"content": [{"type": "text", "text": text}]})
}

fn error_result(text: &str) -> Value {
    json!({"content": [{"type": "text", "text": text}], "isError": true})
}

fn text_arg(args: &Value, key: &str) -> Option<String> {
    args.get(key).and_then(Value::as_str).map(str::to_string)
}

/// One tool call from a chat's agent, answered as an MCP tool result: a
/// refusal is an error result the agent reads, never a failed request.
pub async fn call(
    supervisor: &Arc<Supervisor>,
    caller: &SessionId,
    name: &str,
    args: &Value,
) -> Value {
    if !supervisor.team_settings().tools {
        return error_result("Team tools are turned off in Settings > Agents.");
    }
    let need = |key: &str| {
        text_arg(args, key)
            .filter(|t| !t.trim().is_empty())
            .ok_or_else(|| format!("{name} needs \"{key}\""))
    };
    let answer: Result<String, String> = match name {
        "agents_harnesses" => supervisor.team_harnesses(caller).await,
        "agents_spawn" => {
            let request = SpawnRequest {
                harness: text_arg(args, "harness").unwrap_or_default(),
                brief: text_arg(args, "brief").unwrap_or_default(),
                title: text_arg(args, "title"),
                model: text_arg(args, "model"),
                effort: text_arg(args, "effort"),
                permission: text_arg(args, "permission"),
                workspace: text_arg(args, "workspace"),
            };
            supervisor.team_spawn(caller, request).await
        }
        "agents_list" => supervisor.team_list(caller),
        "agents_read" => match need("agent") {
            Ok(agent) => supervisor.team_read(
                caller,
                &agent,
                args.get("tail").and_then(Value::as_u64).map(|t| t as usize),
            ),
            Err(e) => Err(e),
        },
        "agents_wait" => {
            supervisor
                .team_wait(caller, args.get("timeout_s").and_then(Value::as_u64))
                .await
        }
        "agents_report" => match need("text") {
            Ok(text) => {
                supervisor
                    .team_report(caller, &text, text_arg(args, "status").as_deref())
                    .await
            }
            Err(e) => Err(e),
        },
        "agents_message" => match (need("agent"), need("text")) {
            (Ok(agent), Ok(text)) => {
                let steer = text_arg(args, "mode").as_deref() == Some("steer");
                supervisor.team_message(caller, &agent, &text, steer).await
            }
            (Err(e), _) | (_, Err(e)) => Err(e),
        },
        "agents_stop" => match need("agent") {
            Ok(agent) => supervisor.team_stop(caller, &agent).await,
            Err(e) => Err(e),
        },
        other => Err(format!("there is no tool named {other}")),
    };
    match answer {
        Ok(text) => text_result(text),
        Err(text) => error_result(&text),
    }
}

// ---- routes ----

pub fn routes() -> Router<Arc<App>> {
    Router::new()
        .route("/agents/settings", get(get_settings).post(post_settings))
        .route("/sessions/{id}/agents/tools", get(tool_list))
        .route("/sessions/{id}/agents/call", post(call_tool))
        .route("/sessions/{id}/team/stop", post(stop_team))
}

fn bad(status: StatusCode, message: &str) -> Response {
    (status, Json(json!({"error": message}))).into_response()
}

/// What the user lets teams do.
async fn get_settings(State(app): State<Arc<App>>) -> Response {
    Json(app.supervisor.team_settings()).into_response()
}

/// A change to those: `{name: value}`, a null putting one back to its default.
async fn post_settings(
    State(app): State<Arc<App>>,
    Json(patch): Json<serde_json::Map<String, Value>>,
) -> Response {
    match app.supervisor.change_team_settings(&patch) {
        Ok(settings) => Json(settings).into_response(),
        Err(err) => bad(StatusCode::UNPROCESSABLE_ENTITY, &err.to_string()),
    }
}

/// The team tools one chat's MCP server lists: none where the user turned
/// them off, and only the ones that chat can use.
async fn tool_list(State(app): State<Arc<App>>, AxumPath(id): AxumPath<String>) -> Response {
    let Some(session) = SessionId::parse(&id) else {
        return bad(StatusCode::BAD_REQUEST, "bad session id");
    };
    Json(json!({"tools": listing(app.supervisor.team_offer(&session))})).into_response()
}

#[derive(Deserialize)]
struct CallBody {
    name: String,
    #[serde(default)]
    arguments: Value,
}

/// One of a chat's team tools (OpenRemote's MCP server calls this for the
/// chat's agent). Always answers an MCP tool result.
async fn call_tool(
    State(app): State<Arc<App>>,
    AxumPath(id): AxumPath<String>,
    Json(body): Json<CallBody>,
) -> Response {
    let Some(caller) = SessionId::parse(&id) else {
        return bad(StatusCode::BAD_REQUEST, "bad session id");
    };
    Json(call(&app.supervisor, &caller, &body.name, &body.arguments).await).into_response()
}

/// The user stops everyone under a chat (the team bar's Stop all).
async fn stop_team(State(app): State<Arc<App>>, AxumPath(id): AxumPath<String>) -> Response {
    let Some(lead) = SessionId::parse(&id) else {
        return bad(StatusCode::BAD_REQUEST, "bad session id");
    };
    let stopped = app.supervisor.stop_team_of(&lead).await;
    Json(json!({"stopped": stopped})).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_tool_lists_with_a_schema_and_the_reading_ones_are_named_for_claude() {
        for tool in TOOLS {
            assert_eq!(
                tool.listing()["inputSchema"]["type"],
                "object",
                "{}",
                tool.name
            );
            assert!(tool.name.starts_with("agents_"));
        }
        let reading = reading_tool_names("openremote");
        assert!(reading.contains(&"mcp__openremote__agents_wait".to_string()));
        assert!(reading.contains(&"mcp__openremote__agents_report".to_string()));
        for acting in ["agents_spawn", "agents_message", "agents_stop"] {
            assert!(!reading.contains(&format!("mcp__openremote__{acting}")));
        }
    }

    #[test]
    fn a_chat_is_offered_only_the_tools_it_can_use() {
        let names = |offer| -> Vec<String> {
            listing(offer)
                .iter()
                .map(|t| t["name"].as_str().unwrap().to_string())
                .collect()
        };
        let lead = names(Offer {
            spawn: true,
            manage: true,
            report: false,
        });
        assert!(lead.contains(&"agents_spawn".to_string()));
        assert!(!lead.contains(&"agents_report".to_string()));
        // A worker at the bottom of a team can only report up.
        assert_eq!(
            names(Offer {
                spawn: false,
                manage: false,
                report: true
            }),
            ["agents_report"]
        );
        assert!(names(Offer::default()).is_empty());
    }
}
