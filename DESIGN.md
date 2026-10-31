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
  accent: "#A39BB8"            # grey-violet accent, not a blue wash
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
  default: 12px      # --radius - cards, small controls
  panel: 16px        # menus and the effort panel
  composer: 22px     # the composer box, and the chat's reply box
  full: 9999px       # dots, send buttons, pills

## Overview

A quiet, dense, machine-room dark UI: near-black canvas with a cool cast,
one amber accent, and a single green for life. Chrome recedes; the content
(chats, tools, commands) is the surface. No shadows for depth - 1px
`rgba(255,255,255,0.07)` lines and white-alpha tint steps define structure.
Everything is small and calm: 11–14px type, 12–22px radii, generous but not airy
spacing.

## Colors

- **Canvas `#0A0C0F` / sidebar `#0F1216`:** two dark steps only. The sidebar
  and frameless titlebar share `--bg-side`; the main canvas is `--bg-app`.
  Hover and active states are white-alpha tints (4.5% / 7%), not new colors.
  A third step, 3%, belongs to the unread row (see the sidebar): always below
  hover, so a mark never competes with the row's own states.
- **Text tiers:** `#E8EBEF` primary, `#9AA3AE` secondary, `#5F6873` muted -
  cool greys, not warm.
- **Amber `#DCA24B` is the one accent:** waiting dots, brand marks, the
  affirmative in a decision. Used sparingly - most chrome is grey. The send
  button and the keyboard's focus ring are not amber: they are the one white
  disc and the one white outline in the console.
- **Green `#4CB487` means running/ok:** the running dot (with a 2.4s breathe
  animation), agent activity. Never decorative.

## Typography

- Inter for all UI (400/500/600). Geist Mono strictly for technical content:
  install commands, code, paths, tool I/O.
- 12.5–13px is the working size for controls, facts, and meta; 14px for chat
  message text; 16px chat titles; 24px the New chat greeting only.

## Writing

- **A control is an action in sentence case**, and the sentence beside it
  carries the consequence: "Remove it and its key", "Unregister", "Stop all
  running chats". A noun on a button, or a consequence left to a tooltip, is
  not a control.
- **One noun per thing, everywhere:** chat, rule, machine, phone, project,
  pane, preset. The same object does not get a second word for variety, and a
  word that changes in the UI changes here too.
- **Counts drawn from measurement are numerals; small numbers in prose are
  words.** "3 chats are waiting" sits beside the count it came from; "a second
  press" and "four panes" stay in words.
- **The app says what it knows, and quotes what it does not.** A harness's or a
  daemon's own words appear in quotes with their own spelling; an unknown state
  stays unknown rather than becoming empty, and a guess is never written as a
  fact.

## Layout

- **Frameless window, custom titlebar (36px, `--bg-side`):** sidebar toggle,
  back/forward on the left; the centered title + about; then at the right end
  the split-view toggle (where VS Code keeps its split editor) and, on
  Windows, the caption glyphs - minimize, maximize/restore, close, drawn as
  MDL2's own 10px geometry rather than set in the font, because the font
  resolves some codepoints through fallback on a real Windows 10 box. macOS
  keeps its traffic lights at the left and draws no caption buttons. The
  whole bar is a drag region.
- **Sidebar (264px, collapsible):** mode switch (Local/Cloud), New chat tile,
  search, then nav tiles - Plugins and Automations in Local, Machines in
  Cloud (grid-rows reveal, reduced-motion aware); then the Needs you group,
  rendered only while it holds something: the chats whose harness is waiting
  on a person, oldest wait first, with the count in its head, and no header at
  all when nothing waits. A waiting chat appears here rather than in the lists
  below, and its row carries what the harness is asking under the title: one
  11px muted line, the decision's own first line, cut at the last whole word
  inside 56 characters and marked with an ellipsis, because the whole question
  is only ever read in the chat. The line summarises and never answers - the
  sidebar offers no approve, deny or dismiss anywhere, since a decision is
  answered where its options and what an approval wants to run are on screen
  together. Then Projects (every registered project, each with its collapse
  caret and a line saying what it holds: the chat count while the project is
  collapsed, and how many of its chats are waiting whenever any is - in the
  status column's amber, counted from the same rows the Needs you group draws,
  so the two can never disagree; the `+` in the section head, unregister on
  hover, which asks once before it acts; a project's chats grouped under it,
  and a project with none keeping its row with "0 chats" and one line saying it
  has no chats yet). An expanded project with chats and nothing waiting shows
  just its name, because the list under it is the answer. Then the flat Chats
  list (every chat that belongs to no project), each row led by
  its harness's brand mark with the status dot at the row's right edge; the
  footer is an icon row - Account, Mobile, Metrics, Settings - spaced evenly.
  A chat that moved after it was last read lifts one white-alpha step (3%):
  no colour, no count, nothing in the status column, and no change to its
  order - lists never sort by what is unread, so nothing jumps under the
  pointer. Opening the chat while the window is actually being looked at
  (visible and focused) clears the mark; a chat left open in a background
  window stays unread. A list head - a project, the Chats section - lifts one
  text tier while anything inside it is unread.
  The lists page rather than running on: a project draws five chats and the
  flat Chats list fifteen, and a quiet "Show N more" line under the rows
  reveals the rest of that list for this session. A chat can be pinned from
  its own row menu: its row moves to the top of its project's list (or of the
  Chats list, for one that belongs to no project) in the order the pins were
  made, newest first, and wears a pin mark; Unpin in the same menu puts it
  back, and a pin whose chat is gone is simply not drawn. The same menu moves
  a chat to another project: Move to project opens this machine's project
  picker where the menu was, "No project" first and the chat's current project
  marked, and the row rejoins the list under the project it went to once the
  daemon has said so. A chat with a turn in flight is refused there and the
  daemon's own sentence says why; a running chat whose turn is over restarts
  its harness in the new folder, and the transcript keeps a line where it
  moved. Whether the agent's own conversation continues depends on its harness:
  one that finds its session record continues where it was, and one that does
  not starts a fresh conversation in the new folder.
  The Mobile tile is the phones' status without words: green tile + black
  glyph while one is reachable, a hollow corner dot while away, quiet when
  there is none.
- **Settings is a window over the app:** a sidebar with a settings search
  and one tab per section, grouped General (New tasks, Appearance, Startup,
  Keyboard), System (Cloud, Daemon, Harnesses, MCP servers, Models, Browser),
  Chats (Archived chats), and About (This install). Only the selected section
  fills the right pane, its rows in hairline groups rather than floating cards.
  Phone width stacks the search and a horizontal tab strip above the pane.
  The Browser section is where a person finds out what the built-in browser
  keeps on this computer (bookmarks, the pages read, the profile with its
  cookies and site data, downloads) and takes it back: its facts name where
  each lives, Clear history and Clear bookmarks ask once, and anything the
  shell can only do for the tab you are on says so in the row instead of
  offering a button that would do nothing from here.
- **Main canvas:** one view at a time; views are flat lists/cards, not
  dashboards.
- **Panes (the split view), modelled on VS Code's editor groups:**
  - One pane is the whole main area and carries no header. Two or more
    panes each get a 30px header (`--bg-side`, one hairline under it). On
    the left the header says whose pane it is: a pane holding a chat carries
    the sidebar's own language - its harness mark, the chat's title, its
    project when one of the registered projects holds the chat's folder, and
    the status dot in the same classes and colours the sidebar row uses -
    while a pane holding no chat keeps the role's name ("New chat",
    "Plugins"). On the right the Layouts button, then the split action and an
    overflow that names every direction in words - "Split right", "Split
    down", "Split left", "Split up", each with its shortcut - then close
    on every pane but the primary. Direction is a word, never a bare
    directional glyph: the one icon-only button is split right and wears
    VS Code's split-editor shape (a page divided down the middle), which
    the titlebar's split toggle shares. The Layouts menu reads the saved
    shapes when it opens, so one saved a moment ago from the palette is
    already there; each entry carries its pane count, an entry this window
    cannot hold stays in the menu disabled with its reason under it (an entry
    that vanishes teaches nothing), and "Save this layout…" is offered only
    once the window is already split.
  - Five ways to split, in the order people find them: the titlebar's
    split toggle at the bar's right end, `Ctrl+\` (right) and
    `Ctrl+Shift+\` (down), a chat's context menu in the sidebar or main
    area, and dragging a chat - from the sidebar or from another pane's
    title - onto a pane. The middle of a pane opens the chat there; an
    outer band splits that pane on that side, with the half it is about to
    take tinted in the accent while the chat hovers. Panes are read-only
    drop targets for foreign drags: only a chat payload acts.
  - Focus moves with `Ctrl+1`..`Ctrl+4` in reading order; the focused pane
    carries a hairline accent inset and a brighter title.
  - Dividers are a 7px strip over a 1px line, with the resize cursor on
    the strip; dragging reallocates the two neighbours (neither below 15%)
    and double-clicking evens out that split's panes.
  - Four panes is the most. Every split action disables and says so at the
    cap, and a chat dropped on an edge at the cap opens in that pane
    instead of doing nothing.
  - Named layouts save the window's shape, not its occupants. A layout stores
    the split tree with its sizes and, per pane, a role - "chat" (whichever
    chat is current), "new" (a New chat composer), or one of the console's
    static view names - and never a chat id, because a chat is renamed or
    deleted by tomorrow and a layout should not restore a hole. The primary
    pane is recorded as the primary rather than assumed to be the first: "Split
    left" and "Split up" put a pane in front of it, and a shape that trusted
    reading order would come back with every role shifted. Saving refuses what
    cannot come back as it was: fewer than two panes, more than four, a static
    page parked in a side pane, or a view with no role. Restoring applies the
    shape and drops a role this build no longer knows. The saved list is a
    console preference that never leaves this computer, and the live window
    remembers itself across a reload the same way: the stored shape is
    validated before it is drawn, a phone never restores one, and a pane whose
    chat is gone becomes a New chat rather than pointing at nothing. Both
    halves are reached from two places: the command palette (a shape saved or
    put back from its Layouts page) and the Layouts button in a split pane's
    header (a shape put back, or saved once the window is already split).
    Saving opens the family's own dialog with one field for the name.
  - A phone has no panes: one view, no headers.

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
  share the same ghost. New chat's presets ride that language: a row of
  ghost chips at the top of the box, one per saved setup, ending in the chip
  that saves the current one under a name already filled in for the person
  to keep or retype ("Opus high, this repo"). Saving is one press, not a
  dialog: the chip settles into the new preset in place. At most one preset is
  the
  default, and New chat opens on it. A preset records what was true when it
  was saved, so applying one keeps only the model, effort and approval word
  the harness still offers, applies the rest, and says what it could not
  bring in one calm line rather than failing. A chip's delete control asks
  once, by the same two-step as every destructive row: the first press arms,
  the second press does it, and Escape or a click elsewhere keeps. The armed
  row says what goes and what stays. Only one removal runs at a time: the claim
  is taken in the press itself, not in state that has not rendered yet, and
  while one runs every armed confirm is disabled rather than looking live and
  answering nothing. The floating chat
  composer publishes its measured height (`--cv-clear`) so the transcript's
  bottom padding keeps the last message above it, never under it.
- **Send button:** a 32px circle; a light grey disc (`#d4d4d8`) with a dark
  arrow when ready, transparent with a muted arrow when empty. A hover
  brightens it; the press holds still, with no scale dip and no glow. It is
  still the one recurring primary action, and it no longer competes with the
  amber that means "waiting".
- **PickerMenu:** the one dropdown engine - opens at the cursor, searches,
  optional groups, keyboard-navigated, viewport-clamped.
- **Command palette (`Ctrl+K`):** the one search over everything the console
  can do - actions, rules, chats, views, projects, machines - grouped and
  ranked, each row with its key on the right; an empty query shows recent chats
  and suggestions. It is where actions live, in three shapes: a row opens a
  page inside the palette (a question first, its answers as rows, Esc to step
  back out - the footer says which), it hands off to the surface that owns the
  action, or it answers in one line under the field and leaves the palette
  open. The pages are Switch model, Set effort, Set permission, Stop all
  running chats, Stop one harness's chats, Layouts and Since your last look;
  the hand-offs are Changes in this chat (the review), Race a prompt across
  harnesses (the race), Copy this chat as Markdown (the transcript, in the
  app's own rendering, with long tool output and long tool input shortened and
  counted) and Turn this chat into a recurring job (the rule editor, already
  filled). The one-line answers are the transcript copy and the Rules group,
  which runs a rule through the Automations view's own Run now (never a second
  run path and never a new route) and answers "Running ...", then "Started."
  or the daemon's own refusal.
  A row that would lie is not rendered: the changes row appears only
  while the chat on screen has file changes to read, the stop rows only while
  something runs, since-last-look only while something moved, and the Rules
  group only while the Automations view is mounted to run one - a paused rule,
  or one whose machine has not come online, is not offered at all.
- **Stop everything:** a palette page, one command that ends every running
  chat. It asks with the number ("Stop 3 chats?"), names them oldest first -
  four, then "and N more" - and puts the safe answer first ("Keep them
  running" above "Stop them"), so the Enter of muscle memory keeps the work.
  While the stops land it says "Stopping 3."; then it settles what actually
  happened, in sentences: stopped, on another computer so it kept running,
  already gone, did not answer. A chat on another device is named and
  skipped, never failed. Its sibling page, Stop one harness's chats, picks
  the harness first (only that harness's chats running here are offered,
  and the page says so: "Only chats running on this computer can stop; the
  rest keep running."), then asks "Stop 2 chats in Claude?" and settles the same
  way; with nothing left running it says so instead of asking.
- **Race:** one prompt into two to four harnesses at once, one per pane, the
  same prompt verbatim for each. It refuses before a chat is created:
  fewer than two harnesses installed, fewer than two chosen, more than four
  chosen, not enough free panes (naming the ones that would not fit), or an
  empty prompt. A retry starts from what is missing, not from the whole pick:
  the harnesses that did not produce a chat are still chosen, and the ones that
  did are not started twice.
- **The built-in browser:** its tab strip draws whole tabs only. A tab is a
  fixed width (200px) and never shrinks to a sliver: the strip measures itself,
  draws as many whole tabs as fit, and the rest sit behind one overflow control
  that shows their count and carries the same count in words ("3 more tabs").
  Every tab's name is in its title as well as on it, so a cut name is never cut
  without words carrying it. `Ctrl+Shift+T` reopens the last closed tab - five
  per window, remembered as the address and title it had, because the page went
  with the close, and a new tab page leaves nothing to reopen. A tab's own menu
  closes it, closes the others, or closes everything to its right; all three
  work in strip order, so a window resize never changes what a command does.
  The command palette lists the open tabs in its Browser group, and reopens a
  closed one from a row.
  Bookmarks are the browser's own and stay on this computer: the star beside the
  address bar (or `Ctrl+D`) keeps the page on the bookmarks bar and opens a small
  editor for its name and folder, the same press on a kept page edits that
  bookmark instead of making a second one, and an address is always one row.
  A bookmark with no name is drawn as its address. The bar sits under the tab
  strip, draws the bar folder first and the rest behind one overflow control,
  holds Other bookmarks at its end, and appears only while something is kept;
  Remove is the editor's own second press and the safe answer (Done) is the
  primary. No bookmark is sent anywhere, and none of them needs an account.
  History is this window's reading record, drawn by the console and grouped by the
  day it happened (Today, Yesterday, then the date), newest first. A visit is
  recorded as the pane shows a page - its address and its title - so a title
  that lands late updates that visit instead of making a second one, and the
  same address twice in a row is one entry. A private tab is never recorded, so
  neither the history list nor anything reading it can name a page read in it.
  A row opens its address in the tab, and Clear history asks once before it
  empties the list; the pane's own menu opens the list. Five hundred entries per
  window, on this computer only.
  The new tab page is a search box and a short Recent list under it: the
  addresses this window has read, newest first, one row per address, six at
  most, drawn with the same quiet address line the history list uses and each
  opening in the tab. It is that same record, so nothing in it was read in a
  private tab, and nothing on the page is fetched or sent.
  The address bar suggests as a prefix is typed, from the two lists this device
  kept and read: an exact prefix on a title ranks above the same letters inside
  one, a kept address above a plain history hit of the same quality, and among
  equals the most recent visit wins; one address is one row, six rows at most.
  Up and Down move the highlight while the field keeps the focus (the list is a
  listbox the field points at, so nothing is trapped), Enter opens the
  highlighted row, a modifier or the last row opens a new tab, and Escape closes
  the list and restores the page's own address.
- **Text fields:** right-clicking a field opens the console's own menu - Cut,
  Copy, Paste, Select all - because the engine's default menu is off for the
  whole app. A field never has two menus, and the words on the one it has are
  the console's own.
- **Since your last look:** a palette page over the unread marks - the chats
  that moved after they were last read, newest activity first, headed in
  words ("Two chats moved while you were away."). Each row says what
  happened last in the statuses' own words (the last turn finished, waiting
  on an answer, the last turn failed) and when it happened; nothing is
  ranked by importance, because nothing here can know it. A chat with no
  activity stamp is never listed, and the page is not offered when nothing
  moved. It reads and clears nothing.
- **Changes review ("Changes in this chat"):** every file a harness reported
  changing in this chat, folded by path from the transcript's own tool items,
  one side-by-side diff per file, a long report saying how many rows it
  shows. It is what the harness reported, and says so where it is empty: not
  a git diff - nothing here stages, commits or reverts - and not everything a
  turn did, because a harness that edits from a shell command or a script
  reports no file list at all. Where this window does not hold the chat's
  transcript it says that rather than pretending the chat changed nothing.
- **The daemon notice row:** one thin row above the main area, two tones
  only - a daemon that answered and then stopped ("The daemon stopped
  answering") and one that never answered ("The console is not connected to
  the daemon"). Each quotes the connection's own words when it has them,
  capped to one line at a word boundary, and offers a restart only where
  this build can start the daemon. It never speaks for a signed-out account
  (the Cloud door and views say that themselves), and it stands down where
  the view already prints the same news. Every failure line in the console
  follows one rule: it says what failed and what a person can do; a refusal is
  quoted as the daemon's own words ("The daemon refused this: ..."), a status
  number appears only inside a sentence ("The daemon answered 500 and gave no
  reason for it."), never as a bare code; nothing says "something went wrong",
  nothing promises an action that is not beside it, and a cause the console
  does not know says so instead of inventing one. `src/error-copy.js` is the
  one place those sentences are chosen.
- **View boundary:** a view that throws while drawing itself leaves the rest
  of the window intact - sidebar, titlebar and other panes keep working - and
  answers inside its own pane with a still panel (one hairline box, no
  motion, nothing that asks to be watched): one plain sentence naming the
  kind of failure, the error's own line and first frame in mono, and Try
  again once. No retry is offered when the failure is deterministic (a
  hook-order or property-access error fails the same way on every try), and
  no panel claims the app is broken: the pane failed, not the window.
- **Settings search:** one field above the tabs; searches every setting's
  label, keywords and description across sections. Results name their
  section, and choosing one opens that tab and marks the matching row.
- **Keyboard filter:** the Keyboard tab carries its own filter beside a
  Reset every shortcut action; an action answers by name, group, or current key
  ("Ctrl+N"), and the row being rebound stays visible while it is captured.
- **Switches (`tg-switch`):** real toggles for settings - 30x18 with a
  sliding knob, amber when on; green stays reserved for running/ok. On/Off
  words never stand in for a switch.
- **Modals:** one family (Esc / backdrop / X close): device, plugin, about,
  confirm, the command palette, and every dialog the console has - anything
  that declares itself one (`aria-modal`) holds its layer through the shared
  hook, and `src/esc-layer-wiring.test.js` discovers the family from that
  declaration rather than keeping a list by hand, so a dialog added later is
  covered without editing the test. Panels, not surprise
  shapes. The keyboard is part of the definition, not a bonus: focus moves
  in when a dialog opens (unless a field already took it on purpose), Tab
  stays inside it, everything behind it is `inert` for both the tab order
  and a screen reader (with a Tab loop as the fallback for a WebView without
  the attribute - the titlebar's own window controls stay live, since a
  dialog may own the app and never the window), Esc closes one layer at a
  time through the shared stack, and the key goes back to the control that
  opened the dialog. One hook (`dialog-focus.js`) carries all of it, so a
  dialog that paints a backdrop and nothing else is not one. The rename dialog
  is hosted by the shell, not by a view, because two doors open the same one:
  the sidebar's row menu and the command palette's row. It is one field with
  the current name selected; it saves through the daemon's own rename, and a
  refusal keeps it open with the daemon's words quoted rather than closing
  over a failure. The row shows the saved name at once, for as long as it takes
  the console's own record to carry it, then follows the daemon's title
  wherever it goes, including a rename made on another device. The onboarding
  panel is not one of them: it is a full-screen view with no backdrop and no
  X, one step at a time, each step with its own button out.
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
- **Model selection renders where a harness advertises a catalog** through the
  daemon and stays a reserved slot where it does not: the picker shows the
  harness's own model names, led by each maker's mark, and the effort picker
  follows the chosen model's own tiers. The slot is never invented empty.
- **First run is a setup wizard** - connect, pick, start; one plain action
  per step. When no agent is installed, the pick step offers the installer
  right there ("No agent is installed on this computer yet. Install one and
  it appears in the harness picker.") instead of ending the wizard over a
  composer that cannot send, and the last step says which agents are ready,
  which are not signed in, and how to add one whenever the person is ready.
- **Mobile is first-class:** the same visual language adapts to phone width,
  not a squeezed desktop.
- **Process:** build slowly; overthink single elements; review at real
  milestones only.
- **Vocabulary (2026-10-01, Cloud 2026-10-07, Projects 2026-10-08):** *chat*
  is the UI word for a conversation with a harness (the daemon domain says
  *session*; the console boundary translates). A *device* is any of your
  computers signed in to Cloud; a *machine* is a device that runs chats for
  your other devices. *task* is the prompt text a human or automation sends.
  A *project* is a registered entity the daemon stores (like rules and
  plugins): the folders whose chats group under it - exactly one this
  release, the list is the multi-folder future. A *workspace* is the folder a
  chat runs in - now optional; a chat with none belongs to no project and
  runs in the home folder. *Folder* is the disk thing; the word appears
  only inside the folder picker, never as a noun the UI groups by (the old
  derived basename grouping is gone - the sidebar's projects are the
  registered ones, everything else is the flat Chats section).

---

## Shipped - the parity build (2026-10-02)

Every screen the design specifies, backed by the real daemon:

- **New chat** - the greeting + composer; harness picker (installed only,
  with a pinned "Manage harnesses…" row that opens the harness manager:
  all six harnesses, each installable through its owner's own installer
  on Windows, macOS, and Linux - the command stays the daemon's, never
  shown; pi's Node.js is set up only after a calm go-ahead),
  model picker where the harness advertises (its own catalog, the model
  names alone; the maker's mark rides the command palette's model rows
  instead - Claude's starburst, OpenAI's hexapod, xAI's, Gemini, DeepSeek,
  Qwen, Kimi, Mistral, Meta, each its owner's official path in its official
  color, text alone where the maker is unknown - the effort picker offers the
  chosen model's own tiers
  right after, so the model rows carry no tier sublines), the permission
  picker where the harness has its own words (claude `--permission-mode`,
  codex `approvalPolicy`, grok `--permission-mode` - the vocabulary rides
  /capabilities, the harness's own default row marked "runs this by
  default", unset sends nothing so the default holds; agy/pi/opencode
  offer no words, so no chip), the Fast toggle on the effort panel
  where the harness's own fast mode is usable
  (claude `fastMode` ≥ 2.1.205, codex `fast` service tier ≥ 0.110), the
  project picker (the registered projects on the machine the chat runs
  on, "No project", and the picker's own Add project row - the one way
  in, which the in-app folder picker serves for every surface that asks:
  the sidebar's +, the settings rows, itself).
- **Chat** - the rail transcript (user cards, agent text, notes, tool
  cards with In/Out, decisions in the harness's own words with its
  affirmative first), the facts row (workspace · harness · model ·
  effort · permission word · Fast · «tool» allowed for this chat ·
  status - the permission word shown in the harness's own spelling,
  changeable only while stopped: start-time on every harness, so a
  stopped chat's change lands on resume), stop/resume, the floating
  composer with the same effort panel as New chat, opened above the box,
  and the ContextRing where the harness reports its window
  (codex; claude reports usage but no window on the wire). A harness that needs
  its own sign-in gets a sheet rather than a wall of log: the login command runs
  on the machine and its words stream in, the address it prints is the sheet's
  one press (Open the sign-in page, in the person's own browser, with Copy
  beside it and the host named in place of an unreadable authorize query), the
  aside it prints about a headless machine is quoted under it, and every word it
  said waits behind a press - folded while there is an address to act on, open
  when there is not, because then the words are all there is.
- **Machines** - Cloud's view of the account's devices (2026-10-07): this
  computer first with its "Make this computer a machine" row, then each
  device's card (platform, Machine/Desktop, version, online). A device's
  detail lists the synced chats it runs (from this computer's copies, so
  they show while it's offline) and, for an online machine, its harnesses -
  installed and signed in remotely through the same installer chain and
  sign-in relay. Add a machine mints a one-time Linux install command and
  notices the arrival; Remove is asked twice and says what it deletes.
- **Plugins** - the installed list carries its own field whenever it holds
  anything: it matches the words a row prints, narrows without
  reordering, keeps its count line while a query is on, and Escape clears it
  before leaving it, the same door as the rules list. MCP installs written by hand; the needs-key lifecycle
  (keys never cross the API); the ride-along through each harness's own MCP wire (claude `--mcp-config`
  JSON string, codex `thread/start` config keyPaths).
- **Automations** - rules with real sources only (a schedule on the
  machine's own clock, a webhook with its own key), Run now, the form,
  and the everywhere-pill (the keyword parser,
  the rule-draft action card, honest refusals for connector-only kinds).
  A rule's Chat section carries the permission picker too - the word
  rides the chat the rule fires.
- **Settings** - a view (section rail), not a popup: new-task defaults
  (harness/model/workspace, per-harness effort tier and permission word
  in the harness's own words - the same pickers the composer holds,
  rendered only where the default harness offers them), appearance
  (compact density; the dark theme is contract), startup
  (launch at login, reopen last view vs New chat), the daemon connection
  (the address, connect by address, which opens the manual form while
  nothing is connected, chats on record, restart, two-step Forget), the
  harness list,
  reduce-motion (restored before first paint), first-run replay.
- **Account, Mobile, Metrics** - the footer's views (2026-10-15), each
  rendering before Cloud's sign-in door with its own signed-out row:
  Account carries the sign-in state and its actions, this device's
  identity, and the devices count (its Machines button crosses modes);
  Mobile is the phone companion page in two halves that do not depend on
  each other: first the pairing panel (a QR code, the phones paired with
  this computer on the local network, each one removable), which needs no
  account at all; then the install-and-sign-in lead, the account's phones
  as cards opening the same device detail, and the signed-out row when
  there is no account; Metrics counts the polled record (status
  sentence, the synced split, per-harness counts, a 14-day strip of new
  chats) beside this machine's 48-cell presence band, all derived from
  the poll - cost and tokens stay out until the daemon aggregates them.

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
  Synced chats can be deleted, asked twice, and every other device deletes
  its copy the next time it syncs.
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

Deferred: the updater and its badge, auto-update, signed releases, machine
installers beyond Linux, moving a chat
between machines and switching
its harness mid-chat (one hand-over mechanism), the connector triggers
(pipeline/errors/review/release), the agent behind the pill, claude's
context *window* on the wire (the ring rides a nominal 200k until the
stream-json carries one), grok/pi/opencode/agy MCP wires.

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
- **Sidebar rows** - the harness mark leads, the title follows, and the
  row's actions button sits at its right edge, outside the row's Tab stop.
- **Press feedback on primaries** - both send buttons, the decision
  affirmative; all of it under `data-reduce-motion`.

## Checking this contract

Most of this page can be checked without opening the app: `npm test` runs
the console's rule tests (938 of them the last time this line was checked,
and the run prints the live count) and a handful
of wiring tests read the source text for the parts only a browser could
show, such as the modal family and the waiting screen's doors. The rules
behind the newer surfaces sit in their own modules for that reason - the
palette's pages, the stop question and its sentences, since-last-look, the
waiting line and its cut, the unread marks, presets and their repair, named
layouts and their refusals, what a pane's header says, running a rule from
the palette and the seam that lends it the Automations view's own Run now,
the one-removal-at-a-time lock, the rename rules, the error copy, the
daemon notice row, the view boundary, race, the pane tree's own grammar
(`src/split.js`: what a split is, the floor no pane's size may fall below,
and what happens when a chat is dropped on an edge), the shape every daemon
field passes through (`src/shape.js`: a field that should have been a word
or a list, and the safe answer when it was not), and the beat's cadence and
death rule (`src/beat.js`, with `src/state/`: how often the console asks,
and how long a daemon may stay silent before it is called dead) - so each
is tested as rules
rather than as a screenshot. Colour,
type and motion are not testable that way, so a change to them is checked
by eye at both motion settings (`prefers-reduced-motion` and the in-app
toggle) before it counts as done.
