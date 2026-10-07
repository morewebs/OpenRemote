//! Requests between devices, inside a link: framed so many can run at once
//! and so large bodies fit in Noise's 64 KiB messages.
//!
//! A frame is `[kind][stream id, u32 big-endian][body]`:
//! - OPEN carries the request's JSON header, DATA its body in chunks, END
//!   closes it;
//! - HEAD carries the response's JSON header, RDATA its body, REND closes it;
//! - RESET aborts a stream; NOTE (stream 0) is a one-way JSON message.

use serde::{Deserialize, Serialize};

pub const OPEN: u8 = 0x10;
pub const DATA: u8 = 0x11;
pub const END: u8 = 0x12;
pub const HEAD: u8 = 0x13;
pub const RDATA: u8 = 0x14;
pub const REND: u8 = 0x15;
pub const RESET: u8 = 0x16;
pub const NOTE: u8 = 0x20;

/// Body bytes per frame, well inside one Noise message.
pub const CHUNK: usize = 48 * 1024;
pub const MAX_REQUEST: usize = 4 << 20;
pub const MAX_RESPONSE: usize = 16 << 20;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Request {
    pub method: String,
    /// Path and query, e.g. `/fs/dirs?path=%2Fhome`.
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content_type: Option<String>,
    #[serde(skip)]
    pub body: Vec<u8>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Response {
    pub status: u16,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content_type: Option<String>,
    #[serde(skip)]
    pub body: Vec<u8>,
}

impl Response {
    pub fn json(status: u16, value: &serde_json::Value) -> Self {
        Self {
            status,
            content_type: Some("application/json".into()),
            body: value.to_string().into_bytes(),
        }
    }
}

pub fn frame(kind: u8, stream: u32, body: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(5 + body.len());
    out.push(kind);
    out.extend_from_slice(&stream.to_be_bytes());
    out.extend_from_slice(body);
    out
}

pub fn parse(frame: &[u8]) -> Option<(u8, u32, &[u8])> {
    if frame.len() < 5 {
        return None;
    }
    let stream = u32::from_be_bytes([frame[1], frame[2], frame[3], frame[4]]);
    Some((frame[0], stream, &frame[5..]))
}

/// The frames that carry one request or response: the header, the body in
/// chunks, the end.
pub fn frames(head_kind: u8, stream: u32, header: &[u8], body: &[u8]) -> Vec<Vec<u8>> {
    let (data, end) = if head_kind == OPEN {
        (DATA, END)
    } else {
        (RDATA, REND)
    };
    let mut out = vec![frame(head_kind, stream, header)];
    for chunk in body.chunks(CHUNK) {
        out.push(frame(data, stream, chunk));
    }
    out.push(frame(end, stream, &[]));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bodies_split_into_chunks_and_frames_parse_back() {
        let body = vec![7u8; CHUNK * 2 + 10];
        let out = frames(OPEN, 9, b"{}", &body);
        assert_eq!(out.len(), 5, "header, three chunks, end");
        let (kind, stream, rest) = parse(&out[1]).unwrap();
        assert_eq!((kind, stream, rest.len()), (DATA, 9, CHUNK));
        assert_eq!(parse(&out[4]).unwrap().0, END);
        assert!(out.iter().all(|f| f.len() <= crate::link::MAX_PLAINTEXT));
        assert!(parse(&[1, 2]).is_none());
    }
}
