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
| Browser tool cards (`browserTool`, `BrowserFacts`, folding `OutRow`, work-row labels, approvals) | browser 1a2ab3d (tool cards) | d50fcdc |
| Web console pictures: the peer link answers `blob()` | browser 1a2ab3d (pictures in the chat view) | e1f99f9 |
| Picture ahead on address focus and the star / Ctrl+D | br-console 52705e9 (f118e43 was in main) | 4862afa |
| Browser keys in the shortcut catalog, own scope, focus gate | browser f6668d2, br-console 11944a8 | 286508a |
| Settings > Browser: search engine, bookmarks bar, Clear browsing data | browser 015ea58 | 114b65f |

Deleted (fully in main): `security-policy`, `br-guard`, `br-harness`,
`wip/browser-daemon`, `pr-10`, `feat/delete-chat`, `npm/github-install`,
`br-backlog`, `br-teams`.

## To port

### br-sidebar
- Machine grouping in Cloud mode and on the phone (3bacabb). **Needs a design
  call**: main keys folds, pins and paging by local project id.
- Optional: rollup of failed/running in a folded head, empty rename restores
  the first-message name, double-click to rename, a hint on "Show more".
- Obsolete: separate Pinned group, inline rename, row dots, DESIGN.md.

### Browser (browser, br-console, br-android, br-images)
- Console gaps in main's `BrowserView.jsx`: permission bar, private tabs, full
  screen, session restore (+01aee56), put away past 10, agent badge and take
  over, new-tab shortcuts and dev-server row, inline completion, bookmarks
  manager, screenshot/region to composer (through `attachments.js`
  `addAttachment`, focused composer), recording. Each brings its Settings or
  key piece from 015ea58 / f6668d2: "On startup" with session restore, site
  permissions with Reset with the permission bar, the New private tab key
  with private tabs.
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

### Plugins (br-plugins, br-plugin-ui, br-zcode, br-freebuff-tui)
Main's `plugin_driver` stays the harness base (documented, tested, used by
Freebuff). `plugin_runtime/harness.rs` is dropped; the rest is ported on top.
1. Freebuff model catalogs and live settings (53c803a, 31380b8, 3450ccb).
2. Harness contract additions from `harness.rs` (MCP servers and allowed tools
   on spawn, commands catalog, extra events and manifest fields).
3. Model and store: `contributes`, scopes, `project`.
4. `plugin_runtime` without harnesses: manifest, process tools and views,
   settings, routes, WASM, notices. Keep the 409 that names live chats.
5. Scopes and global sync; console scope UI (496492c).
6. JS UI plugins: routes, frames and bridge, CSP, composer buttons, views,
   settings (78a7035 … 6e475bd).
7. Docs and examples; Claude Code plugin layout (eafd691, 4e0909e, 740c90a,
   a001753, 2272b68).

### Smaller branches
- br-updater: rewire into main's `publish` job (signatures staged at the root,
  `latest.json` written by publish, no separate job); stop the daemon before
  the installer runs.
- br-chatmove remainder: Claude conversation carry and codex cwd resume, peer
  serving of `/workspace`, context-menu submenus, drag and confirm.
- br-chatctl: redesign against main's `PermissionWord` model; take the codex
  `never` sandbox and `OPENCODE_PERMISSION` fixes first.
- br-accounts, br-teleport: design references only.
