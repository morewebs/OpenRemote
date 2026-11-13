// The app's own commands get an ACL: with a manifest, Tauri checks every
// command against the capabilities, which name the console's webview
// ("main") only. Without one, any page at an app-local address could call
// them - and the built-in browser's tabs are webviews in the same window.
const COMMANDS: &[&str] = &[
    "daemon_info",
    "daemon_restart",
    "reveal_in_folder",
    "tray_prefs",
    "set_tray_prefs",
    "window_prefs",
    "set_window_prefs",
    "open_url",
    "browser_open",
    "browser_features",
    "browser_tabs",
    "browser_close",
    "browser_set_bounds",
    "browser_show",
    "browser_hide",
    "browser_navigate",
    "browser_back",
    "browser_forward",
    "browser_reload",
    "browser_capture",
    "browser_cdp",
    "browser_stop",
    "browser_zoom",
    "browser_find",
    "browser_print",
    "browser_devtools",
    "browser_viewport",
    "browser_clear_site",
    "browser_open_external",
    "browser_reveal",
    "browser_permission",
    "browser_pick",
    "browser_screencast",
    "browser_save_file",
    "browser_clear_data",
    "browser_share",
    // The phone's own: desktop site, the share sheet, a link's download
    // from its menu (capabilities/mobile.json grants them; no desktop
    // handler exists).
    "browser_desktop_site",
    "browser_share_sheet",
    "browser_download",
    "update_check",
    "update_install",
    "update_open_page",
];

fn main() {
    tauri_build::try_build(
        tauri_build::Attributes::new()
            .app_manifest(tauri_build::AppManifest::new().commands(COMMANDS)),
    )
    .expect("failed to run the Tauri build script")
}
