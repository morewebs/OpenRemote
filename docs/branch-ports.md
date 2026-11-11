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

Deleted (fully in main): `security-policy`, `br-guard`, `br-harness`,
`wip/browser-daemon`, `pr-10`, `feat/delete-chat`, `npm/github-install`.

## To port

### br-sidebar
- Machine grouping in Cloud mode and on the phone (3bacabb). **Needs a design
  call**: main keys folds, pins and paging by local project id.
- Optional: rollup of failed/running in a folded head, empty rename restores
  the first-message name, double-click to rename, a hint on "Show more".
- Obsolete: separate Pinned group, inline rename, row dots, DESIGN.md.

### Agent teams (br-teams, 13 commits; none on main)
1. ~~Codex approvals (18b21bc)~~ - done.
2. Data: `Session.parent`, `Origin` on queued and chat messages (a5ae3da).
3. Messages carry an origin through prompt, queue and steer (aeb6846).
4. Team core: `team.rs`, `supervisor/team.rs`, MCP tools, routes, `e2e_team.rs`
   (5be0788 + 7920984, f4ba777, e7bc340, 9335af8).
5. Console data, Settings > Agents, chat TeamBar, lifecycle, sidebar nesting
   (308ac64, 0b7548a, 26f689c, 7272ecc, 7297baf).

### Browser (browser, br-console, br-android, br-images)
- Images: daemon blob store and `Prompt{text,images}` (01b3d02), console images
  (2b770c1), composer attachments (5f59a28), codex `mcpToolCall` (e5fccc5).
- Console gaps in main's `BrowserView.jsx`: permission bar, private tabs, full
  screen, session restore (+01aee56), put away past 10, agent badge and take
  over, new-tab shortcuts and dev-server row, inline completion, bookmarks
  manager, screenshot/region to composer, recording.
- Settings > Browser additions (015ea58), browser keys in the catalog
  (f6668d2, 11944a8 gate), focus/Ctrl+D picture-ahead (52705e9).
- Android phone browser, as one block (5df13bd, f768051, 25aee6d, 1c6f04b,
  56371d7, cc0080f, 546c420). Regenerate Cargo.lock and capabilities.
- Obsolete: `openremote://` pages, useBrowserPage refactor, split-pane browser.

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
