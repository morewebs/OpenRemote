# Freebuff TUI Parity, Phases 0 and 1, End to End (revised)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ship a Freebuff plugin whose console offers the real model list, starts a chat on the model the user picked, and changes the model in a running chat, by driving Freebuff's own TUI and verifying every step on a real screen.

**Architecture:** The driver reads a cursor-aware 140x45 screen grid, parses the model picker from the rows a person sees, captures the catalog into `models.json`, selects a model by walking the marked row with the arrow keys and re-reading the picker to verify, and reports the model it actually saw. The daemon reads `models.json` at probe time, carries `detail` and `note` on a model, honours a live `settings` word, and exposes a catalog refresh route. The console renders the second line and the capture age.

**Tech Stack:** Rust (driver: `portable_pty`, `serde_json`, no new dependencies; daemon: axum, tokio, serde), React 19 / Vite console, `node --test`.

**Spec:** `docs/superpowers/specs/2026-11-02-freebuff-tui-parity-design.md`

## What is already landed (do not re-plan)

These commits are on `Projects/OpenRemote-plugins`, branch `br-freebuff-tui`:

| Commit | What it gives this plan |
|--------|-------------------------|
| `429267c` | `screen::Screen` (140x45 grid, `feed`, `tail`, `joined_tail`, `FrameLog`) and its eight tests. |
| `5bc29e3` | `keys::Key`/`bytes`, `session::settle_verdicts`/`SettleOutcome`, `picker::{parse, Picker, Card, find}` with its eight tests. |
| `9c2e4ea` | `catalog::{run, capture_is_a_picker, Catalog, ModelDescriptor}`, `probe::{preflight, Preflight}`, the `--catalog` mode, and the grid's escape-sequence handling (synchronized output, mode queries, erase characters) with three tests. |

The live capture that closes Task 3 of the superseded plan established four facts this plan is built on:

1. Freebuff's TUI draws the picker as **boxed cards** (`┌─┐`, `│ ... │`, `└─┘`), not as the two bare lines the design capture shows.
2. At the driver's own 140x45 the picker prints **no per-model cost line**; the only cost text is the section label (`PREMIUM · 2-4x cost · lower usage (1-2 hours per day)`).
3. At 80x45 the same TUI draws exactly the design capture, cost tiers included. The design fixture is an 80-column frame.
4. The real picker and the status line are already saved as fixtures: `freebuff/spike/screens/picker-0.2.35.txt` (80x45) and `freebuff/spike/screens/picker-0.2.35-140x45.txt` (140x45). Both contain picker rows only, no banner, no tips box, no balance, no path.

The superseded plan (`docs/superpowers/plans/2026-11-02-freebuff-tui-parity.md`) keeps its decisions D1, D2, D4, D5, D6 and D7: the model is machine-wide state the driver borrows, live switching is Freebuff only, the grid is the parse source, screen text never enters an event, a failed switch refuses the prompt, and the catalog is captured on demand. Its D3 is revised below by D9. Its tasks 4 to 8 are re-cut here so one document carries the whole path.

## Global Constraints

- Windows 10 x64 first. `bin/windows-x64/freebuff-driver.exe` is the only driver this plan ships.
- Freebuff CLI 0.2.35 or newer; an older one is refused with a sentence.
- The pseudo terminal is 140x45 (`ptyproc::ROWS`, `ptyproc::COLS`). Every parse reads visible rows only, never scrollback.
- No TUI frame text in any stored event. Screen text goes to stderr only; `error_message` and notes carry at most a redacted, clipped sentence.
- The protocol stays additive: new event types, new optional fields, unknown fields ignored in both directions.
- The driver never puts an address, a token, an account id or a balance in anything it reports. `credentials.json` presence is checked and never read.
- Harness vocabulary is the harness's own: model names, cost words, section words, verbatim.
- A capture never types a prompt and never sends a message. The only keys a capture sends are `/model`, the Enter that opens the picker, the arrow keys, and Escape.
- Only picker card rows may be saved into `spike/screens/`; never a full frame, an address, a token or a balance figure.
- No em dashes in any copy, comment, or document.
- Gates: `cd Projects/OpenRemote-plugins/freebuff/driver && cargo test --offline && cargo clippy --all-targets --offline -- -D warnings && cargo fmt --all --check`, run with `CARGO_TARGET_DIR=D:\dsh-cargo\freebuff-driver`; `cd Projects/OpenRemote/daemon && cargo test --locked && cargo clippy --all-targets --locked -- -D warnings && cargo fmt --all --check`; `cd Projects/OpenRemote && npm test && node scripts/check-tab-acl.mjs`.
- `--probe` stays inside the daemon's 5 s deadline: no PTY work, no catalog capture, no model read.

## Decisions this plan makes

D1, D2, D4 to D7 of the superseded plan still stand. These are new, or replace its D3.

| # | Question | Decision | Why |
|---|----------|----------|-----|
| D8 | How the parser reads the real picker | Two modes in one `parse`. **Boxed mode** when any row is a box content row (`│ ... │`): the first content row of each box whose text contains ` · ` is a card, the next content row is its one body line, a bare all-caps row between boxes is a section label. **Legacy mode** otherwise: today's rules, unchanged. The picker's own footer row (`↑↓ choose model`) delimits the region when it is on screen. | Measured on 0.2.35: boxed at both widths. Legacy mode keeps the design fixture and all eight existing parser tests green, so neither layout is guessed away. |
| D9 | The cost tier in the row (revises D3) | `detail` is the card's body line when Freebuff prints one (`3x · May use data for AI training` at 80 columns), otherwise the card's tagline. The section label is **not** carried in phase 1. | At 140x45 Freebuff prints no per-model multiplier at all; D3's `3x · Smart & Fast` example only exists at 80 columns. Carrying the section needs a new descriptor field and a grouped console menu, which is phase 1.5. |
| D10 | The geometry the catalog is captured at | 140x45, the same PTY the chat uses. | A catalog name the chat's picker does not offer is a refusal on every spawn. Safety beats a cost line the CLI does not print at that width. |
| D11 | A retry after a failed `/model` | Escape, settle, re-read. The retry is typed only when the screen is **not** a picker. | A second Enter that lands on an open picker selects the highlighted row and changes the machine-wide model. The live capture could not rule this out. |
| D12 | How many body lines a card has | One. The first content row after the name, verbatim: `detail` if its first `·`-segment matches `^\d+x$`, otherwise `note`. Later rows are ignored. | Freebuff prints up to three rows inside a card (`3x · May use data...`, then an upsell). One rule keeps an advert out of the hover title. |

## Review Focus

1. **A second Enter landing on an open picker.** Expected: no keystroke is sent unless the screen is known not to be the picker, so a retry can never select a model. Pinned by `a_retry_is_refused_while_the_picker_is_still_open` (Task 1) and used by both the capture and `select_model`.
2. **A rotating tip box above the picker whose first line reads like prose.** Expected: it never becomes a model. Pinned by `a_notice_box_is_not_a_card` (Task 1).
3. **The last box clipped by the status line (no closing border).** Expected: it is still a card, because the box boundary is the next opening border. Pinned by `the_clipped_last_box_is_still_a_card` (Task 1).
4. **A Freebuff release that moves the picker's copy.** Expected: the parse returns `None` loudly, the catalog answers with a fixed sentence, never screen text, and the chat still starts on Freebuff's own model. Pinned by `a_line_without_a_detail_line_below_it_is_not_a_card` (existing), `a_notice_box_is_not_a_card` (Task 1) and `the_failure_line_carries_a_sentence_and_no_models` (Task 2).
5. **A stale or case-mismatched model name.** Expected: an exact match or a refusal, never a nearest row. Pinned by `an_unmatched_name_is_a_refusal_not_a_nearest_row` (existing) and `a_failed_switch_refuses_the_prompt` (Task 3).

---

### Task 1: Read the picker Freebuff actually draws

**Files:**
- Modify: `Projects/OpenRemote-plugins/freebuff/driver/src/picker.rs` (parse and its helpers)
- Test: `Projects/OpenRemote-plugins/freebuff/driver/src/picker.rs` (`#[cfg(test)]`), using the two saved captures

**Interfaces:**
- Consumes: nothing new.
- Produces: `parse(&[String]) -> Option<Picker>` with the same signature, now understanding boxed frames. `Picker::find` unchanged.
- Produces: `pub fn retry_verdict(still_a_picker: bool) -> Retry` with `#[derive(Debug, Clone, Copy, PartialEq, Eq)] pub enum Retry { TypeIt, Refuse }`, the single rule that stops a retry from pressing Enter on a list.

- [ ] **Step 1: Write the failing tests**

Add to `picker.rs`, next to the design capture:

```rust
/// The real picker at 80x45, the width whose layout the design capture shows.
const PICKER_80: &str = include_str!("../../spike/screens/picker-0.2.35.txt");
/// The real picker at the driver's own 140x45.
const PICKER_140: &str = include_str!("../../spike/screens/picker-0.2.35-140x45.txt");

fn rows_of(capture: &str) -> Vec<String> {
    capture.lines().map(str::to_string).collect()
}

#[test]
fn the_eighty_column_capture_reads_six_cards_with_deepseek_marked() {
    let picker = parse(&rows_of(PICKER_80)).expect("a picker");
    let names: Vec<&str> = picker.cards.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(
        names,
        vec![
            "Glyph Cluster • high",
            "Solar Pro 4",
            "Solar Mini 4",
            "MiMo v2.6 Flash Slow",
            "DeepSeek V4.1 Flash Slow • high",
            "DeepSeek V4.1 Flash 10/17 • high",
        ]
    );
    assert_eq!(picker.current, Some(5));
    assert!(picker.cards[5].current);
    assert_eq!(picker.cards[4].section.as_deref(), Some("PREMIUM"));
    // The body line, verbatim, one per card.
    assert_eq!(
        picker.cards[0].detail.as_deref(),
        Some("1x · Preview model: prompts and outputs may be retained for training")
    );
    assert_eq!(picker.cards[3].detail.as_deref(), Some("1x"));
    assert_eq!(
        picker.cards[5].detail.as_deref(),
        Some("3x · May use data for AI training")
    );
    // No cost line: the notice is the body line.
    assert_eq!(picker.cards[2].detail, None);
    assert_eq!(
        picker.cards[2].note.as_deref(),
        Some("May use data for AI training")
    );
    assert_eq!(picker.cards[4].tagline.as_deref(), Some("Slow"));
}

#[test]
fn the_wide_capture_reads_seven_cards_without_costing_lines() {
    let picker = parse(&rows_of(PICKER_140)).expect("a picker");
    let names: Vec<&str> = picker.cards.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(
        names,
        vec![
            "DeepSeek V4.1 Flash 10/17 • high",
            "MiMo v2.6 Flash Fast",
            "Muse Spark 1.3 • high",
            "GLM 5.3 Flash • high",
            "GPT-6 Luna • high",
            "MiMo 2.6 Pro",
            "DeepSeek V4.1 Flash Fast 10/17 • high",
        ]
    );
    assert_eq!(picker.current, Some(1));
    assert_eq!(picker.cards[0].section.as_deref(), Some("OPTIMIZED"));
    assert_eq!(picker.cards[6].section.as_deref(), Some("POWERFUL"));
    // At this width the body line is prose, and there is no cost line to find.
    assert_eq!(
        picker.cards[0].note.as_deref(),
        Some("Recommended · May use data for AI training")
    );
    assert_eq!(picker.cards[1].detail, None);
    assert_eq!(picker.cards[1].note, None);
    assert_eq!(picker.cards[1].tagline.as_deref(), Some("Balanced"));
}

#[test]
fn the_clipped_last_box_is_still_a_card() {
    // The saved 140x45 frame ends inside the last box: the status line
    // overwrote its closing border. The next opening border is the boundary.
    let picker = parse(&rows_of(PICKER_140)).expect("a picker");
    assert_eq!(picker.cards.len(), 7);
    assert_eq!(
        picker.cards[6].note.as_deref(),
        Some("May use data for AI training")
    );
}

#[test]
fn a_notice_box_is_not_a_card() {
    // A tip box has a border and prose, and no ` · ` in its first line. It
    // must stay out of the catalog, because the console would offer it as a
    // model.
    let lines: Vec<String> = [
        " ┌──────────────────────────────────────────┐",
        " │ ✦ Refer friends → earn hours:            │",
        " │ ⎘ Copy invite link  Learn More ↵         │",
        " └──────────────────────────────────────────┘",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    assert!(parse(&lines).is_none());
}

#[test]
fn nothing_above_the_pickers_footer_is_read() {
    // The footer row is the picker's own header. A box above it is the chat's
    // notice area, and one of those boxes carries the balance. Note the
    // second line: a notice that looks exactly like a card must still stay
    // out, which is what the region cut is for.
    let mut lines: Vec<String> = [
        " ┌──────────────────────────────────────────┐",
        " │   Free Hours Pack · 3x · Promotional     │",
        " └──────────────────────────────────────────┘",
        " ↑↓ choose model · Tab reasoning · Enter select · Esc cancel",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    lines.extend(rows_of(PICKER_80));
    let picker = parse(&lines).expect("a picker");
    assert!(picker.cards.iter().all(|c| c.name != "Free Hours Pack"));
    assert_eq!(picker.cards.len(), 6);
}

#[test]
fn a_retry_is_refused_while_the_picker_is_still_open() {
    assert_eq!(retry_verdict(true), Retry::Refuse);
    assert_eq!(retry_verdict(false), Retry::TypeIt);
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --offline picker`
Expected: FAIL, `PICKER_80` unused warning and `retry_verdict` not found.

- [ ] **Step 3: Implement boxed and legacy modes in `parse`**

Keep the existing rule code as `fn parse_legacy(rows: &[&str]) -> Option<Picker>` and call it when no box content row is present. Add:

1. `fn is_box_border(row: &str) -> bool`: the trimmed row is made only of `┌ ─ ┐ ├ ┤ └ ┘ ┬ ┴ ┼` and scrollbar cells (`▄ ▀ █`).
2. `fn box_content(row: &str) -> Option<&str>`: the trimmed row starts with `│` and ends with `│`; the content is the text between, trimmed.
3. Region: when any row contains `↑↓ choose model`, keep only the rows after the last such row.
4. Walk the region. An opening border (`┌`) starts a box. The box's first content row is a card when the content contains ` · `; its name is the text up to the first ` · ` with a leading `›` or `×` stripped and trimmed, its tagline the second segment, its `current` flag true when the marker was `›`. The box's next content row is the body line, D12: `detail` when the first segment matches `^\d+x$`, else `note`.
5. A bare row between boxes whose first `·`-segment is all capitals, digits, dashes, spaces, at least three characters, is the section of the boxes after it. `PREMIUM` carries its range, `OPTIMIZED` and `POWERFUL` carry their description.
6. Keep the existing "at least two distinct card names" gate for both modes.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --offline picker`
Expected: PASS, the eight design-capture tests untouched.

- [ ] **Step 5: Commit**

```bash
cd Projects/OpenRemote-plugins/freebuff
git add driver/src/picker.rs
git commit -m "feat(driver): read the model picker as Freebuff draws it"
```

---

### Task 2: `--catalog` on the real screen

**Files:**
- Modify: `Projects/OpenRemote-plugins/freebuff/driver/src/catalog.rs` (the retry, one test, the mapping test for the wide capture)
- Test: `Projects/OpenRemote-plugins/freebuff/driver/src/catalog.rs`
- Create: `Projects/OpenRemote-plugins/freebuff/spike/catalog-0.2.35.json` (the printed line, committed)

**Interfaces:**
- Consumes: `picker::parse`, `picker::retry_verdict`, `Screen`, `Key`, `probe::preflight`.
- Produces: `pub fn run() -> i32` as today.

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn a_capture_reads_the_real_wide_picker_into_the_descriptor_shape() {
    let picker = picker::parse(&rows_of(PICKER_140)).expect("a picker");
    let catalog = Catalog::from_picker(&picker, 1_762_000_000_000);
    assert_eq!(catalog.models.len(), 7);
    assert_eq!(catalog.default.as_deref(), Some("MiMo v2.6 Flash Fast"));
    assert!(catalog.models[1].is_default);
    // A card with no cost line keeps its tagline as the second line.
    assert_eq!(catalog.models[0].detail.as_deref(), Some("Smart & Fast"));
    assert_eq!(
        catalog.models[0].note.as_deref(),
        Some("Recommended · May use data for AI training")
    );
    let value: serde_json::Value = serde_json::from_str(&catalog.to_line()).unwrap();
    assert_eq!(value["models"][6]["model"], "DeepSeek V4.1 Flash Fast 10/17 • high");
    assert_eq!(value["models"][1]["is_default"], true);
}

#[test]
fn the_failure_line_carries_a_sentence_and_no_models() {
    let line = failure_line(OPEN_REFUSAL);
    assert_eq!(OPEN_REFUSAL, "Freebuff did not open its model picker");
    let value: serde_json::Value = serde_json::from_str(&line).unwrap();
    assert_eq!(value["captured_at"], 0);
    assert!(value["models"].as_array().unwrap().is_empty());
    assert_eq!(value["reason"], OPEN_REFUSAL);
    assert!(!line.contains('│'), "a frame must never reach the reason");
}
```

Add `PICKER_140` (`include_str!` again) and a `rows_of` helper to the test module in
`catalog.rs`, because the two test modules do not share items.

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --offline catalog`
Expected: FAIL, `OPEN_REFUSAL` not found.

- [ ] **Step 3: Implement the retry guard and the reason constant**

1. `const OPEN_REFUSAL: &str = "Freebuff did not open its model picker";` and use it in `open_picker`.
2. In `open_picker`, for the second attempt: send Escape, settle, then `if picker::retry_verdict(capture_is_a_picker(screen).is_some()) == picker::Retry::Refuse { return Err(OPEN_REFUSAL.to_string()) }` before typing `/model` again. No Enter is sent while the screen is a picker, at any point.

- [ ] **Step 4: Run the live capture once and save it**

```powershell
cd Projects/OpenRemote-plugins/freebuff/driver
$env:CARGO_TARGET_DIR='D:\dsh-cargo\freebuff-driver'; cargo build --offline
cd ..
& D:\dsh-cargo\freebuff-driver\debug\freebuff-driver.exe --catalog
```

Expected on stdout: one JSON line with a non-empty `models` array (the real list, seven or more models), a `default`, and a `captured_at`. Save it verbatim as `spike/catalog-0.2.35.json`. If the printed list differs from `spike/screens/picker-0.2.35-140x45.txt`, also refresh that fixture with the card rows from the new frame (picker rows only, never the banner, tips box, status line or help line).

- [ ] **Step 5: Run the gates and commit**

```bash
cd driver && cargo test --offline && cargo clippy --all-targets --offline -- -D warnings && cargo fmt --all --check
cd .. && git add driver/src/catalog.rs spike/catalog-0.2.35.json spike/screens
git commit -m "feat(driver): capture Freebuff's real picker into models.json"
```

---

### Task 3: Start the chat on the model the console picked

**Files:**
- Modify: `Projects/OpenRemote-plugins/freebuff/driver/src/session.rs` (`start`, `prompt`, `maybe_send_prompt`, `handle_start`)
- Modify: `Projects/OpenRemote-plugins/freebuff/driver/src/protocol.rs` (`Events::initialized` stays; add `Events::model_reported`)

**Interfaces:**
- Produces: `pub fn needs_switch(current: Option<&str>, wanted: &str) -> bool`.
- Produces: `pub fn reported_model(verified: Option<&str>, requested: &str) -> String`.
- Produces: `pub enum WalkVerdict { Continue, Found, GiveUp }` and `pub fn walk_verdict(current: Option<&str>, wanted: &str, presses: u32, picker_open: bool) -> WalkVerdict`.
- Produces: `pub enum OpenVerdict { Ready, Retry, GiveUp }` and `pub fn opening_verdict(tail: Option<&[String]>, attempts: u32) -> OpenVerdict`.
- Produces: `Session::select_model(&mut self, wanted: &str) -> Result<(), String>`, `Session::prompt_block(&self) -> Option<String>`.
- Adds to `Session`: `model_error: Option<(String, Option<String>)>`, `frames: screen::FrameLog`.

- [ ] **Step 1: Write the failing tests** (verbatim from the superseded plan, Task 4)

```rust
#[test]
fn no_arrow_is_sent_when_freebuff_already_runs_the_wanted_model() {
    assert!(!needs_switch(Some("DeepSeek V4.1 Flash 10/17 • high"), "DeepSeek V4.1 Flash 10/17 • high"));
    assert!(needs_switch(Some("DeepSeek V4.1 Flash Slow • high"), "DeepSeek V4.1 Flash 10/17 • high"));
    // Nothing read yet: read the picker, never assume.
    assert!(needs_switch(None, "DeepSeek V4.1 Flash 10/17 • high"));
}

#[test]
fn a_prompt_after_a_failed_switch_is_refused_not_sent() {
    let mut session = Session::new();
    session.model_error = Some(("X".into(), Some("Y".into())));
    let block = session.prompt_block().expect("a refusal");
    assert!(block.contains("did not switch to X"));
    assert!(block.contains("still on Y"));
    assert!(block.contains("not sent"));
}

#[test]
fn an_unreadable_picker_refuses_rather_than_guesses() {
    let mut session = Session::new();
    session.model_error = Some(("X".into(), None));
    assert!(session.prompt_block().unwrap().contains("did not switch to X"));
}

#[test]
fn the_walk_gives_up_instead_of_pressing_for_ever() {
    use WalkVerdict::*;
    assert_eq!(walk_verdict(Some("A"), "B", 0, true), Continue);
    assert_eq!(walk_verdict(Some("B"), "B", 7, true), Found);
    assert_eq!(walk_verdict(Some("A"), "B", 240, true), GiveUp);
    assert_eq!(walk_verdict(Some("A"), "B", 3, false), GiveUp);
}

#[test]
fn a_survey_between_keys_gets_one_retry_and_never_a_typed_prompt() {
    use OpenVerdict::*;
    let survey: Vec<String> = ["How did you hear about Freebuff?", "1. A friend", "2. Search"]
        .iter().map(|s| s.to_string()).collect();
    assert_eq!(opening_verdict(Some(&survey), 0), Retry);
    assert_eq!(opening_verdict(Some(&survey), 1), GiveUp);
    assert_eq!(opening_verdict(None, 0), Retry);
}

#[test]
fn initialized_reports_the_verified_model_not_the_requested_one() {
    assert_eq!(reported_model(Some("Y"), "X"), "Y");
    assert_eq!(reported_model(None, "X"), "X");
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --offline session`
Expected: FAIL, `needs_switch` not found.

- [ ] **Step 3: Implement `select_model`**

1. Send `/model` then Enter, settle 350 ms / 3 s; parse `tail(45)` through `opening_verdict`, which decides whether keystrokes continue at all. `GiveUp` returns `"Freebuff did not open its model picker"`. A retry goes through `picker::retry_verdict`, D11.
2. When `needs_switch(current, wanted)` is false, send Escape, settle, and return `Ok(())`. Write `models.json` from this screen with `Catalog::from_picker` (best effort, working directory) because this is the free refresh D7 keeps.
3. Walk up to 240 times: Down, settle, parse, `walk_verdict`. Stop on `Found`. A vanished picker or spent presses returns `"Freebuff does not offer a model called X"`.
4. Send Enter, settle. Picker rows still visible: Escape, settle, return `"Freebuff did not close its picker"`.
5. Verify by reopening: `/model`, Enter, settle, parse; the marked card must equal `wanted`; Escape, settle. Otherwise return `"Freebuff kept Y after the switch to X"`.
6. On every `Err`, leave the TUI picker-free and record the actual model when it was read.

- [ ] **Step 4: Wire it into `start` and `prompt`**

`start` runs `probe::preflight`, spawns the PTY, then when `model` is present runs `select_model`. On `Err` it stores `model_error = Some((requested, actual))`, writes the screen and both names to stderr through `diag`, and continues to a normal `initialized` (never `fail_closed`) with `reported_model(verified, requested)`. `prompt` returns early and emits the turn-completed error with the block sentence instead of setting `pending_prompt`. A successful `select_model` writes `models.json` from the final picker read.

- [ ] **Step 5: Run the tests, then check it against the CLI**

```powershell
cd Projects/OpenRemote-plugins/freebuff/driver
$env:CARGO_TARGET_DIR='D:\dsh-cargo\freebuff-driver'; cargo test --offline
& D:\dsh-cargo\freebuff-driver\debug\freebuff-driver.exe   # then: {"type":"spawn","cwd":"C:\\Users\\moreweb","model":"<a name from models.json>"}
```

Expected on stderr: the version line, `started Freebuff`, and, when the model differs, the arrow walk and the verified model. `initialized` names the verified model. Then `{"type":"shutdown"}`.

- [ ] **Step 6: Commit**

```bash
git add driver/src/session.rs driver/src/protocol.rs
git commit -m "feat(driver): start a chat on the model the console picked, verified"
```

---

### Task 4: Live model switching

**Files:**
- Modify: `Projects/OpenRemote-plugins/freebuff/driver/src/session.rs` (`handle_start` `Command::Settings` branch)
- Modify: `Projects/OpenRemote-plugins/freebuff/driver/src/protocol.rs` (`Events::model_reported`, `Events::settings_refused`)

**Interfaces:**
- Produces: `Events::model_reported(&mut self, model: &str) -> bool` emitting `{"type":"model_reported","model":"..."}`, and `Events::settings_refused(&mut self, message: &str) -> bool` emitting `{"type":"settings_refused","message":"..."}`.
- Consumes: `Session::select_model`.

- [ ] **Step 1: Write the failing tests**

```rust
// protocol.rs
#[test]
fn a_reported_model_is_one_line_with_its_word() {
    let (mut events, buffer) = capture();
    assert!(events.model_reported("DeepSeek V4.1 Flash 10/17 • high"));
    assert_eq!(lines(&buffer), vec![r#"{"model":"DeepSeek V4.1 Flash 10/17 • high","type":"model_reported"}"#]);
}

#[test]
fn a_refused_settings_change_carries_one_sentence() {
    let (mut events, buffer) = capture();
    assert!(events.settings_refused("a turn is in flight; change the model when it lands"));
    assert_eq!(lines(&buffer), vec![r#"{"message":"a turn is in flight; change the model when it lands","type":"settings_refused"}"#]);
}

// session.rs
#[test]
fn a_turn_in_flight_refuses_a_model_change_without_touching_the_screen() {
    let mut session = Session::new();
    session.turn_open = true;
    let message = session.settings_refusal(&Some(serde_json::json!("X")), &None, &None, &None).expect("a refusal");
    assert!(message.contains("turn"));
}

#[test]
fn effort_and_fast_have_no_word_in_freebuff() {
    let mut session = Session::new();
    let message = session.settings_refusal(&None, &Some(serde_json::json!("high")), &Some(serde_json::json!(true)), &None).expect("a refusal");
    assert!(message.contains("effort"));
}
```

- [ ] **Step 2: Implement `settings_refusal` and the live path**

`settings_refusal(&self, model, effort, fast, permission_mode) -> Option<String>` returns the first applicable sentence: a turn is in flight; Freebuff has no effort word; Freebuff has no fast mode; Freebuff has no permission mode. Otherwise, with a model present, `handle_start` runs `select_model` in place, after a `diag` line saying the drive takes seconds (the loop pumps the PTY through `settle`, so this is safe). Success emits `model_reported(verified)`; failure emits `model_reported(actual)` when known, then `settings_refused(sentence)`. Every outcome also goes to `diag`.

- [ ] **Step 3: Run the tests and commit**

```bash
cd driver && cargo test --offline && cargo clippy --all-targets --offline -- -D warnings && cargo fmt --all --check
cd .. && git add driver/src/session.rs driver/src/protocol.rs
git commit -m "feat(driver): change the model in a running chat, or refuse in a sentence"
```

---

### Task 5: The daemon seam

**Files:**
- Modify: `Projects/OpenRemote/daemon/crates/openremote-harness/src/lib.rs` (`ModelDescriptor`)
- Modify: `Projects/OpenRemote/daemon/crates/openremote-harness/src/events.rs` (`DriverEvent`, after `ModelReported`)
- Modify: `Projects/OpenRemote/daemon/crates/openremote-daemon/src/plugin_driver.rs` (`apply_settings`, `parse_event`)
- Modify: `Projects/OpenRemote/daemon/crates/openremote-daemon/src/plugins.rs` (`HarnessManifest`, `HarnessSpec`, `installed_harnesses`, `validate_harness`)
- Modify: `Projects/OpenRemote/daemon/crates/openremote-daemon/src/registry.rs` (Harness build)
- Modify: `Projects/OpenRemote/daemon/crates/openremote-daemon/src/supervisor.rs` (`update_settings`, `pump_one`)
- Modify: `Projects/OpenRemote/daemon/crates/openremote-core/src/model.rs` (`Harness`)
- Modify: `Projects/OpenRemote/daemon/crates/openremote-daemon/src/http.rs` (routes, `PEER_ROUTES`)
- Test: the `#[cfg(test)]` modules of `plugins.rs`, `plugin_driver.rs`, `supervisor.rs`, `http.rs`, plus `daemon/crates/fixture-driver`

**Interfaces:**
- `ModelDescriptor` gains `#[serde(default, skip_serializing_if = "Option::is_none")] pub detail: Option<String>` and `pub note: Option<String>`.
- `HarnessManifest` gains `#[serde(default)] pub models_file: Option<String>` and `#[serde(default)] pub settings: Option<String>`.
- `HarnessSpec` gains `pub models_captured_at: Option<u64>` and `pub settings_live: bool`; `openremote_core::Harness` gains the same two, both skipped when absent.
- `DriverEvent` gains `SettingsRefused { message: String }`; `parse_event` maps `"model_reported"` and `"settings_refused"`.
- `plugins::read_catalog(dir: &Path, name: &str) -> Option<(u64, Vec<ModelDescriptor>)>` joins, canonicalises, refuses an escape with `None`, parses `{"captured_at","models":[..]}` and requires at least one model with a non-empty `model`. Empty falls through to the manifest, never an empty list.
- `PluginDriver::apply_settings` writes `{"type":"settings","model":...,"effort":...,"fast":...}` and returns `Ok(())` when `spec.settings_live`; otherwise today's error verbatim.
- New route `POST /harnesses/{id}/catalog/refresh` returning `{"harness": <Harness>}` through `receipted_plain`; not in `PEER_ROUTES`.
- `update_settings` refuses a live harness in `Starting | Working | Waiting` with `Conflict("wait for this turn to finish, then change the model")`.

- [ ] **Step 1: Write the failing tests** (one per named behaviour; bodies as in the superseded plan, Task 6)

`plugins.rs`: `a_models_file_inside_the_plugin_folder_wins_over_the_manifest`, `a_models_file_that_escapes_the_folder_is_ignored`, `a_malformed_models_file_falls_back_to_the_manifest`, `an_unknown_settings_word_is_refused_at_install`.
`plugin_driver.rs`: `a_reported_model_and_a_refused_setting_map_to_the_drivers_own_words` (the two `matches!` assertions from the superseded plan).
`supervisor.rs`: `a_live_model_change_is_refused_while_a_turn_works`, `a_refused_live_setting_lands_as_a_note_in_the_chat`.
`harness` crate: `a_descriptor_with_a_detail_and_a_note_round_trips_and_the_old_shape_still_parses`.
`http.rs`: `the_refresh_route_returns_the_harness_row_and_refuses_while_a_chat_runs`.

- [ ] **Step 2: Run `cargo test --locked` and expect the new tests to fail**

- [ ] **Step 3: Implement the fields, the mappings and the route**

Route behaviour: refuse with `StatusCode::CONFLICT` and `"a Freebuff chat is running; refresh when it stops"` when `plugin_chats(&plugin_id)` is not empty; otherwise run `<driver> --catalog` with `current_dir(spec.dir)` under a 60 s deadline and `kill_on_drop`, treat a non-zero exit or a line with `reason` as `StatusCode::SERVICE_UNAVAILABLE` carrying that sentence, then `reprobe_harnesses()` and answer with the harness row.

- [ ] **Step 4: Run the daemon gates**

Run: `cd Projects/OpenRemote/daemon && cargo test --locked && cargo clippy --all-targets --locked -- -D warnings && cargo fmt --all --check`
Expected: green.

- [ ] **Step 5: Commit**

```bash
cd Projects/OpenRemote && git add daemon && git commit -m "feat(daemon): plugin model catalogs, live settings, and a catalog refresh route"
```

---

### Task 6: The console

**Files:**
- Modify: `Projects/OpenRemote/src/api/daemon.js` (after `models`)
- Modify: `Projects/OpenRemote/src/state/console.jsx` (`modelsFor`, add `refreshModels`)
- Modify: `Projects/OpenRemote/src/PickerMenu.jsx` (item rendering)
- Create: `Projects/OpenRemote/src/relative-time.js`, `Projects/OpenRemote/src/relative-time.test.js`
- Modify: `Projects/OpenRemote/src/NewChat.jsx` (`modelItems`, the model menu's footer, `chooseModelId`)
- Modify: `Projects/OpenRemote/src/ChatView.jsx` (`modelItems`)
- Modify: `Projects/OpenRemote/src/composer-menus.css`, `Projects/OpenRemote/src/newchat.css` (one rule each)
- Test: `Projects/OpenRemote/src/picker-detail.test.js`

**Interfaces:**
- `api.catalogRefresh(harnessId)` posts `/harnesses/{id}/catalog/refresh` with `timeoutMs: 90_000` and returns `{ harness }`.
- `refreshModels(harnessId, deviceId = null)`: only for this console's own machine, calls `api.catalogRefresh`, then `refreshCapabilities()`, then drops the `modelsByHarness` entry for that key and asks `modelsFor` again. Shaped like `refreshEndpoints`.
- `relativeTime(at, now)` returns `"just now"`, `"3 minutes ago"`, `"2 hours ago"`, `"6 days ago"`, or `"on 2026-10-31"` beyond 30 days.
- `PickerMenu` renders `item.detail` as `<span className="nc-item-detail">` under the name inside `nc-item-main`, and `item.note` as the row's `title`.

- [ ] **Step 1: Write the failing tests**

```js
// relative-time.test.js
test('a capture from two days ago reads as two days ago', () => {
  assert.equal(relativeTime(Date.parse('2026-10-31T10:00:00Z'), Date.parse('2026-11-02T10:00:00Z')), '2 days ago')
})
test('a capture from last month names the day', () => {
  assert.equal(relativeTime(Date.parse('2026-09-30T10:00:00Z'), Date.parse('2026-11-02T10:00:00Z')), 'on 2026-09-30')
})
test('a missing capture is not a date', () => {
  assert.equal(relativeTime(0, Date.parse('2026-11-02T10:00:00Z')), 'not captured yet')
})

// picker-detail.test.js
test('a model row shows its cost tier as a second line', () => { /* items [{id:'m', name:'DeepSeek V4.1 Flash 10/17 • high', detail:'3x · Smart & Fast'}] renders both strings */ })
test('the picker footer offers a refresh with the capture age', () => { /* footer prop renders 'Captured 2 days ago' and a Refresh button */ })
```

- [ ] **Step 2: Run `npm test` and expect them to fail; implement the four pieces; run `npm test` and `node scripts/check-tab-acl.mjs`**

`NewChat.jsx` maps each catalog model to `{ id, name, resolved_model, detail: m.detail ?? null }`; its model menu footer shows `Captured ${relativeTime(harness.models_captured_at, Date.now())}` and a Refresh button that calls `refreshModels(currentHarness.id, target)`, replacing the list in place and showing the daemon's sentence on failure. `ChatView.jsx` maps the same two fields.

- [ ] **Step 3: Commit**

```bash
cd Projects/OpenRemote && git add src && git commit -m "feat(console): Freebuff model rows, capture age, and refresh"
```

---

### Task 7: Ship the manifest, the protocol, and the docs

**Files:**
- Modify: `Projects/OpenRemote-plugins/freebuff/openremote.json`
- Modify: `Projects/OpenRemote-plugins/freebuff/PROTOCOL.md` (§2 modes, §3 commands, §4 events, §5 `spawn`/`settings`, §7 what it never reports)
- Modify: `Projects/OpenRemote-plugins/freebuff/README.md`
- Modify: `Projects/OpenRemote/docs/harness-plugins.md` (the manifest's `models_file` and `settings` keys)
- Binary: `Projects/OpenRemote-plugins/freebuff/bin/windows-x64/freebuff-driver.exe`

**Interfaces:**
- `openremote.json`'s harness block gains `"models"` (the array from `spike/catalog-0.2.35.json`), `"default_model"` (its `default`), `"models_file": "models.json"`, `"settings": "live"`; `version` goes from `0.1.0` to `0.2.0`.

- [ ] **Step 1: Write the manifest and prove it parses**

Paste the catalog's models array and its `default`, add the two keys, bump the version. Run `node -e "JSON.parse(require('fs').readFileSync('openremote.json'))"`.

- [ ] **Step 2: Update `PROTOCOL.md` and the two READMEs**

`--catalog` as a third mode (one JSON line, exit 0 always, writes `models.json`); `model_reported` and `settings_refused` in §4; live `settings` in §5 replacing the blanket refusal; the picker rules and the "no keystroke without a parsed screen" rule, plus the "no Enter while the picker is open" retry rule, in §5 `spawn`; the exact-match model rule and the stderr-only privacy rule in §7 and §8. The README says what now works, what a Freebuff update breaks, the throwaway chat `.catalog-cwd` leaves behind, that Freebuff remembers the last model chosen (D1), and that at 140x45 the picker prints no per-model cost.

- [ ] **Step 3: Build and stage the binary, then run every gate**

```powershell
cd Projects/OpenRemote-plugins/freebuff/driver
$env:CARGO_TARGET_DIR='D:\dsh-cargo\freebuff-driver'; cargo build --release --offline
cp D:\dsh-cargo\freebuff-driver\release\freebuff-driver.exe ..\bin\windows-x64\freebuff-driver.exe
..\bin\windows-x64\freebuff-driver.exe --probe   # {"available":true,...}
cargo test --offline; cargo clippy --all-targets --offline -- -D warnings; cargo fmt --all --check
cd ..\..\..\OpenRemote\daemon; cargo test --locked; cargo clippy --all-targets --locked -- -D warnings; cargo fmt --all --check
cd ..; npm test; node scripts/check-tab-acl.mjs
```

- [ ] **Step 4: End to end in the dev app**

Install the plugin, enable it, start a chat on a model from the picker, send one prompt, change the model mid-chat, send another. Expected: the chip names the model the driver verified both times; with a chat running, Refresh says a chat is running; after stopping, Refresh updates the capture age. Record what happened in the commit message.

- [ ] **Step 5: Commit both repositories**

```bash
cd Projects/OpenRemote-plugins && git add freebuff && git commit -m "feat(freebuff): ship the model catalog and live model switching"
cd ../OpenRemote && git add docs && git commit -m "docs(plugins): the manifest's models_file and settings keys"
```

---

## What this plan does not do

- A per-model cost tier at 140 columns. Freebuff does not print one; carrying the section label is a phase 1.5 change (one optional `section` field plus a grouped model menu).
- A capture at 80x45, which would have cost tiers but could name models the chat's picker does not offer.
- Anything in phases 2 to 5: slash commands, LITE and DEFAULT, balance and spend approvals, history and follow-up suggestions.
- A restore of Freebuff's remembered model on shutdown (D1: resume re-applies it).
- A macOS or Linux driver build, and any new dependency in the driver crate.

## Self-review

- **Spec coverage:** catalog (1.1, D7), seam detail/note (1.2, D9), start on a chosen model (1.3, Task 3), mid-chat switching (1.4, Task 4), console (1.5, Task 6), refresh route and capture age (1.1, Tasks 5 to 6), the data-use notice (D9, D12), the "no keystroke on an unverified screen" rule (D11, Review Focus 1). The spec's cost-tier row is revised by D9 and listed under what this plan does not do.
- **Type consistency:** `parse`, `find`, `retry_verdict`, `needs_switch`, `reported_model`, `walk_verdict`, `opening_verdict`, `select_model`, `prompt_block`, `detail`, `note`, `models_file`, `settings`, `models_captured_at`, `settings_live`, `settings_refused`, `model_reported`, `read_catalog`, `catalogRefresh`, `refreshModels`, `relativeTime` are each defined once and used with the same shape.
- **Review Focus:** each of the five lines names its test.
- **Proportion:** the two driver tasks that hold the risk carry the tests; the daemon, console and ship tasks carry interfaces, one test list and the gates, because their decisions are unchanged from the superseded plan.
