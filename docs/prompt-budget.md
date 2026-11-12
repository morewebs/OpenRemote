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
`tools.listChanged` and lists:

- `read_chat`, always. A mention arrives in the very message that needs the
  tool, so a list change sent then would race the model's first request.
- `browser_tabs` and `browser_tab_open`, always (`browser::tools::ENTRY`), so
  an agent finds the browser on its own. The rest of the browser tools once
  the chat has the browser in play: its agent used any browser tool (kept on
  the session as `browser_used`, never cleared), or the user turned on
  Settings > Browser > Tell every chat about my tabs (`agent_tabs`).
- `agents_harnesses` and `agents_spawn` whenever teams are on; `agents_list`,
  `read`, `wait`, `message` and `stop` once the chat has a worker.
- Plugin agent tools, as before.

The daemon answers what is open at `GET /sessions/{id}/tools/gates`. The
server asks after each tool call, and every 5 seconds while a gate is shut
(for the Settings switch), and sends `notifications/tools/list_changed`
when one opened. What it has seen open stays open, even across a daemon
restart, so a tool a running chat has used is never taken away.

Why only Claude Code: its client re-lists on `list_changed` for a server
that declares `tools.listChanged` (checked in Claude Code 2.1.317). Codex
0.167.0 has that behind `mcp_tool_list_changed`, a feature "under
development" and off by default, and a plugin's App may ignore it. A tool
those Apps could never be told about would be a lost feature, so they are
listed everything from the start, at the shorter sizes below.

**Claude Code loads definitions on demand.** The Claude driver sets
`ENABLE_TOOL_SEARCH=true` when the chat is on Anthropic's own API and the
user has not set it (`openremote-claude/src/driver.rs`, `tool_search`). In
Claude Code 2.1.317 the variable exists and `true` selects its always-defer
mode; it is left alone for a custom endpoint, where Claude Code turns tool
search off itself. The staged server also sends a one-line `instructions`
string naming its three families of tools, so the model knows what to
search for.

**Shorter definitions for every App.** The browser's 22 tools are 18:
`browser_tabs` takes `select` and `downloads` (was `browser_tab_select`,
`browser_downloads`), `browser_navigate` takes `direction` (was
`browser_go`), and `browser_logs {kind}` replaces `browser_console` and
`browser_network`. Only tools that read were merged with tools that read, so
the reading ones still run without asking. Calls by the old names still work
(`browser::tools::current_name`). The `tab`, `ref` and `element` parameter
descriptions, repeated across most tools, are a few words each, and the
team tools' descriptions keep their contract and drop the repetition.

**The tabs note is change-only.** A chat without the browser in play gets no
note. A chat with it gets every tab the first time (at most 5, then "and N
more (browser_tabs lists them)"), then only the tabs opened, moved to
another address, or closed since it was last told, and nothing when nothing
changed. Addresses lose their query and fragment and are cut to 80
characters, titles to 60. Private tabs the user has not handed to agents
never reach the daemon, so they are never in a note.

**Plugin tools are capped.** A plugin offers at most 16 agent tools, each
description at most 1,024 characters and each input schema at most 4,096
(`plugin_runtime/manifest.rs`); a bigger one is refused at install review
with the reason.

**Codex reads without asking.** The read-only OpenRemote tools that Claude
Code gets through `--allowedTools` reach Codex as
`mcp_servers.<server>.tools.<tool>.approval_mode = "approve"`.

## Measured cost per model request

Tokens are chars / 4 of the JSON the server sends: the `tools/list` result,
plus the `instructions` string where it is sent. The test
`mcp_chat::tests::what_a_chat_pays_per_request` prints them and fails if
they grow past its bounds:

```sh
cd daemon && cargo test -p openremote-daemon --lib what_a_chat_pays -- --nocapture
```

| Chat | Before (`cad3ff6`) | After, Claude Code | After, Codex and plugin Apps |
|---|---|---|---|
| Plain chat (never browses, no workers) | 3,725 (30 tools) | 717 | 2,985 (26 tools) |
| Chat that browses | 3,725 | 2,489 | 2,985 |
| Team lead (has workers, no browser) | 3,725 | 1,260 | 2,985 |

On Anthropic's own API, Claude Code's tool search defers even these
definitions until the model looks one up, so what a Claude chat sends per
request there is lower still.

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
