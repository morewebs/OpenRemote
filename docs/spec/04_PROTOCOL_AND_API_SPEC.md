# Protocol and API

Updated September 19, 2026. Source contracts live in `internal/protocol`; routing
lives in `internal/core/server`. All API, WebSocket and SSE requests require the
daemon token. Send `Authorization: Bearer TOKEN`; browser streams can use
`?token=TOKEN`. `/health` and static assets are public. Treat stream URLs as secrets.

## REST

| Method | Route | Body / result |
|---|---|---|
| GET | `/health` | Uptime and session count |
| GET | `/api/v1/agents` | Availability and driver capabilities |
| GET, POST | `/api/v1/sessions` | List or create a session |
| GET | `/api/v1/sessions/{id}` | Session metadata |
| GET | `/api/v1/sessions/{id}?since=SEQ` | Persisted events after the cursor |
| DELETE | `/api/v1/sessions/{id}` | Stop, remove clean worktree and delete history |
| POST | `/api/v1/sessions/{id}/prompt` | `{"prompt":"text"}` |
| POST | `/api/v1/sessions/{id}/input` | `{"data":"BASE64_BYTES"}` |
| POST | `/api/v1/sessions/{id}/resize` | `{"cols":120,"rows":30}` |
| POST | `/api/v1/approval/{id}` | `{"approved":true}` |
| POST | `/api/v1/question/{id}` | `{"answers":["first","second"]}` |
| GET | `/api/v1/files?path=PATH` | Directory entries with name, path, size and isDir |
| GET | `/api/v1/file?path=PATH` | Text content, size and truncation flag |
| GET | `/api/v1/diff/{id}` | Unified Git diff as text |
| GET, POST | `/api/v1/tunnels` | List or `{"name":"cloudflared","action":"start"}` / `{"action":"stop"}` |
| GET | `/api/v1/telegram/status` | Telegram runtime status |
| GET | `/events?sessionId=ID&lastSeq=SEQ` | SSE durable event stream |
| GET | `/ws?sessionId=ID` | WebSocket protocol below |

Create body:

```json
{"agentId":"codex","cwd":"/absolute/project","useWorktree":false,"cols":120,"rows":30}
```

Optional fields are `taskName` and `remoteControl` (Claude). Agent IDs are
`claude-code`, `antigravity`, `opencode`, `codex`, `pi`, and `shell`. Creation returns
HTTP 201 with `sessionId`, `workspaceId`, optional `worktreePath` and `status`.
Missing agents and failed startup return errors; a stopped history record may
remain for diagnostics. A dirty worktree returns 409 on DELETE and stays intact.

File reads enforce allowed roots at open time, including symlinks. Previews
reject binary files and cap at 256 KB. Diffs include staged and unstaged tracked
changes against HEAD, cap at 2 MB, and require an initial commit. File uploads
and arbitrary file writes are not API operations.

## WebSocket frames

Each binary frame begins with opcode (one byte), then session slot (one byte).
The active implementation binds a connection to the `sessionId` query parameter;
slot values are not a multi-session routing mechanism.

| Opcode | Direction | Payload |
|---|---|---|
| `0x01` | Server to client | Raw terminal bytes |
| `0x02` | Client to server | Terminal input bytes |
| `0x03` | Client to server | Big-endian uint16 cols, uint16 rows |
| `0x04` | Client to server | Big-endian uint32 event cursor (legacy catchup) |
| `0x05` | Both | UTF-8 JSON event or JSON-RPC envelope |
| `0x06` | Both | Ping/pong bytes, conventionally a uint64 timestamp |

JSON-RPC also accepts text frames. Dimensions clamp to 20–300 columns and 5–100
rows. Native chat transports do not expose an interactive terminal.

For recoverable delivery, connect with `eventsOnly=1&lastSeq=SEQ`. This mode sends
persisted events including base64 `stream.chunk`, deduplicates replay/live overlap,
and omits raw terminal frames/snapshots. Start at zero for initial hydration;
persist the last processed sequence for reconnect. Slow clients disconnect and
can replay. Decode terminal bytes with a streaming UTF-8 decoder. Legacy clients
receive a PTY snapshot plus live raw bytes and can request event catchup separately.

## JSON-RPC 2.0

```json
{"jsonrpc":"2.0","id":"request-1","method":"session.sendPrompt","params":{"sessionId":"ses_123","prompt":"hello"}}
```

Methods: `session.create`, `session.list`, `session.stop`, `session.sendPrompt`,
`session.approve`, `session.answer`, `session.resize`, `system.status`,
`system.agents`, `system.tunnels`. Approval/answer params include `approvalId` or
`questionId` and the matching REST body fields. Session methods use `sessionId`
from params or the connection. Legacy aliases are `prompt.send`,
`approval.resolve`, `agents.list`.

REST and RPC share handlers. Notifications omit the ID and receive no reply.
Batches are supported; invalid envelopes return standard JSON-RPC errors.
Application errors include `data.httpStatus` for the underlying HTTP failure.

## Durable event schema

All events contain `seq`, `sessionId`, `timestamp` (Unix milliseconds) and `type`.
Sequences increase database-wide and can have gaps within a session. Chat
revisions replace an existing message by `messageId` and `rev`.

```json
{"seq":101,"sessionId":"ses_123","timestamp":1789840000000,"type":"chat.message","messageId":"msg_1","role":"assistant","kind":"text","text":"Hello","rev":1,"streaming":false}
```

```json
{"seq":102,"sessionId":"ses_123","timestamp":1789840000001,"type":"stream.chunk","encoding":"base64","chunk":"SGVsbG8K"}
```

Other types: `approval.requested`, `approval.resolved`, `question.asked`,
`question.answered`, `auth.url`, `diff.generated`, `turn.completed`,
`artifact.updated`, `session.status`. Exact fields are defined in
`internal/protocol/events.go`. `question.asked` carries `options` and
`isMultiSelect`; clients submit selected labels, or a free-text answer.

## SSE

`GET /events` requires a session ID and accepts `lastSeq`. The `Last-Event-ID`
header takes precedence. Each record is `id: SEQ`, `data: JSON`, then a blank
line. The stream subscribes before replay and drains SQLite after live wakeups;
keepalive comments arrive every 20 seconds. Use the same deduplication and byte
decoding as WebSocket event mode. The companion switches to SSE after repeated
WebSocket failures and sends terminal input/resize through REST.
