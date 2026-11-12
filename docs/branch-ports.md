# Branch ports

The feature branches forked from main at `4d28d6d` (2026-10-16). Main then
gained 540 commits and rebuilt some of the same features its own way, so the
branches are **ported, not merged**: main's implementation wins wherever both
exist, and only what main lacks is carried over, one area at a time, with its
tests. Each branch's last commit is kept as an `archive/<branch>` tag (also on
GitHub), so nothing is lost when a branch is deleted.

A branch is deleted once everything in it is ported, recorded as obsolete, or
recorded as dropped below.

## Done

| Ported | From | Commit on main |
|---|---|---|
| `reveal_in_folder` shell command | br-sidebar f09d32d, 22aee00 | be979ed, 31c6ae3 |
| Project menu; Ctrl/Cmd+Shift+N new chat in this project | br-sidebar 3b1044d, 7c4ce53 | 57c7621 |
| Archive undo on every path; polled status on unopened chats | br-sidebar 295ac62, 8074ec7 | 24935cf |
| Browser smoke fixes (`*.localhost`, Mod+Shift+B, CDP port) | browser 051803a | 1d534ff |
| Open-tabs note on delivered prompts | browser 25e69c8 | 8236744 |
| Daemon lifetime: job object, private stdio, parent watch, bounded leave | br-backlog 6872f11 | 5d972a7 |
| Dev servers in replies, "Open beside chat" | browser dba4001, 1a2ab3d (chip) | 18bc2f2 |
| Codex approvals: new request names, MCP elicitation, permissions | br-teams 18b21bc | 706608f |
| Agent teams, data: `Session.parent`, message `origin` | br-teams a5ae3da | 4416eb0 |
| Agent teams, origin through prompt, queue and steer | br-teams aeb6846 | 72ea604 |
| Agent teams, daemon: `agents_*` MCP tools, limits, reports, team stop | br-teams 5be0788, 7920984, f4ba777, e7bc340, 9335af8 | 5e81db4 |
| Agent teams, console: team model, TeamBar, origin chips, tool words | br-teams 308ac64, 26f689c | c82aded, 62d3988 |
| Agent teams, Settings > Agents tab | br-teams 0b7548a | 6df81cf |
| Agent teams, lead archive/delete asks about its workers | br-teams 7272ecc | c13b253 |
| Agent teams, workers nest under their lead in the sidebar | br-teams 7297baf | 5d18d73 |
| Images, daemon: blob store, `Prompt{text,images}`, `/blobs` routes, refusal by `honors.attachments` | br-images 01b3d02 | a51039c |
| Codex MCP tool cards (`mcpToolCall`), screenshots kept | br-images e5fccc5 | c17f429 |
| Images, console: API attachments and `blob()`, reducer pictures, `ChatImages` | br-images 2b770c1 | 6662c73 |
| Composer attachments (paste, drop, pick; `honors.attachments`; errorCopy) | br-images 5f59a28 | a545687 |
| Pictures in the chat view (user, OriginChip, tool screenshots, queue) | br-images 1a2ab3d (pictures half) | 007e28e |
| Browser tool steps (`browserTool`, `BrowserFacts`, folding `OutRow` in the work row, approvals) | browser 1a2ab3d (tool cards) | d50fcdc, 5bce97b |
| Web console pictures: the peer link answers `blob()` | browser 1a2ab3d (pictures in the chat view) | e1f99f9 |
| Picture ahead on address focus and the star / Ctrl+D | br-console 52705e9 (f118e43 was in main) | 4862afa |
| Browser keys in the shortcut catalog, own scope, focus gate | browser f6668d2, br-console 11944a8 | 286508a, 1b8f209 |
| Settings > Browser: search engine, bookmarks bar, Clear browsing data | browser 015ea58 | 114b65f, 762a244 |
| Private tabs (Ctrl+Shift+N, Detective mark, kept from closed list and history, share with agents) | browser 0d3211a, f6668d2 (the key) | 2d0311b |
| Permission bar; site permissions in the site menu and Settings > Browser with Reset | browser 0d3211a, 015ea58 (site permissions) | ea36bfd |
| Full screen: a page in full screen fills the window | browser 0d3211a | 0c703f2 |
| Session restore, "On startup" in Settings > Browser | browser 0d3211a, 01aee56, 015ea58 (On startup) | f9c17fd |
| Put pages away past ten open | browser 0d3211a | a36a3a6 |
| Agent badge and take over (an agent's tab joins without taking the view) | browser 0d3211a | 5fffff9 |
| New tab page shortcuts (daemon tiles) and the dev-server row | browser 0d3211a, dfcf317, 08d720e | f220976 |
| Address bar inline completion (Tab or Right takes it) | browser 0d3211a, 015ea58 (`completion`) | f4f33e9 |
| Bookmarks manager (a dialog: search, reorder, move, rename, delete) | browser 0d3211a | bcceeb0 |
| Screenshot and region to the focused composer; `setFocusedComposer` released on unmount | browser 0d3211a, b06f02d (`screenshotAttachment`) | c0df86f |
| Recording a tab to Downloads (WebM, 90 s, pill) | browser 0d3211a | 8f2e720 |
| Freebuff model catalogs: `models_file` and capture age, `settings: "live"`, catalog refresh route, model rows with `detail`/`note` | br-freebuff-tui 53c803a, 31380b8, 3450ccb | 67350a3, 5d24a5f |
| Harness contract additions on main's `plugin_driver`: `mcp_servers` and `allowed_tools` on spawn, `--commands` catalog, `commands_listed`, ask `interrupts_turn`/`session_grant`, manifest `permission_modes`, `default_permission`, `fast`, `mcp`, `commands`, `one_prompt_at_a_time`, `steers_mid_turn` | br-plugins 7f75c88 (`plugin_runtime/harness.rs`, dropped) | 43d33dd |
| Plugin model and store: `scope` (Global, Machine, ProjectLocal, Project), `project` and `contributes` on an installed plugin, serde defaults so older stores load, a harness summary backfilled | br-plugins 7f75c88 (openremote-core `model.rs`, `lib.rs`) | bfa6a86 |
| Plugin runtime without harnesses: manifest `entry` and `contributes` (tools, composer inputs, views, settings) checked at review, process tools and view requests, WASM agent tools on wasmi 0.46 (no WASI, fuel and memory limits, network only with the power, vetted public addresses), private settings with secrets answered set or unset, console-only routes, the wasmi notice | br-plugins 7f75c88 (`plugin_runtime/` minus `harness.rs`, `scope.rs`, `global.rs`, `install.rs`), 344ea0b, 05ab28b, ab5e1ab | 25e83f8, 7e0a83b |
| Plugin scopes and Global sync: Machine, Project-local, Project (files in the repo, run only while they hash to what was approved) and Global; a project's plugin rides only its chats; a Global record from another device is listed for review and installs nothing on its own; `/sync/plugins` peer routes; the console's scope picker and review lists | br-plugins 7f75c88 (`scope.rs`, `global.rs`, `install.rs`, the store's Global records), 496492c | db89194, 06c0377, a8c1d16 |
| JavaScript UI plugins: console-only routes for a plugin's frame (the chat calls through the runtime's own gate, `fetch` with secrets written in toward their `sendTo` and the internet only), a project's plugin kept to its project's chats, frames sandboxed to scripts alone behind a source-and-origin message gate, the bridge, the CSP's own origins, composer buttons that write only after a click, plugin views as panes and sidebar tiles, Settings > Plugins | br-plugin-ui 78a7035, 64ee1bb, c3608f7, 6e475bd, 029bfbe, 1a54693, b23ee91, 4b92891 | fb718f9, f88d32d |
| Plugins in a sibling layout (built from Claude Code's): rewritten for Claude Code at install, the review's "Adapted" and "Left out" lists, top-level catalogs with `pluginRoot`, zip downloads pinned by sha256 and unpacked from one folder, `Http::download` | br-zcode eafd691, 4e0909e, 740c90a, a001753, 2272b68 | 1b7b303 |
| Plugin docs (`docs/plugins/`: protocol, authoring, JS API) written for what this OpenRemote does; the toy example as one compiled program (tools and a JSON-lines harness driver); dictation's local server only at a typed address; an e2e that installs and runs the examples; the Freebuff plan in `docs/plans/` | br-plugins 03499db, 56e78fb, 76d1eaf; br-plugin-ui 2db6a60, ee8e0ec, 5a16093, 7b9b120; br-freebuff-tui f9629f6 | 76210ef |

Deleted (fully in main): `security-policy`, `br-guard`, `br-harness`,
`wip/browser-daemon`, `pr-10`, `feat/delete-chat`, `npm/github-install`,
`br-backlog`, `br-teams`.

### Ready to delete: br-console

Every br-console commit (head 52705e9; all of them are also in `browser`) is
ported or obsolete. The branch is not deleted here; its `archive/br-console`
tag keeps it.

| Commit | What | State |
|---|---|---|
| f6668d2 | Browser keys in the shortcut catalog | ported (286508a, 2d0311b for the private-tab key) |
| 015ea58 | Settings > Browser; pure rules (history days, folders, permission words, inline completion) | ported (114b65f, 762a244, ea36bfd, f9c17fd, f4f33e9); `openremote://` address-bar pages obsolete |
| b06f02d | Pure rules: session, put-away, attachments, key replay, floating surfaces, devservers | ported (f9c17fd, a36a3a6, c0df86f; devservers.js was in main); the picked element as a chip is obsolete (main's picker inserts text through composer-insert.js) |
| 0d3211a | The Browser view in parts and everything it does | ported (Tasks 4 and 5 rows above); obsolete: the BrowserTabStrip/Toolbar split, `openremote://` history, downloads and settings pages (main has the history dialog and the downloads menu), the element-picker chip |
| 44e53b7 | useBrowserPage refactor | obsolete (main keeps placement in the view) |
| dfcf317 | Dev-server row snapshot fix | ported (f220976: the list is kept in state, since `recentDevServers()` builds a new array per read) |
| 11944a8 | `focused` prop, Browser key group | ported (1b8f209) |
| 08d720e | New-tab button calls openTab with nothing; downloads page Clear all; editor closed twice | in main in effect (openTab(), `if (!editor) return`); downloads page obsolete |
| 01aee56 | Restore when agents opened tabs first | ported (f9c17fd) |
| f118e43 | Bookmarks bar picture ahead | in main |
| 52705e9 | Picture ahead on address focus and the star | ported (4862afa) |

### Ready to delete: br-plugins, br-plugin-ui, br-freebuff-tui, br-zcode

Every commit on these four (heads 56e78fb, 7b9b120, 3450ccb, 2272b68; each
kept as its `archive/<branch>` tag) is ported or obsolete. The branches are
not deleted here.

br-plugins:

| Commit | What | State |
|---|---|---|
| 7f75c88 | Runtimes, scopes, sync, routes | ported: harness parts onto main's `plugin_driver` (43d33dd, f646b13; `plugin_runtime/harness.rs` and the JSON-RPC harness obsolete, main's driver contract wins), model and store (bfa6a86), runtime (25e83f8, c4be417), scopes and Global sync (db89194, a8c1d16, e232841, 7df4c9a, c332f8c; sync lists a plugin for review instead of installing it) |
| ab5e1ab | src-tauri lock for wasmi | ported (25e83f8, regenerated) |
| 05ab28b | wasmi notices | ported (7e0a83b) |
| 03499db | protocol.md, authoring.md | ported (76210ef, rewritten for main: no harness/* methods) |
| 76d1eaf | toy-harness and text-tools examples | text-tools in main (e88d61a); toy-harness ported (76210ef) as one compiled program, since Node from PATH and the JSON-RPC harness are not main's |
| 496492c | Console scopes and runtimes | ported (06c0377) |
| 344ea0b | Entry only when something runs | ported (25e83f8) |
| 56e78fb | Docs: when an entry is needed | ported (76210ef) |

br-plugin-ui:

| Commit | What | State |
|---|---|---|
| 78a7035 | Daemon routes for frames | ported (fb718f9: fetch and the chat calls; files and settings routes were main's) |
| 64ee1bb | Frames, bridge, CSP | ported (f88d32d, 830e4ac) |
| c3608f7 | Composer buttons, sidebar views | ported (f88d32d, 830e4ac) |
| 6e475bd | Settings > Plugins | ported (f88d32d, 830e4ac) |
| 029bfbe | Words land after leaving a chat | ported (f88d32d), then obsolete by 830e4ac: a grant now ends when its composer goes |
| 1a54693 | A page that reloads boots again | ported (f88d32d; its doc part in 76210ef) |
| b23ee91 | CSP names the app's origins | ported (f88d32d) |
| 4b92891 | Settings defaults filled by the console | ported (f88d32d, 830e4ac) |
| ee8e0ec | Dictation | in main (e88d61a); its setting help in 76210ef |
| 7d3e75a | examples/marketplace catalog | obsolete: main's catalog is the repository's own `.claude-plugin/marketplace.json` (e88d61a) |
| 2db6a60 | js-api.md | ported (76210ef) |
| 5a16093 | Dictation: refused-key wording | in main (e88d61a) |
| 7b9b120 | Dictation: no audio in the frame | in main (e88d61a) |

br-freebuff-tui:

| Commit | What | State |
|---|---|---|
| 53c803a | Catalogs, live settings, refresh route | ported (67350a3, 596929c, e0931f6) |
| 31380b8 | Console model rows, capture age | ported (5d24a5f) |
| 3450ccb | Docs: `models_file`, `settings` | ported (5d24a5f, c11c709) |
| f9629f6 | Freebuff TUI parity plan | ported (76210ef, now `docs/plans/2026-11-02-freebuff-tui-parity.md`) |

br-zcode: its own commits are eafd691, 4e0909e, a001753, 740c90a and
2272b68, all ported (1b7b303). Its other 54 commits came in through merges
(a439843, 5dab5b5, 8082760, 2a200b2, 5d0b84f, 37e9450, 23bc1b5) and each is
still on the branch it came from: br-plugins and br-plugin-ui (above),
br-console (above), br-android, br-images and browser (the Browser list
below), br-chatmove (br-chatmove remainder below) and br-updater (c978b92).
Deleting br-zcode loses none of them.

## To port

### br-sidebar
- Machine grouping in Cloud mode and on the phone (3bacabb). **Needs a design
  call**: main keys folds, pins and paging by local project id.
- Optional: rollup of failed/running in a folded head, empty rename restores
  the first-message name, double-click to rename, a hint on "Show more".
- Obsolete: separate Pinned group, inline rename, row dots, DESIGN.md.

### Browser (browser, br-console, br-android, br-images)
- Android phone browser, as one block (5df13bd, f768051, 25aee6d, 1c6f04b,
  56371d7, cc0080f, 546c420), with 1a2ab3d's phone "<agent> is browsing"
  pill. Regenerate Cargo.lock and capabilities.
- Obsolete: `openremote://` pages, useBrowserPage refactor, split-pane browser,
  1a2ab3d's header Browser button (main's FloatingBar toggles the pane), the
  bookmarks-bar key (Ctrl+Shift+B is main's show/hide-browser key).

### br-live, br-adblock, br-sync, br-ssh
- Live views: protocol, mesh binary notes, daemon routing and `Tier::Browser`,
  shell cast, console live view, `e2e_cloud_browser.rs`. Needs a trust-model
  update (`docs/cloud-trust-model.md`) and a web-console answer for NOTE_BIN.
- Ad blocking: daemon setting and routes, WebView2 hooks and lists, plus a new
  console toggle (none exists on any branch).
- Browser import from Chromium, Firefox, Safari: unfinished (no `import/mod.rs`,
  no `safari.rs`); finish, cap imports below the bookmark sync limit.
- br-sync profile sync: **dropped** by policy (cookies and site storage stay on
  the device, `docs/cloud-trust-model.md`). Sealed store kept as a reference.
- SSH plugin: after the plugin runtime; add its missing `ui/ssh.html` and docs.

### Smaller branches
- br-updater: rewire into main's `publish` job (signatures staged at the root,
  `latest.json` written by publish, no separate job); stop the daemon before
  the installer runs.
- br-chatmove remainder: Claude conversation carry and codex cwd resume, peer
  serving of `/workspace`, context-menu submenus, drag and confirm.
- br-chatctl: redesign against main's `PermissionWord` model; take the codex
  `never` sandbox and `OPENCODE_PERMISSION` fixes first.
- br-accounts, br-teleport: design references only.
