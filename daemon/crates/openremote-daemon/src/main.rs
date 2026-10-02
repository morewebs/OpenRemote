//! The daemon binary. Prints `READY 127.0.0.1:<port>` on stdout when it is
//! listening; all logs go to stderr. Exits when stdin closes — the desktop
//! shell owns its lifetime.

use std::sync::Arc;

use openremote_daemon::app::{self, App, AppOptions};

fn main() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("tokio runtime");
    runtime.block_on(async move {
        let data_dir = app::default_data_dir();
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
        })
        .await;

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

        app::serve(Arc::clone(&app) as Arc<App>, listener)
            .await
            .expect("serve");
    });
}
