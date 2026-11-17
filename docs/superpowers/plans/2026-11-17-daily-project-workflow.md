# Daily project workflow implementation plan

> **For agentic workers:** Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Complete project organization, project-aware search, text draft recovery and expandable file cards.

**Architecture:** Pure modules own project ordering/filtering and draft persistence. Sidebar and composer components consume these modules, retaining existing menus, APIs and attachment ownership. A file-card adapter normalizes existing tool inputs.

**Tech Stack:** React 19, existing CSS, Node test runner. No new dependencies.

**Spec:** ../specs/2026-11-17-daily-project-workflow-design.md

## Global constraints

- Preserve all pre-existing edits. Work on the existing feature branch; no merge, push or release.
- Keep current menu focus and phone behavior; no new native command or daemon API.
- Arrangement is local; drafts use session storage with memory fallback.
- No live paid agent turns; UI verification uses sample data.
- File contents come from tool events, never a fresh disk read.

## Review focus

- Loading lists must not erase persisted project arrangement.
- Search must show matching empty projects and all matches beyond pagination.
- Bulk read must include waiting/paged chats without hiding approvals.
- Draft acknowledgements must not clear newer typing; storage denial must not break input.
- File tools differ across Apps; an empty replacement is a deletion, not an unrecognized tool.

### Task 1: Project organization and search

**Files:** Create src/project-workflow.js and src/project-workflow.test.js; modify src/Sidebar.jsx and src/sidebar.css.

**Interfaces:** orderProjects(projects, rows, preferences), moveProject(projects, id, delta, preferences), filterProjectRows(projects, rows, query, projectId), projectRows(project, rows). Preferences are { pins: string[], order: string[], sort: 'manual'|'name'|'recent' }.

- [x] Write behavioral tests for pinned/manual/name/recent ordering, movement boundaries, damaged storage, name/path/title matching, empty projects and full project membership.
- [x] Run node --test src/project-workflow.test.js; observe failure before implementation.
- [x] Implement rules and wire menus, filter chip, bulk read and full match pagination. Replace Remove shortcut with accessible project actions; preserve confirmation.
- [x] Run focused tests and inspect the real sample UI.

### Task 2: Draft recovery

**Files:** Create src/composer-drafts.js, src/composer-drafts.test.js and src/useComposerDraft.js; modify src/NewChat.jsx and src/ChatView.jsx.

**Interfaces:** createDraftStore(storage) returns read/write/acknowledge/subscribe; useComposerDraft(key) returns [text, setText, acknowledge]. Acknowledge only clears an unchanged sent value.

- [x] Test per-key and pane isolation, reload, clearing, denied storage and delayed acknowledgement preserving newer text. Storage contains plain text, so there is no JSON format to corrupt.
- [x] Run node --test src/composer-drafts.test.js; observe failure.
- [x] Implement store/hook and use in both composers, preserving failed-send recovery.
- [x] Run focused tests; verify navigation and reload in the UI.

### Task 3: Expandable file cards

**Files:** Create src/file-change-card.js, src/file-change-card.test.js, src/FileChangeCard.jsx and src/file-change-card.css; modify src/ChatView.jsx.

**Interfaces:** fileChangeCard(item) returns normalized path, label, counts, numbered lines and status, or null for unknown tools.

- [x] Test Claude Write/Edit, Pi write/edit, Codex patch input, empty files, trailing newline, deletion, failure and non-file tools.
- [x] Run node --test src/file-change-card.test.js; observe failure.
- [x] Implement compact expandable cards in tool steps, with bounded scrolling and actual line numbers.
- [x] Verify cards using sample API events and native button controls.

### Task 4: Final verification and review

- [x] Run all console tests, production build and tab capability check.
- [x] Verify project menu keyboard handling, pinning persistence, project filter, drafts and file-card expansion in the sample UI; sorting/movement also have behavioral tests.
- [x] Request one fresh review of only these changes; resolve material findings and rerun relevant checks.
- [x] Record results and remaining daily-use gaps in docs/plans/2026-11-17-daily-driver-status.md. No release or paid live CLI validation.

## Execution ledger

- Baseline: 1,621 tests pass; production build and capability check pass.
- Ruling: implement inline in the existing codex/composer-add-tools branch to preserve the user's UI work and running preview. Only explicitly listed changes belong to this task; no staging of existing edits.
- Pre-flight: Tasks 1, 2 and 3 share JSX integration only; their pure-module interfaces are independent.
- Tasks 1-3 implemented. Full console suite passed at 1,633 tests; Codex crate passed 17 tests, clippy and formatting.
- Ruling: Codex dropped native file-change events, so its driver now forwards started/completed events including final content and failure status. Verified against locally generated CLI schema; no new wire vocabulary or paid turn.
- Review: fixed legitimate patch content mistaken for headers and collapse-others losing hidden projects' folds. Added regressions. Also preserve stored folds while projects are still loading.
- Ruling: changes remain uncommitted because this shared feature branch contains unrelated user edits in several integration files. Preserve the combined working tree rather than committing their work.
- Final console suite: 1,636 passed. Manual verification also caught confirmation focus being lost when a project menu unmounted; focus now hands off after menu teardown, and Escape was verified in the UI.
