//! What this daemon does for the user's other devices: their requests are
//! served by the same handlers as the console's, through `peer_router`,
//! which only lets through what a peer may do.

use std::sync::{Arc, Weak};

use axum::body::Body;
use futures_util::StreamExt;
use futures_util::future::BoxFuture;
use openremote_cloud::rpc::{Request, Response};
use openremote_cloud::{CloudHost, PeerContext};
use openremote_core::DeviceId;
use serde_json::Value;
use tower::ServiceExt;

use crate::app::App;

/// Marks a request as coming from another of the user's devices.
#[derive(Clone, Debug)]
pub struct PeerOrigin(pub PeerContext);

/// How reading a response body for the link ended.
enum BodyRead {
    Read(Vec<u8>),
    /// Past the ceiling one link response carries.
    TooLarge,
    /// The body could not be read: the peer gets a failure, not an empty 200.
    Failed,
}

/// Reads a response body the way the link can carry it. The ceiling exists
/// because `rpc::MAX_RESPONSE` is what the peer accepts before it resets the
/// stream, so answering past it could never arrive.
async fn read_body(body: Body) -> BodyRead {
    let mut out = Vec::new();
    let mut stream = body.into_data_stream();
    while let Some(chunk) = stream.next().await {
        match chunk {
            Ok(chunk) => {
                if out.len() + chunk.len() > openremote_cloud::rpc::MAX_RESPONSE {
                    return BodyRead::TooLarge;
                }
                out.extend_from_slice(&chunk);
            }
            Err(_) => return BodyRead::Failed,
        }
    }
    BodyRead::Read(out)
}

pub struct DaemonHost {
    pub app: Weak<App>,
}

impl CloudHost for DaemonHost {
    fn dispatch(&self, peer: PeerContext, request: Request) -> BoxFuture<'static, Response> {
        let app = self.app.upgrade();
        Box::pin(async move {
            let Some(app) = app else {
                return Response::json(503, &serde_json::json!({"error": "shutting down"}));
            };
            let mut builder = axum::http::Request::builder()
                .method(request.method.as_str())
                .uri(request.path.as_str());
            if let Some(ct) = &request.content_type {
                builder = builder.header(axum::http::header::CONTENT_TYPE, ct.as_str());
            }
            let Ok(mut http_request) = builder.body(Body::from(request.body)) else {
                return Response::json(400, &serde_json::json!({"error": "bad request"}));
            };
            http_request.extensions_mut().insert(PeerOrigin(peer));
            let router = crate::http::peer_router(Arc::clone(&app));
            let Ok(response) = router.oneshot(http_request).await;
            let status = response.status().as_u16();
            let content_type = response
                .headers()
                .get(axum::http::header::CONTENT_TYPE)
                .and_then(|v| v.to_str().ok())
                .map(str::to_string);
            let body = match read_body(response.into_body()).await {
                BodyRead::Read(body) => body,
                // An empty 200 would tell the peer its request worked and hide
                // the cause, so each failure answers with what happened: 413
                // when the answer cannot fit in one link response, 502 when it
                // cannot be read at all.
                BodyRead::TooLarge => {
                    return Response::json(
                        413,
                        &serde_json::json!({
                            "error": "this answer is too large to send to another device; ask for less at a time"
                        }),
                    );
                }
                BodyRead::Failed => {
                    return Response::json(
                        502,
                        &serde_json::json!({
                            "error": "this computer could not read the answer; try again"
                        }),
                    );
                }
            };
            Response {
                status,
                content_type,
                body,
            }
        })
    }

    fn note(&self, peer: PeerContext, note: Value) {
        if let Some(app) = self.app.upgrade() {
            // The account's Global plugins have notes of their own.
            if crate::plugin_runtime::global::is_note(&note) {
                crate::plugin_runtime::global::on_note(&app, peer, note);
                return;
            }
            crate::sync::on_note(&app, peer, note);
        }
    }

    /// A fresh link (this device or the peer restarted, or the relay
    /// reconnected): compare notes in case anything was missed.
    fn linked(&self, peer: DeviceId) {
        self.online(peer);
    }

    fn online(&self, peer: DeviceId) {
        if let Some(app) = self.app.upgrade() {
            let plugins = Arc::clone(&app);
            tokio::spawn(async move { crate::sync::sync_with(&app, peer).await });
            // The account's Global plugin records catch up the same way.
            tokio::spawn(
                async move { crate::plugin_runtime::global::sync_with(&plugins, peer).await },
            );
            // A link event is not the only thing that should trigger a compare:
            // on an account where both machines simply stay on there may never
            // be another one, and the compare is what retries a tombstone this
            // device refused and a copy a cut-off pull left behind.
            start_retry_beat(&self.app);
        }
    }

    /// Removed from the account: the synced chats here go; private ones stay.
    fn revoked(&self) {
        if let Some(app) = self.app.upgrade() {
            let purged = {
                let mut store = crate::supervisor::lock_recovering(&app.store);
                store.purge_synced()
            };
            // This callback has no reply channel, so a failed purge has to
            // reach somebody in the log or nowhere: the removal stands and the
            // account's chats would otherwise sit on disk unmentioned, which is
            // the same silence the sign-in and leave paths were fixed for.
            if let Err(e) = purged {
                eprintln!(
                    "openremote-daemon: this computer was removed from the account, but its copies of the account's chats could not be removed: {e}"
                );
            }
        }
    }
}

/// Whether the periodic compare has been started for this process. One daemon
/// has one mesh, so one beat is enough; `sync_with` itself is idempotent per
/// peer, so a double start would only double the traffic.
static RETRY_STARTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Starts the beat on the first peer that comes up. It must not wait for a link
/// event to be scheduled, but it also must not run before there is a mesh.
fn start_retry_beat(app: &Weak<App>) {
    if RETRY_STARTED.swap(true, std::sync::atomic::Ordering::SeqCst) {
        return;
    }
    let app = app.clone();
    tokio::spawn(async move {
        if let Some(app) = app.upgrade() {
            crate::sync::retry_loop(app).await;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures_util::stream;

    /// An answer past the link's ceiling is refused as too large. It used to
    /// come back as an empty 200, which reads to the peer as success.
    #[tokio::test]
    async fn a_body_the_link_cannot_carry_is_too_large_not_an_empty_success() {
        let over = openremote_cloud::rpc::MAX_RESPONSE + 1;
        match read_body(Body::from(vec![0u8; over])).await {
            BodyRead::TooLarge => {}
            _ => panic!("a body past the ceiling must not be read into memory"),
        }
    }

    /// A body that cannot be read at all is a failure with a sentence, not an
    /// empty 200.
    #[tokio::test]
    async fn a_body_that_cannot_be_read_is_a_failure_not_an_empty_success() {
        let broken = stream::once(async {
            Err::<Vec<u8>, std::io::Error>(std::io::Error::other("the reader went away"))
        });
        match read_body(Body::from_stream(broken)).await {
            BodyRead::Failed => {}
            _ => panic!("a body that cannot be read must not pass as empty"),
        }
    }

    /// The ordinary case is unchanged: everything inside the ceiling arrives.
    #[tokio::test]
    async fn a_body_that_fits_is_read_in_full() {
        match read_body(Body::from(vec![7u8; 1024])).await {
            BodyRead::Read(body) => assert_eq!(body.len(), 1024),
            _ => panic!("a body inside the ceiling is carried whole"),
        }
    }
}
