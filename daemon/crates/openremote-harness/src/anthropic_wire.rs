//! Wire frames of the Anthropic-Messages stream-json family — spoken by
//! Claude Code (`--output-format stream-json`), Grok Build
//! (`--output-format streaming-messages-json`, verified live 2026-10-01),
//! and Antigravity (same flags). Everything unknown stays as raw JSON —
//! the driver forwards what it recognizes and drops what it doesn't, the
//! way the SDK swallows `keep_alive` and hidden lifecycle frames.

use serde_json::Value;

/// A parsed stdout frame, recognized or not.
#[derive(Debug, Clone, PartialEq)]
pub enum Frame {
    /// `system` with `subtype: "init"` — carries the harness session id.
    Init {
        session_id: String,
        model: Option<String>,
        permission_mode: Option<String>,
    },
    /// `assistant` message — content blocks (text / tool_use).
    Assistant {
        content: Vec<Value>,
        model: Option<String>,
    },
    /// `user` frame carrying tool results (tool results come back as user
    /// messages on stdout).
    UserToolResults { content: Vec<Value> },
    /// `stream_event` with a text delta (only with
    /// `--include-partial-messages`).
    TextDelta { text: String },
    /// `stream_event` with a thinking delta (same flag) — the model's own
    /// reasoning streaming, `delta.type: "thinking_delta"`.
    ThinkingDelta { text: String },
    /// `result` — the turn boundary. Never guess turn state before it.
    /// `usage` rides verbatim (the SDK's own fields: input_tokens,
    /// cache_read_input_tokens, cache_creation_input_tokens, …).
    /// `cost_usd` is the SDK's own `total_cost_usd`, a sibling of `usage`.
    Result {
        subtype: String,
        terminal_reason: Option<String>,
        is_error: bool,
        usage: Option<Value>,
        cost_usd: Option<f64>,
    },
    /// `system` with `subtype: "compact_boundary"` — the conversation was
    /// compacted here (the SDK's own marker).
    CompactBoundary,
    /// CLI → host control request, e.g. `can_use_tool`.
    ControlRequest { request_id: String, request: Value },
    /// host ↔ CLI control response (ours out; theirs in for interrupts).
    ControlResponse { response: Value },
    /// Something else (`keep_alive`, hidden lifecycle frames, future types).
    Other { kind: String, raw: Value },
}

/// Content block helpers shared by assistant/user frames.
pub fn block_texts(content: &[Value]) -> Vec<String> {
    content
        .iter()
        .filter(|b| b.get("type").and_then(|t| t.as_str()) == Some("text"))
        .filter_map(|b| b.get("text").and_then(|t| t.as_str()))
        .map(String::from)
        .collect()
}

/// The thinking blocks of an assistant message — claude's own reasoning,
/// `type: "thinking"` (or `redacted_thinking`, which carries no text).
pub fn block_thinking(content: &[Value]) -> Vec<String> {
    content
        .iter()
        .filter(|b| {
            matches!(
                b.get("type").and_then(|t| t.as_str()),
                Some("thinking") | Some("redacted_thinking")
            )
        })
        .filter_map(|b| b.get("thinking").and_then(|t| t.as_str()))
        .map(String::from)
        .collect()
}

pub fn tool_use_blocks(content: &[Value]) -> Vec<(String, String, Value)> {
    content
        .iter()
        .filter(|b| b.get("type").and_then(|t| t.as_str()) == Some("tool_use"))
        .filter_map(|b| {
            let id = b.get("id")?.as_str()?.to_string();
            let name = b.get("name")?.as_str()?.to_string();
            let input = b.get("input").cloned().unwrap_or(Value::Null);
            Some((id, name, input))
        })
        .collect()
}

pub fn tool_result_blocks(content: &[Value]) -> Vec<(String, String, bool)> {
    content
        .iter()
        .filter(|b| b.get("type").and_then(|t| t.as_str()) == Some("tool_result"))
        .filter_map(|b| {
            let tool_use_id = b.get("tool_use_id")?.as_str()?.to_string();
            let text = b
                .get("content")
                .and_then(|c| c.as_str())
                .map(String::from)
                .or_else(|| {
                    b.get("content").and_then(|c| c.as_array()).map(|blocks| {
                        blocks
                            .iter()
                            .filter_map(|x| x.get("text").and_then(|t| t.as_str()))
                            .collect::<Vec<_>>()
                            .join("\n")
                    })
                })
                .unwrap_or_default();
            let is_error = b.get("is_error").and_then(|e| e.as_bool()).unwrap_or(false);
            Some((tool_use_id, text, is_error))
        })
        .collect()
}

/// Parse one stdout NDJSON line into a Frame. Tolerant: unknown shapes
/// become `Other` rather than errors — the wire grows between versions.
pub fn parse_frame(line: &str) -> Option<Frame> {
    let raw: Value = serde_json::from_str(line).ok()?;
    let kind = raw.get("type").and_then(|t| t.as_str())?.to_string();
    match kind.as_str() {
        "system" => match raw.get("subtype").and_then(|s| s.as_str()) {
            Some("init") => Some(Frame::Init {
                session_id: raw.get("session_id").and_then(|s| s.as_str())?.to_string(),
                model: raw.get("model").and_then(|m| m.as_str()).map(String::from),
                permission_mode: raw
                    .get("permissionMode")
                    .and_then(|m| m.as_str())
                    .map(String::from),
            }),
            Some("compact_boundary") => Some(Frame::CompactBoundary),
            _ => Some(Frame::Other { kind, raw }),
        },
        "assistant" => Some(Frame::Assistant {
            content: raw
                .pointer("/message/content")
                .and_then(|c| c.as_array())
                .cloned()
                .unwrap_or_default(),
            model: raw
                .pointer("/message/model")
                .and_then(|m| m.as_str())
                .map(String::from),
        }),
        "user" => {
            let content = raw
                .pointer("/message/content")
                .and_then(|c| c.as_array())
                .cloned()
                .unwrap_or_default();
            if content
                .iter()
                .any(|b| b.get("type").and_then(|t| t.as_str()) == Some("tool_result"))
            {
                Some(Frame::UserToolResults { content })
            } else {
                Some(Frame::Other { kind, raw })
            }
        }
        "stream_event" => {
            let delta_type = raw.pointer("/event/delta/type").and_then(|t| t.as_str());
            if delta_type == Some("text_delta") {
                Some(Frame::TextDelta {
                    text: raw
                        .pointer("/event/delta/text")
                        .and_then(|t| t.as_str())
                        .unwrap_or_default()
                        .to_string(),
                })
            } else if delta_type == Some("thinking_delta") {
                Some(Frame::ThinkingDelta {
                    text: raw
                        .pointer("/event/delta/thinking")
                        .and_then(|t| t.as_str())
                        .unwrap_or_default()
                        .to_string(),
                })
            } else {
                Some(Frame::Other { kind, raw })
            }
        }
        "result" => Some(Frame::Result {
            subtype: raw
                .get("subtype")
                .and_then(|s| s.as_str())
                .unwrap_or("success")
                .to_string(),
            terminal_reason: raw
                .get("terminal_reason")
                .and_then(|r| r.as_str())
                .map(String::from),
            is_error: raw
                .get("is_error")
                .and_then(|e| e.as_bool())
                .unwrap_or(false),
            usage: raw.get("usage").cloned(),
            cost_usd: raw
                .get("total_cost_usd")
                .and_then(|c| c.as_f64()),
        }),
        "control_request" => Some(Frame::ControlRequest {
            request_id: raw.get("request_id").and_then(|r| r.as_str())?.to_string(),
            request: raw.get("request").cloned().unwrap_or(Value::Null),
        }),
        "control_response" => Some(Frame::ControlResponse {
            response: raw.get("response").cloned().unwrap_or(Value::Null),
        }),
        _ => Some(Frame::Other { kind, raw }),
    }
}

/// The user-message envelope written to stdin (the SDK's exact shape).
pub fn user_envelope(text: &str) -> String {
    let envelope = serde_json::json!({
        "type": "user",
        "message": {"role": "user", "content": [{"type": "text", "text": text}]},
        "parent_tool_use_id": null
    });
    serde_json::to_string(&envelope).expect("envelope serializes")
}

/// The `interrupt` control request (host → CLI).
pub fn interrupt_envelope(request_id: &str) -> String {
    let envelope = serde_json::json!({
        "type": "control_request",
        "request_id": request_id,
        "request": {"subtype": "interrupt"}
    });
    serde_json::to_string(&envelope).expect("envelope serializes")
}

/// The host's answer to a `can_use_tool` request: `allow` or `deny`, with
/// `updatedInput` defaulting to the original input on allow (pre-2.1.207
/// CLIs reject an allow without it).
pub fn can_use_tool_response(
    request_id: &str,
    allow: bool,
    original_input: &Value,
    deny_message: &str,
) -> String {
    let response = if allow {
        serde_json::json!({
            "behavior": "allow",
            "updatedInput": if original_input.is_null() { serde_json::json!({}) } else { original_input.clone() }
        })
    } else {
        serde_json::json!({"behavior": "deny", "message": deny_message})
    };
    let envelope = serde_json::json!({
        "type": "control_response",
        "response": {"subtype": "success", "request_id": request_id, "response": response}
    });
    serde_json::to_string(&envelope).expect("envelope serializes")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_init_frames() {
        let frame = parse_frame(
            r#"{"type":"system","subtype":"init","session_id":"abc","model":"sonnet","permissionMode":"default"}"#,
        )
        .unwrap();
        match frame {
            Frame::Init {
                session_id,
                model,
                permission_mode,
            } => {
                assert_eq!(session_id, "abc");
                assert_eq!(model.as_deref(), Some("sonnet"));
                assert_eq!(permission_mode.as_deref(), Some("default"));
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn parses_result_boundaries_and_deltas() {
        let result = parse_frame(
            r#"{"type":"result","subtype":"error_during_execution","is_error":true,"terminal_reason":"aborted_tools"}"#,
        )
        .unwrap();
        assert_eq!(
            result,
            Frame::Result {
                subtype: "error_during_execution".into(),
                terminal_reason: Some("aborted_tools".into()),
                is_error: true,
                usage: None,
                cost_usd: None
            }
        );
        // The turn's own usage rides verbatim, and the compaction marker
        // parses on its own.
        let with_usage = parse_frame(
            r#"{"type":"result","subtype":"success","usage":{"input_tokens":10,"cache_read_input_tokens":0,"cache_creation_input_tokens":0,"output_tokens":5}}"#,
        )
        .unwrap();
        assert_eq!(
            with_usage,
            Frame::Result {
                subtype: "success".into(),
                terminal_reason: None,
                is_error: false,
                usage: Some(json!({"input_tokens": 10, "cache_read_input_tokens": 0, "cache_creation_input_tokens": 0, "output_tokens": 5})),
                cost_usd: None
            }
        );
        // The SDK's cost is a sibling of usage, parsed on its own.
        let with_cost = parse_frame(
            r#"{"type":"result","subtype":"success","usage":{"input_tokens":10,"output_tokens":5},"total_cost_usd":0.042}"#,
        )
        .unwrap();
        assert!(matches!(
            with_cost,
            Frame::Result { cost_usd: Some(0.042), .. }
        ));
        let boundary = parse_frame(r#"{"type":"system","subtype":"compact_boundary"}"#).unwrap();
        assert_eq!(boundary, Frame::CompactBoundary);
        let delta = parse_frame(
            r#"{"type":"stream_event","event":{"type":"content_block_delta","delta":{"type":"text_delta","text":"Hel"}}}"#,
        )
        .unwrap();
        assert_eq!(delta, Frame::TextDelta { text: "Hel".into() });
    }

    #[test]
    fn assistant_blocks_split_into_texts_and_tool_uses() {
        let content = vec![
            json!({"type": "text", "text": "thinking"}),
            json!({"type": "tool_use", "id": "t1", "name": "Bash", "input": {"command": "ls"}}),
        ];
        assert_eq!(block_texts(&content), vec!["thinking".to_string()]);
        assert_eq!(
            tool_use_blocks(&content),
            vec![("t1".into(), "Bash".into(), json!({"command": "ls"}))]
        );
    }

    #[test]
    fn tool_results_come_back_as_user_frames() {
        let line = r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t1","is_error":false,"content":[{"type":"text","text":"ok"}]}]}}"#;
        let frame = parse_frame(line).unwrap();
        match frame {
            Frame::UserToolResults { content } => {
                assert_eq!(
                    tool_result_blocks(&content),
                    vec![("t1".into(), "ok".into(), false)]
                );
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn unknown_frames_stay_raw_never_error() {
        let frame = parse_frame(r#"{"type":"keep_alive"}"#).unwrap();
        assert!(matches!(frame, Frame::Other { .. }));
        assert!(parse_frame("not json").is_none());
    }

    #[test]
    fn envelopes_match_the_sdk_shapes() {
        let env = serde_json::from_str::<Value>(&user_envelope("hi")).unwrap();
        assert_eq!(env["type"], "user");
        assert_eq!(env["message"]["content"][0]["text"], "hi");
        let answer = serde_json::from_str::<Value>(&can_use_tool_response(
            "cr-1",
            true,
            &json!({"command": "ls"}),
            "",
        ))
        .unwrap();
        assert_eq!(answer["response"]["request_id"], "cr-1");
        assert_eq!(answer["response"]["response"]["behavior"], "allow");
        assert_eq!(
            answer["response"]["response"]["updatedInput"]["command"],
            "ls"
        );
        let deny = serde_json::from_str::<Value>(&can_use_tool_response(
            "cr-1",
            false,
            &Value::Null,
            "no",
        ))
        .unwrap();
        assert_eq!(deny["response"]["response"]["behavior"], "deny");
        assert_eq!(deny["response"]["response"]["message"], "no");
    }
}
