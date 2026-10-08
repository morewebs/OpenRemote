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
  search, then nav tiles - Plugins and Automations in Local, Machines in
  Cloud (grid-rows reveal, reduced-motion aware); chats grouped by project
  with collapse carets, each row led by its harness's brand mark with the
  status dot at the row's right edge; Settings pinned in the footer.
- **Settings is a view, not a popup:** a slim section rail on the left
  (New tasks, Appearance, Startup, Cloud, Daemon, Harnesses, This install),
  the panel on the right. Phone width stacks the rail into a top scroller.
- **Main canvas:** one view at a time; views are flat lists/cards, not
  dashboards.

## Components

- **Status dots (`dot--*`):** 6px pills of truth - `running` green (breathing),
  `waiting` amber, `idle` hidden. The same dot language everywhere. The
  chat header's facts dot breathes too, on the same 2.4s cycle.
- **Tool status (`cv-tool-status`):** every tool card carries its state,
  right-aligned on the name line - a breathing green dot and "Running" while
  in flight, a green check and "Done" when the result lands, a red check
  and "Failed" on error. The status flip is the beat of agent work.
- **Decision cards:** the question (where the harness asked one
  structurally) renders as content, 14px/500 `--text-1` inside the card;
  the pending state wears the brand amber (border `rgba(220,162,75,0.35)`,
  background `rgba(220,162,75,0.07)`), and the affirmative option is the
  one amber button. On answer the amber drains back to a settled card over
  0.3s and the label flips to a green "Answered: {choice}".
- **The composer family:** New chat's large box and the chat view's floating
  box share anatomy - autosizing textarea, a footer with pickers
  (Model `via` Harness) on the left, send button right. The foot's chips
  are plain ghost text (`nc-meta`: no fill, no border, the hover wash is
  the only affordance) - the film filled them as pills; the founder
  overrode that the day it shipped (2026-10-08): the pickers read as
  text, not buttons in boxes. The workspace/machine rows under the box
  share the same ghost. The floating chat composer publishes its
  measured height (`--cv-clear`) so the transcript's bottom padding keeps
  the last message above it, never under it.
- **Send button:** 30px radius-8 rounded square; amber (`--accent`) with a
  white arrow when ready, transparent with a muted arrow when empty. The
  press pulses - a scale dip to 0.8 and an amber glow bloom - the one
  recurring primary action, rewarded.
- **PickerMenu:** the one dropdown engine - opens at the cursor, searches,
  optional groups, keyboard-navigated, viewport-clamped.
- **Modals:** one family (Esc / backdrop / X close): onboarding panel, device,
  plugin, about, confirm. Panels, not surprise shapes.
- **Context ring:** a 20px SVG donut of context-window usage (2px stroke,
  round cap, track `rgba(255,255,255,0.12)`), in the composer foot left of
  the send button, hover card with used/remaining/auto-compact rows.
  Renders wherever usage is known: codex reports its window too; claude
  reports usage only, so the fill rides a nominal 200k window and the card
  says so ("~200k tokens (nominal)", "Window: not reported yet").
- **Split-diff tints:** del `rgba(248,113,113,0.18)` / add
  `rgba(76,180,135,0.20)` - a fix reads as a change, not a faint wash
  (text colors unchanged: `#F87171` / `#4CB487`).
- **Press feedback on primaries:** send and decision buttons pulse on press
  (scale ~0.9-0.8 + an amber glow bloom where amber) - implemented once
  in the two families, applies to every primary action in the console.
- **Reduce motion:** a first-class setting (`data-reduce-motion` attr) kills
  the breathe and update animations; the preference is applied before
  first paint at boot, not just carried within a session.

## Motion

- One transition curve family (`cubic-bezier(0.2,0,0,1)`-ish), 0.12–0.18s.
- The running dot breathes 2.4s (sidebar rows, the facts dot, the
  in-flight tool status); the press pulse (scale dip + amber bloom) and
  the answered decision's 0.3s amber-drain are the only other moves.
- Measured, not felt: the Archive documents that native wheel scrolling beat
  a custom takeover; don't fight the compositor.

## Do's and Don'ts

- **Do:** keep grey chrome dominant; let amber mark interaction and green
  mark life; put technical content in mono; keep the dot language consistent.
- **Don't:** add shadows for hierarchy (1px lines + tints only); introduce
  new colors beyond the set; animate beyond the breathe, the press pulse,
  and hover fades; brighten the canvas.

---

## OpenRemote standing adaptations

- **Many-agent power tool:** the console supervises many concurrent harnesses;
  a chat view is a zoom-in, not the whole product.
- **Terminology parity:** present each harness's native options (approval
  modes, model/effort tiers) with the harness's own words - never invent
  OpenRemote vocabulary for them. A model's name is its catalog row's
  display name (claude's carries its own version, "Opus 5.5"); the console
  renders it through one shared lookup (`model-display.js`), which matches
  both spellings a harness reports - the pick word its picker takes and
  the resolved id a running session reports back - so a chat's name never
  changes when the first turn resolves the id. Where the catalog has no
  row, the raw id shows verbatim in a tooltip: never hidden, never
  re-cased, never parsed by the console.
- **No dead UI, ever:** controls that do nothing yet are not rendered.
- **Model selection is a reserved dock slot** until a harness advertises
  models through the daemon.
- **First run is a setup wizard** - connect, pick, start; one plain action
  per step.
- **Mobile is first-class:** the same visual language adapts to phone width,
  not a squeezed desktop.
- **Process:** build slowly; overthink single elements; review at real
  milestones only.
- **Vocabulary (2026-10-01, Cloud 2026-10-07):** *chat* is the UI word for a
  conversation with a harness (the daemon domain says *session*; the console
  boundary translates). A *device* is any of your computers signed in to
  Cloud; a *machine* is a device that runs chats for your other devices.
  *task* is the prompt text a human or automation sends.

---

## Shipped - the parity build (2026-10-02)

Every screen the design specifies, backed by the real daemon:

- **New chat** - the greeting + composer; harness picker (installed only,
  with a pinned "Manage harnesses…" row that opens the harness manager:
  all six harnesses, each installable through its owner's own installer
  on Windows, macOS, and Linux - the command stays the daemon's, never
  shown; pi's Node.js is set up only after a calm go-ahead),
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
- **Machines** - Cloud's view of the account's devices (2026-10-07): this
  computer first with its "Make this computer a machine" row, then each
  device's card (platform, Machine/Desktop, version, online). A device's
  detail lists the synced chats it runs (from this computer's copies, so
  they show while it's offline) and, for an online machine, its harnesses -
  installed and signed in remotely through the same installer chain and
  sign-in relay. Add a machine mints a one-time Linux install command and
  notices the arrival; Remove is asked twice and says what it deletes.
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

## Cloud mode (2026-10-07)

Local is this computer's chats and needs no account. Cloud is the user's
own computers working together through moreweb, end to end encrypted:
moreweb relays ciphertext and never stores a chat.

- **The door:** Cloud shows its sign-in screen (the cloud art, one amber
  action, Back to Local) until this computer is signed in. Signing in
  happens in the system browser; first launch never asks.
- **Lists:** Local lists everything that runs on this computer (synced
  ones carry a small cloud mark). Cloud lists every synced chat, with the
  device it runs on named in muted text and a hollow ring while that
  device is away. A copy stays readable offline.
- **New chat in Cloud:** a machine picker (this computer first, then the
  account's machines, online first) and that machine's own harnesses and
  models; its folders are browsed in the app (FolderBrowserModal), never
  typed. Chats made in Cloud sync to every device.
- **A chat on another device:** prompts, stop, resume and answers travel
  to it. When they can't, the composer says why (offline, not a machine,
  signed out, gone) and offers "Make it a machine" where that is the fix.
  Synced chats can be deleted everywhere, asked twice.
- **Notices:** a thin row above the main area for a device that joined the
  account ("Not you?" - any sign-in adds one), and for this computer made
  a machine from elsewhere (Undo). Shown once each.
- **While OpenRemote is open:** a desktop that is a machine serves its
  devices only while the app runs; the copy says so.
- **Keep running in the tray:** a Settings > Startup row, on by default
  while this computer is a machine. Closing the window then hides it;
  Quit lives in the tray menu. Where no tray can be shown (Linux without
  appindicator) the row is absent.
- **Sync this chat:** a private chat's facts row offers Sync once
  signed in, asked first ("tool output included"); the chat then syncs
  like one made in Cloud, running where it is.

Deferred: the updater and its badge, auto-update, the Linux AppImage
build, signed releases, machine installers beyond Linux, moving a chat
between machines and switching
its harness mid-chat (one hand-over mechanism), the connector triggers
(pipeline/errors/review/release), the agent behind the pill, claude's
context *window* on the wire (the ring rides a nominal 200k until the
stream-json carries one), grok/pi/opencode/agy MCP wires,
per-harness approval-mode defaults (no daemon-side vocabulary surface yet).

## Shipped - the film's design, adopted (2026-10-08)

The launch film's console mock beat the app in nine places
(`DESIGN-FROM-THE-FILM.md` is the delta; the rule is where the film's
design wins, the film leads). All adopted, every value transcribed
from the film's own components:

- **Tool cards carry status** - breathing green "Running" in flight,
  green check "Done" / red check "Failed" on landing, right-aligned on
  the name line. A tool left unresolved by an interrupted turn shows
  no status word - never a false Running.
- **Decision cards** - the structured question as content, pending
  amber (border 0.35 / fill 0.07), amber affirmative, settle to grey
  over 0.3s, green "Answered: {choice}" (the choice's label, never its
  wire id).
- **Amber send with press bloom** - ready amber + white arrow, empty
  quiet, press scales 1→0.8→1 with an 18→62px amber glow; the
  everywhere-pill's send speaks the same language.
- **Not adopted - the film's filled pill chips** (its §4): the founder
  overrode them the day the adoption shipped (2026-10-08) - the
  Model/Effort/Harness/Fast pickers stay plain ghost text, no fill, no
  border, same as the workspace/machine rows. The founder's word beats
  the film.
- **Diff tints doubled** - del 0.18 / add 0.20.
- **The context ring always renders where usage is known** - moved left
  of the send button, 2px stroke, track 0.12; claude rides a nominal
  200k window (the hover card says so) until its wire carries one.
- **The facts dot breathes** (2.4s, all statuses).
- **Sidebar rows** - the harness mark leads, the title follows, the
  status dot at the row's right edge (one vertical column of statuses).
- **Press feedback on primaries** - both send buttons, the decision
  affirmative; all of it under `data-reduce-motion`.
