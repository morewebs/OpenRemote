# OpenRemote design contract

The design system for the OpenRemote console. This file is the reference of
record; the console transcribes it, never improvises past it.

---

name: OpenRemotePrototype
colors:
  app-canvas: "#0A0C0F"        # --bg-app - the whole main canvas
  sidebar: "#0F1216"           # --bg-side - sidebar + titlebar
  hover: "rgba(255,255,255,0.045)"
  active: "rgba(255,255,255,0.07)"
  line: "rgba(255,255,255,0.07)"   # 1px separators everywhere
  text-primary: "#E8EBEF"      # --text-1
  text-secondary: "#9AA3AE"    # --text-2
  text-muted: "#5F6873"        # --text-3
  accent: "#DCA24B"            # amber - primary interactive accent
  green: "#4CB487"             # running / ok
  idle-dot: "#4A525C"
typography:
  sans:
    fontFamily: "'Inter', -apple-system, 'Segoe UI', system-ui, sans-serif"
    weights: [400, 500, 600]
  mono:
    fontFamily: "'Geist Mono', ui-monospace, Consolas, monospace"
    use: install commands, code, paths - technical content only
  scale:
    greeting: 24px   # "What are we working on?"
    chat-title: 16px
    body: 14px       # chat messages
    ui: 13px         # most controls and labels
    dense: 12.5px    # facts rows, meta
    caption: 12px    # section titles, search
spacing:
  rhythm: 4px grid; paddings 6–16px; view gutters 32px
rounded:
  default: 7px       # --radius - cards, boxes, pickers
  full: 9999px       # dots, send buttons, pills

## Overview

A quiet, dense, machine-room dark UI: near-black canvas with a cool cast,
one amber accent, and a single green for life. Chrome recedes; the content
(chats, tools, commands) is the surface. No shadows for depth - 1px
`rgba(255,255,255,0.07)` lines and white-alpha tint steps define structure.
Everything is small and calm: 11–14px type, 7px radii, generous but not airy
spacing.

## Colors

- **Canvas `#0A0C0F` / sidebar `#0F1216`:** two dark steps only. The sidebar
  and frameless titlebar share `--bg-side`; the main canvas is `--bg-app`.
  Hover and active states are white-alpha tints (4.5% / 7%), not new colors.
- **Text tiers:** `#E8EBEF` primary, `#9AA3AE` secondary, `#5F6873` muted -
  cool greys, not warm.
- **Amber `#DCA24B` is the one accent:** primary buttons (send), waiting
  dots, brand marks, focus. Used sparingly - most chrome is grey.
- **Green `#4CB487` means running/ok:** the running dot (with a 2.4s breathe
  animation), agent activity. Never decorative.

## Typography

- Inter for all UI (400/500/600). Geist Mono strictly for technical content:
  install commands, code, paths, tool I/O.
- 12.5–13px is the working size for controls, facts, and meta; 14px for chat
  message text; 16px chat titles; 24px the New chat greeting only.

## Layout

- **Frameless window, custom titlebar (36px, `--bg-side`):** sidebar toggle,
  back/forward, centered title + about, Windows caption glyphs (Segoe MDL2).
  The whole bar is a drag region.
- **Sidebar (264px, collapsible):** mode switch (Local/Cloud), New chat tile,
  search, then nav tiles; chats grouped by project with collapse carets and
  status dots; Settings pinned in the footer.
- **Settings is a view, not a popup:** a slim section rail on the left
  (New tasks, Appearance, Startup, Daemon, Harnesses, This install), the
  panel on the right. Phone width stacks the rail into a top scroller.
- **Main canvas:** one view at a time; views are flat lists/cards, not
  dashboards.

## Components

- **Status dots (`dot--*`):** 6px pills of truth - `running` green (breathing),
  `waiting` amber, `idle` hidden. The same dot language everywhere.
- **The composer family:** New chat's large box and the chat view's floating
  box share anatomy - autosizing textarea, a footer with pickers
  (Model `via` Harness) on the left, send button right. The floating chat
  composer publishes its measured height (`--cv-clear`) so the transcript's
  bottom padding keeps the last message above it, never under it.
- **Send button:** 30px circle, amber, white arrow; disabled goes quiet.
- **PickerMenu:** the one dropdown engine - opens at the cursor, searches,
  optional groups, keyboard-navigated, viewport-clamped.
- **Modals:** one family (Esc / backdrop / X close): onboarding panel, device,
  plugin, about, confirm. Panels, not surprise shapes.
- **Context ring:** a 20px SVG donut of context-window usage, hover card with
  used/remaining/auto-compact rows.
- **Reduce motion:** a first-class setting (`data-reduce-motion` attr) kills
  the breathe and update animations; the preference is applied before
  first paint at boot, not just carried within a session.

## Motion

- One transition curve family (`cubic-bezier(0.2,0,0,1)`-ish), 0.12–0.18s.
- The running dot breathes 2.4s. Nothing else animates by default.
- Measured, not felt: the Archive documents that native wheel scrolling beat
  a custom takeover; don't fight the compositor.

## Do's and Don'ts

- **Do:** keep grey chrome dominant; let amber mark interaction and green
  mark life; put technical content in mono; keep the dot language consistent.
- **Don't:** add shadows for hierarchy (1px lines + tints only); introduce
  new colors beyond the set; animate beyond the breathe and hover fades;
  brighten the canvas.

---

## OpenRemote standing adaptations

- **Many-agent power tool:** the console supervises many concurrent harnesses;
  a chat view is a zoom-in, not the whole product.
- **Terminology parity:** present each harness's native options (approval
  modes, model/effort tiers) with the harness's own words - never invent
  OpenRemote vocabulary for them.
- **No dead UI, ever:** controls that do nothing yet are not rendered.
- **Model selection is a reserved dock slot** until a harness advertises
  models through the daemon.
- **First run is a setup wizard** - connect, pick, start; one plain action
  per step.
- **Mobile is first-class:** the same visual language adapts to phone width,
  not a squeezed desktop.
- **Process:** build slowly; overthink single elements; review at real
  milestones only.
- **Vocabulary (2026-10-01):** *chat* is the UI word for a conversation with a
  harness (the daemon domain says *session*; the console boundary translates).
  *machine* is a computer you own (the Devices view renames to Machines).
  *task* is the prompt text a human or automation sends.

---

## Shipped - the parity build (2026-10-02)

Every screen the design specifies, backed by the real daemon:

- **New chat** - the greeting + composer; harness picker (installed only),
  model picker where the harness advertises (its own catalog, efforts as
  sublines), the Fast toggle where the harness's own fast mode is usable
  (claude `fastMode` ≥ 2.1.205, codex `fast` service tier ≥ 0.110), the
  workspace (recents + free path), the In-progress list.
- **Chat** - the rail transcript (user cards, agent text, notes, tool
  cards with In/Out, decisions in the harness's own words with its
  affirmative first), the facts row (workspace · harness · model · Fast ·
  «tool» allowed for this chat · status), stop/resume, the floating
  composer with the ContextRing where the harness reports its window
  (codex; claude reports usage but no window on the wire).
- **Machines** - hidden with remote check-in deferred (2026-10-03): the
  view's Add-a-machine flow would point at an install command that
  doesn't exist yet, and no-dead-UI is the rule. The components stay in
  the tree (MachinesView, MachineModal, AddMachineModal); the sidebar
  tile and route return when remote machines check in for real. Until
  then a stale `#machines` hash or stored history falls back to New
  chat. Previously shipped: this machine real from first boot (its
  inventory, sessions with jumps, 24h presence band), the local
  harness install chain (claude/codex/pi/opencode - their own npm
  packages; grok/agy install through their own roots, no row).
- **Plugins** - MCP installs written by hand; the needs-key lifecycle
  (keys never cross the API); the ride-along through each harness's own MCP wire (claude `--mcp-config`
  JSON string, codex `thread/start` config keyPaths).
- **Automations** - rules with real sources only (a schedule on the
  machine's own clock, a webhook with its own key), Run now, the form,
  and the everywhere-pill (the keyword parser,
  the rule-draft action card, honest refusals for connector-only kinds).
- **Settings** - a view (section rail), not a popup: new-task defaults
  (harness/model/workspace, per-harness effort tier in the harness's own
  words), appearance (compact density; the dark theme is contract), startup
  (launch at login, reopen last view vs New chat), the daemon connection
  (port, version, data dir, restart, two-step Forget), the harness list,
  reduce-motion (restored before first paint), first-run replay.

Deferred with the map's tickets: remote-machine check-in + relay + cloud
placement, the connector triggers (pipeline/errors/review/release), the
agent behind the pill, the updater, per-chat model/effort switching,
claude's context window (no wire fact yet), grok/pi/opencode/agy MCP wires,
per-harness approval-mode defaults (no daemon-side vocabulary surface yet).
