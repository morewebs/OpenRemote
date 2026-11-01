# Freebuff Plugin Fix & Official Brand Icon Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix the Freebuff harness plugin driver so prompts are reliably submitted and processed with proper error detection and environment handling, and implement the official Freebuff brand icon across OpenRemote harness marks and plugin cards.

**Architecture:** 
1. In `OpenRemote-plugins/freebuff/driver`:
   - Split paste and Enter sequences in `session.rs` with a measured 250ms asynchronous delay to prevent React/OpenTUI state clobbering where Enter fires while `inputValue` is still empty.
   - Inherit parent environment variables and proxy configuration (`HTTP_PROXY`, `HTTPS_PROXY`, `ALL_PROXY`, auto-detecting local proxy if present) into `PtyProc::spawn`.
   - Update `logtail.rs` and `session.rs` to detect server reachability / session hold failures (e.g. `Can't reach Freebuff's servers` or `Freebuff session over; holding queued messages until rejoin`) and terminate stuck turns with clean error messages rather than hanging forever.
   - Recompile `freebuff-driver.exe` and commit both binary and driver source code to `OpenRemote-plugins`.
2. In `OpenRemote`:
   - Register `FreebuffMark` in `src/brand-marks.jsx` using Freebuff's official SVG polygon geometry (`viewBox="0 12.74 77.18 77.19"`, brand accent `#7CFF3F`) and register in `MARKS.freebuff`.
   - Add `freebuff` to `resolvePluginBrand` in `src/plugins.js` and `BRAND_ICONS` in `src/PluginIcon.jsx`.
   - Add unit tests verifying brand mark registration, icon rendering, and helper resolution.

**Tech Stack:** Rust (`portable-pty`, `serde_json`), React 18, JSX, Vitest / Node test runner.

**Spec:** The Freebuff CLI harness driver contract (`freebuff/PROTOCOL.md`) and OpenRemote brand marks specification (`src/brand-marks.jsx`).

## Global Constraints

- Never echo or print credentials, secrets, or auth tokens.
- Maintain documentation integrity and preserve existing docstrings/comments.
- Both pre-built binary (`freebuff/bin/windows-x64/freebuff-driver.exe`) and full source (`freebuff/driver/`) must be updated in `Projects/OpenRemote-plugins`.
- No regressions in OpenRemote test suite (`npm test`).

## Review Focus

1. Prompt delivery timing: rapid successive prompts must not race or fire Enter prematurely.
2. Error handling: unreachable Freebuff backend must cleanly fail the turn instead of leaving the session in permanent "working" status.
3. Proxy inheritance: proxy environment variables must be propagated to the child PTY process.
4. Optical sizing: `FreebuffMark` must balance visually with existing marks (`claude`, `codex`, `grok`, `pi`, `opencode`, `agy`).
5. Plugin card icon: `PluginIcon` must resolve `freebuff` and display the official mark rather than a fallback monogram.

---

### Task 1: Freebuff Driver Prompt Delivery, Error Handling & Proxy Support

**Files:**
- Modify: `Projects/OpenRemote-plugins/freebuff/driver/src/session.rs`
- Modify: `Projects/OpenRemote-plugins/freebuff/driver/src/ptyproc.rs`
- Modify: `Projects/OpenRemote-plugins/freebuff/driver/src/logtail.rs`
- Modify: `Projects/OpenRemote-plugins/freebuff/driver/src/protocol.rs`
- Binary: `Projects/OpenRemote-plugins/freebuff/bin/windows-x64/freebuff-driver.exe`

**Interfaces:**
- Consumes: `PtyProc`, `LogTail`, `TurnSignal`
- Produces: Reliable prompt submission in `freebuff-driver.exe`, proxy propagation, network error signal classification

- [ ] **Step 1: Write unit tests in `logtail.rs` and `session.rs` for network/session failure classification**
  - Add test `logtail::tests::detects_session_over_or_offline_errors_as_failed_turn`:
    Verify that log lines containing `Freebuff session over; holding queued messages until rejoin` or `Can't reach Freebuff's servers` or `offline` produce `TurnSignal::Failed`.
- [ ] **Step 2: Run `cargo test` in `Projects/OpenRemote-plugins/freebuff/driver` to verify the new test fails**
- [ ] **Step 3: Implement failure detection in `logtail.rs` and prompt submission timing in `session.rs`**
  - In `logtail.rs`, update `classify_line` to recognize session over / network hold lines.
  - In `session.rs`, separate bracketed paste from Enter with a 250ms delay, and check `screen` for `Can't reach Freebuff's servers` during activity checks to fail an unresponsive turn.
  - In `ptyproc.rs`, inherit environment variables (including `HTTP_PROXY`, `HTTPS_PROXY`, `ALL_PROXY`, `NO_PROXY`, or auto-detect `127.0.0.1:10808` if listening) in `PtyProc::spawn`.
- [ ] **Step 4: Run `cargo test` to verify unit tests pass**
- [ ] **Step 5: Compile release driver and update `bin/windows-x64/freebuff-driver.exe`**
- [ ] **Step 6: Commit changes to `Projects/OpenRemote-plugins`**

---

### Task 2: Freebuff Brand Mark and Plugin Icon in OpenRemote

**Files:**
- Modify: `Projects/OpenRemote/src/brand-marks.jsx`
- Modify: `Projects/OpenRemote/src/PluginIcon.jsx`
- Modify: `Projects/OpenRemote/src/plugins.js`
- Modify: `Projects/OpenRemote/src/plugins.test.js`

**Interfaces:**
- Consumes: `FREEBUFF_POLYGONS` SVG paths
- Produces: `FreebuffMark`, `MARKS.freebuff`, `resolvePluginBrand` matching `freebuff`

- [ ] **Step 1: Write failing unit test in `src/plugins.test.js`**
  - Add test: `assert.equal(resolvePluginBrand('freebuff', 'Freebuff'), 'freebuff')`
  - Add test: `assert.equal(resolvePluginBrand('morewebs/freebuff', 'freebuff-tool'), 'freebuff')`
- [ ] **Step 2: Run `npm test` in `Projects/OpenRemote` to verify test fails**
- [ ] **Step 3: Implement `FreebuffMark` in `src/brand-marks.jsx`, update `src/plugins.js` and `src/PluginIcon.jsx`**
  - Export `FREEBUFF_POLYGONS = ['77.18 12.74 77.18 32.04 19.3 32.04 19.3 89.93 0 89.93 0 12.74 77.18 12.74', '63.62 37.63 63.62 56.93 44.32 56.93 44.32 89.93 25.03 89.93 25.03 37.63 63.62 37.63']`
  - Implement `FreebuffMark()` with `viewBox="0 12.74 77.18 77.19"` and `color="#7CFF3F"`.
  - Add `freebuff: { M: FreebuffMark, optical: 1.0 }` to `MARKS`.
  - Add `freebuff` to `BRAND_ICONS` in `PluginIcon.jsx`.
  - Add `if (text.includes('freebuff')) return 'freebuff'` to `resolvePluginBrand` in `plugins.js`.
- [ ] **Step 4: Run `npm test` to verify all tests pass**
- [ ] **Step 5: Commit changes to `Projects/OpenRemote`**

---

### Task 3: Full End-to-End Verification & Finishing Development Branch

**Files:**
- Verify: Full daemon and frontend test suite
- Verify: Desktop dev app
- Push: `OpenRemote` and `OpenRemote-plugins` repos

- [ ] **Step 1: Run full test suite in `Projects/OpenRemote` (`npm test` and `cargo test --lib`)**
- [ ] **Step 2: Run end-to-end Freebuff driver test**
- [ ] **Step 3: Push changes to `morewebs/OpenRemote-plugins` (both main and master)**
- [ ] **Step 4: Execute `finishing-a-development-branch` workflow for OpenRemote**
