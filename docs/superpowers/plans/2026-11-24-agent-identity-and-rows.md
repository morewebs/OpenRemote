# Agent Identity And Rows Implementation Plan

> **For agentic workers:** Use the executing-plans skill. Steps use checkboxes for tracking.

**Goal:** Make chats easy to tell apart on desktop, phone, and web before adding triage cards.

**Architecture:** A chat's session id is its stable identity. Start with one shared avatar component that derives initials and a color without storing new metadata. Add custom pictures to the same component when session storage and authorized asset delivery are ready.

**Tech Stack:** React 19, CSS, Node test runner, Rust daemon in later slices.

**Spec:** Cortext feature specification, Phase 1: Identity & List Clarity.

Desktop, the phone drawer, and the web console already share `Sidebar.jsx`.
`MobileView.jsx` lists paired devices and opens `MachineModal.jsx`; the
modal, not the device list, owns those chat rows. `AgentsSection.jsx` manages
worker permissions rather than listing individual workers.

## Global Constraints

- OpenRemote's existing App marks stay available; initials identify the chat, not its App.
- Private chats stay local. Do not send names or images to a third-party avatar service.
- Respect privacy mode, compact density, and both reduced-motion settings.

## Review Focus

- Unicode titles, punctuation-only titles, and titles that change after a rename.
- One identity across the desktop list, phone drawer, workers, and machine details.
- Existing open, rename, drag, and overflow-menu behavior must remain intact.
- Later slices must verify that a different device cannot read a private chat's avatar or preview.

### Slice 1: Default avatars

- [x] Test Unicode initials, empty titles, deterministic colors, and stable colors across renames in `src/agent-avatar.test.js`.
- [x] Implement `defaultAgentAvatar(id, title)` in `src/agent-avatar.js`; render it with optional App attribution in `src/AgentAvatar.jsx`.
- [x] Wire the avatar into `Sidebar.jsx`, which also renders the phone drawer and web list, plus `TeamBar.jsx` and `MachineModal.jsx`. Display initials with the privacy treatment that covers chat titles.
- [x] Verify the desktop, phone, and worker rendering in a browser preview. Run `npm test`, `npx vite build`, and the web build.

### Later slices

- [ ] Add authenticated custom-picture upload, retrieval, and reset. Test image bounds and format, then verify Local and Cloud reachability before exposing the picker.
- [ ] Extract shared chat row presentation and add the activity, attention, App, host, and preview fields without adding another source of status truth.
- [ ] Add the quick actions and list filters to the phone shell. Keep the existing chat drawer navigation and keyboard behavior.
