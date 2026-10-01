# OpenRemote design contract — the prototype design

The design system for the OpenRemote console, transcribed from the owner's
prototype (the former `frontend-prototype/`, now the repo's `src/`). The
prototype is the reference of record; this file is its durable index. The
console transcribes it, never improvises past it.

---

name: OpenRemotePrototype
colors:
  app-canvas: "#0A0C0F"        # --bg-app — the whole main canvas
  sidebar: "#0F1216"           # --bg-side — sidebar + titlebar
  hover: "rgba(255,255,255,0.045)"
  active: "rgba(255,255,255,0.07)"
  line: "rgba(255,255,255,0.07)"   # 1px separators everywhere
  text-primary: "#E8EBEF"      # --text-1
  text-secondary: "#9AA3AE"    # --text-2
  text-muted: "#5F6873"        # --text-3
  accent: "#DCA24B"            # amber — primary interactive accent
  green: "#4CB487"             # running / ok
  idle-dot: "#4A525C"
typography:
  sans:
    fontFamily: "'Inter', -apple-system, 'Segoe UI', system-ui, sans-serif"
    weights: [400, 500, 600]
  mono:
    fontFamily: "'Geist Mono', ui-monospace, Consolas, monospace"
    use: install commands, code, paths — technical content only
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
  default: 7px       # --radius — cards, boxes, pickers
  full: 9999px       # dots, send buttons, pills

## Overview

A quiet, dense, machine-room dark UI: near-black canvas with a cool cast,
one amber accent, and a single green for life. Chrome recedes; the content
(chats, tools, commands) is the surface. No shadows for depth — 1px
`rgba(255,255,255,0.07)` lines and white-alpha tint steps define structure.
Everything is small and calm: 11–14px type, 7px radii, generous but not airy
spacing.

## Colors

- **Canvas `#0A0C0F` / sidebar `#0F1216`:** two dark steps only. The sidebar
  and frameless titlebar share `--bg-side`; the main canvas is `--bg-app`.
  Hover and active states are white-alpha tints (4.5% / 7%), not new colors.
- **Text tiers:** `#E8EBEF` primary, `#9AA3AE` secondary, `#5F6873` muted —
  cool greys, not warm.
- **Amber `#DCA24B` is the one accent:** primary buttons (send), waiting
  dots, brand marks, focus. Used sparingly — most chrome is grey.
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
- **Main canvas:** one view at a time; views are flat lists/cards, not
  dashboards.

## Components

- **Status dots (`dot--*`):** 6px pills of truth — `running` green (breathing),
  `waiting` amber, `idle` hidden. The same dot language everywhere.
- **The composer family:** New chat's large box and the chat view's floating
  box share anatomy — autosizing textarea, a footer with pickers
  (Model `via` Harness) on the left, send button right. The floating chat
  composer publishes its measured height (`--cv-clear`) so the transcript's
  bottom padding keeps the last message above it, never under it.
- **Send button:** 30px circle, amber, white arrow; disabled goes quiet.
- **PickerMenu:** the one dropdown engine — opens at the cursor, searches,
  optional groups, keyboard-navigated, viewport-clamped.
- **Modals:** one family (Esc / backdrop / X close): onboarding panel, device,
  plugin, about, confirm. Panels, not surprise shapes.
- **Context ring:** a 20px SVG donut of context-window usage, hover card with
  used/remaining/auto-compact rows.
- **Reduce motion:** a first-class setting (`data-reduce-motion` attr) kills
  the breathe and update animations.

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

## OpenRemote standing adaptations (owner directives)

- **Many-agent power tool:** the console supervises many concurrent harnesses;
  a chat view is a zoom-in, not the whole product.
- **Terminology parity:** present each harness's native options (approval
  modes, model/effort tiers) with the harness's own words — never invent
  OpenRemote vocabulary for them.
- **No dead UI, ever:** controls that do nothing yet are not rendered.
- **Model selection is a reserved dock slot** until a harness advertises
  models through the daemon.
- **First run is a setup wizard** — connect, pick, start; one plain action
  per step.
- **Mobile is first-class:** the same visual language adapts to phone width,
  not a squeezed desktop.
- **Process:** build slowly; overthink single elements; the owner reviews at
  real milestones only.
- **Vocabulary (2026-10-01):** *chat* is the UI word for a conversation with a
  harness (the daemon domain says *session*; the console boundary translates).
  *machine* is a computer you own (the Devices view renames to Machines).
  *task* is the prompt text a human or automation sends.
