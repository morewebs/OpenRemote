# Mobile Home Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Give the Android homescreen a quiet welcome and a compact bottom composer with honest connection guidance.

**Architecture:** Preserve NewChat and its existing picker engine. Scope layout changes to mobile, add a small pure state helper for unavailable-machine guidance, and use the existing Android keyboard insets.

**Tech Stack:** React 19, CSS, Node test runner, browser DevTools protocol.

**Spec:** docs/superpowers/specs/2026-11-14-mobile-home-design.md

## Global Constraints

- Existing Inter typography and dark theme; no new dependencies.
- Mobile touch targets at least 44px; 16px screen margins.
- Desktop layout and picker behavior remain available.
- Do not publish a new release as part of this change.

## Review Focus

- Keyboard-reduced viewport must retain a visible composer and usable options.
- Long machine and App names must not create horizontal page overflow.
- Offline and unanswered capabilities must not appear as an empty installed-App list.
- Expanded options, menus and attachments must remain reachable on small screens.
- Desktop saved setups, greeting and browser shortcut must retain their behavior.

### Task 1: Mobile home layout and state

**Files:** Modify src/NewChat.jsx, src/newchat.css, src/TitleBar.jsx, src/titlebar.css, src/App.jsx. Create src/mobile-home.js and src/mobile-home.test.js.

**Interfaces:** mobileHomeState({ machine, apps, devicesAnswered = true }) accepts a machine picker item or null, the raw App capability array or null, and the provider's inventory-answer flag; returns null when ready or { kind, title, detail, action } for guidance. action is machines, machine, or null.

- [x] Write state tests covering no machine, offline, loading, empty Apps and ready, including offline with stale Apps.
- [x] Run node --test src/mobile-home.test.js; observe the missing behavior.
- [x] Add the helper and wire one mobile status into NewChat; suppress desktop-only first-run copy on mobile.
- [x] Add the bottom layout, compact composer, expanded Chat options, touch spacing and mobile header Browser access.
- [x] Run state tests, full npm test and npm run build. Expected: all pass.
- [x] Inspect the actual UI at 360px and 390px, with reduced height, options and long labels; inspect desktop. Correct any overflow or unreachable controls.
- [x] Review the complete change and record verification.

## Execution notes

- User opted out of further questions and authorized the recommended direction; execute inline.
- Working on codex/mobile-home-redesign in the current checkout, leaving pre-existing unrelated changes untouched.
- Baseline: 1566 console tests pass.
- Final: 1574 console tests pass, production build succeeds, capability check passes. Existing build chunk-size and dynamic-import warnings remain.
- Real React component browser checks passed at 360px and 390px widths, 480px keyboard-sized height, and 320px short height. Verified long names, draft retention, Enter as newline, sending to the selected machine, picture attachments with multiline drafts, options, App/model/permission picker dismissal and an operable Effort slider.
- Verified offline, empty, pending machine inventory, pending App inventory and installed-but-disabled App states. The desktop grid, saved setups and browser shortcut remain available.
- Review findings corrected: Effort portal positioning and mobile targets, exiting picker anchor lifetime, disabled-App guidance, unanswered machine inventory and centered greeting. Independent reviewer confirmed all five fixes and reran the eight state tests.
- Physical Android keyboard behavior has not been exercised in this change; browser checks simulate the reduced available viewport. Native inset handling is unchanged.
