//! A minimal localhost HTTP/1.1 client for the opencode surface: JSON
//! GET/POST with basic auth, plus one long-lived SSE reader. Loopback
//! only — no TLS, no redirects, no cookies.

use openremote_harness::DriverError;
use serde_json::Value;
use std::time::Duration;

use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;

pub struct HttpClient {
    pub port: u16,
    auth: String,
}

impl HttpClient {
    pub fn new(port: u16, password: &str) -> Self {
        // OpenCode's basic auth is `opencode:<password>` (verified live).
        let encoded = base64_encode(format!("opencode:{password}").as_bytes());
        Self {
            port,
            auth: format!("Basic {encoded}"),
        }
    }

    pub async fn get(&self, path: &str) -> Result<Value, DriverError> {
        self.request("GET", path, None).await
    }

    pub async fn post(&self, path: &str, body: Value) -> Result<Value, DriverError> {
        self.request("POST", path, Some(body)).await
    }

    async fn request(
        &self,
        method: &str,
        path: &str,
        body: Option<Value>,
    ) -> Result<Value, DriverError> {
        let body = body.map(|b| b.to_string()).unwrap_or_default();
        let request = format!(
            "{method} {path} HTTP/1.1\r\nHost: localhost\r\nAuthorization: {}\r\n\
             Content-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            self.auth,
            body.len()
        );
        let mut stream = TcpStream::connect(("127.0.0.1", self.port)).await?;
        let _ = stream.set_nodelay(true);
        stream.write_all(request.as_bytes()).await?;
        // A harness must never hang the daemon: every read is bounded.
        // Read the header block, then exactly Content-Length body bytes —
        // the server keeps the connection alive, so read_to_end would hang.
        let (rx, mut tx) = stream.split();
        let mut lines = BufReader::new(rx);
        let mut head = String::new();
        let mut content_length = 0usize;
        let mut status = 0u16;
        loop {
            let mut line = String::new();
            let read = tokio::time::timeout(Duration::from_secs(30), lines.read_line(&mut line))
                .await
                .map_err(|_| {
                    DriverError::Protocol("opencode response header timed out".into())
                })??;
            if read == 0 {
                return Err(DriverError::Protocol(
                    "opencode closed the connection mid-response".into(),
                ));
            }
            let line = line.trim_end_matches(['\r', '\n']);
            if line.is_empty() {
                break;
            }
            if head.is_empty() {
                status = line
                    .split_whitespace()
                    .nth(1)
                    .and_then(|c| c.parse().ok())
                    .unwrap_or(0);
            }
            if let Some(len) = line
                .to_ascii_lowercase()
                .strip_prefix("content-length:")
                .and_then(|v| v.trim().parse().ok())
            {
                content_length = len;
            }
            head.push_str(line);
            head.push('\n');
        }
        let mut body_bytes = vec![0u8; content_length];
        if content_length > 0 {
            tokio::time::timeout(Duration::from_secs(30), lines.read_exact(&mut body_bytes))
                .await
                .map_err(|_| DriverError::Protocol("opencode response body timed out".into()))??;
        }
        let _ = tx.shutdown().await;
        if status >= 400 {
            return Err(DriverError::Harness(format!(
                "opencode {method} {path} → HTTP {status}: {}",
                String::from_utf8_lossy(&body_bytes)
                    .chars()
                    .take(200)
                    .collect::<String>()
            )));
        }
        if body_bytes.is_empty() {
            Ok(Value::Null)
        } else {
            serde_json::from_slice(&body_bytes)
                .map_err(|e| DriverError::Protocol(format!("opencode sent bad JSON: {e}")))
        }
    }

    /// One SSE subscription: `on_event` gets each parsed data payload.
    /// Runs until the connection drops or the future is dropped.
    pub async fn sse(
        &self,
        path: &str,
        on_event: impl Fn(Value) + Send + Sync + 'static,
    ) -> Result<(), DriverError> {
        let request = format!(
            "GET {path} HTTP/1.1\r\nHost: localhost\r\nAuthorization: {}\r\nConnection: close\r\n\r\n",
            self.auth
        );
        let mut stream = TcpStream::connect(("127.0.0.1", self.port)).await?;
        stream.write_all(request.as_bytes()).await?;
        let (rx, _) = stream.split();
        let mut lines = BufReader::new(rx);
        let mut in_body = false;
        let mut buffer = String::new();
        loop {
            buffer.clear();
            // ICRNL lesson: both delimiters are legal.
            match lines.read_line(&mut buffer).await {
                Ok(0) => return Ok(()),
                Ok(_) => {}
                Err(e) => return Err(e.into()),
            }
            if !in_body {
                if buffer.trim_end_matches(['\r', '\n']).is_empty() {
                    in_body = true;
                }
                continue;
            }
            let line = buffer.trim_end_matches(['\r', '\n']);
            if let Some(data) = line.strip_prefix("data: ") {
                if let Ok(value) = serde_json::from_str::<Value>(data) {
                    on_event(value);
                }
            }
        }
    }
}

// Minimal base64 (standard alphabet, padded) — avoids a crate for one
// header. Self-checked against a known vector at compile time.
const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

fn base64_encode(input: &[u8]) -> String {
    let mut out = String::with_capacity(input.len().div_ceil(3) * 4);
    for chunk in input.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = ((b[0] as u32) << 16) | ((b[1] as u32) << 8) | b[2] as u32;
        out.push(B64[(n >> 18) as usize & 63] as char);
        out.push(B64[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 {
            B64[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            B64[n as usize & 63] as char
        } else {
            '='
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_matches_known_vectors() {
        assert_eq!(base64_encode(b""), "");
        assert_eq!(base64_encode(b"f"), "Zg==");
        assert_eq!(base64_encode(b"fo"), "Zm8=");
        assert_eq!(base64_encode(b"foo"), "Zm9v");
        assert_eq!(base64_encode(b"foob"), "Zm9vYg==");
        assert_eq!(base64_encode(b"fooba"), "Zm9vYmE=");
        assert_eq!(base64_encode(b"foobar"), "Zm9vYmFy");
    }
}
