# The OpenRemote plugin protocol

How OpenRemote talks to a plugin's code. Two runtimes run in the daemon
and share one tool contract:

- **Process plugins** (`"runtime": "process"`): a program in the plugin's
  folder that OpenRemote starts and talks to over its stdin and stdout. It
  can give agents tools and answer its own views. It runs as the user, not
  sandboxed.
- **WASM plugins** (`"runtime": "wasm"`): agent tools compiled to
  WebAssembly, run inside the daemon, sandboxed.

A harness is not part of this protocol. A plugin adds a harness through
the `harness` block of `openremote.json`, and its driver speaks the JSON
lines contract in [../harness-plugins.md](../harness-plugins.md): one
driver process per chat, started by the daemon. One plugin may carry
both: a `harness` block for its driver and an `entry` for its tools.
JavaScript plugins run in the console instead; [js-api.md](js-api.md) is
theirs.

A plugin's manifest (`openremote.json`, described in
[authoring.md](authoring.md)) declares what it contributes. OpenRemote
reads it when the plugin is staged, shows it to the user for approval,
and keeps what the user approved in the plugin's install record. From
then on the record is what counts: the agent tools a chat lists come from
it, so listing them never starts a program, and the powers every call is
checked against are the record's. Editing `openremote.json` after install
grants nothing; a change waits for a new install and review.

This is protocol version **1**.

## Process plugins

### Framing

- JSON-RPC 2.0 messages, one per line: UTF-8 JSON, ended by `\n`. No
  `Content-Length` headers.
- stdout carries protocol messages and nothing else. A line that isn't JSON
  is dropped with a note on OpenRemote's stderr.
- stderr is the plugin's log: each line appears on OpenRemote's stderr as
  `plugin <id>: <line>`, cut at 2000 characters.
- A line may be up to 16 MiB (a tool result can carry pictures).
- Both sides send requests (with an `id`), answers (with the same `id`
  and `result` or `error`) and notifications (no `id`, no answer).
- **When stdin closes, OpenRemote is gone: exit.** This is what keeps a
  plugin from outliving the daemon if the daemon is killed.

### The program

The program is the manifest's `entry`: a file inside the plugin's own
folder (a per-system override may name another file there for Windows,
macOS or Linux). It is checked when the plugin is staged and again before
every start, after every link is followed, so it can't point outside the
folder. A program found on PATH, such as `node` or `python`, is refused:
ship a compiled program, or a launcher inside the folder, for each system
you support.

### Lifecycle

- One program per plugin per machine, started the first time something
  needs it: an agent's tool call, or one of its views asking it something.
  Nothing starts at install or at boot.
- It is started only while the plugin is on, its install record holds
  `run_programs` (a process plugin always declares it), and every setting
  marked `required` is set. Otherwise the call fails and says which
  settings to set, under Settings > Plugins.
- It runs in the plugin's folder. Its environment is **not** the daemon's:
  it is cleared, then given only these variables, when the daemon has
  them (names matched without regard to case):
  - `PATH`, `HOME`, `USER`, `LOGNAME`, `SHELL`, `TMPDIR`, `LANG`,
    `LC_ALL`, `LC_CTYPE`, `TZ`, `XDG_RUNTIME_DIR`;
  - on Windows, `USERPROFILE`, `USERNAME`, `HOMEDRIVE`, `HOMEPATH`,
    `SystemRoot`, `SYSTEMDRIVE`, `windir`, `ComSpec`, `PATHEXT`, `TEMP`,
    `TMP`, `APPDATA`, `LOCALAPPDATA`, `ProgramData`, `ProgramFiles`,
    `ProgramFiles(x86)`, `ProgramW6432`, `CommonProgramFiles`,
    `CommonProgramFiles(x86)`, `NUMBER_OF_PROCESSORS`,
    `PROCESSOR_ARCHITECTURE`, `OS`;

  and OpenRemote's own:
  - `OPENREMOTE_PLUGIN_ID`: its id, `name@marketplace` (with
    `~<project>` for a project install);
  - `OPENREMOTE_PLUGIN_ROOT`: the folder it runs from;
  - `OPENREMOTE_PLUGIN_DATA`: a folder of its own for what it keeps,
    private to the user.

  A proxy, a certificate setting or any other variable a plugin needs is
  not passed: make it a setting.
- Its settings, secrets included, reach it in `initialize` and in
  `settings/changed`, never on the command line or in the environment.
  OpenRemote never logs them.
- OpenRemote stops the program when the plugin is turned off, updated or
  removed, and when the daemon exits. The next need starts it again.
- A program that stops by itself is started again at the next need, after
  a pause: 1 s, then 2 s, 4 s and so on up to a minute while it keeps
  stopping within a minute of starting. Calls during the pause fail with a
  message that says when it starts again.
- A plugin installed for one project in its repo runs from a private
  snapshot of the files the user reviewed, not from the repo. If the
  repo's copy changes, a running program is stopped and the plugin stays
  off for that project until it is reviewed again.

### `initialize` (OpenRemote to plugin)

The first call. Answer within 20 seconds.

```json
{"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {
  "protocol": 1,
  "openremote": "0.4.0",
  "plugin": {
    "id": "toy-harness@local",
    "name": "toy-harness",
    "version": "0.2.0",
    "root": "/home/me/.openremote/plugins/local/toy-harness",
    "data": "/home/me/.openremote/plugin-data/toy-harness@local",
    "scope": "machine"
  },
  "powers": ["agent_tools", "run_programs"],
  "settings": {"greeting": "Hello", "token": "..."},
  "platform": "linux"
}}
```

- `scope` is `global`, `machine`, `project_local` or `project`.
- `powers` are the ones in the install record: what the user approved.
- `settings` holds every declared setting that has a value (set by the
  user, else its default). Secrets are included.
- `platform` is `windows`, `macos` or `linux`.

Answer with the protocol version you speak. Anything else is ignored for
now:

```json
{"jsonrpc": "2.0", "id": 1, "result": {"protocol": 1}}
```

An answer naming another version, or none, stops the program with an
error the user sees.

### `settings/changed` (OpenRemote to plugin, notification)

The user saved new values. Same shape as `initialize`'s `settings`:

```json
{"jsonrpc": "2.0", "method": "settings/changed", "params": {"settings": {"greeting": "Hi"}}}
```

### `log` (plugin to OpenRemote, notification)

```json
{"jsonrpc": "2.0", "method": "log", "params": {"level": "info", "message": "ready"}}
```

The line lands on OpenRemote's stderr as `plugin <id> [<level>]:
<message>`, cut at 2000 characters. Never log secrets. Any other
notification a plugin sends is noted on stderr and otherwise ignored.

### Agent tools: `tools/call` (OpenRemote to plugin)

A chat's agent called one of the tools the plugin's install record
declares. The agent sees each tool on the chat's `openremote` MCP server
as `<plugin name>__<tool name>` (two underscores); the plugin is called
with its own tool name.

```json
{"jsonrpc": "2.0", "id": 7, "method": "tools/call", "params": {
  "name": "word_count",
  "arguments": {"text": "one two three"},
  "chat": {"id": "2320d324-78ce-4ee1-b13e-18e7ca310f14", "workspace": "/home/me/code/site"}
}}
```

`chat.workspace` is null for a chat in no project. Answer with an MCP tool
result:

```json
{"jsonrpc": "2.0", "id": 7, "result": {
  "content": [
    {"type": "text", "text": "3 words, 1 line, 13 characters"},
    {"type": "image", "data": "<base64>", "mimeType": "image/png"}
  ],
  "isError": false
}}
```

- `content` items are MCP's: `text` (`text`) and `image` (`data` in
  base64, `mimeType`). Other MCP content types pass through as they are.
- A plain string `result` is taken as one text item.
- A tool that failed in a way the agent should read answers a result with
  `"isError": true`. A JSON-RPC `error` reaches the agent as an error
  result too, prefixed with the plugin's name.
- The call waits for the tool's `timeout_secs` from the manifest (1 to
  600), else 120 seconds; then the agent is told it timed out.
- Only plugins whose record holds `agent_tools` (any plugin with tools
  does) offer tools, and a chat gets only the plugins it rides: every
  Global and Machine plugin, and the plugins installed for the chat's own
  project.
- Every call is approved like any other MCP tool call: the chat's
  permission mode decides whether the user is asked. A tool's
  `read_only` flag reaches the agent as MCP's `readOnlyHint` and never
  skips that approval.

### Views: `ui/request` (OpenRemote to plugin)

A view's page asked the plugin's program something, through the daemon's
`POST /plugins/{id}/request`:

```json
{"jsonrpc": "2.0", "id": 12, "method": "ui/request", "params": {"name": "hosts", "params": {}}}
```

Answer with whatever JSON the view expects. The call waits 60 seconds.
The route exists, but the console does not show a process plugin's views
yet: today only a JavaScript plugin's sidebar views have a place to
render.

### Calls to OpenRemote (plugin to OpenRemote)

A plugin can ask OpenRemote for a few things, each only with a power in
its install record. The power is checked before anything else; a call
without it fails with `-32003`. These are the same calls, behind the same
check, that a JavaScript plugin's frame reaches through the console.

| Method | Power | Params | Result |
|---|---|---|---|
| `chats/list` | `read_chats` | | `{"chats": [{"id", "title", "harness", "model", "status", "workspace", "updated_at"}]}` |
| `chats/read` | `read_chats` | `{"chat"}` | `{"text"}`: the chat's messages, oldest first |
| `chats/start` | `agent_tools` | `{"harness", "workspace"?, "model"?, "effort"?, "prompt"?}` | `{"chat"}` |
| `chats/prompt` | `agent_tools` | `{"chat", "text"}` | `{"queued": true}`: sent now, or after the running turn |
| `chats/stop` | `agent_tools` | `{"chat"}` | `{"stopped": true}` |

- `harness` is required for `chats/start`; `workspace` is an absolute
  folder on this machine.
- A plugin installed for one project reaches only that project's chats:
  `chats/list` lists only them, the others answer `-32003`, and
  `chats/start` starts in the project's folder when no workspace is named
  and refuses a folder outside it.
- A chat a plugin starts runs on this machine with the harness's default
  permissions: a plugin can't grant a chat more than one the user starts
  gets.

### Errors

Standard JSON-RPC codes, plus OpenRemote's own:

| Code | Meaning |
|---|---|
| `-32601` | No such method. |
| `-32602` | Bad params. |
| `-32000` | The call failed (its message says why). From OpenRemote: the plugin stopped, or OpenRemote is stopping. |
| `-32003` | From OpenRemote: the plugin lacks the power for that call, or the chat is outside its project. |

A call to the plugin that runs past its time limit fails as a timeout,
which the agent reads as an error result.

## WASM plugins

A WASM plugin is a WebAssembly module whose exports OpenRemote calls for
each agent tool call. It runs inside the daemon on an interpreter,
sandboxed: it sees its own memory and the host functions its powers
grant, nothing else. There is no WASI: no files, no environment, no clock,
no sockets. Each call runs on a fresh instance, so nothing carries over
between calls or chats.

### The module

Exports:

- `memory`: its linear memory.
- `or_alloc(len: i32) -> i32`: reserve `len` bytes, return where.
- `or_call(ptr: i32, len: i32) -> i64`: run one call. The input is `len`
  bytes of JSON at `ptr`. Return `(out_ptr << 32) | out_len`: where the
  answer's JSON lies in memory.

The review refuses a module that doesn't compile or doesn't export all
three.

The input:

```json
{
  "tool": "slugify",
  "arguments": {"text": "Hello, World"},
  "settings": {"separator": "-"},
  "chat": {"id": "2320d324-...", "workspace": "/home/me/code/site"}
}
```

`settings` holds the plugin's declared settings (set, else default),
secrets included. The answer is an MCP tool result, as for process
plugins (`{"content": [...], "isError": false}`); a plain JSON string is
taken as one text item.

### Host functions

Imported from the module `openremote`. A module may import only what its
powers grant: the review refuses one that imports more (WASI included),
and at run time the network functions are linked only when the install
record holds `network`.

| Import | Signature | Power | Does |
|---|---|---|---|
| `log` | `(ptr: i32, len: i32)` | none | A line on OpenRemote's stderr (up to 4096 bytes). |
| `http` | `(ptr: i32, len: i32) -> i32` | `network` | One HTTP request; returns the answer's length. |
| `take` | `(ptr: i32, len: i32) -> i32` | `network` | Copy the last answer into memory at `ptr`; returns bytes copied. |

`http`'s request, JSON at `ptr`:

```json
{"method": "GET", "url": "https://api.example.com/v1/items", "headers": {"accept": "application/json"},
 "body": "optional text", "content_type": "application/json"}
```

The answer (reserve `len` bytes with your allocator, then `take` them):

```json
{"status": 200, "headers": {"content-type": "application/json"}, "body": "..."}
```

A body that isn't UTF-8 comes as `body_base64` instead. A request that
can't be made answers `{"error": "..."}`.

- Requests go through the daemon's own HTTP client to http and https
  addresses **on the public internet only**. The host is looked up once,
  and the request is refused if any address it gives is not public:
  `localhost` and `*.localhost`, loopback, private (10/8, 172.16/12,
  192.168/16), link-local (such as a cloud's metadata service at
  169.254.169.254), carrier-grade NAT (100.64/10), unspecified,
  broadcast, multicast, documentation, benchmarking (198.18/15), the
  192.0.0/24 and 0/8 blocks, 240/4, and on IPv6 loopback, unique local
  (fc00::/7), link-local, site-local, and mapped, compatible, NAT64 and
  6to4 forms of a non-public IPv4 address.
- The connection goes to the very address that was checked (a second
  lookup can't move it), TLS still checks the host's name, and redirects
  are never followed: a redirect comes back as its own answer.
- The module can't set `Host`, `Content-Length`, `Transfer-Encoding`,
  `Connection`, `Upgrade`, `TE`, `Trailer`, `Keep-Alive` or the proxy
  headers.
- 16 requests per call, each within 30 seconds and within the call's
  deadline, an answer up to the client's body limit.

### Limits

- About two billion instructions of fuel per call: a few seconds of work.
  A call that runs out is stopped, and the agent is told it ran past its
  time limit.
- 64 MiB of memory per instance (one instance, at most 4 tables and
  100,000 table elements); growing past it fails.
- Input and output up to 16 MiB each; a module up to 32 MiB.
- The call's deadline is the tool's `timeout_secs`, else 120 seconds. It
  bounds the wait for a turn, every lookup and every request.
- Four calls run at once; more wait their turn, within their deadline.

## Versions

This is protocol 1. Fields and methods may be added within version 1;
plugins ignore what they don't know, and OpenRemote does the same. A
change that would break a plugin gets a new version number, which
`initialize` names.
