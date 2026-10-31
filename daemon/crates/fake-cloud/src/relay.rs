//! The relay's contract: one socket per device, binary frames `[16-byte
//! device id][payload]` forwarded between one account's devices with the
//! destination swapped for the sender, the JSON control messages
//! (`hello`, `presence`, `devices_changed`, `revoked`, `undeliverable`),
//! and the frame ceiling the hello advertises, enforced.

use std::collections::{HashMap, HashSet};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use axum::extract::State;
use axum::extract::ws::{CloseFrame, Message, WebSocket, WebSocketUpgrade};
use axum::http::HeaderMap;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use futures_util::{SinkExt, StreamExt};
use serde_json::json;
use tokio::sync::mpsc;

use crate::FakeCloud;

pub const CLOSE_REPLACED: u16 = 4001;
pub const CLOSE_REVOKED: u16 = 4003;
pub const CLOSE_ROTATED: u16 = 4005;
pub const CLOSE_RESTART: u16 = 1012;
/// RFC 6455's "message too big": the relay's answer to a frame past the
/// ceiling its hello advertises.
pub const CLOSE_TOO_BIG: u16 = 1009;

/// The largest frame the relay carries for a device: a 16-byte device id
/// plus the biggest Noise message a link ever sends. The hello advertises
/// this ceiling; a frame past it is closed, not buffered.
pub const MAX_FRAME: usize = 16 + 65536;

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
            "online": online, "ping_interval_s": 25, "max_frame": MAX_FRAME,
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

/// The credential a browser offers where it cannot offer a header: the
/// `cred.<credential>` WebSocket subprotocol. `None` when none is offered.
fn subprotocol_credential(headers: &HeaderMap) -> Option<&str> {
    headers
        .get(axum::http::header::SEC_WEBSOCKET_PROTOCOL)
        .and_then(|value| value.to_str().ok())
        .and_then(|offered| {
            offered
                .split(',')
                .map(str::trim)
                .find_map(|one| one.strip_prefix("cred."))
        })
        .filter(|credential| !credential.trim().is_empty())
}

pub async fn relay_ws(
    State(cloud): State<FakeCloud>,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> Response {
    // The header comes first, as it does on the real gateway, because the
    // daemons have always sent it there. A browser cannot set it on a WebSocket,
    // so the same credential may arrive as a subprotocol instead.
    let device = match cloud.device_of(&headers) {
        Ok(device) => device,
        Err(StatusCode::UNAUTHORIZED) => match subprotocol_credential(&headers) {
            Some(credential) => match cloud.device_with_credential(credential) {
                Ok(device) => device,
                Err(status) => return status.into_response(),
            },
            None => return StatusCode::UNAUTHORIZED.into_response(),
        },
        Err(status) => return status.into_response(),
    };
    // The selected subprotocol must be echoed back, or the browser closes the
    // socket before a byte is read.
    let ws = match subprotocol_credential(&headers) {
        Some(credential) => ws.protocols([format!("cred.{credential}")]),
        None => ws,
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
    let id = hub.register(&account, &device, tx.clone());
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
            // The advertised ceiling, enforced: a frame past it closes the
            // socket with 1009 rather than buffering it, and the device
            // reconnects, as it would against the real relay.
            Message::Binary(data) if data.len() > MAX_FRAME => {
                let _ = tx.send(Out::Close(CLOSE_TOO_BIG));
                break;
            }
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
#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::header::SEC_WEBSOCKET_PROTOCOL;

    fn offered(value: &str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(SEC_WEBSOCKET_PROTOCOL, value.parse().unwrap());
        headers
    }

    #[test]
    fn the_credential_may_ride_the_subprotocol_where_a_header_cannot() {
        assert_eq!(
            subprotocol_credential(&offered("cred.abc123")),
            Some("abc123")
        );
        // A browser may offer more than one protocol; the credential is one of
        // them, not necessarily the first.
        assert_eq!(
            subprotocol_credential(&offered("chat, cred.abc, x")),
            Some("abc")
        );
        // Nothing offered, or a malformed offer, is no credential at all.
        assert_eq!(subprotocol_credential(&HeaderMap::new()), None);
        assert_eq!(subprotocol_credential(&offered("cred.")), None);
        assert_eq!(subprotocol_credential(&offered("credential-abc")), None);
        assert_eq!(subprotocol_credential(&offered("abc")), None);
    }
}
