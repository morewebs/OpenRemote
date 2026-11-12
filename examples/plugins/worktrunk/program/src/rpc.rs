//! JSON-RPC 2.0 over stdio, one message per line (protocol.md). OpenRemote
//! calls the plugin (`initialize`, `tools/call`, `ui/request`), and the
//! plugin calls OpenRemote back (`chats/start`). Every request runs on its
//! own thread, so a long `wt merge` never holds up a view's quick question;
//! writes go through one lock so lines never interleave.

use std::collections::HashMap;
use std::io::Write;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{channel, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{json, Value};

pub type Reply = Result<Value, String>;

pub struct Rpc {
    out: Mutex<Box<dyn Write + Send>>,
    pending: Mutex<HashMap<u64, Sender<Reply>>>,
    next: AtomicU64,
}

impl Rpc {
    pub fn new(out: Box<dyn Write + Send>) -> Arc<Self> {
        Arc::new(Self {
            out: Mutex::new(out),
            pending: Mutex::new(HashMap::new()),
            // Our own ids stay clear of small numbers, which makes a log easy
            // to read: OpenRemote's ids are its own.
            next: AtomicU64::new(1_000_000),
        })
    }

    pub fn send(&self, message: &Value) {
        let mut line = message.to_string();
        line.push('\n');
        let mut out = self.out.lock().unwrap_or_else(|p| p.into_inner());
        // A write that fails means OpenRemote is gone; stdin closing ends us.
        let _ = out.write_all(line.as_bytes());
        let _ = out.flush();
    }

    pub fn answer(&self, id: &Value, result: Reply) {
        let message = match result {
            Ok(result) => json!({"jsonrpc": "2.0", "id": id, "result": result}),
            Err(message) => {
                json!({"jsonrpc": "2.0", "id": id, "error": {"code": -32000, "message": message}})
            }
        };
        self.send(&message);
    }

    pub fn log(&self, level: &str, message: &str) {
        self.send(&json!({"jsonrpc": "2.0", "method": "log", "params": {"level": level, "message": message}}));
    }

    /// Ask OpenRemote something and wait for its answer.
    pub fn call(&self, method: &str, params: Value, wait: Duration) -> Reply {
        let id = self.next.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = channel();
        self.pending
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(id, tx);
        self.send(&json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}));
        let reply = rx
            .recv_timeout(wait)
            .unwrap_or_else(|_| Err(format!("OpenRemote did not answer {method} in time")));
        self.pending
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .remove(&id);
        reply
    }

    /// An answer to one of our own calls arrived. Answers whether it was one.
    pub fn settle(&self, message: &Value) -> bool {
        if message.get("method").is_some() {
            return false;
        }
        let Some(id) = message.get("id").and_then(Value::as_u64) else {
            return false;
        };
        let Some(tx) = self
            .pending
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .remove(&id)
        else {
            return true;
        };
        let reply = match (message.get("result"), message.get("error")) {
            (_, Some(error)) => Err(error
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("OpenRemote refused the call")
                .to_string()),
            (Some(result), None) => Ok(result.clone()),
            (None, None) => Ok(Value::Null),
        };
        let _ = tx.send(reply);
        true
    }
}
