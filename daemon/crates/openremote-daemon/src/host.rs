//! What this daemon does for the user's other devices: their requests are
//! served by the same handlers as the console's, through `peer_router`,
//! which only lets through what a peer may do.

use std::sync::{Arc, Weak};

use axum::body::Body;
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
            let body =
                axum::body::to_bytes(response.into_body(), openremote_cloud::rpc::MAX_RESPONSE)
                    .await
                    .map(|b| b.to_vec())
                    .unwrap_or_default();
            Response {
                status,
                content_type,
                body,
            }
        })
    }

    fn note(&self, peer: PeerContext, note: Value) {
        if let Some(app) = self.app.upgrade() {
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
            tokio::spawn(async move { crate::sync::sync_with(&app, peer).await });
        }
    }

    /// Removed from the account: the synced chats here go; private ones stay.
    fn revoked(&self) {
        if let Some(app) = self.app.upgrade() {
            if let Ok(mut store) = app.store.lock() {
                let _ = store.purge_synced();
            }
        }
    }
}
