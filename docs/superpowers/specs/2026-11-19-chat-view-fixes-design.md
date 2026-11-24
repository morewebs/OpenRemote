# Chat view: command output, call pairing and step labels

2026-11-19. Found by running the first real Codex session through the console
(about 60 commands, two turns, two of them in parallel at times) and replaying
its events through the reducer and the renderer.

## What the transcript showed

- Every command was labelled `commandExecution`, followed by a block of its raw
  output that looked like something the assistant had said.
- In those blocks: asterisks gone (`100*r[...]` read `100r[...]`), `&quot;` and
  `&amp;&amp;` where `"` and `&&` belong, PowerShell `$_` typeset as math, a blank
  line after every line, words broken in the middle, and the output of two
  commands run together line by line.
- Non-ASCII text garbled (an apostrophe as three characters).
- The title cut inside a word, and a reply ending in raw `[label](<C:/...md>)`.

## Causes

1. **Command output was sent as the assistant's words.** The Codex driver turned
   every `item/commandExecution/outputDelta` into a `TextDelta` with a newline in
   front. The console folds those into a stream item. When the reply settles it
   removes only the last stream item, and the rest are retired into agent
   messages at the end of the turn, so raw stdout rendered as markdown. One cause
   for the eaten asterisks, the entities, the math, the doubled newlines, the
   mid-word breaks and the interleaving across parallel commands.
2. **Results paired by name.** `tool.result` completed the latest unanswered tool
   of the same name. Two calls of one tool in flight get the first result on the
   wrong card: the card for `cat A` showed the output of `cat B`. The wire events
   carried no call id, and the supervisor dropped the driver's.
3. **A protocol word as a label.** Codex's shell tool is named after the item
   type it comes from.
4. **The inline math rule** typeset any `$...$` with an `_`, `^`, `{`, `}` or `\`
   inside, which two shell variables on one line satisfy.
5. **`titleFrom`** cut at a fixed 44 characters.
6. **Links** were understood only when the address was `http(s)`. A path, which
   Codex links constantly, was left as raw markdown.
7. **Garbled characters** are made upstream of the app. Windows PowerShell 5.1
   reads a UTF-8 file without a byte order mark as ANSI, so a command that prints
   one hands over text already damaged. Output from `rg` or Python was fine.

## What changed

Daemon:

- The Codex driver no longer turns command output deltas into reply text. A
  command's whole output reaches its card with `item/completed`, as the tool
  result.
- `tool.started` and `tool.result` carry an optional `call_id` (the driver's
  `tool_use_id`; absent when a driver has none). Old events decode without it.

Console:

- A result goes to the unanswered tool with the same call id, and falls back to
  the old name-and-turn rule when there is no id or none matches, so older
  daemons and drivers without ids behave as before and no result is lost.
- A Codex command reads as what it ran: `Ran 3 commands` in the folded row,
  `Running <command>` while live, and `Ran <command>` when opened, with the
  shell wrapper (`powershell.exe -Command`, `bash -lc`, `cmd /c`) taken off. A
  wrapper that cannot be read with certainty is shown as it was.
- A command's output has its garbled characters repaired for display, only when
  the characters taken back to windows-1252 bytes are valid UTF-8. A note under
  the output says so and shows the text as received on a click.
- Inline math needs a non-space after the opening `$`, a non-space before the
  closing one, and no digit after it; `${` is shell.
- A title is cut where a word ends. A single long word is still cut where it
  stands.
- `[label](path)` and `[label](<path with spaces>)` read as the label with the
  path on hover. Only addresses with no URL scheme are read this way;
  `javascript:`, `data:` and `file:` stay as the text they were and never link.

## Conversation header (2026-11-24)

The old facts row showed up as a string of labels and controls when the whole
page was selected. The header now has a title, then a single line for the
folder, branch, App, and current state. Idle chats have no state badge. Split
panes already name the chat in their pane bar, so they only show the metadata
and actions. On narrow screens long names give up space before the state does.

Click the title to rename in the desktop shell. Copy as Markdown copies the
conversation instead of the page chrome. Details opens the same dialog as
`/status`, including the model, effort, permissions, and usage. Model, effort,
and permissions still live in the composer for changes.

Run controls sit where you send messages. During an active reply, Stop
interrupts it and holds any queued messages without ending the chat. A draft
can still be queued with the arrow next to Stop. End run is in the menu
beside Stop. Resume takes the place of Send only when the stopped or failed
chat has a conversation to resume. Resume queue stays with the queue.
The changed control eases in unless reduced motion is enabled in OpenRemote
or the operating system.

Sync to Cloud and Delete chat keep their confirmation visible if the request
fails. Only synced chats can be deleted; private chats can still be archived
from their existing controls.

## Left as it is

- Chats stored before this change still hold the old output stream events, so
  replaying them shows the old blocks. New sessions do not.
- A running command shows its output when it finishes, not as it is written. A
  `tool.output` event carrying the chunks to the card (never the reply text)
  would bring live output back.
- A path reference is not clickable. Opening a file from a remote client needs a
  read path through the daemon.
- The garbling itself stays upstream. Configure the Windows shell for UTF-8 (set
  `[Console]::OutputEncoding` and `$OutputEncoding`, and
  `$PSDefaultParameterValues['Get-Content:Encoding'] = 'utf8'` in the profile), or
  read files with `rg`.

## Tests

- `daemon`: `events::tests::a_tool_call_carries_its_id_so_a_parallel_result_finds_its_card`,
  `supervisor::tests::a_call_id_rides_the_wire_only_when_the_driver_has_one`, and
  the Codex approvals end-to-end test, which now sends an output delta and asserts
  that none of it reaches the reply stream and that the start and the result
  share an id.
- `src`: the reducer (pairing by id, in both orders, and the fallbacks; the
  title), `quiet-rail` (labels, live label, repair), `command-card` (the
  wrapper), `text-repair`, and `markdown` (shell versus math, path links).
