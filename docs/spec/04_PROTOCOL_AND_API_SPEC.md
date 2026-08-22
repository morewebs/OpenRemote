# 04. Protocol & API Specification (`@openremote/shared-protocol`)

This document defines the WebSocket binary framing protocol, Zod schemas, REST routes, and Server-Sent Events (SSE) contracts.

---

## 1. Binary WebSocket Framing Protocol

To achieve maximum throughput and minimum CPU overhead for raw terminal streaming, OpenRemote uses a **2-byte binary header**:

```
 0                   1                   2                   3
 0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|  Opcode (1B)  | Session Slot  |         Payload Data...       |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
```

### Opcodes Table:

| Opcode | Direction | Description | Payload Format |
| :--- | :--- | :--- | :--- |
| `0x01` | Server -> Client | Raw PTY Output Chunk | Raw UTF-8 / ANSI bytes |
| `0x02` | Client -> Server | Raw Keystroke / Prompt Input | Raw UTF-8 bytes |
| `0x03` | Client -> Server | Viewport Resize Request | `[Cols: uint16, Rows: uint16]` (4 bytes) |
| `0x04` | Client -> Server | Catchup Request | `[LastSeq: uint32]` (4 bytes) |
| `0x05` | Bidirectional | Structured JSON RPC / Event | UTF-8 JSON String |
| `0x06` | Bidirectional | Heartbeat Ping / Pong | Timestamp `uint64` (8 bytes) |

---

## 2. Shared Zod Schemas (`events.ts`)

```typescript
import { z } from 'zod';

// Base Monotonic Event Schema
export const BaseEventSchema = z.object({
  seq: z.number().int().positive(),
  sessionId: z.string(),
  timestamp: z.number(),
});

// 1. Stream Chunk Event
export const StreamChunkEventSchema = BaseEventSchema.extend({
  type: z.literal('stream.chunk'),
  chunk: z.string(), // Base64 or plain string for JSON channels
});

// 2. Tool Approval Requested Event
export const ApprovalRequestedEventSchema = BaseEventSchema.extend({
  type: z.literal('approval.requested'),
  approvalId: z.string(),
  toolName: z.string(),
  command: z.string(),
  description: z.string().optional(),
  autoDenyTimeoutMs: z.number().default(300000), // 5 min default
});

// 3. Disambiguation Question Event
export const QuestionAskedEventSchema = BaseEventSchema.extend({
  type: z.literal('question.asked'),
  questionId: z.string(),
  questionText: z.string(),
  options: z.array(z.string()),
  isMultiSelect: z.boolean().default(false),
});

// 4. File Diff Generated Event
export const DiffGeneratedEventSchema = BaseEventSchema.extend({
  type: z.literal('diff.generated'),
  filePath: z.string(),
  diffPatch: z.string(),
  additions: z.number(),
  deletions: z.number(),
});

// 5. Turn Completed Event
export const TurnCompletedEventSchema = BaseEventSchema.extend({
  type: z.literal('turn.completed'),
  summary: z.string().optional(),
  costUsd: z.number().optional(),
  durationMs: z.number(),
});

// Discriminated Union
export const AgentEventSchema = z.discriminatedUnion('type', [
  StreamChunkEventSchema,
  ApprovalRequestedEventSchema,
  QuestionAskedEventSchema,
  DiffGeneratedEventSchema,
  TurnCompletedEventSchema,
]);

export type AgentEvent = z.infer<typeof AgentEventSchema>;
```

---

## 3. REST API Routes

### 1. Sessions Management
* `POST /api/v1/sessions`
  - Body:
    ```json
    {
      "agentId": "claude-code",
      "cwd": "/path/to/project",
      "useWorktree": true,
      "taskName": "refactor-auth"
    }
    ```
  - Response: `201 Created`
    ```json
    {
      "sessionId": "ses_9a81f",
      "workspaceId": "wks_c3b2",
      "worktreePath": "/path/to/project/.openremote/worktrees/task-refactor-auth",
      "status": "running"
    }
    ```
* `GET /api/v1/sessions` — List all active and background sessions.
* `DELETE /api/v1/sessions/:id` — Terminate and archive session.

### 2. Human-in-the-Loop Interaction
* `POST /api/v1/approval/:id`
  - Body: `{ "approved": true }`
  - Response: `200 OK`
* `POST /api/v1/question/:id`
  - Body: `{ "answers": ["Option 1"] }`
  - Response: `200 OK`

### 3. File Explorer & Diffs
* `GET /api/v1/files?dir=...` — List workspace files safely (enforcing sandbox boundary).
* `GET /api/v1/diff/:sessionId` — Get accumulated git diff for the current task worktree.

---

## 4. Error Codes Table

| Code | Status | Meaning | Recovery Action |
| :--- | :---: | :--- | :--- |
| `ERR_AUTH_REQUIRED` | 401 | Missing or invalid Bearer token | Prompt user for auth token in client UI |
| `ERR_PATH_TRAVERSAL` | 403 | Requested file is outside workspace root | Abort file read and log security warning |
| `ERR_SESSION_NOT_FOUND` | 404 | Target session ID is not active | Client falls back to session list |
| `ERR_CONPTY_EXCEPTION` | 500 | ConPTY worker crashed | Server auto-restarts worker; client sends `lastSeq` |
| `ERR_RATE_LIMITED` | 429 | Telegram / API rate limit reached | Client applies exponential backoff curve |
