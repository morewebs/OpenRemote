# Daily driver status

Date: 2026-11-17. Source checkout: 0.6.7, with existing user edits preserved.

## Completed in this pass

- Visible project actions, including pin/unpin, movement, scoped search,
  mark all read, copy path, native folder opening, folds and confirmed removal.
- Manual/name/recent project order, persistent pins, and fold preservation
  during initial loading and filtered searches.
- Search by project, folder or chat title. Matching projects retain their
  chats, matching chats are not paged away, and approvals remain visible.
- Per-composer text recovery across navigation and page reload, with
  independent split drafts and protection against late send acknowledgements.
- Compact expandable file cards with line counts and numbered content.
  Unknown writes say Wrote rather than claiming a new file was created;
  edits label partial contents as a changed excerpt. Failures remain failures.
- Codex file-change events now reach the transcript, including their final
  diffs and failed/declined status. The installed CLI's generated schema
  supplied the event shape.

## Next gaps, in useful order

1. **Project settings and instructions.** Projects still consist of folders;
   their labels come from folder names (`src/projects.js`). No project-scoped
   default App/model or instruction editor. This is the next project-management
   slice after the menus.
2. **One workspace per parallel chat.** Agent workers still share a folder;
   separate worktrees are explicitly rejected in
   `daemon/crates/openremote-daemon/src/supervisor/team.rs`. Parallel coding
   can therefore collide even though the sidebar is easier to manage.
3. **Actual git review and undo.** `src/changes.js` folds reported tool inputs,
   not git state. Edits through shell commands are omitted. A git diff and
   checkpoint lifecycle are needed before review is dependable across Apps.
4. **First-run readiness.** `src/Onboarding.jsx` still ends before sign-in is
   required, and service failures fall back to a manual address/key form.
   The running installation reported Claude Code unavailable during this audit;
   Claude Desktop itself is not proof that a supported Claude Code CLI exists.
5. **App event parity.** Antigravity does not expose the same tool/file trace
   as Claude and Codex through its current driver. These new cards can render
   reported file data; they cannot recover data an App never emits.

## Evidence and limits

- Console: 1,636 tests passed after review corrections.
- Codex driver: 17 tests passed; crate clippy and formatting passed.
- Production frontend and tab capability checks passed.
- A Windows debug executable was built successfully, with the updated daemon
  staged beside it: `src-tauri/target/debug/openremote-ui-desktop.exe`.
  Fully quit the installed app from its tray before running this executable;
  it uses the same application identity and normal local data. The installed
  executable was not replaced. Native startup was not exercised against the
  live profile while its existing app remained running.
- Actual UI with sample API data: project pin survives reload, project search
  retains its chats, scoped search takes focus, chat and new-task drafts survive
  navigation/reload independently, acknowledged sends clear drafts, removal
  takes a second step with working Escape, and file cards expand to content.
- Independent review found two issues; both have regression coverage and fixes.
- No paid live agent turn, native folder opening or multi-day soak was run.
  This is a completed workflow slice, not a claim of full daily-driver parity.
