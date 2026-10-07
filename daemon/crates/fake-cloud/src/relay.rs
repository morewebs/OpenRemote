//! The relay's contract, without its limits: one socket per device, binary
//! frames `[16-byte device id][payload]` forwarded between one account's
//! devices with the destination swapped for the sender, and the JSON control
//! messages (`hello`, `presence`, `devices_changed`, `revoked`,
//! `undeliverable`).

use std::collections::{HashMap, HashSet};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use axum::extract::State;
use axum::extract::ws::{CloseFrame, Message, WebSocket, WebSocketUpgrade};
use axum::http::HeaderMap;
use axum::response::{IntoResponse, Response};
use futures_util::{SinkExt, StreamExt};
use serde_json::json;
use tokio::sync::mpsc;

use crate::FakeCloud;

pub const CLOSE_REPLACED: u16 = 4001;
pub const CLOSE_REVOKED: u16 = 4003;
pub const CLOSE_ROTATED: u16 = 4005;
pub const CLOSE_RESTART: u16 = 1012;

enum Out {
    Message(Message),
    Close(u16),
}

struct Conn {
    id: u64,
    tx: mpsc::UnboundedSender<Out>,
}

#[derive(Default)]
pub struct Hub {
    rooms: Mutex<HashMap<String, HashMap<String, Conn>>>,
    next: AtomicU64,
}

fn text(value: serde_json::Value) -> Out {
    Out::Message(Message::Text(value.to_string().into()))
}

impl Hub {
    fn rooms(&self) -> std::sync::MutexGuard<'_, HashMap<String, HashMap<String, Conn>>> {
        self.rooms.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn online(&self, account: &str) -> HashSet<String> {
        self.rooms()
            .get(account)
            .map(|r| r.keys().cloned().collect())
            .unwrap_or_default()
    }

    pub fn devices_changed(&self, account: &str) {
        if let Some(room) = self.rooms().get(account) {
            for conn in room.values() {
                let _ = conn.tx.send(text(json!({"type": "devices_changed"})));
            }
        }
    }

    pub fn revoke(&self, account: &str, device: &str) {
        if let Some(conn) = self.rooms().get(account).and_then(|r| r.get(device)) {
            let _ = conn.tx.send(text(json!({"type": "revoked"})));
            let _ = conn.tx.send(Out::Close(CLOSE_REVOKED));
        }
    }

    pub fn close_device(&self, account: &str, device: &str, code: u16) {
        if let Some(conn) = self.rooms().get(account).and_then(|r| r.get(device)) {
            let _ = conn.tx.send(Out::Close(code));
        }
    }

    /// Drops every socket, as a relay restart would.
    pub fn restart(&self) {
        for conn in self.rooms().values().flat_map(|r| r.values()) {
            let _ = conn.tx.send(Out::Close(CLOSE_RESTART));
        }
    }

    fn register(&self, account: &str, device: &str, tx: mpsc::UnboundedSender<Out>) -> u64 {
        let id = self.next.fetch_add(1, Ordering::Relaxed);
        let mut rooms = self.rooms();
        let room = rooms.entry(account.to_string()).or_default();
        let online: Vec<String> = room.keys().filter(|k| *k != device).cloned().collect();
        let _ = tx.send(text(json!({
            "type": "hello", "protocol": 1, "device_id": device, "account_id": account,
            "online": online, "ping_interval_s": 25, "max_frame": 16 + 65536,
        })));
        match room.insert(device.to_string(), Conn { id, tx }) {
            Some(old) => {
                let _ = old.tx.send(Out::Close(CLOSE_REPLACED));
            }
            None => {
                for (peer, conn) in room.iter() {
                    if peer != device {
                        let _ = conn.tx.send(text(
                            json!({"type": "presence", "device_id": device, "online": true}),
                        ));
                    }
                }
            }
        }
        id
    }

    fn unregister(&self, account: &str, device: &str, id: u64) {
        let mut rooms = self.rooms();
        let Some(room) = rooms.get_mut(account) else {
            return;
        };
        if room.get(device).is_some_and(|c| c.id == id) {
            room.remove(device);
            for conn in room.values() {
                let _ = conn.tx.send(text(
                    json!({"type": "presence", "device_id": device, "online": false}),
                ));
            }
        }
    }

    fn forward(&self, account: &str, to: &str, frame: Vec<u8>) -> bool {
        self.rooms()
            .get(account)
            .and_then(|r| r.get(to))
            .is_some_and(|c| {
                c.tx.send(Out::Message(Message::Binary(frame.into())))
                    .is_ok()
            })
    }

    fn reply(&self, account: &str, device: &str, id: u64, out: Out) {
        if let Some(conn) = self
            .rooms()
            .get(account)
            .and_then(|r| r.get(device))
            .filter(|c| c.id == id)
        {
            let _ = conn.tx.send(out);
        }
    }
}

pub async fn relay_ws(
    State(cloud): State<FakeCloud>,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> Response {
    let device = match cloud.device_of(&headers) {
        Ok(d) => d,
        Err(status) => return status.into_response(),
    };
    let hub = cloud.hub.clone();
    ws.on_upgrade(move |socket| run(hub, device.account, device.id, socket))
}

async fn run(hub: std::sync::Arc<Hub>, account: String, device: String, socket: WebSocket) {
    let Ok(src) = uuid::Uuid::parse_str(&device) else {
        return;
    };
    let (mut sink, mut stream) = socket.split();
    let (tx, mut rx) = mpsc::unbounded_channel();
    let id = hub.register(&account, &device, tx);
    let writer = tokio::spawn(async move {
        while let Some(out) = rx.recv().await {
            match out {
                Out::Message(m) => {
                    if sink.send(m).await.is_err() {
                        break;
                    }
                }
                Out::Close(code) => {
                    let _ = sink
                        .send(Message::Close(Some(CloseFrame {
                            code,
                            reason: "".into(),
                        })))
                        .await;
                    break;
                }
            }
        }
    });
    while let Some(Ok(msg)) = stream.next().await {
        match msg {
            Message::Binary(data) if data.len() > 16 => {
                let Ok(dst) = uuid::Uuid::from_slice(&data[..16]) else {
                    continue;
                };
                let mut frame = src.as_bytes().to_vec();
                frame.extend_from_slice(&data[16..]);
                let dst = dst.to_string();
                if !hub.forward(&account, &dst, frame) {
                    hub.reply(
                        &account,
                        &device,
                        id,
                        text(json!({"type": "undeliverable", "peer": dst})),
                    );
                }
            }
            Message::Text(t) if t.as_str().contains("\"ping\"") => {
                hub.reply(&account, &device, id, text(json!({"type": "pong"})));
            }
            Message::Close(_) => break,
            _ => {}
        }
    }
    hub.unregister(&account, &device, id);
    let _ = tokio::time::timeout(std::time::Duration::from_secs(1), writer).await;
}
