# Design from the film

**Status: applied to the console, 2026-10-08** (the session note has the
commit). Eight of the nine numbered items below ship; §4 (pill chips)
was applied and then removed the same day on the founder's override -
see its note. The file stays as the record of the delta and its values.
`DESIGN.md` now carries the adopted rules as contract.

What the launch film's console mock does better than the app. During the
film's polish pass (2026-10-08) the mock was first made byte-for-byte
app-true, then the places where the film's original design beat the app's
were restored. The rule going forward: **where the film's design wins, the
film leads** — the app adopts from the video. This file is that delta.

Visual reference: the film at `Video-Studio/OpenRemote/Launch-Video/` —
composer close-up 00:17–00:19, working scene 00:23.6–00:31.4, decision
00:36.6–00:37.6. Every value below is lifted from the film's components
(`studio/src/ui.tsx`), not from memory.

---

## 1. Tool cards carry their status (highest impact)

**Today:** a tool in flight shows nothing; a successful tool shows nothing
(only failures get an amber "Failed" text label; there are no status icons
anywhere).

**The film:** every tool card shows its state, right-aligned on the name
line (12px/500, `--text-3` label):

- **Running** — breathing green dot + "Running" while in flight
- **Done** — green check (13px, bold, `#4CB487`) + "Done" the moment the
  result lands
- **Failed** — red check (13px, bold, `#F87171`) + "Failed"

Rail-dot colors stay as they are (ok→green, pending→amber, failed keeps
the grey default).

**Why:** the status flip is the beat of agent work. Without it a
transcript reads as a log; with it, it reads as live work — the diff
between "the app did things" and "watch the agent work."

## 2. Decision cards: the question is the content

**Today:** tool name + "Needs a decision" + an In row + ghost buttons
(first one white-tinted). No amber, no question on screen.

**The film:**

- The question renders as content: 14px/500 `--text-1`, inside the card
- The pending state wears the brand amber — border `1px solid rgba(220,162,75,0.35)`,
  background `rgba(220,162,75,0.07)`, radius 8, card padding `10px 12px`
- Buttons: affirmative first, **amber** — background `#DCA24B`
  (`--accent`), text `#1a1206`, height 28, padding `0 12px`, radius 7; the
  other option ghost
- On answer: the amber drains back to a settled card (border `--line`,
  background `rgba(0,0,0,0.28)`), the label flips to a green
  "Answered: {choice}", and the buttons fade out over 0.3s

**Why:** "answer from one place" is the product moment — the question
should be the loudest thing on screen, and amber is the app's own
attention color. Primary actions should wear it.

## 3. The send button is amber, with a press bloom

**Today:** 30×30 radius-8 square, white-alpha background, no amber, no
feedback.

**The film:** same 30×30 radius-8 shape, but:

- Ready (input has text): background `#DCA24B`, white arrow
- Empty: transparent background, `--text-3` arrow
- On press: scale to ~0.8 and back (~0.24s), plus a glow bloom —
  `box-shadow: 0 0 (18 → 62px) rgba(220,162,75, 0.25 → 0.75)` peaking on
  the click

**Why:** the send/enter moment is the product's one recurring primary
action; it should carry the accent and reward the press.

## 4. Composer chips are pills, not ghosts — OVERRIDDEN (2026-10-08)

**Today:** borderless text chips (4px padding, transparent until hover) —
they read as nothing at a glance.

**The film:** filled pills — background `rgba(255,255,255,0.045)`, height
26, padding `0 9px`, radius 7, 12.5px/500, color `--text-2`, 16px brand
mark on the harness chip. The "via" separator stays plain
(11px, `rgba(153,163,174,0.55)`).

**Founder's override, the same day the adoption shipped:** the pills
were applied and then removed on the founder's word - "remove the
border around the harness picker and model… remove it from the
buttons." The Model/Effort/Harness/Fast pickers stay plain ghost text
(`nc-meta`: no fill, no border, the hover wash is the only affordance).
The film wins over the app; the founder wins over the film.

## 5. Split-diff tints: double the alphas

**Today:** del `rgba(248,113,113,0.08)` / add `rgba(76,180,135,0.09)`.

**The film:** del **0.18** / add **0.20** (text colors unchanged — `#F87171`
/ `#4CB487`). At 0.08 a fix reads faint; at 0.18 it reads as a change.

## 6. Show the context ring for claude chats too

**Today:** the ring hides when the harness reports no context window
(claude's stream-json carries none) — the composer is fully static.

**The film:** the ring always renders — 20px donut, 2px stroke, track
`rgba(255,255,255,0.12)`, fill `#4CB487` round-cap rotated −90°, filling
0.18 → 0.41 across a turn, in the composer foot left of the send button.

If real usage data is unavailable, derive a nominal fill (tokens-based).
The composer needs one live element; a static composer reads as a
screenshot.

## 7. Breathe the facts-row status dot

**Today:** static. **The film:** breathing, same cycle as the sidebar
running dots (2.4s, opacity 1 → 0.35) — the app already ships the keyframe;
apply the same class to the facts dot. One-line change.

## 8. Sidebar rows: the harness mark leads

**Today:** status dot + title; no harness identity on the row.

**The film:** the 13px brand mark **first**, then the title (device label
in cloud mode), and the status dot at the row's right edge
(`margin-left: auto`).

**Why:** "every harness, one sidebar" is the identity; the mark is the
at-a-glance signal of which agent owns each chat. The dot at the right
edge keeps every status scannable in one vertical column.

## 9. Generalize: press feedback on primary actions

Send and decision buttons pulse on press (scale ~0.9 + glow where amber).
Cheap to implement once, applies to every primary action in the console.

---

## Keep as-is — the film already matches the app

Rail thread (34px offset, 7px dots + 1px connecting line), full-width user
cards, In/Out cards with "In"/"Out" 10px uppercase labels, the split
diff structure (old-left red / new-right green), the facts row and its
'·' separators, Projects groups with counts, the Machines view, real
status strings (Running / Needs your decision / Idle), the Fast chip
rules (grey off default; claude is start-time-only), placeholder strings
("Describe the task", "Steer the session"), the failed tool's grey rail
dot.

## Do not port — film-only language

Camera choreography and framing, beat-grid pacing, cut flashes,
vignette/grain, the thinking-token counter's cadence, marketing captions.

---

*From the launch-film session, 2026-10-08. The film's studio is the
living reference; when this file and the film disagree, the film wins.*
