//! The fixture's OpenCode serve mode: a real HTTP server speaking the
//! REST + SSE surface the openremote-opencode driver expects - basic
//! auth (`opencode:<OPENCODE_SERVER_PASSWORD>`), `POST /api/session`,
//! `POST …/prompt`, `POST …/interrupt`, permission/question replies,
//! and `GET /api/event` (the SSE bus every event broadcasts to).
//!
//! Scenarios: `plain` completes a turn; `ask` requests a permission
//! (`permission.v2.asked`) and reflects the reply (once/always → run,
//! reject → denied); `question` asks a structured question.

use std::io::{BufRead, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};

use serde_json::{Value, json};

pub fn main_opencode() {
    let scenario = std::env::var("FIXTURE_AGENT_SCENARIO")
        .ok()
        .or_else(|| {
            std::fs::read_to_string("fixture-scenario")
                .ok()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
        })
        .unwrap_or_else(|| "plain".to_string());
    let server = Arc::new(Server {
        scenario,
        subscribers: Mutex::new(Vec::new()),
        pending: Mutex::new(None),
    });
    let listener = match TcpListener::bind(("127.0.0.1", 0)) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("fixture-opencode: bind failed: {e}");
            return;
        }
    };
    let port = listener.local_addr().expect("local addr").port();
    println!("opencode server listening on http://127.0.0.1:{port}");
    let _ = std::io::stdout().flush();
    for stream in listener.incoming() {
        let Ok(stream) = stream else { continue };
        let server = Arc::clone(&server);
        std::thread::spawn(move || {
            let _ = server.handle(stream);
        });
    }
}

struct Server {
    scenario: String,
    /// SSE subscriber pipes (sync senders → their writer threads).
    subscribers: Mutex<Vec<Sender<Value>>>,
    /// A parked turn waiting for its reply: (request kind, answer sender).
    pending: Mutex<Option<(String, Sender<String>)>>,
}

impl Server {
    fn handle(&self, mut stream: TcpStream) -> std::io::Result<()> {
        loop {
            let request = match read_request(&mut stream)? {
                Some(r) => r,
                None => return Ok(()),
            };
            let (method, path, body) = request;
            let _ = (&path, &method);
            match (method.as_str(), path.as_str()) {
                ("POST", "/api/session") => {
                    let id = body
                        .get("id")
                        .and_then(Value::as_str)
                        .unwrap_or("ses_fixture_1")
                        .to_string();
                    respond_json(&mut stream, 200, json!({"data": {"id": id}}))?;
                }
                ("POST", p) if p.ends_with("/prompt") => {
                    let session = session_of(p);
                    let text = body
                        .pointer("/prompt/text")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string();
                    respond_json(
                        &mut stream,
                        200,
                        json!({"data": {"id": "msg_fixture", "sessionID": session, "delivery": "queue"}}),
                    )?;
                    self.run_turn(&session, &text);
                }
                ("POST", p) if p.ends_with("/interrupt") => {
                    let session = session_of(p);
                    respond_json(&mut stream, 200, json!({}))?;
                    self.broadcast(json!({
                        "id": evt(), "type": "session.next.step.ended",
                        "properties": {"sessionID": session, "finish": "aborted"}
                    }));
                    // Wake any parked turn: the abort already ended it.
                    self.resolve_pending("permission", "interrupted".into());
                    self.resolve_pending("question", "interrupted".into());
                }
                ("POST", p) if p.contains("/permission/") && p.ends_with("/reply") => {
                    let choice = body
                        .get("reply")
                        .and_then(Value::as_str)
                        .unwrap_or("reject")
                        .to_string();
                    respond_json(&mut stream, 200, json!({}))?;
                    self.resolve_pending("permission", choice);
                }
                ("POST", p) if p.contains("/question/") && p.ends_with("/reply") => {
                    let choice = body
                        .pointer("/answers/0/0")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string();
                    respond_json(&mut stream, 200, json!({}))?;
                    self.resolve_pending("question", choice);
                }
                ("GET", "/api/event") => {
                    let (tx, rx) = channel::<Value>();
                    self.subscribers.lock().unwrap().push(tx);
                    stream_sse(stream, rx);
                    return Ok(());
                }
                (m, p) => {
                    eprintln!("fixture-opencode: {m} {p} not handled");
                    respond_json(&mut stream, 200, json!({}))?;
                }
            }
        }
    }

    fn run_turn(&self, session: &str, text: &str) {
        self.broadcast(json!({
            "id": evt(), "type": "session.next.text.delta",
            "properties": {"sessionID": session, "delta": "work"}
        }));
        match self.scenario.as_str() {
            "ask" => {
                self.broadcast(json!({
                    "id": evt(), "type": "permission.v2.asked",
                    "properties": {
                        "id": "per_fixture_1", "sessionID": session,
                        "action": "bash", "resources": ["echo fixture"]
                    }
                }));
                let choice = self
                    .wait_for_reply("permission")
                    .unwrap_or_else(|| "reject".into());
                if choice == "interrupted" {
                    // The abort broadcast already ended the turn.
                    return;
                }
                let allowed = choice == "once" || choice == "always";
                self.broadcast(json!({
                    "id": evt(), "type": "session.next.tool.called",
                    "properties": {"sessionID": session, "callID": "call_1", "tool": "bash", "input": {"command": "echo fixture"}}
                }));
                self.broadcast(json!({
                    "id": evt(), "type": if allowed { "session.next.tool.success" } else { "session.next.tool.failed" },
                    "properties": {"sessionID": session, "callID": "call_1", "tool": "bash",
                        "result": if allowed { "fixture output" } else { "user rejected" }}
                }));
                self.finish(session, text, allowed);
            }
            "question" => {
                self.broadcast(json!({
                    "id": evt(), "type": "question.v2.asked",
                    "properties": {
                        "id": "que_fixture_1", "sessionID": session,
                        "questions": [{"question": "Which database?", "header": "Database",
                            "options": [{"label": "Postgres"}, {"label": "SQLite"}]}]
                    }
                }));
                let answer = self.wait_for_reply("question").unwrap_or_default();
                if answer == "interrupted" {
                    return;
                }
                self.broadcast(json!({
                    "id": evt(), "type": "session.next.text.ended",
                    "properties": {"sessionID": session, "text": format!("done: {text} → {answer}")}
                }));
                self.broadcast(json!({
                    "id": evt(), "type": "session.next.step.ended",
                    "properties": {"sessionID": session, "finish": "stop"}
                }));
            }
            // A provider failure: step.failed ends the turn on its own -
            // no step.ended follows (the real CLI's shape, observed live).
            "fail" => {
                self.broadcast(json!({
                    "id": evt(), "type": "session.next.step.failed",
                    "properties": {"sessionID": session,
                        "error": {"type": "unknown", "message": "Provider request failed with HTTP 403"}}
                }));
            }
            _ => {
                self.broadcast(json!({
                    "id": evt(), "type": "session.next.text.ended",
                    "properties": {"sessionID": session, "text": format!("done: {text}")}
                }));
                self.broadcast(json!({
                    "id": evt(), "type": "session.next.step.ended",
                    "properties": {"sessionID": session, "finish": "stop"}
                }));
            }
        }
    }

    fn finish(&self, session: &str, text: &str, allowed: bool) {
        let note = if allowed {
            format!("done: {text}")
        } else {
            format!("rejected: {text}")
        };
        self.broadcast(json!({
            "id": evt(), "type": "session.next.text.ended",
            "properties": {"sessionID": session, "text": note}
        }));
        self.broadcast(json!({
            "id": evt(), "type": "session.next.step.ended",
            "properties": {"sessionID": session, "finish": if allowed { "stop" } else { "error" }}
        }));
    }

    fn wait_for_reply(&self, kind: &str) -> Option<String> {
        let (tx, rx) = channel::<String>();
        *self.pending.lock().unwrap() = Some((kind.to_string(), tx));
        // The reply POST resolves the matching kind; 30s is plenty for a
        // test and never hangs forever.
        match rx.recv_timeout(std::time::Duration::from_secs(30)) {
            Ok(answer) => Some(answer),
            Err(_) => {
                *self.pending.lock().unwrap() = None;
                None
            }
        }
    }

    fn resolve_pending(&self, kind: &str, choice: String) {
        if let Some((pending_kind, _)) = self.pending.lock().unwrap().as_ref() {
            if pending_kind != kind {
                return;
            }
        }
        if let Some((_, tx)) = self.pending.lock().unwrap().take() {
            let _ = tx.send(choice);
        }
    }

    fn broadcast(&self, event: Value) {
        let mut subscribers = self.subscribers.lock().unwrap();
        subscribers.retain(|tx| tx.send(event.clone()).is_ok());
    }
}

fn evt() -> String {
    format!("evt_{}", std::process::id())
}

fn session_of(path: &str) -> String {
    path.trim_start_matches("/api/session/")
        .split('/')
        .next()
        .unwrap_or("ses_fixture_1")
        .to_string()
}

/// Read one HTTP request: (method, path, body). Enforces the fixture's
/// basic auth against the password the driver generated.
fn read_request(stream: &mut TcpStream) -> std::io::Result<Option<(String, String, Value)>> {
    let mut reader = std::io::BufReader::new(stream.try_clone()?);
    let mut request_line = String::new();
    if reader.read_line(&mut request_line)? == 0 {
        return Ok(None);
    }
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or_default().to_string();
    let path = parts.next().unwrap_or("/").to_string();
    let mut content_length = 0usize;
    let mut authorized = false;
    let expected = format!(
        "Basic {}",
        b64(format!("opencode:{}", env_password()).as_bytes())
    );
    loop {
        let mut header = String::new();
        if reader.read_line(&mut header)? == 0 {
            break;
        }
        let line = header.trim_end();
        if line.is_empty() {
            break;
        }
        if let Some(len) = line
            .to_ascii_lowercase()
            .strip_prefix("content-length:")
            .and_then(|v| v.trim().parse().ok())
        {
            content_length = len;
        }
        if let Some(auth) = line.strip_prefix("Authorization:") {
            authorized = auth.trim() == expected;
        }
    }
    if !authorized {
        respond(stream, 401, "")?;
        return Ok(Some((method, path, Value::Null)));
    }
    let mut body_bytes = vec![0u8; content_length];
    if content_length > 0 {
        reader.read_exact(&mut body_bytes)?;
    }
    let body = serde_json::from_slice::<Value>(&body_bytes).unwrap_or(Value::Null);
    Ok(Some((method, path, body)))
}

fn env_password() -> String {
    std::env::var("OPENCODE_SERVER_PASSWORD").unwrap_or_else(|_| "fixture".into())
}

fn respond(stream: &mut TcpStream, status: u16, body: &str) -> std::io::Result<()> {
    let reason = match status {
        200 => "OK",
        401 => "Unauthorized",
        _ => "Error",
    };
    write!(
        stream,
        "HTTP/1.1 {status} {reason}\r\nContent-Length: {}\r\nConnection: keep-alive\r\n\r\n{body}",
        body.len()
    )?;
    stream.flush()
}

fn respond_json(stream: &mut TcpStream, status: u16, body: Value) -> std::io::Result<()> {
    let body = body.to_string();
    write!(
        stream,
        "HTTP/1.1 {status} OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: keep-alive\r\n\r\n{body}",
        body.len()
    )?;
    stream.flush()
}

fn stream_sse(mut stream: TcpStream, rx: Receiver<Value>) {
    let _ = write!(
        stream,
        "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n"
    );
    let _ = stream.flush();
    while let Ok(event) = rx.recv() {
        if write!(stream, "data: {event}\n\n").is_err() {
            return;
        }
        if stream.flush().is_err() {
            return;
        }
    }
}

fn b64(input: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in input.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = ((b[0] as u32) << 16) | ((b[1] as u32) << 8) | b[2] as u32;
        out.push(T[(n >> 18) as usize & 63] as char);
        out.push(T[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 {
            T[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            T[n as usize & 63] as char
        } else {
            '='
        });
    }
    out
}
