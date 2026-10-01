// Prevents an additional console window on Windows in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    #[cfg(target_os = "windows")]
    {
        // dev builds expose the CDP port so the real window can be
        // profiled from outside (fps, gpu state). Release never does.
        // 9333: Edge's background process squat-claims 9222 on this
        // machine, and WebView2 fails its debug port silently.
        #[cfg(debug_assertions)]
        std::env::set_var(
            "WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS",
            "--remote-debugging-port=9333",
        );
    }
    openremote_ui_desktop_lib::run()
}
