# Apps and Accounts Across Machines Implementation Plan

> **For agentic workers:** Use superpowers:subagent-driven-development or superpowers:executing-plans. Verify each task against the design before moving on.

**Goal:** Install Apps on local or Cloud machines and run chats under a selected provider account.

**Architecture:** The selected daemon owns installation, account configuration and account bindings. The desktop Apps manager and composer consume its inventory and account metadata through the existing device API.

**Tech Stack:** React, Tauri, Rust, Axum, Node test runner and the Cloud peer transport.

**Spec:** `docs/superpowers/specs/2026-11-18-machine-app-accounts-design.md`

## Global constraints

Keep existing sign-ins usable. Advertise managed accounts for Codex first. Select accounts on desktop and support resuming their chats through existing clients. Only the executor stores or resolves credentials. Preserve the existing Cloud drive allowlist and local operation without Cloud.

## Review focus

- Switching machines during load or install cannot affect another machine's rows.
- Two chats started or resumed with different selections cannot inherit each other's credentials.
- Reconnect with a different provider identity cannot replace the original account.
- Existing chats, older daemons and custom endpoints retain their existing behavior.
- Cancelling a sign-in on one controller cannot stop a different run.

## 1. Daemon account and chat binding

- [ ] Add tests for private machine-scoped account metadata, stable identity on reconnect, and resolution of the same configuration on create and resume.
- [ ] Implement the account store and Codex device-auth relay in the daemon. Share the isolated configuration binding with the Codex driver. Persist optional `app_account_id` in chat creation and session storage. Reject a missing, disconnected or mismatched account before spawning.
- [ ] Advertise `app_accounts_supported` in App capabilities. Serve account records `{ id, label, status }` with status `ready` or `disconnected`. Add GET/POST `/harnesses/{id}/accounts`, PATCH `/harnesses/{id}/accounts/{account}`, POST `/harnesses/{id}/accounts/{account}/signin`, GET `/harnesses/{id}/accounts/{account}/signin/{run}` and POST on the same run's `/stop`.
- [ ] Exercise local and peer scope, an unavailable account, and distinct login run IDs. Verify the changed Rust crates and the Cloud contract.

## 2. Machine Apps manager

- [ ] Add a regression for the selected machine's inventory and installer result staying scoped to that machine.
- [ ] Adapt `HarnessManagerModal.jsx` and `harness-manager.js` to select a machine, show compact App rows, and reuse the same manager from `MachineModal.jsx`. Reuse the runtime confirmation and provider sign-in dialogue. Add account list, add, rename, disconnect and reconnect controls for Apps advertising support.
- [ ] Verify offline and loading states, retry, keyboard focus, narrow layout and navigation from the composer and machine details.

## 3. Client contract and chat selection

- [ ] Add tests for transmitting a selected account and omitting it for legacy chats, and for clearing stale choices when changing machine or App.
- [ ] Extend `src/api/daemon.js` and `src/state/console.jsx` with the account endpoints and account selection. Bind `NewChat.jsx` to the selected executor's capabilities. If a managed account is selected, pass `app_account_id` to create. Honor existing endpoint handling and show the saved account when a chat needs reconnection.
- [ ] Run the full JS suite, Rust checks and the Tauri smoke check. Verify an end-to-end device-auth login if two authorized test accounts are available.
