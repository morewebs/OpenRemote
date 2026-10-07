//! The daemon inside an app. Android has no sidecars, so the app runs the
//! daemon in its own process: the same API on loopback for the console,
//! no harnesses (a phone drives the user's machines and runs nothing), and
//! it lives exactly as long as the app.

use std::path::PathBuf;
use std::sync::{Arc, Mutex as StdMutex};

use axum::extract::{Query, State};
use axum::http::Uri;
use openremote_cloud::CloudConfig;
use openremote_core::Store;

use crate::app::{self, App, AppOptions};
use crate::registry::HarnessRegistry;
use crate::supervisor::Supervisor;

pub struct Embedded {
    pub app: Arc<App>,
    /// `http://127.0.0.1:<port>`, for the console.
    pub url: String,
    pub token: String,
    pub data_dir: PathBuf,
}

/// Opens the store in `data_dir` and serves the API on a loopback port the
/// OS picks. Call it on the app's tokio runtime.
pub async fn start(data_dir: PathBuf, cloud: CloudConfig) -> std::io::Result<Embedded> {
    let token = app::load_or_create_token(&data_dir)?;
    let store = Store::open(data_dir.clone()).map_err(std::io::Error::other)?;
    let store = Arc::new(StdMutex::new(store));
    let supervisor = Supervisor::new(store.clone(), HarnessRegistry::none());
    let app = App::assemble(
        AppOptions {
            data_dir: data_dir.clone(),
            token: token.clone(),
            cloud,
        },
        store,
        supervisor,
    );
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0)).await?;
    let port = listener.local_addr()?.port();
    // Known before the first request, so a sign-in can start at once.
    let _ = app.port.set(port);
    tokio::spawn(app::serve(Arc::clone(&app), listener));
    Ok(Embedded {
        app,
        url: format!("http://127.0.0.1:{port}"),
        token,
        data_dir,
    })
}

/// The OS handed the app the sign-in's return address
/// (`space.moreweb.openremote:/cloud/callback?code=..&state=..`): finish
/// the sign-in exactly as the loopback callback would. True when it did.
pub async fn sign_in_returned(app: &Arc<App>, url: &str) -> bool {
    let Some((_, query)) = url.split_once('?') else {
        return false;
    };
    let Ok(uri) = format!("/cloud/callback?{query}").parse::<Uri>() else {
        return false;
    };
    let Ok(query) = Query::try_from_uri(&uri) else {
        return false;
    };
    crate::cloud_http::callback(State(Arc::clone(app)), query)
        .await
        .status()
        .is_success()
}
