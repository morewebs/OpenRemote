// Prevents an additional console window on Windows in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    #[cfg(target_os = "windows")]
    {
        // dev builds expose the CDP port so the real window can be
        // profiled from outside (fps, gpu state). Release never does.
        // 9333: Edge's background process squat-claims 9222 on this
        // machine, and WebView2 fails its debug port silently.
        // OPENREMOTE_CDP_PORT picks another, so a second dev instance (a
        // test run beside the one in use) doesn't fight for the first's.
        #[cfg(debug_assertions)]
        {
            let port = std::env::var("OPENREMOTE_CDP_PORT").unwrap_or_else(|_| "9333".into());
            std::env::set_var(
                "WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS",
                format!("--remote-debugging-port={port}"),
            );
        }
    }
    openremote_ui_desktop_lib::run()
}
