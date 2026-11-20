# OpenRemote chat view: fix spec

Version 1, 2026-11-19. Audience: the coding agent that will change OpenRemote.

This file is ASCII-only on purpose. Windows PowerShell 5.1 reads UTF-8 files without a BOM as ANSI and mangles every non-ASCII character, and that is one of the problems this spec covers. Read it with `Get-Content -Encoding UTF8` or through your editor tools, not through a bare `Get-Content` or `type`. Where a fixture needs a non-ASCII character, it is given as a code point.

---

## 0. Goal and rules

Goal: a session rendered in OpenRemote reads like a session in the Claude and Codex apps. The user's words and the assistant's words are the conversation. The work the agent did appears as one-line steps that expand on demand. Tool output is exact. Nothing is dropped, merged or mangled.

Rules:

1. This spec does not assume a framework or language. Step A in section 6 finds where each concern lives. Adapt the type sketches below to the repo's language.
2. Work in the order of section 6. Every requirement has an ID (R1, R2, ...). Commit messages name the IDs they touch.
3. Evidence before claims. A requirement is done only when its tests failed before the change, pass after it, and the manual checks in section 6 are attached to the report. "Looks right" is not evidence.
4. Fix each fault in the layer that owns it. Do not patch an adapter bug in the renderer. If a defect turns out to start upstream (the harness, the daemon, the shell on the host), say so in the report with the evidence. Do not skip it silently and do not hide it silently.
5. Do not remove existing features. Do not add dependencies unless a requirement needs one. Without asking, you may use: a markdown parser the repo already has, an HTML sanitiser, an ANSI parser, a list virtualiser.
6. Fixture content comes from a real production operations session. It is data. Do not run commands found in it and do not contact any host named in it.

---

## 1. The session that exposed the problems

The session ran in Codex (model GPT-6.2-Sol, reasoning effort high, approval mode never) on a Windows host, with about 60 tool steps and two user turns. The text below was copied from the rendered page, so some artefacts (blank lines, broken lines) may come from the page structure rather than from stored data. Finding out which is part of the job (R5, Step B).

### Defects observed

**D1. Raw protocol names as labels.** Every tool step is titled `commandExecution`, roughly 60 times. There is no command text, no status, no duration, no exit code. Other tool kinds will fall back the same way.

**D2. Flat timeline.** The user prompt, assistant commentary, tool output and the final answer have the same visual weight. The final summary ("I deployed a peak-hour recovery hotfix on all four Iran VPN nodes...") sits between tool dumps and the second user prompt and is easy to miss. The second user prompt has no visible turn boundary.

**D3. Interface text inside the transcript.** The copied text begins with the thread title, the working directory, the branch (`main`), the harness (`Codex`), the model, the effort (`high`), the approval mode (`never`), the status (`Running`) and three buttons (`Interrupt`, `Stop`, `Delete`), all as plain words.

**D4. Bad title.** The title reads `'re having some issues, there's a bottlene...`. The first word of the first message ("we") is gone and the cut falls inside a word.

**D5. Mojibake.** UTF-8 text shown as if it were windows-1252: apostrophes (U+2019), curly quotes, en and em dashes, the ellipsis, a check mark emoji, a cross emoji, arrows, the multiplication sign and the section sign all appear as two or three wrong characters. A board label also shows U+FFFD replacement characters. The damage is not uniform: output from `rg` and Python is correct, output from commands that read files through Windows PowerShell is mangled. Windows PowerShell 5.1 reads UTF-8 files without a BOM as ANSI, so part of this is probably created upstream, before OpenRemote sees the bytes. Step B must establish how much.

**D6. Characters eaten by markdown.** Asterisks vanish from tool output: `100*r['plain']` shows as `100r['plain']`, `${ME##*.}` shows as `${ME##.}`, and `max(...)*math.ceil(...)*1.5` loses both multiplication signs. Tool output appears to pass through a markdown renderer, where a pair of `*` becomes emphasis.

**D7. Dollar signs typeset as math.** A PowerShell snippet containing `$_.IPAddress ... $_.AddressState` was rendered as italic math, one letter per line. Single-dollar math is switched on for tool output.

**D8. Double HTML escaping.** Script text shows `&quot;` and `&amp;&amp;` literally where `"` and `&&` belong.

**D9. Whitespace destroyed.** Python and shell code lost all leading indentation. File listings have a blank line after every line. Long lines are broken in the middle of words (for example "carr" / "y auth headers", "abso" / "lute.").

**D10. Outputs of different commands interleaved.** Lines of `GLOSSARY.md` alternate with lines of `est.ps1` inside one block, and the text of two `SKILL.md` files runs together. This looks like output deltas of commands that ran in parallel being appended to the same block, probably because deltas are attached to "the current item" instead of to the item id they name. Verify before fixing.

**D11. Silent truncation.** The text of a skill file jumps from the middle of one phase to another, and a table loses its header row, with no "lines omitted" marker anywhere.

**D12. File edits as loose text.** "Edited", the file name, "+14" and the minus count appear as separate lines, followed by an unstyled diff.

**D13. Link not rendered.** The final answer ends with the raw text `[Incident record and rollback](<C:/Users/.../472-peak-traffic-carrier-recovery-2026-11-19.md>)`. Two faults: the angle-bracket link destination is not parsed, and the target is a local Windows path that is useless from a phone or another computer.

**D14. Everything expanded.** Whole skill files, documentation indexes and file lists are printed in full inline. The session produced thousands of lines that all sit in the transcript.

**D15. Ambiguous controls.** `Interrupt` and `Stop` sit next to each other with no visible difference, and `Delete` is in the same row.

---

## 2. Target experience

Principles:

1. The conversation is what the user wrote and what the assistant said. Work is secondary and collapsed.
2. Every tool step says what it did in one line.
3. Output is verbatim. What the command printed is what the screen shows.
4. Nothing is silently dropped, merged or reordered. If something is shortened, the screen says so.
5. After the user's own message, the final answer is the most prominent thing in a turn.
6. All of it works the same at 390 px wide.

Rough layout (ASCII sketch, not a design):

```
+------------------------------------------------------------------------+
| VPN-Management  [main]     Codex  GPT-6.2-Sol  High  Never ask    ...  |
| Peak-hour traffic throttling                                           |
+------------------------------------------------------------------------+
|  [ you ]  we're having some issues, there's a bottleneck causing ...   |
|                                                                        |
|  > Worked for 6m 12s   (ran 34 commands, edited 1 file)                |
|                                                                        |
|  I deployed a peak-hour recovery hotfix on all four Iran VPN nodes     |
|  without restarting their services. After an outage, ...               |
|  [ifup  +14 -4]   [472-peak-traffic-carrier-recovery-....md]           |
|                                                                        |
|  [ you ]  create a doc explaining what you found out ...               |
|  * Working 0m 41s - Running: python build_doc.py                       |
+------------------------------------------------------------------------+
|  Message...                                                [ Stop ]    |
+------------------------------------------------------------------------+
```

---

## 3. Requirements

Priority: P0 = the text cannot be trusted until fixed. P1 = structure. P2 = chrome and controls. P3 = polish.

### P0: correctness

#### R1. Text decoding and mojibake (fixes D5)

- R1.1 Decode every byte stream and every file read as UTF-8 with a stateful (streaming) decoder, so a multi-byte character split across two chunks survives. Never use the platform code page and never Latin-1.
- R1.2 First establish where the bytes go wrong (Step B). If the raw session log is already mangled, the app cannot recover the original text. Then do R1.3 and report the upstream cause.
- R1.3 Display-time repair, for tool output only (never for user or assistant text). If a line or an output contains the typical signs of UTF-8 read as windows-1252, and re-encoding it as windows-1252 then decoding as UTF-8 succeeds, show the repaired text. Use a "sloppy" windows-1252 table in which the bytes 0x81, 0x8D, 0x8F, 0x90 and 0x9D map to the code points of the same value. This is needed because the second byte of the cross emoji and of the closing curly quote is 0x9D, which is invisible in the damaged text. Leave text that already decodes cleanly untouched. Prefer a maintained library for the stack (ftfy is the reference implementation in Python).
- R1.4 A repaired block carries a small "repaired" marker with a "show original" toggle. The behaviour sits behind a setting, "Repair garbled characters in output", on by default.
- R1.5 OpenRemote never creates U+FFFD itself. U+FFFD already present in the data stays as it is.

Done when: fixtures F1 pass, and a chunk-split test (one 3-byte character split across two chunks) yields the correct character.

Note for the report and for the repo's AGENTS.md or equivalent: on Windows hosts the agent's shell should be configured for UTF-8 (a PowerShell profile that sets `[Console]::OutputEncoding`, `$OutputEncoding` and `$PSDefaultParameterValues['Get-Content:Encoding'] = 'utf8'`), or the agent should read files with `rg` or Python. This reduces the damage at the source. Mention it; do not apply it to the user's machine.

#### R2. Verbatim rendering of tool output (fixes D6, D7, D8, D9)

- R2.1 Command output, diffs, file contents, JSON and every other tool result are rendered as preformatted text through text nodes. No innerHTML, no markdown, no math, no emoji shortcodes, no autolinking that changes the text. Escape once, at render time. Stored data holds the raw text, not escaped text.
- R2.2 Preserve whitespace exactly: leading spaces, tabs, and each blank line once. Use `white-space: pre` with horizontal scroll, or `pre-wrap` with `overflow-wrap: anywhere` behind a per-block "Wrap" toggle (default: wrap on phones, scroll on desktop). Never insert newline characters into long lines.
- R2.3 ANSI: turn SGR sequences (colour, bold, dim, underline) into styled spans. Strip every other escape sequence (cursor movement, OSC, bell). Treat a bare carriage return as "overwrite the current line", so a progress bar shows its last state and not 400 lines.
- R2.4 Font stack for output: `ui-monospace, "Cascadia Mono", Consolas, "SF Mono", Menlo, monospace`, 12 to 13 px, line height about 1.45.

Done when: fixtures F2 pass.

#### R3. Per-item streaming and ordering (fixes D10)

- R3.1 Every event carries a thread id, a turn id and an item id. An output delta is appended to the buffer of the item it names, never to "the last item". An event for an unknown item id creates that item.
- R3.2 When an item completes and the harness supplies the final aggregated output, replace the accumulated buffer with it. The final value is authoritative.
- R3.3 Items keep the order in which they started. Items that run at the same time render as separate cards, each with its own output.
- R3.4 Apply events idempotently (dedupe by sequence number, or by item id plus delta index). After a reconnect, resume from the last applied sequence number or reload a snapshot. No duplicated text and no missing text.
- R3.5 Every harness adapter (Claude Code, Codex, grok, pi, opencode, agy) meets R3 the same way.

Done when: fixture F4 passes for every adapter that can run tools in parallel, and a reconnect test (drop the socket in the middle of a stream, reconnect) ends with text identical to an uninterrupted run.

#### R4. Explicit truncation (fixes D11)

- R4.1 Any layer that shortens output (harness, daemon, relay, UI preview) records how much it removed: `{ omittedLines, omittedBytes, position }`. The UI shows a one-line marker at the exact position ("... 214 lines omitted"). If the host still holds the full text, the marker offers "Show full output", fetched on demand.
- R4.2 Never cut inside a UTF-8 character, and never cut a line without a marker.
- R4.3 If a harness shortens output and gives no counts, the marker reads "Output shortened by <harness>".

Done when: fixture F7 passes.

#### R5. Copy fidelity and clean selection (fixes D3, D9)

- R5.1 Selecting and copying a message or an output block gives exactly its text: no doubled blank lines, no inserted line breaks, no button labels, chips, timestamps or status words. Controls use `user-select: none` and sit outside the text blocks in the DOM.
- R5.2 Every code block and output block has a Copy button. It copies the raw text without wrap or truncation artefacts. If only part of the output is loaded, the button says "Copy loaded part" and the block says how much is loaded.
- R5.3 Select all in the thread, paste into a plain-text editor: only conversation content appears. No title, no chips, no status, no buttons.

Done when: fixture F6 passes and a 200-line output round-trips byte for byte through copy and paste.

### P1: structure

#### R6. Normalised item model (fixes D1)

Adapters translate each harness's events into one model. The renderer knows only this model. Adapt to the repo's language.

```ts
type Turn = {
  id: string; startedAt: number; endedAt?: number;
  status: 'running' | 'completed' | 'interrupted' | 'failed';
  items: Item[];
};

type OutputBuffer = {
  text: string;
  totalLines?: number;
  truncated?: { omittedLines?: number; omittedBytes?: number; at: 'head' | 'middle' | 'tail' };
  repaired?: boolean;
};

type Item =
  | { kind: 'user_message';      id: string; text: string; attachments?: Attachment[] }
  | { kind: 'assistant_message'; id: string; text: string; phase: 'commentary' | 'final'; streaming: boolean }
  | { kind: 'reasoning';         id: string; summary?: string }
  | { kind: 'command';           id: string; command: string; rawCommand?: string; cwd?: string;
      status: 'running' | 'ok' | 'failed' | 'declined' | 'interrupted' | 'awaiting_approval';
      exitCode?: number; durationMs?: number; output: OutputBuffer }
  | { kind: 'file_change';       id: string; status: string;
      files: { path: string; op: 'add' | 'edit' | 'delete' | 'rename'; added: number; removed: number; diff?: string }[] }
  | { kind: 'tool_call';         id: string; server?: string; tool: string; args?: unknown; result?: unknown; status: string }
  | { kind: 'web_search';        id: string; query: string; status: string }
  | { kind: 'plan';              id: string; steps: { text: string; status: 'todo' | 'doing' | 'done' }[] }
  | { kind: 'error';             id: string; message: string; detail?: string }
  | { kind: 'unknown';           id: string; rawType: string; raw: unknown };
```

- R6.1 No protocol string (`commandExecution`, `fileChange`, `agentMessage`, `mcpToolCall`, or any equivalent from another harness) may appear in the UI as a label.
- R6.2 An unknown item type renders a generic collapsed card titled with the tool name if there is one, otherwise "Tool step". The raw JSON sits behind "Details".
- R6.3 Threads stored before this change still render. Convert at read time. Do not require migrating stored data.
- R6.4 Collapsed labels:
  - command: `Ran <first line of the command>`
  - file_change: `Edited <file name> +A -R` (also `Created`, `Deleted`, `Renamed`; several files: `Edited 3 files +40 -12`)
  - tool_call: `<server>: <tool>` plus a short argument summary
  - web_search: `Searched the web: <query>`
  - reasoning: `Thought`, with the summary text inside if the harness provides one
  - plan: a checklist with a state per step

Done when: a unit test per adapter maps a recorded log to the model, and a DOM test finds none of the protocol strings in the rendered thread.

#### R7. Command card (fixes D1, D2)

- R7.1 Collapsed row: a status glyph (spinner while running, check mark on success), the command's first line in monospace on one line with an ellipsis, and the duration at the right. A non-zero exit code shows as a muted "exit N", not as an error colour, unless the harness marks the step failed or declined (many commands return 1 on purpose).
- R7.2 If the harness wraps the command (`powershell.exe -Command "..."`, `bash -lc '...'`), show the inner command. Keep the full wrapper under "Raw command" in the expanded view.
- R7.3 Expanded view: the full command with a Copy button, the cwd when it differs from the thread's, the output (R9), and a footer "exit 0 - 3.2 s".
- R7.4 A running command is open and streams its last 8 lines. When it finishes it collapses, unless it failed or was declined.
- R7.5 A step awaiting approval stays open, shows the command and the reason if the harness gave one, and offers Approve and Decline. It does not collapse.

#### R8. Turn layout and the final answer (fixes D2)

- R8.1 The user message has its own filled block, left-aligned inside a centred column (max width about 760 px on desktop). Assistant text has no bubble.
- R8.2 While a turn runs, items appear in order. Consecutive non-message items form a group with a header such as "Ran 5 commands, edited 2 files". Commentary messages stay visible between groups as normal assistant text.
- R8.3 When a turn ends, everything between the user message and the final assistant message folds into one row, "Worked for 6m 12s", which expands to the full timeline with its groups. The final message stays outside the fold. The final message is the last assistant message marked final, or, if the harness does not mark phases, the last assistant message of the turn.
- R8.4 If a turn was interrupted or failed with no final message, show the last assistant message followed by a status line ("Stopped by you", "Failed: <reason>").
- R8.5 While a turn runs, a "Working 2m 14s - <current step label>" indicator stays at the bottom of the message list.
- R8.6 Turn boundaries are spacing, not rules. Timestamps appear on hover or focus. Hover or focus on a message shows Copy (and Retry on the last user message).

#### R9. Large output (fixes D14)

- R9.1 A finished command shows no output when collapsed. Expanded, it shows the first 10 lines, then "Show N more lines". Fully expanded, the block scrolls inside a max height of about 360 px and loads in pages of 300 lines.
- R9.2 "Open full output" opens a panel or full-screen view with a virtualised list of lines and in-view search.
- R9.3 Remote clients do not receive more than the preview by default. For output over 256 KB the daemon sends the first 100 lines, the last 100 lines and the totals, and serves the rest on request (R4.1).
- R9.4 No more than 2,000 output lines are ever mounted in the DOM at once.

Done when: fixture F8 passes.

#### R10. Assistant text, links and file paths (fixes D13)

- R10.1 Render CommonMark plus GFM (tables, task lists, strikethrough, fenced code with a language label), through a sanitiser. Raw HTML in markdown is shown as text, not executed. Math is off. If the repo wants math, it may use `$$ ... $$` blocks only, never single `$`.
- R10.2 Code spans and fenced blocks never lose characters (`*`, `_`, `$`, `\`, `&`).
- R10.3 Parse link destinations in angle brackets, `[text](<C:/path with spaces/file.md>)`, as CommonMark specifies.
- R10.4 A link whose target is a local path (a Windows drive path with `/` or `\`, a UNC path, an absolute POSIX path, `file://`, or a relative path inside the thread's cwd) renders as a file chip: file icon, base name, full path in the tooltip. Click behaviour: on the host's own desktop, open the file in the OpenRemote file viewer or the OS default app. On a remote device, ask the host daemon for a read-only preview, restricted to the workspace root and any other allowed roots. If that fails or the path is outside the roots, show a disabled chip labelled "on host". Never navigate a remote browser to `file://`.
- R10.5 `http` and `https` links open outside the app with `rel="noopener noreferrer"`. Never emit `javascript:` or `data:` links.
- R10.6 Streaming: an unclosed fence renders as a code block while it streams. Do not show stray markup markers or flicker between states.

Done when: fixture F5 passes.

#### R11. File change card (fixes D12)

- R11.1 Collapsed: `Edited ifup +14 -4`, with the full path in the tooltip. The minus sign in the original is U+2212; either that or ASCII `-` is fine.
- R11.2 Expanded: a unified diff per file with a line-number gutter, a `+`/`-` marker column (colour must not be the only cue), green and red line backgrounds, dimmed hunk headers, monospace, whitespace preserved, horizontal scroll.
- R11.3 Hunks over 200 lines are collapsed with "Show N more lines". A binary file shows "Binary file changed". Git messages such as "LF will be replaced by CRLF" belong to command output, not to the diff.

### P2: header, status, controls

#### R12. Thread title (fixes D4)

- R12.1 Use the harness's thread name when it provides one. Otherwise derive the title from the first user message: take the first non-empty line, strip markdown and surrounding whitespace, collapse runs of whitespace, capitalise the first letter, and cut at 60 characters on a word boundary with a single ellipsis.
- R12.2 Never drop leading characters and never cut inside a word.
- R12.3 The title can be renamed inline. A renamed title is never overwritten by the derived one.

Done when: fixture F3 passes.

#### R13. Header chips (fixes D3)

- R13.1 The header shows: project name (the cwd's base name, full path in the tooltip), a branch chip, the harness name and icon, the model, the reasoning effort, and the approval mode.
- R13.2 Chips have words, not bare values. Effort `high` reads "High" with the tooltip "Reasoning effort: high". Approval mode `never` reads "Never ask" with the tooltip "Approval policy: never ask before running commands". Map the other values the harness supports in the same way (for Codex: untrusted, on-failure, on-request, never), and show the sandbox mode if the harness reports one.
- R13.3 A chip that can be changed in a running thread opens a picker. Others are read-only. No chip text ever appears inside the transcript body.

#### R14. Status and controls (fixes D15)

- R14.1 Thread states: idle, running, waiting for approval, error, disconnected. Each has a visible indicator and an accessible name. Running shows the elapsed time.
- R14.2 One primary button in the composer: Send when idle, Stop when running. Stop interrupts the current turn without a confirmation dialog.
- R14.3 If the harness distinguishes "interrupt the turn" from "kill the session or process", offer the second one as "Stop session" in the overflow menu, with a one-line explanation. Do not show two stop-like buttons side by side.
- R14.4 Delete lives only in the thread's overflow menu, styled as destructive, with a confirmation dialog that names the thread.
- R14.5 Esc in the composer interrupts a running turn. Messages typed while a turn runs are queued and shown as queued.
- R14.6 When the connection to the host drops, show "Reconnecting to <host>..." in a banner. The transcript stays visible and read-only. On reconnect, R3.4 applies.

#### R15. Scroll and performance

- R15.1 Stick to the bottom only while the user is within about 80 px of it. Otherwise show a "Jump to latest" button with a new-activity dot.
- R15.2 Expanding or collapsing something above the viewport must not move what the user is reading (scroll anchoring).
- R15.3 Batch streaming updates to animation frames and coalesce deltas.
- R15.4 A thread with 1,500 items that includes one 20,000-line output opens in under 1 second, and scrolling produces no long task over 100 ms (measure with a `PerformanceObserver` for `longtask`). Virtualise the list, and do not mount the bodies of collapsed groups.

### P3: polish

#### R16. Copy and export the conversation

- R16.1 The thread menu has "Copy as Markdown" and "Export .md".
- R16.2 Format: a title line, then user and assistant messages in full. Each tool step is one line, `> ran: <command> (exit 0, 3.2 s)`, with no output by default. A toggle "Include tool output" adds each output, shortened to its first and last 20 lines with the R4 marker.
- R16.3 The export contains no UI chrome and none of the protocol strings from R6.1.
- R16.4 The export is UTF-8 with a BOM so Windows tools read it correctly.

#### R17. Errors and resilience

- R17.1 A harness error (rate limit, authentication, lost connection) is an inline error item with a plain message, "Retry" where it makes sense, and "Copy details". Never show raw JSON as the message.
- R17.2 Each item renders inside its own error boundary. If one item throws, the rest of the thread still renders and that item shows "Could not display this step" with "Copy raw".

#### R18. Phone layout (390 px)

- R18.1 No horizontal page scroll. Code and output scroll inside their own blocks, or wrap if Wrap is on.
- R18.2 Tap targets are at least 44 px. Header chips wrap to a horizontally scrollable row.
- R18.3 The composer stays above the on-screen keyboard and respects the safe-area insets.

#### R19. Accessibility

- R19.1 Groups and cards expand with the keyboard (Enter, Space) and show a visible focus ring.
- R19.2 The streaming final answer is announced politely (`aria-live="polite"`). Tool output is not announced.
- R19.3 Respect `prefers-reduced-motion`. Light and dark themes both pass contrast checks (WCAG AA for text).

---

## 4. Fixtures and tests

Generate mojibake fixtures in code. Do not paste them: pasted mojibake changes shape when it passes through a clipboard, a console or an editor. Method: start from the expected string, encode it as UTF-8, decode the bytes as windows-1252 using the sloppy table from R1.3. Keep the generator next to the tests.

Everything below is redacted or generic. Store fixtures under the repo's test fixtures folder. Never commit auth files, tokens, real hostnames, real IP addresses or real user names. Replace them in recorded logs before committing.

### F1. Mojibake (R1)

Expected strings, each to be produced by the generator and repaired back:

- `don` + U+2019 + `t` (right single quote)
- U+201C + `Clear header paragraph` + U+201D (curly double quotes)
- `rate-limited ` + U+2014 + ` never` (em dash)
- `5` + U+2013 + `25` + U+00D7 + ` for one node` (en dash, multiplication sign)
- `sync users, ` + U+2026 (ellipsis)
- U+2705 + ` Run test command`, U+274C + ` Should pass now` (check mark, cross)
- `Write ` + U+2192 + ` Run` (arrow)
- `doc 367 ` + U+00A7 + `5` (section sign)
- Negative case: text that is already correct and contains U+2019 must come out unchanged.
- Negative case: a string with U+FFFD in it is left as it is.
- Chunk split: a 3-byte character (U+2019) split after its first byte across two chunks decodes to one U+2019.

### F2. Verbatim output (R2)

Each of these, shown as the output of a command, must display character for character, with the indentation kept:

```
NET="${ME%.*}.$(( ${ME##*.} / 4 * 4 ))"
```

```
f"(plain {100*r['plain']/max(r['sent'],1):.0f}%"
```

```
Where-Object { $_.IPAddress -notlike '169.254.*' -and $_.AddressState -eq 'Preferred' }
```

```
[ -n "$DRAIN" ] && echo "add element ip qpdn_carrier drain { $DRAIN }"
```

```
for k in keys:
    out = []
    for d in days:
        v = rows.get((d, k))
```

Assertions: the DOM text of the block equals the input string exactly. No `&quot;` and no `&amp;` appear. No element with math classes appears. The four leading spaces on the nested lines are present.

### F3. Title (R12)

Input (first user message):

```
we're having some issues, there's a bottleneck causing throttle of users' traffic, and i did see some dips too

i want you to apply a hotfix so peak hours go on smoothly
```

Expected title: `We're having some issues, there's a bottleneck causing` followed by one ellipsis character (U+2026). Also test: a message that starts with markdown (`## Fix the login`) gives `Fix the login`; a message of 200 characters with no spaces is cut at 60 characters plus the ellipsis.

### F4. Parallel items (R3)

Replay this synthetic log (map the event names to the harness's real ones):

```
item.started   id=A  command="Get-Content GLOSSARY.md"
item.started   id=B  command="Get-Content est.ps1"
output.delta   id=A  "Locations\n"
output.delta   id=B  "<#\n"
output.delta   id=A  "Location:\n"
output.delta   id=B  "est -- estate toolbox launcher\n"
item.completed id=A
item.completed id=B
```

Expected: A's output is `Locations\nLocation:\n`. B's output is `<#\nest -- estate toolbox launcher\n`. Two separate cards. Then replay the same log with every delta delivered twice and with the sequence shuffled within a 3-event window: the result is the same.

### F5. Links (R10)

Input: `[Incident record and rollback](<C:/Users/example/Documents/Workspaces/Example/Documentation/472-peak-traffic-carrier-recovery-2026-11-19.md>)`.
Expected: no literal `[`, `](` or `<C:` in the DOM text. A file chip labelled `472-peak-traffic-carrier-recovery-2026-11-19.md` with the full path in its tooltip. On a simulated remote client with the file inside the allowed roots: a preview opens. Outside the roots: a disabled "on host" chip. `javascript:alert(1)` as a link target renders as plain text.

### F6. Clean text (R5, R6, R16)

After replaying the redacted fixture session, in the rendered thread element and in the Markdown export, none of these strings appear: `commandExecution`, `fileChange`, `agentMessage`, `Interrupt`, `Stop`, `Delete`, `Running`. The title, model name, effort and approval mode do not appear in the select-all text of the message list.

### F7. Truncation (R4)

A 5,000-line output shortened to head 100 and tail 100 shows a marker `... 4800 lines omitted` between them. A harness that truncates without counts shows `Output shortened by <harness>`. No truncated output ends in the middle of a UTF-8 character.

### F8. Large output (R9, R15)

A command with a 20,000-line output: collapsed it mounts zero output lines. Expanded it mounts at most 10 (preview), then at most 300 per page, and never more than 2,000. A remote client receives at most head 100 plus tail 100 plus totals until it asks for more.

---

## 5. Constraints

- Do not change the wire protocol between the app, daemon and cloud relay without a version field and backward compatibility. Old clients must not break on new events and new clients must not break on old daemons.
- Per-harness differences live in the adapters and nowhere else.
- Test at least the Codex adapter and the Claude Code adapter against recorded logs. For other adapters, add tests if a recorded log is available, and list in the report which ones were not covered.
- Do not change what any agent does. This spec is about how its work is shown.

### Privacy

Tool output is the most sensitive content in the app. The session behind this spec contains hostnames, IP addresses and incident details of a production system. While doing R9.3, check what the relay and the cloud service store or log of transcripts, and say in the report whether tool output is persisted or transmitted in the clear beyond what is needed. Do not change that behaviour without being asked. Report it.

---

## 6. Order of work and verification

**Step A. Discovery (no code changes).** Answer in the report, with file paths:

1. Where do events from each harness enter the app, and what is the adapter for each?
2. Where are those events turned into messages or items, and how is that state stored and sent between daemon, relay and app?
3. Where is the transcript rendered, and which component renders tool output?
4. Where are bytes decoded to text, on the host and in the app?
5. How is markdown configured (math, raw HTML, line breaks, escaping)?
6. Where is the thread title produced?
7. How are deltas, sequence numbers and reconnects handled today?
8. Which tests exist and how are they run?

**Step B. Find the layer of each defect.** Take the raw record of the session. Codex keeps session logs as JSONL files in the `sessions` folder of its home directory; Claude Code keeps them as JSONL files under `~/.claude/projects/`. Locate the file by the text of the first user message. Redact it (section 4). Replay it through each stage: raw log, adapter output, stored item, rendered DOM. For each of D1 to D15 record the stage where it first appears. This table decides where each fix goes. For D5 state whether the raw bytes are already mangled.

**Step C. Red tests.** Write the tests for F1 to F8 first. Show them failing on the current code.

**Step D. Implement.** Order: R1 to R5 (P0), then R6 to R11 (P1), then R12 to R15 (P2), then R16 to R19 (P3). One commit per requirement ID or small group of IDs, message starting with the IDs. After P0 and P1, post an interim report, then continue with P2 and P3 without waiting.

**Step E. Verify and report.** Attach, for each claim, the command and its output, not a summary:

- the full automated test run, all green, with the F1 to F8 tests named;
- screenshots before and after of the same redacted fixture session at 1280 px and at 390 px width (the first screen of a turn, an expanded command, an expanded diff, the final answer with its file chip);
- the R15.4 measurement: open time and the longest task during a scripted scroll;
- a grep of the rendered text for the strings in F6;
- the console log of a full replay with no errors or warnings.

Report format. Fill one row per requirement:

```
ID     status (done / partial / blocked)   evidence (test name, screenshot, measurement)   notes
R1.1   ...
R1.2   ...
```

Add three short lists at the end: defects whose cause was upstream of OpenRemote (with evidence), requirements not done and why, and anything you changed that this spec did not ask for.
