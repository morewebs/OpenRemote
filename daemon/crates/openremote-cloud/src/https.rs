//! A small HTTP/1.1 client for the registry and sign-in: HTTPS with the
//! Mozilla roots, or plain HTTP for a local backend. Also opens the TCP/TLS
//! stream the relay's WebSocket rides on.

use std::sync::Arc;
use std::time::Duration;

use http_body_util::{BodyExt, Full, Limited};
use hyper_util::rt::TokioIo;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::TcpStream;

/// Any byte stream a request or WebSocket can ride on.
pub trait Io: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send> Io for T {}

const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_BODY: usize = 4 << 20;

#[derive(Debug, thiserror::Error)]
pub enum HttpError {
    #[error("bad url: {0}")]
    Url(String),
    #[error("can't reach {0}")]
    Connect(String),
    #[error("{0}")]
    Protocol(String),
    #[error("timed out")]
    Timeout,
}

pub struct Response {
    pub status: u16,
    pub headers: http::HeaderMap,
    pub body: Vec<u8>,
}

impl Response {
    pub fn json(&self) -> serde_json::Value {
        serde_json::from_slice(&self.body).unwrap_or(serde_json::Value::Null)
    }
}

#[derive(Clone)]
pub struct Http {
    tls: tokio_rustls::TlsConnector,
}

impl Default for Http {
    fn default() -> Self {
        Self::new()
    }
}

impl Http {
    pub fn new() -> Self {
        let mut roots = rustls::RootCertStore::empty();
        roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
        let config = rustls::ClientConfig::builder_with_provider(Arc::new(
            rustls::crypto::ring::default_provider(),
        ))
        .with_safe_default_protocol_versions()
        .expect("ring supports the default TLS versions")
        .with_root_certificates(roots)
        .with_no_client_auth();
        Self {
            tls: tokio_rustls::TlsConnector::from(Arc::new(config)),
        }
    }

    /// A TCP stream to `uri`'s host, wrapped in TLS for `https`/`wss`.
    pub async fn connect(&self, uri: &http::Uri) -> Result<Box<dyn Io>, HttpError> {
        let host = uri
            .host()
            .ok_or_else(|| HttpError::Url(uri.to_string()))?
            .trim_start_matches('[')
            .trim_end_matches(']')
            .to_string();
        let secure = matches!(uri.scheme_str(), Some("https" | "wss"));
        let port = uri.port_u16().unwrap_or(if secure { 443 } else { 80 });
        let tcp = tokio::time::timeout(CONNECT_TIMEOUT, TcpStream::connect((host.as_str(), port)))
            .await
            .map_err(|_| HttpError::Timeout)?
            .map_err(|_| HttpError::Connect(host.clone()))?;
        let _ = tcp.set_nodelay(true);
        if !secure {
            return Ok(Box::new(tcp));
        }
        let name = rustls::pki_types::ServerName::try_from(host.clone())
            .map_err(|_| HttpError::Url(host.clone()))?;
        let tls = tokio::time::timeout(CONNECT_TIMEOUT, self.tls.connect(name, tcp))
            .await
            .map_err(|_| HttpError::Timeout)?
            .map_err(|e| HttpError::Protocol(format!("TLS with {host}: {e}")))?;
        Ok(Box::new(tls))
    }

    /// One request on a fresh connection. `body` is (content type, bytes).
    pub async fn request(
        &self,
        method: &str,
        url: &str,
        headers: &[(&str, &str)],
        body: Option<(&str, Vec<u8>)>,
    ) -> Result<Response, HttpError> {
        tokio::time::timeout(REQUEST_TIMEOUT, self.send(method, url, headers, body))
            .await
            .map_err(|_| HttpError::Timeout)?
    }

    async fn send(
        &self,
        method: &str,
        url: &str,
        headers: &[(&str, &str)],
        body: Option<(&str, Vec<u8>)>,
    ) -> Result<Response, HttpError> {
        let uri: http::Uri = url.parse().map_err(|_| HttpError::Url(url.to_string()))?;
        let stream = self.connect(&uri).await?;
        let (mut sender, conn) = hyper::client::conn::http1::handshake(TokioIo::new(stream))
            .await
            .map_err(|e| HttpError::Protocol(e.to_string()))?;
        tokio::spawn(async move {
            let _ = conn.await;
        });
        let authority = uri
            .authority()
            .map(|a| a.as_str().to_string())
            .unwrap_or_default();
        let path = uri
            .path_and_query()
            .map(|p| p.as_str().to_string())
            .unwrap_or_else(|| "/".to_string());
        let mut request = http::Request::builder()
            .method(method)
            .uri(path)
            .header(http::header::HOST, authority)
            .header(
                http::header::USER_AGENT,
                concat!("OpenRemote/", env!("CARGO_PKG_VERSION")),
            );
        for (name, value) in headers {
            request = request.header(*name, *value);
        }
        let bytes = match body {
            Some((content_type, bytes)) => {
                request = request.header(http::header::CONTENT_TYPE, content_type);
                bytes
            }
            None => Vec::new(),
        };
        let request = request
            .body(Full::new(bytes::Bytes::from(bytes)))
            .map_err(|e| HttpError::Protocol(e.to_string()))?;
        let response = sender
            .send_request(request)
            .await
            .map_err(|e| HttpError::Protocol(e.to_string()))?;
        let status = response.status().as_u16();
        let headers = response.headers().clone();
        let body = Limited::new(response.into_body(), MAX_BODY)
            .collect()
            .await
            .map_err(|e| HttpError::Protocol(e.to_string()))?
            .to_bytes()
            .to_vec();
        Ok(Response {
            status,
            headers,
            body,
        })
    }

    pub async fn json(
        &self,
        method: &str,
        url: &str,
        bearer: Option<&str>,
        body: Option<&serde_json::Value>,
    ) -> Result<Response, HttpError> {
        let auth = bearer.map(|t| format!("Bearer {t}"));
        let mut headers = vec![("accept", "application/json")];
        if let Some(auth) = &auth {
            headers.push(("authorization", auth.as_str()));
        }
        let body = body.map(|b| ("application/json", b.to_string().into_bytes()));
        self.request(method, url, &headers, body).await
    }

    pub async fn form(&self, url: &str, pairs: &[(&str, &str)]) -> Result<Response, HttpError> {
        self.request(
            "POST",
            url,
            &[("accept", "application/json")],
            Some((
                "application/x-www-form-urlencoded",
                form_encode(pairs).into_bytes(),
            )),
        )
        .await
    }
}

/// `application/x-www-form-urlencoded`, also good for query strings.
pub fn form_encode(pairs: &[(&str, &str)]) -> String {
    pairs
        .iter()
        .map(|(k, v)| format!("{}={}", percent(k), percent(v)))
        .collect::<Vec<_>>()
        .join("&")
}

fn percent(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// Decodes one query-string value.
pub fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => out.push(b' '),
            b'%' if i + 2 < bytes.len() => {
                let value = std::str::from_utf8(&bytes[i + 1..i + 3])
                    .ok()
                    .and_then(|hex| u8::from_str_radix(hex, 16).ok());
                match value {
                    Some(v) => {
                        out.push(v);
                        i += 2;
                    }
                    None => out.push(b'%'),
                }
            }
            b => out.push(b),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn forms_round_trip() {
        let encoded = form_encode(&[
            ("redirect_uri", "http://127.0.0.1:5/cloud/callback"),
            ("x", "a b&c"),
        ]);
        assert_eq!(
            encoded,
            "redirect_uri=http%3A%2F%2F127.0.0.1%3A5%2Fcloud%2Fcallback&x=a%20b%26c"
        );
        assert_eq!(percent_decode("a%20b%26c+d"), "a b&c d");
        assert_eq!(percent_decode("100%"), "100%");
    }
}
