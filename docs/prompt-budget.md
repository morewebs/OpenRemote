# What OpenRemote adds to an agent's requests

OpenRemote never writes a system prompt. What it adds reaches the model in
two ways, and both are kept small because they are paid again and again:

1. **Tool definitions** from each chat's `openremote` MCP server
   (`daemon/crates/openremote-daemon/src/mcp_chat.rs`). Every tool listed is
   sent with every model request: every step of a turn, not just every
   message.
2. **The open-tabs note** added to a delivered message
   (`browser/mod.rs`, `tabs_note`). It stays in the conversation, so every
   note is paid for the rest of the chat.

Claude Code, Codex and plugin Apps whose manifest says `"mcp": true` get
both. OpenCode, Pi, Antigravity and Grok get neither.

## How each part is kept small

**Tools are listed as a chat comes to need them (Claude Code).** The server
is started with `--staged` for Claude Code only
(`supervisor/composer.rs`, `stages_tools`). It declares
`tools.listChanged`, sends a one-line `instructions` string naming its three
families of tools, and lists:

- `read_chat`, always. A mention arrives in the very message that needs the
  tool, so a list change sent then would race the model's first request.
- `browser_tabs` and `browser_tab_open`, always (`browser::tools::ENTRY`), so
  an agent finds the browser on its own. The rest of the browser tools once
  the chat has the browser in play: its agent called a real browser tool
  (kept by this device in the store's `browser_used` set, never in the
  chat's events, never cleared while the chat lives), or the user turned on
  Settings > Browser > Give every chat my tabs and browser tools
  (`agent_tabs`).
- `agents_harnesses` and `agents_spawn` whenever teams are on; `agents_list`,
  `read`, `wait`, `message` and `stop` once the chat has a worker.
- Plugin agent tools, as before.

The daemon answers what is open at `GET /sessions/{id}/tools/gates`. The
server reads it when it starts, when the client lists, and after each tool
call while a gate is still shut, and sends
`notifications/tools/list_changed` when one opened. Nothing runs on a
timer, so the Settings switch reaches a running chat at its next tool call
(the switch says so). What the server has seen open stays open, so a tool a
running chat has used is never taken away.

Why only Claude Code: its client re-lists on `list_changed` for a server
that declares `tools.listChanged` (checked in Claude Code 2.1.317). Codex
0.167.0 has that behind `mcp_tool_list_changed`, a feature "under
development" and off by default, and a plugin's App may ignore it. A tool
those Apps could never be told about would be a lost feature, so they are
listed everything from the start, at the shorter sizes below.

OpenRemote does not set `ENABLE_TOOL_SEARCH`. Claude Code already loads
tool definitions on demand on Anthropic's own API by default; setting the
variable would only change chats on a custom endpoint, Bedrock or Vertex,
where it can break them.

**Shorter definitions for every App.** The browser's 22 tools are 18:
`browser_tabs` takes `select` or `downloads` (was `browser_tab_select`,
`browser_downloads`), `browser_navigate` takes `url` or `direction` (was
`browser_go`), and `browser_logs {kind}` replaces `browser_console` and
`browser_network`. A call that names two actions at once is refused, never
guessed at. Only tools that read were merged with tools that read, so the
reading ones still run without asking. Calls by the old names still work
(`browser::tools::current_name`). The `tab`, `ref` and `element` parameter
descriptions, repeated across most tools, are a few words each, and the
team tools' descriptions keep their contract and drop the repetition.

**The tabs note is change-only.** A chat without the browser in play gets no
note. A chat with it gets every tab the first time (at most 5, then "and N
more (browser_tabs lists them)"), then only the tabs opened, moved to
another address (a new query counts), or closed since it was last told, and
nothing when nothing changed. A note counts as told only once the message
carrying it reached the harness: a failed send, a fresh conversation (a
move the conversation could not follow, `/clear`) or a deleted chat leaves
the chat told nothing, so its next note says it all. The note shows each
address without its query and fragment, cut to 80 characters, and each
title cut to 60, to keep it short. That is not a privacy boundary:
`browser_tabs` gives the agent the whole address. Private tabs the user has
not handed to agents never reach the daemon, so they are never in a note.

**Plugin tools are capped at install review.** A plugin offering more than
16 agent tools, a description over 1,024 characters or an input schema over
4,096 bytes is refused when the user reviews the install, with the reason
(`plugin_runtime/manifest.rs`, `tool_budget`). A plugin installed before
the caps keeps working; the daemon notes on stderr that it is over.

**Codex reads without asking.** The read-only OpenRemote tools that Claude
Code gets through `--allowedTools` reach Codex as
`mcp_servers.<server>.tools.<tool>.approval_mode = "approve"`, except
`agents_report`, which sends a note to the lead and so still asks.

## Measured cost per model request

Tokens are chars / 4 of the JSON the server sends: the `tools/list` result,
plus the `instructions` string where it is sent. The test
`mcp_chat::tests::what_a_chat_pays_per_request` prints them and fails if
they grow past its bounds:

```sh
cd daemon && cargo test -p openremote-daemon --lib what_a_chat_pays -- --nocapture
```

| Chat | Before (`cad3ff6`) | After, Claude Code (staged list) | After, Codex and plugin Apps |
|---|---|---|---|
| Plain chat (never browses, no workers) | 3,725 (30 tools) | 717 | 2,985 (26 tools) |
| Chat that browses | 3,725 | 2,489 | 2,985 |
| Team lead (has workers, no browser) | 3,725 | 1,260 | 2,985 |

These are what the server sends. Claude Code's own default deferral on
Anthropic's API may keep some of it out of a given request; OpenRemote does
not count on it.

The tabs note, with 5 typical tabs (a pull request, docs, a search, a spec
page, a dev server):

| | Before | After |
|---|---|---|
| Chat that never used the browser | 220 per message | 0 |
| First message once the browser is in play | 220 | 163 |
| Later message, tabs unchanged | 220 | 0 |
| Later message, one tab opened and one closed | 220 | 58 |
| 50 messages, tabs unchanged | about 11,000 kept in history | 163 |

Not changed here: the size of what a tool returns (a page snapshot up to
60,000 characters, a worker's report up to 40,000). Lowering those trades
tokens for extra calls and is left as a separate decision.
