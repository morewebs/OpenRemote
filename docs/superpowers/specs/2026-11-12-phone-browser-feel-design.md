# The phone browser, feeling like a browser

The user's words: "the android browser should feel like an actual browser while
being synced across devices". The reference is Chrome for Android. The user
handed the design over ("build it, the project is yours"), so everything below
not quoted from them is the builder's call.

## Where it starts

The phone browser (`src/BrowserPhone.jsx`, the Kotlin plugin under
`src-tauri/gen/android/.../browser/`) already has Chrome's shape in many
places: a top bar that slides away as the page scrolls, pull to refresh with
its ring, a tab grid with a Private section, the overflow menu, find in page,
desktop site, the share sheet, downloads, a long-press menu with a haptic
tick, and pages for history, bookmarks and settings. Android's own edge swipe
already goes back through the page's history (the plugin's
`OnBackPressedCallback`), so edge-swipe back needs nothing new.

Syncing today: the phone runs its own daemon, and bookmarks and browser
settings sync between the account's devices as `sync.browser` notes. History
and the open tabs were deliberately kept per device.

## What changes

### The menu, as Chrome's

The user showed Chrome's menu and asked what we could add. The top row becomes
back, forward, star, download this page and reload. The list becomes:

- New tab, New private tab, **Add tab to new group** (normal tabs only)
- History, **Delete browsing data**
- Downloads, Bookmarks, **Recent tabs**
- Share, Find in page, Desktop site, (private: Let agents use this tab), Open in another browser
- Settings, **Customize new tab page**

Help & feedback, Translate and Add to Home screen are left out.

### Recent tabs and syncing open tabs

Recent tabs shows two things. **Recently closed** lists the tabs closed on this
phone, from the store's `closed` list. Each one reopens. Under it, **each of
the user's other devices** is listed with its open tabs and when they were
last updated. Tapping a tab opens it here in a new tab.

Open tabs travel as a new note, `sync.browser.tabs`, next to `sync.browser`.
The two notes differ in these ways:

- Each device sends only its own saved session: `sessionOf`'s tabs, which
  never include a private tab, as `{url, title}`, http(s) only. The receiver
  re-checks all of it.
- The receiver keeps only the latest list per device, in
  `<data>/browser/devices.json`. That file is rewritten each time, never added
  to the op log: a session changes with every navigation, and foreign rows in
  the op log are never compacted.
- The device's id and name come from the mesh's `PeerContext` and never from
  the note, so a device can only describe itself.
- Ceilings: 32 devices, 100 tabs per device, 2048-byte URLs and 300-character
  titles. A note over a ceiling is refused whole. A device that has sent
  nothing for 28 days drops out of the list.
- The note is sent on link-up, and by the existing browser watcher when this
  device's session or the setting below changes.
- A new browser setting, `sync_tabs` (default off, synced like the other
  settings), shown in the phone's settings as "Share open tabs with your
  devices". When it is off, this device sends an empty `off` note, and peers
  drop it from their lists.
- A new route, `GET /browser/devices`, answers the list for this device's
  console. The desktop's browser view can use the same route; that is a
  follow-up.

Private tabs never sync: they are not in the session, and the daemon's note
is built from the session only.

### Tab groups

Chrome's tab groups, kept small. A normal tab may belong to one group
(`tab.group`). Groups are `{id, name, color}` in the store's state, saved with
the session so they come back with the tabs. The grid shows a group as one
card with up to four of its tabs' pictures, its name and its count. Tapping
the card opens the group's own page, where the user can rename it, add a new
tab to it, ungroup it or close it. "Open in new tab" from a grouped tab's
long-press menu keeps the new tab in the group, as Chrome does. A group whose
last tab closes is gone. Groups are not synced.

### Gestures, bar and feel

- **Swipe the address bar sideways** to go to the next or previous tab of the
  same kind (normal or private), with a haptic tick. This is Chrome's toolbar
  swipe.
- **Address bar at the bottom**, a setting (`bar_bottom`, default off), as in
  current Chrome. The bar's rows stack the other way. As the page scrolls the
  bar slides down, and the page shows more of itself by growing into the space
  the bar leaves (a clip on the native view in place of the top bar's slide
  up).
- **Haptics**: the long press already ticks. A new `browser_haptic` command
  (`tick` or `confirm`) is used for a tab swipe, starring a page and closing
  a tab in the grid.
- **Reading progress**: the shell's `browser:chrome` event also carries how
  far down the page is (0 to 1). The bar draws it as a thin line along its
  page-side edge whenever no load is in progress.
- **Customize new tab page**: lists the shortcut tiles (`/browser/tiles`).
  Each can be removed, and the page on screen can be added.

## Kept from the security reviews

Everything in the task's list stays as it is: intent: links lose their grant
flags, the FileProvider paths, a fresh profile per private session, the
FLAG_SECURE rule, Safe Browsing, no mixed content, no addJavascriptInterface,
bounded blob reads, and private tabs keeping no history, no pictures and no
sync. No new code path reads a private tab's address into anything that
leaves the device. The source-pin tests assert this.

## Still to check on a real phone

None of this has run on a device. The Kotlin compiles only in CI's Android
build, and `mobile_browser.rs` only for Android.

1. The Android build compiles: `PageView.readFraction()` (the protected
   `computeVertical*` calls), `View.clipBounds`, and the `haptic` command.
2. **Bottom bar:** turn it on in Settings. The page stays put while the bar
   slides down, with no gap or flicker at the seam. Then check the keyboard,
   a rotation, find in page, and switching back to the top bar mid-scroll.
3. **Reading line:** it tracks scrolling on long pages and is absent on pages
   that don't scroll. Make sure it doesn't cost frames, since the chrome event
   now fires on every scroll and is coalesced to one per frame.
4. **Swipe along the bar:** it changes tabs with a tick. It must not open the
   address editor, fight Android's back-gesture edges or the gesture
   handle with the bar at the bottom, or move between normal and private tabs.
5. **Haptics:** a tick on swipe and on grid close, a firmer tap on star and
   new group. Below Android 11 a confirm falls back to a long-press buzz.
6. **Groups:** create, rename, add a new tab in the group, ungroup, close the
   group. "Open in new tab" from a grouped tab joins the group. Groups come
   back after the app restarts with "Continue where you left off".
7. **Recent tabs:** with a desktop and the phone on one account, the
   desktop's open tabs show on the phone with its name and "x min ago".
   Opening a private tab on the desktop adds nothing. Turning off "Share open
   tabs" on one device clears it from the other's list.
8. **Menu:** back, forward, star, download page and reload all work. Download
   page on an HTML page saves a file in Downloads. Delete browsing data opens
   the dialog. Customize new tab page adds and removes shortcuts.
9. The security list still holds. In particular, FLAG_SECURE with a private
   tab open while the group and Recent tabs pages are up.

## Not in this change

- the desktop browser view drawing Recent tabs from `/browser/devices`;
- history sync (the existing per-device decision stands);
- a reading list;
- dragging tabs into groups.
