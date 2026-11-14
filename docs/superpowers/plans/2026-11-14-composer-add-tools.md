# Composer Add and Tools Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Deliver a polished + menu with working content ingestion and scoped tools across both composers.

**Architecture:** Keep document extraction on the selecting device in a cancellable worker. Extend the per-composer attachment store for pending documents and reuse the current outgoing assembly. Add a specialized presentation to the shared PickerMenu engine, plus scoped command/plugin and browser dispatch.

**Tech Stack:** React 19, Vite, Phosphor icons, PDF.js, fflate, fast-xml-parser, Node behavior tests.

**Spec:** `docs/superpowers/specs/2026-11-14-composer-add-tools-design.md`

## Global Constraints

- Preserve the existing dark theme, Apps vocabulary, native capability checks, plugin grants, and both reduced-motion preferences.
- 10 pending attachments total; 10 MB per source document; 200,000 extracted characters per document and 500,000 across documents.
- No OCR, external conversion, provider additions, spending, publishing or unrelated changes.
- The user delegated decisions and execution. Work inline on `codex/composer-add-tools` in the existing checkout; preserve the pre-existing mesh.rs edit and untracked files.

## Review Focus

- Late async results must not reappear after removal, closure, send or a split-pane focus change.
- ZIP expansion, non-text files, encrypted/scanned PDFs and malformed Office input must fail clearly and stay bounded.
- The final prompt and image envelope must fit local and peer transport limits; document-only queue/steer must retain content.
- Plugin actions must retain the initiating composer/project grant and not change permissions on menu open.
- Mobile keyboard height, platform browser support, focus restoration and stale App capability changes must not produce inaccessible or dead actions.

### Task 1: Bounded document ingestion and attachment lifecycle

**Files:** create `src/document-parse.js`, `src/document-worker.js`, `src/document-input.js`, `src/document-input.test.js`; modify `src/attachments.js`, `src/composer-attachments.js`, and their tests.

**Interfaces:** `parseDocumentBytes({name, type, bytes}) -> Promise<{text, format}>`; `takeFiles(key, files, options) -> Promise<id[]>`; pending items `{kind:'document', status:'pending'|'ready'|'error', name, text?, error?, sourceBytes}`. `outgoing` keeps its existing payload signature and includes named document reference blocks.

- [ ] Write failing tests for text/Office extraction, unsupported/oversized/binary documents, pending/remove races, 10-item and total-text limits, named outgoing blocks and complete payload bounds.
- [ ] Run the targeted tests and observe the missing behavior.
- [ ] Implement bounded Office ZIP/XML extraction and PDF.js text extraction in a worker with timeout and termination. Extend store updates and file ingestion so slots are reserved synchronously and completions update only existing items.
- [ ] Verify tests pass, then run the console suite. Commit this independently usable ingestion path.

### Task 2: Shared Add to chat presentation and controls

**Files:** modify `src/PickerMenu.jsx`, `src/ComposerAttachments.jsx`, `src/composer.css`; create `src/ComposerAddMenu.jsx`, `src/composer-add.css`, `src/composer-actions.js`, `src/composer-actions.test.js`, and `src/DocumentPreview.jsx`.

**Interfaces:** `ComposerAddMenu({composerKey, inputRef, setText, takesImages, disabled, workspace, onReference, onCommands, onPlugins, onEffort})`. `useAttachInput` retains event handlers and returns picker actions; its `button` becomes the real menu entry point. Document chips preview/retry/remove through the existing store.

- [ ] Write failing action-model tests proving text actions survive image-incompatible Apps and unsupported camera/browser/clipboard actions are not offered.
- [ ] Run tests, implement the shared PickerMenu presentation with Camera/Photos/Files tiles on phones, keyboard navigation, backdrop/back dismissal, and viewport-aware sheet sizing.
- [ ] Connect image/document/camera/clipboard input and pending/error/preview chips. Use the same plugin registration/dispatch hook in quick controls and menu actions.
- [ ] Run targeted tests and a production build; commit the menu and controls.

### Task 3: Integrate both composers and browser context

**Files:** modify `src/NewChat.jsx`, `src/ChatView.jsx`, `src/useComposerMenus.jsx`, `src/PluginComposerButtons.jsx`, `src/BrowserView.jsx`, `src/browser-placement.js`, `src/composer-insert.js`; create a tested `src/browser-composer-actions.js` dispatch module.

**Interfaces:** `menus.begin('@'|'/')` opens the existing completion flow at the saved selection. Browser requests carry `{action, composerKey}` and deliver only to that live composer. Browser action availability comes from `browser-store.js` features/tabs. No shell command additions are needed.

- [ ] Add failing tests for explicit composer delivery, stale recipient rejection, command insertion and browser capability filtering.
- [ ] Implement the dispatch and wire browser page text, screenshot, region and element actions to the existing paths. Keep page selection explicit and retain browser share notifications.
- [ ] Update both composers to use the menu and complete assembled-message bounds; ensure pending/failed documents block every send path. Preserve successful send clearing and failed-send retry.
- [ ] Run targeted tests and the console suite, then commit integration.

### Task 4: Visual verification and final review

**Files:** update design/plan status and add only necessary regression fixes.

- [ ] Run the full console suite, desktop frontend build and web frontend build, plus `git diff --check`.
- [ ] Inspect actual rendered desktop and phone menus with keyboard open, long labels, previews and error/pending states; fix observed layout problems.
- [ ] Exercise real text, DOCX, XLSX and PDF ingestion through the browser and verify resulting payloads. Record native camera/platform limitations honestly.
- [ ] Request one fresh whole-branch review, resolve material findings with regression tests, rerun affected checks, and commit final fixes.

## Execution ledger

The per-task evidence and implementation decisions live in the ignored `.superpowers/sdd/2026-11-14-composer-add-tools/progress.md` during implementation. No worktree creation or plan confirmation is required after the user's instruction to handle it.
