# Freebuff TUI parity: the plugin becomes a real client

**Date:** 2026-11-02
**Status:** design, awaiting approval
**Scope:** the seam (OpenRemote daemon + plugin protocol), the freebuff driver, and the console

## Intent

OpenRemote shows Freebuff as a harness that can start a chat and run a prompt. Everything that makes Freebuff
Freebuff, the model picker, the modes, the slash commands, the balance, live in its TUI and is invisible in the
console. The goal is a console that offers what the terminal offers, without the user ever opening a terminal, and
without the console lying about what it can do.

The chosen foundation is **TUI-driven**: the plugin learns Freebuff's facts and performs Freebuff's actions by
driving the same terminal a person would, reading the same screens. Nothing depends on undocumented internals, so a
Freebuff update breaks a parse we can fix, not an integration we cannot.

## What the terminal actually offers

Measured on Freebuff CLI 0.2.35 on this machine, by driving it:

| Area | What it is | Where |
|------|------------|-------|
| Model picker | Cards with a name, a tagline, a cost multiplier (`1x`/`2x`/`3x`), a `PREMIUM` section, `›` on the current one, and a per-model data-use notice | `/model` |
| Modes | `LITE` and `DEFAULT`; transcripts carry a `mode-divider` block naming the mode | `settings.json`, transcripts |
| Slash commands | `/interview`, `/plan`, `/review`, `/deploy`, `/history`, `/new`, `/theme`, `/feedback`, `/model` | the composer |
| Balance and cost | "3 hours from your balance", "Uses 1 hour from your wallet", a confirm-before-spend dialog | status line, send confirmation |
| Session history | Browse past chats, start a new one | `/history`, `/new` |
| Follow-up suggestions | Three clickable next steps after an answer | end of a turn |

A real picker capture, for reference (the tail of the screen):

```
 Solar Mini 4 · Fast and light · Promotional
 May use data for AI training

 MiMo v2.6 Flash Slow · Slow · cheapest · Images
 1x

 PREMIUM · 2–4x cost · lower usage (1–2 hours per day)
 DeepSeek V4.1 Flash Slow • high · Slow · DeepSeek · Images
 2x · Recommended · May use data for AI training

› DeepSeek V4.1 Flash 10/17 • high · Smart & Fast · Images
 3x · May use data for AI training
```

## Goals

1. The console shows Freebuff's real model list, with the cost tier and the current selection, and a chat starts on
   the model the user picked.
2. The model can be changed in a running chat, as the TUI allows.
3. Freebuff's slash commands are offered by the composer, the way other harnesses' commands already are.
4. The console shows what a turn costs and what balance is left, truthfully.
5. Every one of these degrades to a clear sentence rather than a hang or an invented fact.

## Non-goals

- Reproducing terminal aesthetics: themes, ads, spinner art. The console has its own design.
- Reimplementing Freebuff's agent loop, subagents, or tool behaviour.
- Making OpenRemote's own `/model`, `/effort`, `/fast` mean Freebuff's models. Those are OpenRemote commands; the
  harness's own vocabulary stays the harness's.
- Cloud, phone, and multi-device behaviour for any of this in phase 1.

## Anti-goals (this work has failed even if it "works" if)

- The driver types into a screen that is not the picker and spends the user's balance on a model they did not choose.
- A picker that silently keeps the old model while the console says it changed.
- A model list that is stale for months with no way to refresh and no sign that it is cached.
- A chat that cannot start because a capture timed out.

## Constraints found

- **The probe has a 5 s deadline** (`PROBE_DEADLINE` in `plugin_driver.rs`). Driving the TUI to read the picker
  takes longer, so the catalog can never be captured during a probe. It has to be a cached artifact.
- **`ModelDescriptor` carries** `model` (the pick word), `display_name`, and `resolved_model`. There is no field for
  a cost tier or tagline; showing `2x` and `Recommended` needs either a contract addition or console-side text.
- **The daemon refuses live settings for plugin harnesses** (`PluginDriver::apply_settings` answers "stop it and
  resume"). Switching a model mid-chat needs that refusal to become a call the driver can honour.
- **The screen buffer only grows.** Every matcher must read the tail a person can see, not the scrollback; the send
  confirmation taught that the hard way.
- **Freebuff's startup is noisy**: a resume notice, a survey, ads, a cost confirmation. Any sequence of keystrokes
  must tolerate all of them.

## Approaches considered

1. **Everything through the TUI (chosen).** The driver types slash commands, reads the screen, and parses. No
   dependency on undocumented internals. Cost: every feature is timing-sensitive, and a screen change breaks a
   parse.
2. **Read the store, drive only to act.** Facts from `settings.json` and the logs; the TUI touched only to change
   something. More stable reads, but the facts it can read are thin (the model is only in log lines, the balance
   only on screen), and it still needs the TUI for every action, so it keeps approach 1's fragility at the moment
   of action while adding a second source of truth.
3. **Talk to Freebuff's local service.** `freebuff-live-<pid>.json` carries an instance id and a token and the logs
   show an SDK. Structured models, costs, sessions. Rejected for now: undocumented, invisible to us when it changes,
   and it can disappear in any release.

Chosen: **1**, for a foundation that only depends on what a person can see.

## This is five phases, not one

| Phase | What | Why it is separate |
|-------|------|--------------------|
| 0 | The screen engine: send keys, wait for the screen to settle, parse a region | Everything else rides it; it must be right before anything uses it |
| 1 | Model picker: catalog, selection at start, switching mid-chat | The user's first ask; needs one seam addition |
| 2 | Slash commands | Nearly free: the seam already carries `CommandsListed` |
| 3 | Modes: LITE/DEFAULT and plan mode | Needs a settings surface and a transcript fact |
| 4 | Balance, cost, and real spend approvals | The auto-Enter becomes an approval the user sees |
| 5 | History and follow-up suggestions | Conveniences over machinery that already exists |

This document designs **phase 0 and phase 1**. Phases 2-5 get their own designs once 1 is proven.

## Phase 0: the screen engine

A small module in the driver, `screen`:

- `settle(after_send, quiet_for, limit)`: wait until the TUI has stopped printing for `quiet_for` (default 350 ms)
  after the last byte, bounded by `limit` (default 3 s). Every interaction goes through it.
- `tail(lines)`: the last N non-empty lines of the stripped screen, the only part a matcher may read.
- `keys(...)`: a named-key vocabulary (`enter`, `escape`, `up`, `down`, `tab`, `text(String)`), so a call site never
  writes raw escapes.
- A **frame log**: every interaction records the screen it acted on, into a ring buffer the driver can quote in a
  diagnostic. When a parse breaks, the bug report has the screen.

A parse is a pure function from `tail` text to data, with tests built from captured screens (the picker capture
above is one).

## Phase 1: the model picker

### 1.1 The catalog

- `freebuff-driver --catalog` spawns a throwaway chat in the plugin's own folder, opens `/model`, captures the whole
  picker by paging, parses the cards into `{name, tagline, cost, section, current}`, closes it with `Escape`, and
  prints one JSON line: `{"models":[...],"default":{...}}`. Exit 0 always, an unavailable CLI is an answer.
- The same parser runs at every `spawn`, before the first prompt, so a `models.json` cache next to the driver stays
  fresh whenever the plugin is actually used. The cache is best-effort: a failed refresh keeps the old file.
- `openremote.json` ships the catalog `--catalog` produced, so a fresh install shows the picker with no extra step.
  A stale catalog is visible and refreshable, never silent: the daemon reports the catalog's `captured_at`.

### 1.2 The seam

- `ModelDescriptor` gains optional `detail` (a short line, e.g. `2x · Recommended`) and `note` (the data-use line).
  Additive, so an older console ignores them.
- The harness `harness` block gains `"models_file": "models.json"`: the daemon reads that file at probe time (cheap,
  inside the 5 s) and uses it as the harness's model list, falling back to the manifest's `models` when absent.
- A manifest `"settings": "live"` tells the daemon not to restart a chat to apply model or effort: it calls the
  driver's `settings` command, and only report a refusal if the driver refuses.
- Forward compatibility already holds: unknown command types are ignored, and the protocol says additions are
  additive.

### 1.3 Starting on a chosen model

`spawn` already carries `model`. When it is present, the driver:

1. Opens `/model` and confirms the picker is on screen by a structural match (a `×`/`›` row and at least two
   distinct model cards). If it is not, it types again once, then gives up.
2. Moves with `up`/`down` until the row whose name matches the requested model is marked `›`, verifying after each
   step. Wrapping is handled; an unmatched name is a refusal, never a nearest match.
3. Presses `enter`, waits for the picker to close, and verifies the status line now names that model.
4. Only then types the first prompt.

If any step fails, the driver reports the failure through `turn_completed` with the screen quoted, and the model is
left as Freebuff had it. The console says "Freebuff did not switch to X; it is still on Y" rather than pretending.

### 1.4 Switching mid-chat

`settings` with a new model drives the same routine against the running TUI, between turns. A turn in flight refuses
with a sentence the console shows. On success the console updates the chat's model the way it does for harnesses
that report a model change today.

### 1.5 The console

- The New chat composer and the chat header show Freebuff's models with `display_name` and `detail` (`3x ·
  Smart & Fast`), the current one marked.
- A `Captured 2 days ago · Refresh` line under the picker, with the refresh running `--catalog` through a new route.
- The model chip in a chat reads the model the driver verified, not what was requested.

## What is left to the builder

- The exact key sequence the picker needs (does typing filter? does `enter` select and close?), to be measured in a
  spike before the parse is written.
- The picker's total height and paging behaviour on a 140x45 terminal.
- Where the catalog cache lives (plugin folder vs. data dir) once the daemon's write rules are checked.
- Whether `note`/`detail` should be its own contract addition or folded into `display_name` text.

## Calls I made (check these while reading)

- **Phase 0 is a real 140x45 screen, not a growing text buffer.** `strip_ansi` and the append-only `screen` string
  were enough to notice a banner, but not to verify that `›` sits on the row we think it does. A diff-based TUI
  repaint makes a text tail a mix of rows from different frames, and the wrong-model-spend anti-goal cannot be
  enforced on that. Phase 0 keeps a cursor-aware grid fed from the raw byte stream, and the test corpus is captured
  byte streams played through it.
- **A failed switch never puts a screen in `error_message`.** That field is stored as a note on the chat and syncs
  between devices, while PROTOCOL.md promises it is redacted and clipped; picker and status frames carry balance
  text and sometimes the signed-in address. The event carries only the requested and the actual model name; the
  frame goes to local stderr and the frame log, which never leaves the machine.
- **`--catalog` runs in its own scratch directory, refuses while a chat is live, and cleans up after itself.** It
  leaves at most a short-lived folder in Freebuff's own history, which is disclosed in the console's refresh line
  rather than hidden.
- **`model_reported` is mapped in `plugin_driver::parse_event`.** `DriverEvent::ModelReported` and the supervisor
  path already exist; only that mapping is missing, so the console can show the model the driver verified.

## Questions for you

1. **Whose model does a pick change?** Freebuff keeps the choice machine-wide: `settings.json` carries a model id, and
   no per-chat selection exists in the store. So a pick in OpenRemote also changes what your own terminal gets next.
   Restore the old model when the chat ends, let it stand, or refuse two console chats that want different models?
2. **Mid-chat switching: Freebuff only for now, or hold it back until every harness can?** It decides whether the
   seam grows a live-settings capability in phase 1 or in a later design.
3. **When the switch fails at start, should the first prompt be dropped or sent?** Dropped means the message stays
   visibly unsent and retryable; sent contradicts "never spend on a model you did not choose".
4. **Does the cost tier sit in the model row (`3x · Smart & Fast`) or one hover away?** It decides the row layout and
   whether the model contract grows fields other harnesses would then inherit.

