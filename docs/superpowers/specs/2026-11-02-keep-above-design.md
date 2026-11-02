# Keep above other windows: a setting, off by default

2026-11-02

## What this is

The OpenRemote window sat in front of every other app, and there was no way
to turn that off from inside the app. The topmost flag belongs to the window
and Windows keeps it until something clears it, so whichever process set it
last owns it - and the app had no part in that. This gives the app the part:
the shell remembers whether the window should stay in front, writes the flag
itself on every launch, and offers the choice in Settings.

## What was decided

- A row in **Settings > Startup**, beside "Keep running in the tray" - the
  two are both shell window/process preferences, written by Rust, stored in
  the app's config dir. One shape, not two.
- **Off by default**, and never applied without a stored choice that says
  otherwise. A missing file, or a file from before this setting, is off.
- Applying it on launch is the point: a topmost flag left set by anything
  outside the app is corrected at the next launch instead of being stuck.

## Approaches

- **A. The shell owns it (chosen).** `window_prefs.json` beside `tray.json`
  and `window.json`; `window_prefs` and `set_window_prefs` commands; loaded
  and applied in `setup` before the first paint. Mirrors `tray.rs` exactly,
  and it is the only option that also clears a stale flag at launch.
- **B. The console owns it.** Store it with the console's other preferences
  and call `setAlwaysOnTop` over the window API. Fewer Rust lines, but it
  needs a new window capability handed to the console, and the window's
  z-order stops being the shell's business.
- **C. A plus a tray-menu check item.** Two places to set one value; dropped.

## Behaviour

- The row answers with the setting the shell holds; one that comes back the
  other way round says so instead of looking changed.
- Turning it off clears the flag immediately, not at the next launch.
- A file the shell cannot read is off, not on.

## Boundaries

- The app cannot stop another program re-pinning the window while it runs;
  the next launch, or the next toggle, puts it back to the setting.
- Android has no floating window: the row is desktop-only, like the tray row,
  and the commands live in the shell's desktop module. The Startup section is
  already hidden on a phone.

## Not this

- No keyboard shortcut, no tray-menu duplicate, no per-chat or per-pane
  variant.
