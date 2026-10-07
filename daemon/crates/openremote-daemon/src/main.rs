//! The daemon binary.
//!
//! - `openremote-daemon`: the desktop app's sidecar. Prints
//!   `READY 127.0.0.1:<port>` on stdout when it is listening (all logs go to
//!   stderr) and exits when stdin closes - the desktop shell owns its
//!   lifetime.
//! - `openremote-daemon agent`: a machine's long-running service (systemd
//!   runs it). No stdin watchdog; exits 78 when this computer isn't in
//!   OpenRemote Cloud, so the service manager stops retrying.
//! - `openremote-daemon enroll <code>`: joins this computer to the account
//!   that minted the install code.
//! - `openremote-daemon status`: exits 0 when this computer is in Cloud.
//! - `openremote-daemon unenroll`: leaves Cloud (keeps private chats).
//! - `openremote-daemon --version`.

use std::process::ExitCode;
use std::sync::Arc;

use openremote_cloud::{Cloud, CloudConfig};
use openremote_daemon::app::{self, App, AppOptions};

/// sysexits' EX_CONFIG: not enrolled, and retrying won't change that.
const NOT_ENROLLED: u8 = 78;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("tokio runtime");
    match args.first().map(String::as_str) {
        None => runtime.block_on(serve(false)),
        Some("agent") => runtime.block_on(serve(true)),
        Some("enroll") => match args.get(1) {
            Some(code) => runtime.block_on(enroll(code)),
            None => {
                eprintln!("usage: openremote-daemon enroll <code>");
                ExitCode::from(2)
            }
        },
        Some("status") => status(),
        Some("unenroll") => runtime.block_on(unenroll()),
        Some("--version" | "-V") => {
            println!("openremote-daemon {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        Some(other) => {
            eprintln!(
                "openremote-daemon: unknown command '{other}' (agent, enroll, status, unenroll, --version)"
            );
            ExitCode::from(2)
        }
    }
}

async fn serve(agent: bool) -> ExitCode {
    let data_dir = app::default_data_dir();
    let cloud = CloudConfig::from_env();
    if agent && !Cloud::open(cloud.clone(), &data_dir).enrolled() {
        eprintln!(
            "openremote-daemon: this computer isn't in OpenRemote Cloud - run `openremote-daemon enroll <code>` first"
        );
        return ExitCode::from(NOT_ENROLLED);
    }
    let token = app::load_or_create_token(&data_dir).expect("token");
    // Bind and announce BEFORE the app assembles: the registry probe
    // spawns each harness's `--version` (bounded), and the shell should
    // know the address long before that finishes. The listener's
    // backlog holds any early request until `serve` picks it up.
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
        .await
        .expect("bind loopback");
    let addr = listener.local_addr().expect("local addr");
    println!("READY 127.0.0.1:{}", addr.port());
    println!("DATA_DIR {}", data_dir.display());
    eprintln!(
        "openremote-daemon: token at {}",
        data_dir.join("token").display()
    );

    let app = App::new(AppOptions {
        data_dir: data_dir.clone(),
        token: token.clone(),
        cloud,
    })
    .await;

    if agent {
        // A service is stopped with SIGTERM (or Ctrl-C when run by hand).
        tokio::spawn(async {
            shutdown_signal().await;
            eprintln!("openremote-daemon: stopping");
            std::process::exit(0);
        });
    } else {
        // The sidecar watchdog: when the shell dies, our stdin closes, and
        // the daemon must not outlive its owner.
        tokio::spawn(async {
            use tokio::io::AsyncReadExt;
            let mut stdin = tokio::io::stdin();
            let mut scratch = [0u8; 512];
            loop {
                match stdin.read(&mut scratch).await {
                    Ok(0) | Err(_) => break,
                    Ok(_) => {}
                }
            }
            eprintln!("openremote-daemon: stdin closed, exiting");
            std::process::exit(0);
        });
    }

    app::serve(Arc::clone(&app) as Arc<App>, listener)
        .await
        .expect("serve");
    ExitCode::SUCCESS
}

async fn shutdown_signal() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};
        if let Ok(mut term) = signal(SignalKind::terminate()) {
            tokio::select! {
                _ = term.recv() => {}
                _ = tokio::signal::ctrl_c() => {}
            }
            return;
        }
    }
    let _ = tokio::signal::ctrl_c().await;
}

async fn enroll(code: &str) -> ExitCode {
    let data_dir = app::default_data_dir();
    let cloud = Cloud::open(CloudConfig::from_env(), &data_dir);
    match cloud.enroll(code.trim()).await {
        Ok(joined) => {
            // The store learns its device id at the next start too; setting
            // it now keeps a crash in between harmless.
            if let Ok(mut store) = openremote_core::Store::open(data_dir) {
                let _ = store.set_this_device(Some(joined.device_id));
            }
            println!(
                "This computer joined {}'s OpenRemote Cloud as {}.",
                joined.account.email,
                cloud.config().device_name
            );
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("openremote-daemon: couldn't join: {e}");
            ExitCode::FAILURE
        }
    }
}

fn status() -> ExitCode {
    let cloud = Cloud::open(CloudConfig::from_env(), &app::default_data_dir());
    let view = cloud.view();
    match view["account"]["email"].as_str() {
        Some(email) if cloud.enrolled() => {
            println!("In OpenRemote Cloud as {email}.");
            ExitCode::SUCCESS
        }
        _ => {
            println!("Not in OpenRemote Cloud.");
            ExitCode::FAILURE
        }
    }
}

async fn unenroll() -> ExitCode {
    let data_dir = app::default_data_dir();
    let cloud = Cloud::open(CloudConfig::from_env(), &data_dir);
    // Best effort: tell the registry this device is gone, then forget it.
    if let (Some(credential), Some(id)) = (cloud.device_credential(), cloud.device_id()) {
        let _ = cloud.registry().delete(&credential, &id.to_string()).await;
    }
    match cloud.signout().await {
        Ok(()) => {
            println!("This computer left OpenRemote Cloud.");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("openremote-daemon: {e}");
            ExitCode::FAILURE
        }
    }
}
