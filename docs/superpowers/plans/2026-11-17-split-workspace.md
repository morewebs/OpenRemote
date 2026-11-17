# Split Workspace Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [x]`) syntax for tracking.

**Goal:** Provide reliable, Windows-like docking with one sidebar layout control and readable split conversations.

**Architecture:** Keep the existing persisted split tree. Add pure workspace transformations and normalized geometry, and render pane contents as stable siblings whose rectangles change with the tree. App owns primary-view promotion and navigation; SplitArea owns transient drag/zoom and the workspace panel.

**Tech Stack:** React 19, existing Phosphor icons, CSS, node:test, Tauri 2. No dependencies or daemon changes.

**Spec:** `docs/superpowers/specs/2026-11-17-split-workspace-design.md`

## Global Constraints

- Up to four panes; keep existing session and saved-layout formats compatible.
- Existing dark theme, Inter/Geist Mono, no em dashes in UI copy.
- Preserve mobile single-pane behavior and both reduced-motion switches.
- Keep surviving pane identities and composer scopes on rearrangement and primary promotion.
- Keep existing in-flight changes. Work in the current feature branch; no installation over the running app.

## Review Focus

- Moving at the four-pane cap must work without creating a fifth pane.
- Closing the primary must preserve the survivor's draft scope and displayed route.
- Canceling a gesture must restore the exact original arrangement.
- Existing chats already visible must focus/move rather than duplicate.
- Viewport shrink and browser/sidebar docks must not make action previews lie.

### Task 1: Pure workspace operations

**Files:** Create `src/workspace-layout.js`, `src/workspace-layout.test.js`.

**Interfaces:** `workspaceGeometry(layout)` returns normalized pane and divider rectangles; `dockPane(layout, sourceId, targetId, zone)` swaps center or removes/reinserts existing leaf at edges; `presetLayout(layout, preset)` preserves existing leaf objects and creates only missing New chat leaves; `closeWorkspacePane(layout,id,primaryView)` returns `{layout,primaryView}`; `layoutFits(layout,width,height)` checks 240px horizontal and 180px vertical floors.

- [x] Write failing behavioral tests for nested moves, original/empty panes, four-pane moves, same-pane no-op, preset IDs/scopes, primary promotion, impossible rectangles, and geometry coverage.
- [x] Run `node --test src/workspace-layout.test.js`, confirm failures, implement pure operations, rerun green.

### Task 2: Stable frame, gestures, chrome, panel

**Files:** Modify `src/SplitArea.jsx`, `src/split.css`; create `src/WorkspaceLayouts.jsx`, `src/workspace-layouts.css`; adjust affected wiring tests.

**Interfaces:** SplitArea adds optional `onClosePane(id)`, `layoutControl` state/close callback from App, and uses Task 1 helpers. WorkspaceLayouts consumes layout, actual width/height, preset/select/save/restore callbacks. App triggers panel from sidebar/titlebar.

- [x] Render flat keyed panes with normalized absolute rectangles, plus independent divider overlays. Maximize keeps neighbors mounted and inert.
- [x] Add thresholded pointer drag on every pane header. Preview candidate layout, center swap, edge dock, Escape/capture-loss rollback, one release commit. Keep sidebar HTML drag supported and show honest action labels.
- [x] Add 32px header actions, hover/focus visibility, double-click maximize, compact context menu for splits/moves. Remove per-pane layout controls and duplicate chat heading in split mode.
- [x] Resize by painting normalized rectangles without pointer-rate React commits. Add keyboard divider controls and cancellation rollback.
- [x] Build footer-anchored visual preset panel, equalize, saved layout entries, save; disable smaller/impossible presets with reasons. Reuse menu dismissal/focus conventions.
- [x] Run targeted split/layout/header tests; update source-wiring assertions only where behavior intentionally changed. Browser-check gestures and absence of remounts.

### Task 3: App/sidebar navigation integration and responsive content

**Files:** Modify `src/App.jsx`, `src/Sidebar.jsx`, `src/sidebar.css`, `src/newchat.css`, `src/composer.css`, `src/chatview.css`, and tests as required.

- [x] Add one Workspace layout footer icon. Route titlebar control to the same panel; retain split shortcuts/palette.
- [x] Use unified chat/NewChat rendering for primary and secondary leaves. Preserve leaf composer scope on primary promotion; use closeWorkspacePane from all close actions.
- [x] Opening an already-visible chat focuses its pane; edge drops move the existing pane. Keyboard focus selects active pane too.
- [x] Use pane container queries for gutters, grid min-width, greeting wrapping, and composer control rows. Keep send/attachment controls reachable at 240px pane width. No duplicate full chat heading in split mode.
- [x] Check pane state survives another split, swap/dock, presets, zoom/restore, primary close. Test impossible width/height menus and navigation routing.

### Task 4: Verify and review

- [x] Run full console suite with `node --test "src/**/*.test.js" "npm/**/*.test.mjs" "scripts/**/*.test.mjs"` and frontend build.
- [x] Verify sample backend in browser at current narrow window and wide layout: drag, cancel, grid/stack/rows, keyboard divider, zoom, primary close, draft/model/project/file expansion and scroll preservation.
- [x] Request independent implementation review and fix actionable findings.
- [x] Run `git diff --check` and native debug build. Report actual result and any unverified native behavior.

## Execution

The user said "continue" after selecting the design and viewing the interactive mockup. Execute in this session without another method/approval round. Scope is one cohesive workspace change; preserve the existing branch and other edits. The responsive CSS task can be independently delegated while the frame/navigation work is implemented.

## Verification record

- Full console suite: 1,656 tests passed, zero failed, final run recorded in the session.
- Frontend production build and native Windows debug build passed. Native output is src-tauri/target/debug/openremote-ui-desktop.exe. Installed application was not replaced; native interaction was not launched during this pass.
- Browser sample backend checked at 909x883 and 1400x900: columns, rows, main/stack, grid, four-pane swap/dock, maximize/restore, original-pane close/promotion, keyboard pane focus and divider sizing, explicit picker portals, full-size App manager, bottom-pane effort menu.
- Actual two-chat composers measured 110px tall with identical bottom positions. New and existing chat panes use matching gutters and a one-row toolbar; App/effort/permission choices remain in Chat options.
- Draft, selected App/project and expanded file content survived real rearrangement. Pure tests cover canceled/stale gesture commit rules, late creation, cross-mode promotion and skewed nested resize floors. No paid live-agent turn or streaming soak was run.
- Independent review identified ownership, lifecycle, geometry, popup and entry-point issues. All actionable findings were fixed in one pass; six regression tests were watched fail and then pass. No deferred review findings.
- Existing feature branch and all changes are kept for ongoing UX iteration. No merge, push or installation performed.
