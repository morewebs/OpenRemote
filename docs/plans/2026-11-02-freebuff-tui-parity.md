# Freebuff TUI Parity, Phases 0 and 1, Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Give the OpenRemote console Freebuff's real model list, a chat that starts on the model the user picked, and a model switch in a running chat, all by driving Freebuff's own TUI and verifying every step on a real screen.

**Architecture:** Phase 0 turns the driver's growing text scrape into a cursor-aware 140x45 screen, adds a key vocabulary and a settle wait, and makes every parse a pure function of the visible rows. Phase 1 builds the model catalog (`freebuff-driver --catalog` writes `models.json` and prints the same JSON), selects a model at spawn by arrowing to the row marked `›` and verifying by re-reading the picker, and changes a model between turns through the existing `settings` command. The daemon gains two optional descriptor fields, a `models_file` and `settings` word in the plugin manifest, a `captured_at` stamp and refresh route, and the `model_reported` / `settings_refused` events the rest of the daemon already understands.

**Tech Stack:** Rust (driver: `portable_pty`, `serde_json`, no new dependencies; daemon: axum, tokio, serde), React 19 / Vite console, `node --test`.

**Spec:** `docs/superpowers/specs/2026-11-02-freebuff-tui-parity-design.md`

## Global Constraints

- Windows 10 x64 first. `bin/windows-x64/freebuff-driver.exe` is the only driver this plan builds; the manifest's `default` path stays as it is and is not a phase 1 deliverable.
- Freebuff CLI 0.2.35 or newer; the driver refuses older with a sentence. Store layout is internal to the CLI.
- The pseudo terminal is 140x45 (`ptyproc::ROWS`, `ptyproc::COLS`). Every parse reads visible rows only, never the scrollback.
- No TUI frame text in any event that is stored or synced. Screen text goes to stderr and the `stderr` event only; `error_message` and notes carry at most redacted, clipped sentences.
- The protocol stays additive: new event types, new optional fields, unknown fields ignored in both directions (`PROTOCOL.md` §2).
- The driver never puts an address, a token, or an account id in anything it reports. `credentials.json` presence is checked and never read.
- Harness vocabulary is the harness's own: Freebuff's model names, its cost words, its sections, verbatim.
- No em dashes in any copy, comment, or document.
- Gates: `cd Projects/OpenRemote-plugins/freebuff/driver && cargo test && cargo clippy --all-targets -- -D warnings && cargo fmt --all --check`; `cd Projects/OpenRemote/daemon && cargo test --locked && cargo clippy --all-targets --locked -- -D warnings && cargo fmt --all --check`; `cd Projects/OpenRemote && npm test && node scripts/check-tab-acl.mjs`.
- `--probe` stays inside the daemon's 5 s deadline: no PTY work, no catalog capture, no model read.
- Phase 1 ships no balance or cost reporting, no slash-command menu, no mode picker, no history surface. Those are phases 2 to 5 and stay untouched.
- Paths in this plan are written from the moreweb workspace root (`C:\Users\moreweb\Documents\Workspaces\moreweb`). Run the gates from the repository they name.

## Decisions this plan makes

The spec's two open questions and the four items it left to the builder are answered here. Each answer is cheap to reverse before its task starts and expensive after.

| # | Question | Decision | Why |
|---|----------|----------|-----|
| D1 | Where Freebuff keeps the model picked in `/model` (per chat, or machine-wide) | The driver treats it as **machine-wide state it borrows**, honestly. Every spawn reads the picker's current row first, reports it in `initialized`, and switches only when the wanted model differs. A stored chat model means a resume switches Freebuff back to that chat's model. No automatic restore on shutdown. | A restore would be a second TUI drive inside the daemon's 3 s shutdown deadline, and can fail halfway. Resume already restores: the chat's stored model is the truth it re-applies. If the pick turns out to be per chat after all, nothing above misbehaves. |
| D2 | Mid-chat switching: Freebuff only, or wait for every harness | **Ship it for Freebuff only**, declared by a new manifest word `"settings": "live"`. The console already posts model changes for any harness; only the daemon's refusal changes. | The spec's phase table already puts switching in phase 1, and the manifest word is exactly how a plugin says what it can do. Holding it back would make Freebuff the one harness that cannot do what its own TUI does. |
| D3 | Cost tier in the row, or one hover away | **In the row**: `display_name` on the first line, `detail` (`3x · Smart & Fast`) as a muted second line, the data-use `note` as the row's hover title. Both new fields are optional on `ModelDescriptor`. | A cost tier a user has to hover to find is a tier they discover after the spend. Keeping `note` on hover keeps the row one line tall for harnesses that fill neither field. |
| D4 | Screen representation | **A cursor-aware 140x45 grid is the parse source** (`screen::Screen`). The stripped byte stream stays, used only by the diagnostic frame log. An escape sequence the grid does not implement marks the screen unparseable, and every parse returns `None` until a clear or a full repaint. **Deviation from the spec's `tail(lines)` over the stripped buffer.** | The TUI repaints by rewriting changed rows. The stripped buffer then holds the old row and the new one, so a "the `›` is on the row I asked for" check can pass on a stale copy. The anti-goals require a verified screen, not a plausible one. |
| D5 | Screen text in `turn_completed` | **No screen text in events.** A failed switch reports the requested and the actual model, and puts the quoted screen on stderr. **Deviation from the spec's 1.3 wording.** | `error_message` becomes a `DaemonError` note in the chat transcript, and a synced chat carries that note to the user's other devices. `PROTOCOL.md` §6 already promises that field is redacted and clipped, and the driver's own comment refuses to put a frame there. |
| D6 | A failed model switch at spawn | **The prompt is not sent.** The chat opens on Freebuff's current model, `initialized` names that model, and the first `prompt` is refused with a `turn_completed` error naming both models. The chat stays alive so the user can pick again or resend. | Anti-goal 1 forbids spending on a model the user did not choose. Sending anyway is exactly that spend; dropping the prompt silently is a lie, so it is refused out loud. |
| D7 | The catalog capture process | **No capture at every spawn.** `--catalog` is the capture, run on demand and by the console's Refresh. A spawn that opens the picker for a model choice writes `models.json` from the screen it already has. A refresh is refused while any chat on that plugin is live, and its throwaway chat lands in one dedicated project folder. **Deviation from the spec's 1.1 "same parser at every spawn".** | A settle round is 350 ms quiet or 3 s spent, per interaction, so a paged capture per chat start is 10 to 30 s of user-visible latency before the first prompt. Refresh is rare and a written-down button; a chat start is not. |

Items the spec left to the builder, now decided:

1. **Key sequence.** Arrows only, never a typed filter: `/model` then `enter` opens the picker, `down` walks the rows, `enter` selects, `escape` closes. Typing is not used at all, so whether typing filters never needs answering.
2. **Picker height and paging.** Never read the height. Walk with `down` one row at a time and stop when the selection has not moved for two presses (bottom of the list), bounded at 240 presses. The visible window scrolls itself, so PageUp/PageDown support never needs answering.
3. **Cache location.** `<plugin folder>/models.json`, joined and contained exactly like the driver path in `HarnessManifest::resolved_driver`. The driver writes it from its own working directory, which the daemon already sets to the plugin folder.
4. **`detail` and `note`.** Their own optional `ModelDescriptor` fields. Folding them into `display_name` would make one harness's price list unparseable by the picker rows that match a session's model.

## Review Focus

1. **A repaint mid-read.** A row rewritten while the picker is open, or a screen that scrolled between two presses. Expected: the driver refuses and says so; it never acts on a row it has not seen marked. Pinned by `a_line_rewrite_replaces_the_old_row_not_append` (Task 1) and `the_walk_gives_up_instead_of_pressing_for_ever` (Task 4).
2. **Freebuff's startup noise.** A resume notice, a survey, an ad, and the cost dialog can all appear between two keystrokes. Expected: a keystroke is only sent after a verified picker; anything else gets one retry, then a sentence. Pinned by `a_survey_between_keys_gets_one_retry_and_never_a_typed_prompt` (Task 4).
3. **A model name that no longer exists.** A stale catalog naming a model Freebuff dropped, or a name that differs by case or punctuation. Expected: an exact match or a refusal, never a nearest row. Pinned by `an_unmatched_name_is_a_refusal_not_a_nearest_row` (Task 2).
4. **A Freebuff update under us.** A moved store, a new wording, a new screen. Expected: the parse fails loudly, the chat still starts on Freebuff's own model, and the console says why. Pinned by `an_unknown_escape_sequence_marks_the_screen_unparseable` (Task 1) and `a_prompt_after_a_failed_switch_is_refused_not_sent` (Task 4).
5. **Two chats at once.** A second chat's model switch changes Freebuff's remembered model while the first chat is live. Expected: each chat's chip shows the model its own session verified, and a resume re-applies that chat's model. Pinned by `initialized_reports_the_verified_model_not_the_requested_one` (Task 4) and `a_reported_model_and_a_refused_setting_map_to_the_drivers_own_words` (Task 6).

---

### Task 1: The screen grid

**Files:**
- Create: `Projects/OpenRemote-plugins/freebuff/driver/src/screen.rs`
- Modify: `Projects/OpenRemote-plugins/freebuff/driver/src/main.rs` (module list, line 13)
- Modify: `Projects/OpenRemote-plugins/freebuff/driver/src/session.rs` (replace the `screen: String` field and its uses: `pump_pty` 481-505, `screen_tail` 813-833, `scan_screen` 714-759, `note_no_activity` 761-808, and the tests that set `session.screen` at 991, 1003, 1028, 1044, 1069, 1113, 1128)

**Interfaces:**
- Produces:
  - `pub struct Screen { .. }` with `pub fn new(rows: u16, cols: u16) -> Self`, `pub fn feed(&mut self, bytes: &[u8])`, `pub fn tail(&self, lines: usize) -> Option<Vec<String>>`, `pub fn size(&self) -> (u16, u16)`.
  - `tail` returns `None` when the screen has seen an escape sequence this grid does not implement since its last clear, and `Some` rows otherwise, last `lines` non-empty rows bottom-up then reversed so the newest row is last.
  - `pub fn joined_tail(&self, lines: usize) -> Option<String>` for the existing substring matchers, rows joined with `" | "`.
  - `pub struct FrameLog { .. }` with `pub fn new(capacity: usize) -> Self`, `pub fn push(&mut self, label: &str, tail: String)`, `pub fn last(&self) -> Option<(&str, &str)>`. It keeps the last 8 interaction frames; the driver quotes only the last one, and only on stderr.

- [ ] **Step 1: Write the failing tests in `screen.rs`**

```rust
#[test]
fn a_line_rewrite_replaces_the_old_row_not_append() {
    let mut screen = Screen::new(45, 140);
    screen.feed(b"one\r\ntwo");
    screen.feed(b"\x1b[1A\r\x1b[2Ktwo-changed");
    assert_eq!(screen.tail(2), Some(vec!["one".into(), "two-changed".into()]));
}

#[test]
fn erasing_the_screen_empties_the_tail() {
    let mut screen = Screen::new(45, 140);
    screen.feed(b"a\r\nb\x1b[2J\x1b[Hc");
    assert_eq!(screen.tail(3), Some(vec!["c".into()]));
}

#[test]
fn a_newline_at_the_bottom_scrolls_and_keeps_the_last_frame() {
    let mut screen = Screen::new(3, 20);
    screen.feed(b"1\r\n2\r\n3\r\n4");
    assert_eq!(screen.tail(3), Some(vec!["2".into(), "3".into(), "4".into()]));
}

#[test]
fn an_unknown_escape_sequence_marks_the_screen_unparseable() {
    let mut screen = Screen::new(45, 140);
    screen.feed(b"models here\x1b[?1049h");
    assert_eq!(screen.tail(10), None);
    // A clear brings it back: the screen is again something a person sees.
    screen.feed(b"\x1b[2J\x1b[Hfresh");
    assert_eq!(screen.tail(10), Some(vec!["fresh".into()]));
}

#[test]
fn a_multibyte_char_split_across_reads_still_lands() {
    let mut screen = Screen::new(45, 140);
    let bytes = "· ok".as_bytes();
    screen.feed(&bytes[..1]);
    screen.feed(&bytes[1..]);
    assert_eq!(screen.tail(1), Some(vec!["· ok".into()]));
}

#[test]
fn the_tail_skips_blank_rows_and_keeps_order() {
    let mut screen = Screen::new(45, 140);
    screen.feed(b"a\r\n\r\nb\r\n\r\nc");
    assert_eq!(screen.tail(2), Some(vec!["b".into(), "c".into()]));
}

#[test]
fn a_wrapped_long_line_keeps_reading_order() {
    let mut screen = Screen::new(45, 10);
    screen.feed(b"0123456789ABCDEF");
    assert_eq!(screen.tail(2), Some(vec!["0123456789".into(), "ABCDEF".into()]));
}

#[test]
fn the_frame_log_keeps_the_last_eight_interactions() {
    let mut log = FrameLog::new(8);
    for i in 0..12 {
        log.push(&format!("step {i}"), format!("screen {i}"));
    }
    let (label, tail) = log.last().expect("a frame");
    assert_eq!(label, "step 11");
    assert_eq!(tail, "screen 11");
}
```

- [ ] **Step 2: Run `cargo test` in `Projects/OpenRemote-plugins/freebuff/driver`**
Expected: FAIL, `Screen` not found.

- [ ] **Step 3: Implement `Screen` in `screen.rs`**

A `rows: Vec<Vec<char>>` of `cols` cells, a cursor at `(row, col)`, and `unparseable: bool`. `feed` decodes incrementally: keep undecoded bytes in `pending`, use `std::str::from_utf8` and `valid_up_to`, and only consume complete characters. Handle exactly these, and mark `unparseable` on any other CSI or ESC form:

`\r` (col 0), `\n` (LF: row + 1, scrolling at the bottom), `\t` (col to the next multiple of 8), printable characters (write at the cursor, wrapping at `cols`), `ESC [` with `A B C D` (relative moves), `H`/`f` (absolute, default 1;1), `G` (column), `d` (row), `J` with 2 (clear), `K` with 0, 1, 2 (erase row or part of it), `m` (ignored), `?25l`/`?25h` (ignored), `S`/`T` (scroll up/down), `ESC [` `n` where `n` is `6` (ignored: the cursor report), and `ESC ]` OSC up to BEL or `ESC \` (ignored). The existing `strip_ansi` in `ptyproc.rs` stays for the diagnostic stream and is not used here.

- [ ] **Step 4: Run `cargo test`**
Expected: all `screen` tests PASS.

- [ ] **Step 5: Replace `Session.screen` with the grid**
`Session.screen: Screen::new(ROWS, COLS)`; `pump_pty` feeds bytes to the grid and appends the stripped text to a new `frame_text: String` (the diagnostic buffer, capped as today); `scan_screen`, `note_no_activity` and `screen_tail` read `joined_tail`. Existing tests that assign a literal screen feed bytes instead, for example `session.screen.feed(b"Can't reach Freebuff's servers")`.

- [ ] **Step 6: Run `cargo test`, `cargo clippy --all-targets -- -D warnings`, `cargo fmt --all --check`**
Expected: all green, no test renamed except where it fed a literal string.

- [ ] **Step 7: Commit**

```bash
cd Projects/OpenRemote-plugins/freebuff
git add driver/src/screen.rs driver/src/main.rs driver/src/session.rs
git commit -m "feat(driver): parse the TUI from a cursor-aware screen grid"
```

---

### Task 2: Keys, settle, and the picker parser

**Files:**
- Create: `Projects/OpenRemote-plugins/freebuff/driver/src/keys.rs`
- Create: `Projects/OpenRemote-plugins/freebuff/driver/src/picker.rs`
- Modify: `Projects/OpenRemote-plugins/freebuff/driver/src/main.rs` (module list)
- Modify: `Projects/OpenRemote-plugins/freebuff/driver/src/session.rs` (add `settle`, use `keys` for the interrupt escape and the prompt's paste plus enter)

**Interfaces:**
- Produces in `keys`:
  - `pub enum Key { Enter, Escape, Up, Down, Tab, Text(String) }`
  - `pub fn bytes(key: Key) -> Vec<u8>`: Enter is `b"\r"`, Escape `b"\x1b"`, Up `b"\x1b[A"`, Down `b"\x1b[B"`, Tab `b"\t"`, Text is the UTF-8 bytes with `\r` and `\n` removed.
- Produces in `picker`:
  - `pub struct Card { pub name: String, pub tagline: Option<String>, pub detail: Option<String>, pub note: Option<String>, pub section: Option<String>, pub current: bool }`
  - `pub struct Picker { pub cards: Vec<Card>, pub current: Option<usize>, pub section: Option<String> }`
  - `pub fn parse(tail: &[String]) -> Option<Picker>`: `None` when the tail holds no card, so a caller can never mistake a normal transcript for a picker.
  - `pub fn find(&self, name: &str) -> Option<usize>`: exact match on the trimmed `name` only.
- Produces in `session`: `pub fn settle(&mut self, quiet: Duration, limit: Duration) -> bool` and `pub fn settle_verdicts(last_output_elapsed: Duration, quiet: Duration, waited: Duration, limit: Duration) -> SettleOutcome` with `pub enum SettleOutcome { Quiet, TimedOut, Waiting }`, so the rule is testable without a pseudo terminal.

Rules `parse` pins (the spec's captured picker is the fixture). A tail can be cut at the top, so the rules never depend on a card's predecessor being present:

- A **detail line** is a line whose first `·`-separated segment matches `^\d+x$` (`1x`, `2x`, `3x`). It is attached to the line above it as `detail`, verbatim, including any flags after it (`2x · Recommended`).
- A **card** is a line that a detail line follows, or a line carrying a `›` or `×` marker, or the first non-blank line of the tail when it contains ` · `. Its `name` is the text up to the first ` · `, stripped of the leading marker and trimmed; its `tagline` is the next `·`-separated segment.
- A **section header** is a line whose first `·`-separated segment is all uppercase letters, digits, dashes or spaces (at least 3 characters) and whose next non-blank line is a card, not a detail line. `PREMIUM · 2–4x cost · lower usage (1–2 hours per day)` is a header; `GPT-6-Codex · Fast · Images` is a card because a detail line follows it.
- A line that is none of those, with a space and a lowercase letter in it, is a **note** for the card above it (`May use data for AI training`).
- `current` is the card that carried `›`. Several sightings of the same name keep the first, and the marked sighting wins.
- `parse` returns `None` unless it saw **at least two distinct card names**, the spec's structural check that the screen is the picker and not some other list. A single card alone is not a picker.

- [ ] **Step 1: Write the failing tests**

In `keys.rs`:

```rust
#[test]
fn every_key_is_the_sequence_a_terminal_sends() {
    assert_eq!(bytes(Key::Enter), b"\r".to_vec());
    assert_eq!(bytes(Key::Escape), b"\x1b".to_vec());
    assert_eq!(bytes(Key::Up), b"\x1b[A".to_vec());
    assert_eq!(bytes(Key::Down), b"\x1b[B".to_vec());
    assert_eq!(bytes(Key::Tab), b"\t".to_vec());
}

#[test]
fn typed_text_never_carries_an_enter() {
    assert_eq!(bytes(Key::Text("a\nb\rc".into())), b"abc".to_vec());
}
```

In `picker.rs`, using the spec's capture verbatim as `const CAPTURE: &[&str]`:

```rust
#[test]
fn the_design_capture_parses_into_four_cards_with_deepseek_marked_current() {
    let picker = parse(CAPTURE).expect("a picker");
    assert_eq!(picker.cards.len(), 4);
    let names: Vec<&str> = picker.cards.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(names, vec![
        "Solar Mini 4", "MiMo v2.6 Flash Slow",
        "DeepSeek V4.1 Flash Slow • high", "DeepSeek V4.1 Flash 10/17 • high",
    ]);
    assert_eq!(picker.current, Some(3));
    assert!(picker.cards[3].current);
}

#[test]
fn a_section_header_with_a_cost_range_is_not_a_card() {
    let picker = parse(CAPTURE).expect("a picker");
    assert!(picker.cards.iter().all(|c| c.name != "PREMIUM"));
    assert_eq!(picker.cards[3].section.as_deref(), Some("PREMIUM"));
}

#[test]
fn the_data_use_line_is_a_note_not_a_card() {
    let picker = parse(CAPTURE).expect("a picker");
    assert!(picker.cards.iter().all(|c| !c.name.starts_with("May use data")));
    // The spec's capture is a tail: the first card's detail line scrolled off,
    // so the card is still a card and the prose below it is its note.
    assert_eq!(picker.cards[0].detail, None);
    assert_eq!(picker.cards[0].note.as_deref(), Some("May use data for AI training"));
    assert_eq!(picker.cards[1].detail.as_deref(), Some("1x"));
}

#[test]
fn an_all_caps_model_name_is_a_card_not_a_header() {
    let lines: Vec<String> = [
        "GPT-6-Codex · Fast · Images", "2x · Recommended",
        "PREMIUM · 2–4x cost · lower usage",
        "DeepSeek V4.1 Flash · Smart · Images", "3x",
    ].iter().map(|s| s.to_string()).collect();
    let picker = parse(&lines).expect("a picker");
    assert_eq!(picker.cards.len(), 2, "an all-caps name with a cost line below it is a card");
    assert_eq!(picker.cards[0].name, "GPT-6-Codex");
    assert_eq!(picker.cards[0].section, None);
    assert_eq!(picker.cards[1].section.as_deref(), Some("PREMIUM"));
}

#[test]
fn a_line_without_a_detail_line_below_it_is_not_a_card() {
    let lines: Vec<String> = ["just some transcript text", "and more of it"].iter().map(|s| s.to_string()).collect();
    assert!(parse(&lines).is_none());
}

#[test]
fn one_card_alone_is_not_a_picker() {
    let lines: Vec<String> = ["› Only Model · Fast · Images", "2x"].iter().map(|s| s.to_string()).collect();
    assert!(parse(&lines).is_none(), "the spec asks for at least two distinct cards");
}

#[test]
fn an_unmatched_name_is_a_refusal_not_a_nearest_row() {
    let picker = parse(CAPTURE).expect("a picker");
    assert_eq!(picker.find("DeepSeek V4.1 Flash 10/17 • high"), Some(3));
    assert_eq!(picker.find("deepseek v4.1 flash 10/17 • high"), None);
    assert_eq!(picker.find("DeepSeek V4.1 Flash"), None);
}
```

In `session.rs`:

```rust
#[test]
fn settle_reports_quiet_once_the_tui_stopped_printing() {
    use SettleOutcome::*;
    assert_eq!(settle_verdicts(Duration::from_millis(400), Duration::from_millis(350), Duration::from_secs(1), Duration::from_secs(3)), Quiet);
}

#[test]
fn settle_gives_up_at_the_limit_instead_of_waiting_for_ever() {
    use SettleOutcome::*;
    assert_eq!(settle_verdicts(Duration::from_millis(50), Duration::from_millis(350), Duration::from_secs(3), Duration::from_secs(3)), TimedOut);
}

#[test]
fn settle_waits_while_the_tui_is_still_printing() {
    use SettleOutcome::*;
    assert_eq!(settle_verdicts(Duration::from_millis(50), Duration::from_millis(350), Duration::from_secs(1), Duration::from_secs(3)), Waiting);
}
```

- [ ] **Step 2: Run `cargo test`** Expected: FAIL, modules missing.

- [ ] **Step 3: Implement `keys`, `picker`, and `settle`**

`settle` loops in 25 ms steps: pump the PTY, ask `settle_verdicts`, return on `Quiet` or `TimedOut`. Callers ignore its return value on purpose: the parse that follows decides, and a timed-out settle means "read the screen and see", never "assume it is ready". `handle_start` and `interrupt` use `keys::bytes` instead of literal escapes.

- [ ] **Step 4: Run `cargo test`** Expected: all PASS.

- [ ] **Step 5: Commit**

```bash
git add driver/src/keys.rs driver/src/picker.rs driver/src/main.rs driver/src/session.rs
git commit -m "feat(driver): key vocabulary, settle wait, and a picker parser"
```

---

### Task 3: `--catalog` and `models.json`

**Files:**
- Create: `Projects/OpenRemote-plugins/freebuff/driver/src/catalog.rs`
- Modify: `Projects/OpenRemote-plugins/freebuff/driver/src/main.rs` (arg table, `USAGE` text, module list)
- Modify: `Projects/OpenRemote-plugins/freebuff/driver/src/probe.rs` (extract the preflight checks)
- Modify: `Projects/OpenRemote-plugins/freebuff/driver/src/session.rs` (`start` calls the extracted preflight)
- Modify: `Projects/OpenRemote-plugins/freebuff/.gitignore` (add `.catalog-cwd/`)

**Interfaces:**
- Consumes: `Screen`, `Key`, `picker::parse`, `picker::find`, `probe::freebuff_path`.
- Produces: `pub fn catalog::run() -> i32`, called by `main` for `--catalog`; always 0, one JSON line on stdout.
- Produces: `pub fn catalog::capture_is_a_picker(screen: &Screen) -> Option<picker::Picker>`, the one place `run` asks whether the picker is open, so the question is a single named call.
- Produces: `pub struct Catalog { pub captured_at: u64, pub default: Option<String>, pub models: Vec<ModelDescriptor> }` with `pub fn from_picker(picker: &picker::Picker, captured_at: u64) -> Catalog` and `pub fn to_line(&self) -> String`, where `ModelDescriptor` is a local struct serialised with the daemon's own field names: `model`, `display_name`, `detail`, `note`, `is_default`. The fields the daemon has and Freebuff does not (`resolved_model`, `reasoning_efforts`) are omitted: every one of them is `#[serde(default)]` on the daemon side, so the file parses as it stands.
- Produces in `probe.rs`: `pub struct Preflight { pub exe: PathBuf, pub version: String }` and `pub fn preflight() -> Result<Preflight, String>` returning the same three sentences `Session::start` prints today (CLI not found, too old, not signed in).

- [ ] **Step 1: Write the failing tests in `catalog.rs`**

```rust
#[test]
fn the_catalog_line_has_the_daemons_descriptor_shape() {
    let picker = picker::parse(CAPTURE).expect("a picker");
    let catalog = Catalog::from_picker(&picker, 1_762_000_000_000);
    assert_eq!(catalog.default.as_deref(), Some("DeepSeek V4.1 Flash 10/17 • high"));
    assert_eq!(catalog.models[3].detail.as_deref(), Some("3x · May use data for AI training"));
    assert_eq!(catalog.models[3].model, "DeepSeek V4.1 Flash 10/17 • high");
    assert!(catalog.models[3].is_default);
    let line = catalog.to_line();
    let value: serde_json::Value = serde_json::from_str(&line).unwrap();
    assert_eq!(value["captured_at"], 1_762_000_000_000u64);
    assert!(value["models"].as_array().unwrap().len() == 4);
}

#[test]
fn a_screen_that_is_not_a_picker_captures_nothing() {
    let screen = Screen::new(45, 140);
    assert!(capture_is_a_picker(&screen).is_none());
}
```

- [ ] **Step 2: Run `cargo test`** Expected: FAIL.

- [ ] **Step 3: Implement `catalog::run`**

Steps inside it, each one a call the plan pins:

1. `probe::preflight()?`, mapping an error to the one JSON line `{"models":[],"default":null,"captured_at":0,"reason":"<sentence>"}` and exit 0.
2. Create `.catalog-cwd` inside the process's working directory (the plugin folder) and spawn the TUI **there**, so the throwaway chat lands in one project folder whose name is stable. The driver process itself never changes directory.
3. Wait for paint and quiet using `Screen` and the same 800 ms / 8 s bounds `Session` uses.
4. Send `/model`, then `enter`; settle 350 ms / 3 s; parse the tail. If the tail is not a picker, send `escape`, retry once, then give up with the same JSON shape carrying `"reason"`.
5. Page: send `down`, settle, parse, merge cards by name; stop when the selection index has not moved for two presses, or after 240 presses. Record whether the marked row was seen at any point.
6. Send `escape`, settle, kill the PTY.
7. Write `models.json` into the driver's own working directory, which is the plugin folder the daemon reads from, with `catalog.to_line()`; best effort, and the daemon falls back to the manifest's list when it is missing.
8. Print `catalog.to_line()` and exit 0.

- [ ] **Step 4: Wire `--catalog` in `main.rs`**

`Some("--catalog") => catalog::run()` plus a `USAGE` line. The mode is documented in `PROTOCOL.md` in Task 8.

- [ ] **Step 5: Run it once against the real CLI**

```bash
cd Projects/OpenRemote-plugins/freebuff
./driver/target/debug/freebuff-driver --catalog
```

Expected: one JSON line with a non-empty `models` array, a `default`, and a `captured_at`. Save the printed line to `spike/catalog-0.2.35.json` and the picker's **card rows only** to `spike/screens/picker-0.2.35.txt` for Task 8, and commit them. Copy the rows below any header, never the whole frame: a Freebuff startup header can carry the signed-in address, and that must not enter a repository. If the picker never appears, the `reason` names the screen: capture the picker-area rows into `spike/screens/` and stop this task to report, do not loosen a matcher.

- [ ] **Step 6: Verify the gates and commit**

```bash
cd driver && cargo test && cargo clippy --all-targets -- -D warnings && cargo fmt --all --check
cd .. && git add driver/src/catalog.rs driver/src/probe.rs driver/src/session.rs driver/src/main.rs .gitignore spike
git commit -m "feat(driver): --catalog captures Freebuff's model picker into models.json"
```

---

### Task 4: Starting a chat on a chosen model

**Files:**
- Modify: `Projects/OpenRemote-plugins/freebuff/driver/src/session.rs` (`start` 315-403, `prompt` 405-423, `maybe_send_prompt` 449-479, `handle_start` 258-313)
- Modify: `Projects/OpenRemote-plugins/freebuff/driver/src/protocol.rs` (`Events::initialized` stays, add `Events::model_reported`)

**Interfaces:**
- Produces: `pub fn needs_switch(current: Option<&str>, wanted: &str) -> bool` (trimmed exact comparison; `None` means unknown and asks for a switch, so the picker is read and the truth is discovered).
- Produces: `pub fn reported_model(verified: Option<&str>, requested: &str) -> String`, what `initialized` names: the verified model when the picker was read, the requested one when it was never read. The console chip then always shows what the driver actually saw.
- Produces: `pub enum WalkVerdict { Continue, Found, GiveUp }` and `pub fn walk_verdict(current: Option<&str>, wanted: &str, presses: u32, picker_open: bool) -> WalkVerdict`, so the walk's stopping rule is testable without a pseudo terminal.
- Produces: `pub enum OpenVerdict { Ready, Retry, GiveUp }` and `pub fn opening_verdict(tail: Option<&[String]>, attempts: u32) -> OpenVerdict`: `Ready` when the tail parses as a picker, `Retry` when it does not and no retry was spent, `GiveUp` after that one retry. No keystroke is sent unless the verdict is `Ready`.
- Produces: `pub fn prompt_block(&self) -> Option<String>` on `Session`: `Some(sentence)` when a model switch failed and no prompt may be typed.
- Produces: `Session::select_model(&mut self, wanted: &str) -> Result<(), String>` used by Task 5 as well.
- Adds to `Session`: `model_error: Option<(String, Option<String>)>` (requested, actual when known) and `frames: screen::FrameLog`. Every key send in `select_model` pushes a frame; on any `Err`, `diag` quotes the last frame's label and tail, on stderr only.
- `initialized` names `reported_model(verified, requested)`, so the chat's chip always shows what the driver actually saw.

- [ ] **Step 1: Write the failing tests in `session.rs`**

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
    // The picker closed under us: no more presses, the screen is not the picker.
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

- [ ] **Step 2: Run `cargo test`** Expected: FAIL.

- [ ] **Step 3: Implement `select_model`**

1. Read the current model: send Text("/model") then Enter, settle; parse `tail(45)` through `opening_verdict`, which decides whether the keystrokes continue at all. If `GiveUp`, return the sentence `"Freebuff did not open its model picker"`.
2. If `needs_switch(current, wanted)` is false, send Escape, settle, and return `Ok(())`. Write `models.json` from this screen (Task 3's `Catalog::from_picker`, best effort, same working directory) because this is the free refresh D7 keeps.
3. Walk: up to 240 times, send Down, settle, parse, and ask `walk_verdict` after each press. Stop when it answers `Found`. If the picker vanishes mid-walk or the presses run out, return `"Freebuff does not offer a model called X"`.
4. Select: send Enter, settle. If picker rows are still visible, send Escape, settle, and return `"Freebuff did not close its picker"`.
5. Verify by reopening: `/model`, Enter, settle, parse; the marked card must equal `wanted`. Send Escape, settle. If it does not, return `"Freebuff kept Y after the switch to X"`. This is the only verification, and it is the same structural fact both times.
6. On every `Err`, leave the TUI on the picker-free screen (Escape already sent) and record the actual model when it was read.

- [ ] **Step 4: Wire it into `start` and `prompt`**

`start` runs `preflight`, spawns the PTY, then, when `model` is present, runs `select_model`; on `Err` it stores `model_error = Some((requested, actual))`, writes the screen and both names to stderr through `diag`, and continues to a normal `initialized` (never `fail_closed`) with `reported_model(verified, requested)`. `prompt` returns early and emits the turn-completed error with the block sentence instead of setting `pending_prompt`. On a successful `select_model`, `models.json` is written from the final picker read, so a resume with the chat's stored model re-applies it (D1) with no extra keystrokes when Freebuff already runs it.

- [ ] **Step 5: Run `cargo test`** Expected: PASS, including the six new tests.

- [ ] **Step 6: Manual check against the CLI**

```bash
cd Projects/OpenRemote-plugins/freebuff
./driver/target/debug/freebuff-driver   # then: {"type":"spawn","cwd":"C:\\Users\\moreweb","model":"<a name from models.json>"}
```

Expected on stderr: the version line, `started Freebuff`, and, when the model differs, a line naming the arrow walk and the verified model. `initialized` on stdout names the verified model. Then `{"type":"shutdown"}`.

- [ ] **Step 7: Commit**

```bash
git add driver/src/session.rs driver/src/protocol.rs
git commit -m "feat(driver): start a chat on the model the console picked, verified"
```

---

### Task 5: Live settings in the driver

**Files:**
- Modify: `Projects/OpenRemote-plugins/freebuff/driver/src/session.rs` (`handle_start` `Command::Settings` branch 279-307)
- Modify: `Projects/OpenRemote-plugins/freebuff/driver/src/protocol.rs` (`Events::model_reported`, `Events::settings_refused`, and their unit tests)

**Interfaces:**
- Produces: `Events::model_reported(&mut self, model: &str) -> bool` emitting `{"type":"model_reported","model":"..."}`, and `Events::settings_refused(&mut self, message: &str) -> bool` emitting `{"type":"settings_refused","message":"..."}`.
- Consumes: `Session::select_model` from Task 4.

- [ ] **Step 1: Write the failing tests**

In `protocol.rs`:

```rust
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
```

In `session.rs`:

```rust
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

- [ ] **Step 2: Run `cargo test`** Expected: FAIL.

- [ ] **Step 3: Implement `settings_refusal` and the live path**

`settings_refusal(&self, model, effort, fast, permission_mode) -> Option<String>` returns the first applicable sentence: a turn is in flight; Freebuff has no effort word; Freebuff has no fast mode; Freebuff has no permission mode. With nothing to refuse and a model present, `handle_start` runs `select_model` in place (the loop pumps the PTY through `settle`, so this is safe, and it takes seconds: say so in a `diag` line before it starts). Success emits `model_reported(verified)`; failure emits `model_reported(actual)` when the actual is known, then `settings_refused(sentence)`. Every outcome also goes to `diag`.

- [ ] **Step 4: Run `cargo test`** Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add driver/src/session.rs driver/src/protocol.rs
git commit -m "feat(driver): change the model in a running chat, or refuse in a sentence"
```

---

### Task 6: The daemon seam

**Files:**
- Modify: `Projects/OpenRemote/daemon/crates/openremote-harness/src/lib.rs` (`ModelDescriptor` 206-232)
- Modify: `Projects/OpenRemote/daemon/crates/openremote-harness/src/events.rs` (`DriverEvent`, after `ModelReported` 51-56)
- Modify: `Projects/OpenRemote/daemon/crates/openremote-daemon/src/plugin_driver.rs` (`apply_settings` 190-195, `parse_event` 278-389)
- Modify: `Projects/OpenRemote/daemon/crates/openremote-daemon/src/plugins.rs` (`HarnessManifest` 92-113, `HarnessSpec` 172-188, `installed_harnesses` 193-246, `validate_harness` 279-318)
- Modify: `Projects/OpenRemote/daemon/crates/openremote-daemon/src/registry.rs` (Harness build 507-532)
- Modify: `Projects/OpenRemote/daemon/crates/openremote-daemon/src/supervisor.rs` (`update_settings` 1616-1714, `pump_one` around 2560 and 2657)
- Modify: `Projects/OpenRemote/daemon/crates/openremote-core/src/model.rs` (`Harness` struct)
- Modify: `Projects/OpenRemote/daemon/crates/openremote-daemon/src/http.rs` (routes 190-260, `harness_models` 346, `PEER_ROUTES` 100-146)
- Test: `Projects/OpenRemote/daemon/crates/openremote-daemon/src/plugins.rs` (`#[cfg(test)]`), `plugin_driver.rs` tests, `supervisor.rs` tests, `http.rs` tests, and `daemon/crates/fixture-driver` (a `--catalog` answer)

**Interfaces:**
- `ModelDescriptor` gains `#[serde(default, skip_serializing_if = "Option::is_none")] pub detail: Option<String>` and `pub note: Option<String>`.
- `HarnessManifest` gains `#[serde(default)] pub models_file: Option<String>` and `#[serde(default)] pub settings: Option<String>`.
- `HarnessSpec` gains `pub models_captured_at: Option<u64>` and `pub settings_live: bool`.
- `openremote_core::Harness` gains `#[serde(default, skip_serializing_if = "Option::is_none")] pub models_captured_at: Option<u64>` and `#[serde(default)] pub settings_live: bool`.
- `DriverEvent` gains `SettingsRefused { message: String }`.
- `plugin_driver::parse_event` maps `"model_reported"` to `DriverEvent::ModelReported { model }` and `"settings_refused"` to `DriverEvent::SettingsRefused { message }`.
- `PluginDriver::apply_settings` writes `{"type":"settings","model":...,"effort":...,"fast":...}` and returns `Ok(())` when `spec.settings_live`; otherwise returns today's error verbatim, because `supervisor::update_settings` recognises the substring `when the chat starts`.
- New route: `POST /harnesses/{id}/catalog/refresh` returning `{"harness": <Harness>}` through `receipted_plain`; not in `PEER_ROUTES`.
- The console's chip needs no new code path for a change: `pump_one` already folds `DriverEvent::ModelReported` into the session and emits `SessionUpdated`, so the chip follows the verified model. `models_captured_at` and `settings_live` are the only two new facts the console reads, both off the `/harnesses` row.

- [ ] **Step 1: Write the failing tests**

`plugins.rs`:

```rust
#[test]
fn a_models_file_inside_the_plugin_folder_wins_over_the_manifest() { /* models.json with one model; assert spec.models names it and models_captured_at is Some */ }
#[test]
fn a_models_file_that_escapes_the_folder_is_ignored() { /* "../models.json" -> manifest models kept, captured_at None */ }
#[test]
fn a_malformed_models_file_falls_back_to_the_manifest() { /* "{not json" -> manifest models kept, captured_at None */ }
#[test]
fn an_unknown_settings_word_is_refused_at_install() { /* "settings":"sometimes" -> Invalid naming the accepted word */ }
```

`plugin_driver.rs`:

```rust
#[test]
fn a_reported_model_and_a_refused_setting_map_to_the_drivers_own_words() {
    assert!(matches!(parse_event(r#"{"type":"model_reported","model":"m"}"#), Ok(DriverEvent::ModelReported { model }) if model == "m"));
    assert!(matches!(parse_event(r#"{"type":"settings_refused","message":"no"}"#), Ok(DriverEvent::SettingsRefused { message }) if message == "no"));
}
```

`supervisor.rs`:

```rust
#[test]
fn a_live_model_change_is_refused_while_a_turn_works() { /* status Working, settings_live spec -> 409 with "wait for this turn to finish" and no driver write */ }
#[test]
fn a_refused_live_setting_lands_as_a_note_in_the_chat() { /* SettingsRefused through pump_one emits DaemonError */ }
```

`harness` crate, `lib.rs`:

```rust
#[test]
fn a_descriptor_with_a_detail_and_a_note_round_trips_and_the_old_shape_still_parses() { /* serde both ways */ }
```

`http.rs`:

```rust
#[test]
fn the_refresh_route_returns_the_harness_row_and_refuses_while_a_chat_runs() { /* fixture driver --catalog answers; second half spawns a chat and expects 409 */ }
```

- [ ] **Step 2: Run `cargo test --locked` in `Projects/OpenRemote/daemon`**
Expected: FAIL on the new tests only.

- [ ] **Step 3: Implement the fields and the mappings**

`plugins::read_catalog(dir: &Path, name: &str) -> Option<(u64, Vec<ModelDescriptor>)>` joins, canonicalises, refuses an escape with `None` (the same shape as `resolved_driver`), parses `{"captured_at","models":[..]}` and requires at least one model with a non-empty `model`. `installed_harnesses` uses it when the manifest names a file, and keeps `harness.models` otherwise. Empty means fall through to the manifest, never an empty list.

- [ ] **Step 4: Implement the live settings path and the route**

`update_settings` checks before writing, for a live harness, that the session is not `Starting | Working | Waiting`, returning `SupervisorError::Conflict("wait for this turn to finish, then change the model")`, because the driver cannot see the session's status. The route: refuse when `plugin_chats(&plugin_id)` is not empty (`StatusCode::CONFLICT`, `"a Freebuff chat is running; refresh when it stops"`), run `<driver> --catalog` with `current_dir(spec.dir)` under a 60 s deadline and `kill_on_drop`, treat a non-zero exit or a line with `reason` as `StatusCode::SERVICE_UNAVAILABLE` carrying that sentence, otherwise `reprobe_harnesses()` and answer with the harness row.

- [ ] **Step 5: Run the daemon gates**

```bash
cd Projects/OpenRemote/daemon && cargo test --locked && cargo clippy --all-targets --locked -- -D warnings && cargo fmt --all --check
```

Expected: green.

- [ ] **Step 6: Commit**

```bash
cd Projects/OpenRemote && git add daemon
git commit -m "feat(daemon): plugin model catalogs, live settings, and a catalog refresh route"
```

---

### Task 7: The console

**Files:**
- Modify: `Projects/OpenRemote/src/api/daemon.js` (after `models` 770)
- Modify: `Projects/OpenRemote/src/state/console.jsx` (`modelsFor` 858-879, add `refreshModels`)
- Modify: `Projects/OpenRemote/src/PickerMenu.jsx` (item rendering 226-252)
- Create: `Projects/OpenRemote/src/relative-time.js`
- Create: `Projects/OpenRemote/src/relative-time.test.js`
- Modify: `Projects/OpenRemote/src/NewChat.jsx` (`modelItems` 864-871, the model menu's footer, `chooseModelId` 891-899)
- Modify: `Projects/OpenRemote/src/ChatView.jsx` (`modelItems` 448-455)
- Modify: `Projects/OpenRemote/src/composer-menus.css` and `Projects/OpenRemote/src/newchat.css` (one rule each)
- Test: `Projects/OpenRemote/src/picker-detail.test.js` (follow `picker-controls.test.js`'s setup)

**Interfaces:**
- `api.catalogRefresh(harnessId)` posts `/harnesses/{id}/catalog/refresh` with `timeoutMs: 90_000` and returns `{ harness }`.
- `refreshModels(harnessId, deviceId = null)`: only for this console's own machine (a phone or another device's catalog is not refreshed from here), it calls `api.catalogRefresh`, then `refreshCapabilities()` (271-273) so the new `models_captured_at` lands, then deletes the `modelsByHarness` entry for that key and asks `modelsFor` again. It is shaped like `refreshEndpoints` (901-914) and returns the list.
- `relativeTime(at, now)` returns `"just now"`, `"3 minutes ago"`, `"2 hours ago"`, `"6 days ago"`, or `"on 2026-10-31"` beyond 30 days.
- `PickerMenu` renders `item.detail` as `<span className="nc-item-detail">` inside `nc-item-main` under the name, and `item.note` as the row's `title`.

- [ ] **Step 1: Write the failing tests**

`relative-time.test.js`:

```js
test('a capture from two days ago reads as two days ago', () => {
  assert.equal(relativeTime(Date.parse('2026-10-31T10:00:00Z'), Date.parse('2026-11-02T10:00:00Z')), '2 days ago')
})
test('a capture from last month names the day', () => {
  assert.equal(relativeTime(Date.parse('2026-09-30T10:00:00Z'), Date.parse('2026-11-02T10:00:00Z')), 'on 2026-09-30')
})
test('a missing capture is not a date', () => {
  assert.equal(relativeTime(0, Date.parse('2026-11-02T10:00:00Z')), 'not captured yet')
})
```

`picker-detail.test.js`:

```js
test('a model row shows its cost tier as a second line', () => { /* items [{id:'m', name:'DeepSeek V4.1 Flash 10/17 • high', detail:'3x · Smart & Fast'}] renders both strings */ })
test('the picker footer offers a refresh with the capture age', () => { /* footer prop renders 'Captured 2 days ago' and a Refresh button */ })
```

- [ ] **Step 2: Run `npm test` in `Projects/OpenRemote`** Expected: FAIL.

- [ ] **Step 3: Implement the four pieces**

`relative-time.js` and `refreshModels` as above. `NewChat.jsx` maps each catalog model to `{ id, name, resolved_model, detail: m.detail ?? null }`; its model menu footer shows `Captured ${relativeTime(harness.models_captured_at, Date.now())}` and a Refresh button that calls `refreshModels(currentHarness.id, target)`, replacing the list in place and showing the daemon's sentence on failure. `ChatView.jsx` maps the same two fields so a chat's picker reads the same way. `PickerMenu` renders the second line and the hover title.

- [ ] **Step 4: Run `npm test` and the ACL gate**

```bash
cd Projects/OpenRemote && npm test && node scripts/check-tab-acl.mjs
```

Expected: green, and `tab acl ok: 2 capability file(s)...` with the same command count as before, because no Tauri command is added.

- [ ] **Step 5: Look at it**

`npm run tauri:dev`, open New chat, pick Freebuff: the model rows carry the cost tier, the footer names the capture age, Refresh works with no chat running, and a chat's chip names the model the driver verified.

- [ ] **Step 6: Commit**

```bash
git add src
git commit -m "feat(console): Freebuff model rows, capture age, and refresh"
```

---

### Task 8: Ship the manifest, the protocol, and the docs

**Files:**
- Modify: `Projects/OpenRemote-plugins/freebuff/openremote.json`
- Modify: `Projects/OpenRemote-plugins/freebuff/PROTOCOL.md` (§2 modes, §3 commands, §4 events, §5 `spawn`/`settings`, §7 what it never reports)
- Modify: `Projects/OpenRemote-plugins/freebuff/README.md`
- Modify: `Projects/OpenRemote/docs/harness-plugins.md` (the manifest's `models_file` and `settings` keys)
- Binary: `Projects/OpenRemote-plugins/freebuff/bin/windows-x64/freebuff-driver.exe`
- Test: `Projects/OpenRemote-plugins/freebuff/driver` tests unchanged, plus `Projects/OpenRemote` `npm test`

**Interfaces:**
- `openremote.json`'s harness block gains `"models"` (the array from Task 3's printed line), `"default_model"` (its `default`), `"models_file": "models.json"`, `"settings": "live"`; `version` goes from `0.1.0` to `0.2.0`.

- [ ] **Step 1: Write the manifest from the captured catalog**

Paste `spike/catalog-0.2.35.json`'s models array and its `default` into `openremote.json`, add the two keys, bump the version. Run `python -c "import json;json.load(open('openremote.json'))"` or `node -e "JSON.parse(require('fs').readFileSync('openremote.json'))"` to prove it parses.

- [ ] **Step 2: Update `PROTOCOL.md`**

Add, in the existing tables and sections: `--catalog` as a third mode (one JSON line, exit 0 always, writes `models.json`); `model_reported` and `settings_refused` to §4; the live `settings` behaviour to §5, replacing the blanket refusal; the picker rules and the "no keystroke without a parsed screen" rule to §5 `spawn`; the model matching rule (exact, trimmed, never nearest) and the privacy rule (screen text on stderr only) to §7 and §8.

- [ ] **Step 3: Build and stage the binary**

```bash
cd Projects/OpenRemote-plugins/freebuff/driver
cargo build --release
cp target/release/freebuff-driver.exe ../bin/windows-x64/freebuff-driver.exe
../bin/windows-x64/freebuff-driver.exe --probe
```

Expected: `{"available":true,"reason":null,"signed_in":true,"version":"0.2.35"}`.

- [ ] **Step 4: Update the two READMEs**

The plugin README: what now works (real model list, start on a chosen model, change between turns), the fixable things a Freebuff update breaks, the throwaway chat a refresh leaves in `.catalog-cwd`'s project, the Windows-only note that is still true, and the fact that Freebuff remembers the last model chosen (D1). `docs/harness-plugins.md`: the two new manifest keys with their allowed values, one paragraph each.

- [ ] **Step 5: Run every gate**

```bash
cd Projects/OpenRemote-plugins/freebuff/driver && cargo test && cargo clippy --all-targets -- -D warnings && cargo fmt --all --check
cd ../../../OpenRemote/daemon && cargo test --locked && cargo clippy --all-targets --locked -- -D warnings && cargo fmt --all --check
cd .. && npm test && node scripts/check-tab-acl.mjs
```

Expected: all green.

- [ ] **Step 6: End to end in the dev app**

Install the plugin from its folder, enable it, New chat on Freebuff, pick a premium model, send one prompt, then change the model mid-chat and send another. Expected: the chip names the model the driver verified both times; with a chat running, Refresh says a chat is running; after stopping, Refresh updates the capture age. Record what happened in the commit message.

- [ ] **Step 7: Commit both repositories**

```bash
cd Projects/OpenRemote-plugins && git add freebuff && git commit -m "feat(freebuff): ship the model catalog and live model switching"
cd ../OpenRemote && git add docs && git commit -m "docs(plugins): the manifest's models_file and settings keys"
```

---

## What this plan deliberately does not do

- Anything in phases 2 to 5: the slash-command menu, LITE and DEFAULT, balance and spend approvals, history and follow-up suggestions.
- A restore of Freebuff's remembered model on shutdown (D1: resume re-applies it, and the 3 s shutdown deadline is not enough for a TUI drive).
- A macOS or Linux driver build.
- Any new dependency in the driver crate: the grid, the keys, and the parser are plain Rust.
