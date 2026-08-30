# 04. Protocol & API Specification

This document defines the 2-byte binary WebSocket framing protocol, JSON-RPC 2.0 contracts, typed event models, REST routes, and Server-Sent Events (SSE) interfaces for **OpenRemote**.

---

## 1. 2-Byte Binary WebSocket Framing Protocol

To achieve maximum streaming throughput and minimal CPU overhead on mobile and embedded clients, OpenRemote utilizes a **2-byte compact binary header** for all WebSocket frames:

```text
 0                   1                   2                   3
 0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|  Opcode (1B)  | Session Slot  |         Payload Data...       |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
```

### Opcodes Table:

| Opcode | Direction | Description | Payload Format |
| :--- | :--- | :--- | :--- |
| `0x01` (`OpcodePTYOutput`) | Server $\rightarrow$ Client | Raw PTY stdout / stderr chunk | Raw UTF-8 / ANSI bytes |
| `0x02` (`OpcodeKeystroke`) | Client $\rightarrow$ Server | Raw keystroke or prompt input | Raw UTF-8 bytes |
| `0x03` (`OpcodeViewportResize`)| Client $\rightarrow$ Server | Viewport resize request | `[Cols: uint16, Rows: uint16]` (4 bytes, Big-Endian) |
| `0x04` (`OpcodeCatchup`) | Client $\rightarrow$ Server | Monotonic sequence catchup request | `[LastSeq: uint32]` (4 bytes, Big-Endian) |
| `0x05` (`OpcodeJSONRPC`) | Bidirectional | JSON-RPC 2.0 method / event envelope | UTF-8 JSON String |
| `0x06` (`OpcodePingPong`) | Bidirectional | Connection latency ping / pong | `[Timestamp: uint64]` (8 bytes, Big-Endian) |

---

## 2. JSON-RPC 2.0 over Opcode `0x05`

Structured RPC invocations between client and daemon are multiplexed over WebSocket opcode `0x05`:

### Request Format:
```json
{
  "jsonrpc": "2.0",
  "id": "req_01",
  "method": "session.create",
  "params": {
    "agentId": "claude-code",
    "cwd": "/home/user/project",
    "useWorktree": true,
    "taskName": "feat-login",
    "cols": 120,
    "rows": 30
  }
}
```

### Response Format:
```json
{
  "jsonrpc": "2.0",
  "id": "req_01",
  "result": {
    "sessionId": "ses_9a81f",
    "workspaceId": "wks_c3b2",
    "worktreePath": "/home/user/project/.openremote/worktrees/task-feat-login",
    "status": "running"
  }
}
```

### Core RPC Methods:
- `session.create` — Provision a new agent session.
- `session.list` — Retrieve all active and persisted sessions.
- `session.stop` — Terminate an active session and prune worktrees.
- `session.sendPrompt` — Submit an interactive prompt.
- `session.approve` — Approve or deny a pending tool confirmation.
- `session.answer` — Submit answer to a disambiguation question.
- `session.resize` — Adjust terminal viewport dimensions.
- `system.status` — Query server uptime, memory, and active sessions.
- `system.agents` — Query available AI agent drivers and detection status.
- `system.tunnels` — Query active Cloudflare / Tailscale tunnel URLs.

---

## 3. Monotonic Typed Event Envelopes

Every event emitted across the event bus contains a monotonic sequence number (`seq`) assigned by SQLite AUTOINCREMENT:

```go
type BaseEvent struct {
    Seq       int64  `json:"seq"`
    SessionID string `json:"sessionId"`
    Timestamp int64  `json:"timestamp"` // Unix milliseconds
}
```

### Event Schemas:

#### 1. Chat Message (`chat.message`)
```json
{
  "seq": 101,
  "sessionId": "ses_9a81f",
  "timestamp": 1756475800100,
  "type": "chat.message",
  "role": "assistant",
  "content": "I will update `auth.go` to add token verification."
}
```

#### 2. Stream Chunk (`stream.chunk`)
```json
{
  "seq": 102,
  "sessionId": "ses_9a81f",
  "timestamp": 1756475800200,
  "type": "stream.chunk",
  "chunk": "Compiling internal/core/auth...\n"
}
```

#### 3. Tool Approval Requested (`approval.requested`)
```json
{
  "seq": 103,
  "sessionId": "ses_9a81f",
  "timestamp": 1756475800300,
  "type": "approval.requested",
  "approvalId": "app_54f",
  "toolName": "Bash",
  "command": "go test -v ./internal/core/auth",
  "description": "Run authentication unit tests",
  "autoDenyTimeoutMs": 300000
}
```

#### 4. Disambiguation Question Asked (`question.asked`)
```json
{
  "seq": 104,
  "sessionId": "ses_9a81f",
  "timestamp": 1756475800400,
  "type": "question.asked",
  "questionId": "q_18c",
  "questionText": "Which authentication token format should we adopt?",
  "options": [
    "256-bit Hex Bearer Token (Recommended)",
    "JWT with HMAC-SHA256",
    "Ed25519 Signed Challenge"
  ],
  "isMultiSelect": false
}
```

#### 5. File Diff Generated (`diff.generated`)
```json
{
  "seq": 105,
  "sessionId": "ses_9a81f",
  "timestamp": 1756475800500,
  "type": "diff.generated",
  "filePath": "internal/core/auth/auth.go",
  "diffPatch": "--- a/internal/core/auth/auth.go\n+++ b/internal/core/auth/auth.go\n@@ -12,2 +12,4 @@\n+func VerifyToken(tok string) bool {\n+    return subtle.ConstantTimeCompare([]byte(tok), []byte(validToken)) == 1\n+}",
  "additions": 3,
  "deletions": 0
}
```

#### 6. Turn Completed (`turn.completed`)
```json
{
  "seq": 106,
  "sessionId": "ses_9a81f",
  "timestamp": 1756475800600,
  "type": "turn.completed",
  "summary": "Implemented token verification and passing tests.",
  "costUsd": 0.042,
  "durationMs": 4820
}
```

#### 7. Artifact Updated (`artifact.updated`)
```json
{
  "seq": 107,
  "sessionId": "ses_9a81f",
  "timestamp": 1756475800700,
  "type": "artifact.updated",
  "artifactPath": "implementation_plan.md",
  "summary": "Updated phase 2 implementation details"
}
```

---

## 4. REST API Routes

| Method | Route | Description | Auth Required |
| :--- | :--- | :--- | :---: |
| `GET` | `/health` | Heartbeat & uptime status | No |
| `POST` | `/api/v1/sessions` | Create or launch agent session | Yes |
| `GET` | `/api/v1/sessions` | List active & stored sessions | Yes |
| `GET` | `/api/v1/sessions/:id` | Get session details & event history (`?since=<seq>`) | Yes |
| `DELETE`| `/api/v1/sessions/:id` | Stop session & prune worktree | Yes |
| `POST` | `/api/v1/approval/:id` | Submit tool approval decision (`{"approved": true}`) | Yes |
| `POST` | `/api/v1/question/:id` | Submit question answer (`{"answers": ["Option 1"]}`) | Yes |
| `GET` | `/api/v1/agents` | Query supported and detected agents | Yes |
| `GET` | `/api/v1/files` | List workspace directory entries safely (`?dir=...`) | Yes |
| `GET` | `/api/v1/diff/:sessionId` | Get unified git diff for active worktree | Yes |
| `GET` | `/api/v1/tunnels` | Query Cloudflare / Tailscale ingress status | Yes |
| `GET` | `/api/v1/telegram/status` | Query Telegram bot status and paired chat IDs | Yes |

---

## 5. Server-Sent Events (`GET /events`)

For lightweight clients, mobile background listeners, and webhook relays, OpenRemote provides an SSE stream with reconnection catchup:

```http
GET /events?sessionId=ses_9a81f&lastSeq=1420 HTTP/1.1
Host: 127.0.0.1:4097
Authorization: Bearer 9a4f...
Accept: text/event-stream
```

### SSE Stream Payload Example:
```text
event: approval.requested
data: {"seq":1421,"sessionId":"ses_9a81f","approvalId":"app_54f","toolName":"Bash","command":"go test ./...","autoDenyTimeoutMs":300000}

event: stream.chunk
data: {"seq":1422,"sessionId":"ses_9a81f","chunk":"=== RUN   TestAuthToken\n--- PASS: TestAuthToken (0.00s)\n"}

: keepalive
```

---

## 6. Standard Error Codes

| Code | HTTP Status | Meaning | Resolution |
| :--- | :---: | :--- | :--- |
| `ERR_AUTH_REQUIRED` | 401 | Missing or invalid Bearer token | Enter token in Settings or verify `token` file |
| `ERR_PATH_TRAVERSAL` | 403 | Requested file path escapes workspace sandbox | Confine file queries to workspace root |
| `ERR_SESSION_NOT_FOUND`| 404 | Target session ID does not exist | Refresh session list or create a new session |
| `ERR_PTY_CRASH` | 500 | PTY process encountered an unhandled exit | Server logs error; client issues catchup request |
| `ERR_RATE_LIMITED` | 429 | Telegram / external API throttle reached | Client applies exponential backoff delay |

