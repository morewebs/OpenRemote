# Architectural Review: claudecodeui

> **Target Repository**: [`claudecodeui`](file:///c:/Users/W/Documents/GitHub/OpenRemote/ref/claudecodeui) (`@cloudcli-ai/cloudcli`, formerly `@siteboon/claude-code-ui`)  
> **Review Date**: August 2026  
> **Version Audited**: `1.37.2` (AGPL-3.0-or-later)  
> **Repository Scope**: Full-stack codebase audit spanning Next.js / React 18 frontend, Node.js / Express backend server, provider orchestration engine (Claude Agent SDK, Cursor CLI, OpenAI Codex SDK, OpenCode CLI), PTY terminal subsystem, SQLite storage, Docker sandbox environments, and Electron desktop shell.

---

## 1. Executive Summary

[`claudecodeui`](file:///c:/Users/W/Documents/GitHub/OpenRemote/ref/claudecodeui/package.json) (branded commercially as **CloudCLI**) is a web-based, multi-provider IDE and GUI designed to bridge terminal-centric AI coding agents with a modern web and desktop user interface. Originally developed as a dedicated web UI for Anthropic's **Claude Code CLI**, the project evolved into an agent gateway and developer workspace supporting multiple agent backends: Anthropic Claude Code, Cursor CLI, OpenAI Codex, and OpenCode.

```
+---------------------------------------------------------------------------------------+
|                                    CloudCLI Web / Desktop                             |
|  +--------------------+  +----------------------+  +---------------+  +------------+  |
|  |   Chat Interface   |  |   Standalone Shell   |  |  Code Editor  |  | File Tree  |  |
|  | (Rich Tool Cards)  |  | (xterm.js + node-pty)|  | (CodeMirror 6)|  |  & Git     |  |
|  +---------+----------+  +----------+-----------+  +-------+-------+  +-----+------+  |
+------------|------------------------|----------------------|----------------|---------+
             | JSON/WebSocket         | PTY WebSocket        | REST / SSE     |
             v (/ws)                  v (/shell)             v (/api/...)     v
+---------------------------------------------------------------------------------------+
|                                 Node.js / Express Backend                             |
|  +--------------------+  +----------------------+  +-------------------------------+  |
|  |  ChatRunRegistry   |  |   PtySessionsMap     |  |     SQLite (better-sqlite3)   |  |
|  |  & Event Sequencer |  |   & Terminal Stream  |  |  Auth, Sessions, Models, MCP  |  |
|  +---------+----------+  +----------+-----------+  +-------------------------------+  |
+------------|------------------------|-------------------------------------------------+
             |                        |
             +-----------+------------+
                         |
      +------------------+------------------+------------------+
      |                  |                  |                  |
      v                  v                  v                  v
+------------+    +------------+    +---------------+    +------------+
|   Claude   |    |   Cursor   |    |     Codex     |    |  OpenCode  |
| Agent SDK  |    | Agent CLI  |    |   OpenAI SDK  |    |  CLI (IPC) |
+------------+    +------------+    +---------------+    +------------+
```

### Core Design Goals & Capabilities
1. **Dual-Mode Workflow (GUI + Raw PTY)**: Seamless coexistence between a high-level chat interface (with interactive tool approvals, diff widgets, markdown/math/mermaid rendering) and a persistent, low-level ANSI terminal powered by `node-pty` and `xterm.js`.
2. **Unified Provider Abstraction (`IProvider`)**: A normalized architectural interface decoupling UI components from specific CLI/SDK implementations. The backend maps disparate streaming protocols, token metrics, and execution models into standard `NormalizedMessage` events.
3. **Background Agent Persistence**: Custom async stream holding (`createHeldPromptStream`) keeps CLI subprocesses and background tasks alive across turns until asynchronous background work (e.g. `Bash` background jobs, `Monitor`, `ScheduleWakeup`) completes and reports back.
4. **Resilient Realtime Protocol**: Sequence-numbered WebSocket events (`seq`) allowing transparent replay on client reconnection (e.g. mobile network drops or page reloads), paired with automatic token refreshing and session resurrection.
5. **Multi-Platform Deployment**: Packaged simultaneously as a local web server (`npx @cloudcli-ai/cloudcli`), a standalone Electron desktop application (`electron/`), and Docker sandbox environments (`docker/`).

---

## 2. Architecture & Data Flow

The project is architected as an event-driven decoupled client-server application. The backend serves both static Vite-bundled assets and authenticated REST/WebSocket endpoints, while the client maintains persistent bidirectional channels for chat events, terminal sessions, and notifications.

### 2.1 System Architecture Diagram

```mermaid
flowchart TB
    subgraph Client_Layer ["Client Layer (Browser / Electron / Mobile)"]
        UI_MAIN["MainContent.tsx\n(View Orchestrator)"]
        UI_CHAT["ChatInterface.tsx\n(Messages, Tools, Diff Cards)"]
        UI_SHELL["StandaloneShell.tsx\n(xterm.js Terminal + Addons)"]
        UI_EDITOR["CodeEditor.tsx\n(CodeMirror 6 / Merge Diff)"]
        UI_TREE["FileTree.tsx / GitPanel.tsx\n(Workspace Explorer)"]
        UI_BROWSER["BrowserUsePanel.tsx\n(Live Screenshot / Viewport)"]
    end

    subgraph Gateway_Layer ["Transport & Gateway Layer (server/index.ts)"]
        HTTP_ROUTER["Express HTTP Router\n(/api/auth, /api/providers, /api/file-tree, /api/git)"]
        WS_GATEWAY["WebSocket Gateway Server\n(server/modules/websocket/services/websocket-server.service.ts)"]
        AUTH_MW["Auth Middleware\n(JWT Verification & 7-Day Auto-Rotation)"]
    end

    subgraph Service_Core ["Core Application Services"]
        CHAT_REG["ChatRunRegistry\n(Run Lifecycle & Sequence Replay)"]
        PTY_MGR["PtySessionManager\n(node-pty Spawn & 30m Retention)"]
        PROV_SVC["ProviderRuntimeService\n(Runtime Dispatcher)"]
        BROWSER_SVC["BrowserUseService\n(Playwright Headless Browser)"]
        NOTIF_SVC["NotificationService\n(Web Push VAPID / Desktop WS)"]
        DB_REPO[("SQLite Singleton (better-sqlite3)\n~/.cloudcli/auth.db")]
    end

    subgraph Provider_Adapters ["Provider Adapters (server/modules/providers/list/)"]
        CLAUDE_ADP["ClaudeRuntimeProvider\n(@anthropic-ai/claude-agent-sdk)"]
        CURSOR_ADP["CursorRuntimeProvider\n(cross-spawn cursor-agent)"]
        CODEX_ADP["CodexRuntimeProvider\n(@openai/codex-sdk)"]
        OPENCODE_ADP["OpenCodeRuntimeProvider\n(cross-spawn opencode CLI)"]
    end

    UI_CHAT <-->|WS /ws (JSON Events)| WS_GATEWAY
    UI_SHELL <-->|WS /shell (Raw ANSI)| WS_GATEWAY
    UI_MAIN <-->|HTTP REST / SSE| HTTP_ROUTER
    
    WS_GATEWAY --> AUTH_MW
    HTTP_ROUTER --> AUTH_MW
    
    AUTH_MW --> CHAT_REG
    AUTH_MW --> PTY_MGR
    AUTH_MW --> PROV_SVC
    AUTH_MW --> DB_REPO

    CHAT_REG --> PROV_SVC
    PROV_SVC --> CLAUDE_ADP
    PROV_SVC --> CURSOR_ADP
    PROV_SVC --> CODEX_ADP
    PROV_SVC --> OPENCODE_ADP

    BROWSER_SVC --> UI_BROWSER
    NOTIF_SVC --> UI_MAIN
```

---

### 2.2 Interactive Chat Execution & Tool Approval Flow

The following sequence diagram details the lifecycle of a prompt requiring interactive tool permission approval (e.g. file edits or bash executions), highlighting the `canUseTool` callback, WebSocket notification, user decision resolution, and the held-prompt mechanism.

```mermaid
sequenceDiagram
    autonumber
    actor User as User / Web Client
    participant WS as WebSocket Gateway (/ws)
    participant Registry as ChatRunRegistry
    participant Runtime as ClaudeRuntimeProvider
    participant SDK as Claude Agent SDK (@anthropic-ai/claude-agent-sdk)
    participant Subprocess as Claude CLI Engine

    User->>WS: send { type: "chat.send", sessionId, content, options }
    WS->>Registry: startRun(sessionId, ws, userId)
    Registry-->>WS: emit session_created (if new)
    WS->>Runtime: run(command, options, writer, context)
    Runtime->>Runtime: buildPromptMessages() & createHeldPromptStream()
    Runtime->>SDK: query({ prompt: heldStream, options: sdkOptions })
    SDK->>Subprocess: spawn CLI / send user message

    Subprocess-->>SDK: tool_use event (e.g. Bash / WriteFile)
    SDK->>Runtime: canUseTool(toolName, input, context)
    Runtime->>Runtime: generate requestId & create Promise resolver
    Runtime->>WS: emit { kind: "permission_request", requestId, toolName, input }
    WS->>User: Render Interactive Permission Dialog / Modal

    User->>WS: send { type: "chat.permission-response", requestId, allow: true }
    WS->>Runtime: resolveToolApproval(requestId, { allow: true })
    Runtime-->>SDK: return { behavior: "allow", updatedInput }
    SDK->>Subprocess: execute tool action

    Subprocess-->>SDK: streaming response chunks
    SDK-->>Runtime: yield message
    Runtime->>WS: emit { kind: "stream_delta" | "tool_result", seq, ... }
    WS->>User: Render text delta / tool output

    Subprocess-->>SDK: result event (turn completed)
    Runtime->>WS: emit { kind: "complete", exitCode: 0 }
    Runtime->>Runtime: Check startsBackgroundWork()
    alt Background Work Pending (Bash bg / Monitor)
        Runtime->>Runtime: scheduleRelease(BG_WAIT_CEILING_MS: 30m)
        Note over Runtime,Subprocess: Prompt stream kept open; CLI subprocess stays alive
    else No Background Tasks
        Runtime->>Runtime: releasePromptStream() (stdin closed)
        SDK->>Subprocess: wind down process
    end
    Registry->>Registry: completeRun(sessionId)
```

---

### 2.3 Standalone Terminal (PTY) Lifecycle & URL Detection

```mermaid
sequenceDiagram
    autonumber
    actor User as User / Terminal Tab
    participant WS as WebSocket Gateway (/shell)
    participant ShellService as ShellWebSocketService
    participant PTY as node-pty Subprocess (PowerShell/Bash)
    participant OS as Host OS Shell

    User->>WS: Connect WebSocket (/shell)
    User->>WS: send { type: "init", projectPath, sessionId, cols, rows, provider }
    ShellService->>ShellService: Build session key (${projectPath}_${sessionId})
    
    alt Existing PTY Active in Map
        ShellService->>WS: emit "[Reconnected to existing session]\r\n"
        ShellService->>WS: replay buffered chunks (up to 5000)
    else Spawn New PTY
        ShellService->>PTY: spawn(shell, ["-Command", cmd], { cols, rows, cwd, env })
        PTY->>OS: Execute powershell.exe / bash
    end

    loop PTY Data Stream
        OS-->>PTY: stdout / stderr chunks
        PTY-->>ShellService: onData(chunk)
        ShellService->>ShellService: Buffer chunk in memory (ring buffer <= 5000)
        ShellService->>ShellService: stripAnsiSequences() & extractUrlsFromText()
        opt Auth URL Detected (e.g. claude login)
            ShellService->>WS: emit { type: "auth_url", url, autoOpen }
        end
        ShellService->>WS: emit { type: "output", data: chunk }
        WS->>User: xterm.js renders ANSI terminal
    end

    User->>WS: send { type: "input", data: keystrokes / paste }
    WS->>PTY: write(data)

    User->>WS: Socket Closes (Page navigation / network drop)
    ShellService->>ShellService: Detach ws from session; arm 30min timeout timer
    alt User Reconnects within 30 mins
        User->>WS: Re-init with same session key
        ShellService->>ShellService: Cancel timeout timer; re-attach socket
    else Timeout Expires (30 mins)
        ShellService->>PTY: kill()
        ShellService->>ShellService: delete from ptySessionsMap
    end
```

---

## 3. Core Tech Stack & Dependencies

```
+----------------------------------------------------------------------------------------+
|                                    Core Tech Stack                                     |
+--------------------------+----------------------------+--------------------------------+
| Layer                    | Primary Technologies       | Key Packages & Libraries       |
+--------------------------+----------------------------+--------------------------------+
| Frontend Framework       | React 18.2, Vite 7.0       | react, react-dom, react-router |
| Styling & UI Components  | Tailwind CSS 3.4           | @tailwindcss/typography, clsx  |
| Editor & Diff Viewer     | CodeMirror 6               | @codemirror/merge, @uiw/react  |
| Terminal Subsystem       | xterm.js 5.5               | @xterm/addon-fit, webgl, osc52 |
| Backend Server           | Node.js (ES2022), Express  | express 4.18, cors, multer     |
| Realtime Transport       | WebSockets (ws 8.14) & SSE | ws, express event streaming    |
| Database Engine          | SQLite 3                   | better-sqlite3 12.6            |
| Process & PTY Manager    | node-pty, cross-spawn      | node-pty 1.2-beta.12           |
| Multi-Agent Runtimes     | Anthropic & OpenAI SDKs    | @anthropic-ai/claude-agent-sdk |
|                          |                            | @openai/codex-sdk              |
| Internationalization     | i18next                    | i18next 25.7, react-i18next    |
| Desktop Packaging        | Electron 38                | electron-builder 26.15         |
+--------------------------+----------------------------+--------------------------------+
```

### Critical Dependencies Breakdown
- [`@anthropic-ai/claude-agent-sdk`](file:///c:/Users/W/Documents/GitHub/OpenRemote/ref/claudecodeui/package.json#L141): Directly queries Claude Code subprocesses via SDK streaming APIs without raw CLI parsing.
- [`@openai/codex-sdk`](file:///c:/Users/W/Documents/GitHub/OpenRemote/ref/claudecodeui/package.json#L152): Orchestrates Codex reasoning and coding pipelines.
- [`node-pty`](file:///c:/Users/W/Documents/GitHub/OpenRemote/ref/claudecodeui/package.json#L185): Native pseudoterminal binding powering full ANSI terminal emulation on Windows (`winpty`/`conpty`), macOS, and Linux.
- [`better-sqlite3`](file:///c:/Users/W/Documents/GitHub/OpenRemote/ref/claudecodeui/package.json#L163): Synchronous, ultra-fast C++ SQLite engine managing user accounts, API keys, session metadata, MCP servers, and push subscriptions.
- [`@codemirror/merge`](file:///c:/Users/W/Documents/GitHub/OpenRemote/ref/claudecodeui/package.json#L148) & [`@uiw/react-codemirror`](file:///c:/Users/W/Documents/GitHub/OpenRemote/ref/claudecodeui/package.json#L155): In-browser code editing, syntax highlighting, and two-way merge/diff visualization.
- [`@xterm/xterm`](file:///c:/Users/W/Documents/GitHub/OpenRemote/ref/claudecodeui/package.json#L161): Industrial-grade canvas and WebGL terminal renderer.

---

## 4. Distinctive & Smart Engineering Decisions

### 4.1 Chat vs. Terminal Parallel View Preservation
In [`MainContent.tsx`](file:///c:/Users/W/Documents/GitHub/OpenRemote/ref/claudecodeui/src/components/main-content/view/MainContent.tsx#L159-L232), switching between the `Chat` view and the `Shell` (terminal) tab does **not** unmount or destroy the underlying React component tree. The chat pane is preserved in the DOM using CSS visibility classes:
```tsx
<div className={`h-full ${activeTab === 'chat' ? 'block' : 'hidden'}`}>
  <ErrorBoundary showDetails>
    <ChatInterface isActive={activeTab === 'chat'} ... />
  </ErrorBoundary>
</div>
```
This guarantees that when developers switch between viewing tool execution cards and testing terminal commands, active text selection, scroll offsets, form inputs, and pending tool approvals remain intact without state loss.

### 4.2 Held Prompt Async Generator for Long-Running Agent Work
Standard CLI tool runs terminate their process as soon as stdout/stdin closes. However, Claude Code agents frequently spawn background commands (`Bash` with `run_in_background: true`, `Monitor`, `ScheduleWakeup`). If the parent SDK closes stdin upon receiving the turn's `result`, the CLI kills all child background jobs.

[`claude-runtime.provider.js`](file:///c:/Users/W/Documents/GitHub/OpenRemote/ref/claudecodeui/server/modules/providers/list/claude/claude-runtime.provider.js#L477-L501) solves this with `createHeldPromptStream`:
```javascript
function createHeldPromptStream(messages) {
  let release;
  const held = new Promise((resolve) => { release = resolve; });

  const stream = (async function* () {
    for (const message of messages) {
      yield message;
    }
    // Keeps stdin open — the CLI stays alive until release() is called.
    await held;
  })();

  return { stream, release };
}
```
When background work is detected (`startsBackgroundWork(message)`), the runtime defers `release()` and starts an idle backstop timer (`BG_WAIT_CEILING_MS = 30 * 60 * 1000`). If the background task finishes and pushes a follow-up turn, the timer resets; when work concludes, `release()` is invoked.

### 4.3 Compact Inline Diff Visualizer (`ToolDiffViewer`)
Rather than forcing users to open full-screen editors for minor file modifications, [`ToolDiffViewer.tsx`](file:///c:/Users/W/Documents/GitHub/OpenRemote/ref/claudecodeui/src/components/chat/tools/components/ToolDiffViewer.tsx#L22-L93) provides a compact, VS Code-inspired inline diff widget directly inside the chat stream:
- Compares `oldContent` and `newContent` line-by-line.
- Visualizes additions (green `+`) and removals (red `-`) with line-wrapping support.
- Provides a clickable filepath header that opens the file in the split-pane CodeEditor.

### 4.4 Automated Terminal URL & OAuth Flow Detection
When CLIs initiate browser-based login flows (e.g. `claude auth login`, `cursor-agent login`), users in headless or remote server setups cannot click local localhost URLs.
In [`shell-websocket.service.ts`](file:///c:/Users/W/Documents/GitHub/OpenRemote/ref/claudecodeui/server/modules/websocket/services/shell-websocket.service.ts#L60-L90), terminal chunks pass through an ANSI stripper and URL extractor:
- Strips ANSI escape sequences via regex.
- Joins multi-line wrapped URL strings across column breaks.
- Detects phrases like `"press enter to open"` or `"open this url"` and emits a structured `{ type: 'auth_url', url, autoOpen }` event to the web client, which triggers a clickable toast notification.

### 4.5 Hierarchical Tool Call Grouping (`SubagentContainer`)
When an agent invokes subagents or chains nested tool calls (e.g., executing multiple Bash commands inside a sub-task), [`SubagentContainer.tsx`](file:///c:/Users/W/Documents/GitHub/OpenRemote/ref/claudecodeui/src/components/chat/tools/components/SubagentContainer.tsx) uses `parentToolUseId` tracking from the SDK stream to render nested collapsible tree containers, preventing visual clutter in the main transcript.

### 4.6 Multi-Language Internationalization (i18n)
The application provides out-of-the-box localization across **11 languages** ([`src/i18n/locales/`](file:///c:/Users/W/Documents/GitHub/OpenRemote/ref/claudecodeui/src/i18n/locales)): German (`de`), English (`en`), Spanish (`es`), French (`fr`), Italian (`it`), Japanese (`ja`), Korean (`ko`), Russian (`ru`), Turkish (`tr`), Simplified Chinese (`zh-CN`), and Traditional Chinese (`zh-TW`). Language detection occurs automatically via `i18next-browser-languagedetector` with local storage persistence.

---

## 5. Process Lifecycle & Terminal/PTY Management

### 5.1 Pseudoterminal (PTY) Architecture
The standalone terminal subsystem is implemented in [`shell-websocket.service.ts`](file:///c:/Users/W/Documents/GitHub/OpenRemote/ref/claudecodeui/server/modules/websocket/services/shell-websocket.service.ts).

```
+-------------------+                    +-----------------------+
|  xterm.js Client  | <=== WebSocket ===> |  shell-websocket.ts   |
| (React Component) |      (/shell)       |  - Ring Buffer (5000) |
+-------------------+                     |  - 30min Disconnect   |
                                          +-----------+-----------+
                                                      |
                                     node-pty.spawn() | (PowerShell / Bash)
                                                      v
                                          +-----------------------+
                                          |  Host Shell Process   |
                                          |  (claude / cursor /   |
                                          |   codex / bash)       |
                                          +-----------------------+
```

1. **Process Spawning**:
   - On Windows: Spawns `powershell.exe -Command <command>`.
   - On Linux/macOS: Spawns `bash -c <command>`.
   - Injects normalized terminal environment: `TERM=xterm-256color`, `COLORTERM=truecolor`, `FORCE_COLOR=3`, and prioritizes user global npm bin paths.
2. **Session Key Resolution**:
   Sessions are keyed by:
   $$\text{SessionKey} = \text{projectPath} + \text{"\_"} + (\text{sessionId} \lor \text{"default"}) + \text{commandSuffix}$$
3. **Ring Buffer & Screen Reconstruction**:
   Each PTY session maintains a memory buffer of the latest 5,000 output chunks (`session.buffer`). When a browser tab reconnects or reloads, the server replays the entire buffer before attaching the live stream, fully restoring the terminal state.
4. **Graceful Disconnect Retention**:
   When a WebSocket disconnects, the server does **not** terminate the process immediately. It arms a 30-minute timeout (`PTY_SESSION_TIMEOUT = 30 * 60 * 1000`). If a client reconnects within that window, the timer is cleared and the socket reattached; otherwise, `session.pty.kill()` executes.
5. **OSC 52 Clipboard Synchronization**:
   In [`useShellTerminal.ts`](file:///c:/Users/W/Documents/GitHub/OpenRemote/ref/claudecodeui/src/components/shell/hooks/useShellTerminal.ts#L32-L49), an `IClipboardProvider` handles OSC 52 ANSI sequences emitted by CLIs (such as device-login copy commands), bridging terminal copy commands to `navigator.clipboard` even across non-standard remote bindings.

---

## 6. Communication & Protocol

The system employs a hybrid transport architecture: **HTTP REST** for static assets and transactional CRUD operations, and **WebSockets** for high-frequency bidirectionally streamed telemetry.

### 6.1 WebSocket Channel Specifications

| Path | Purpose | Protocol Format | Lifecycle / Heartbeat |
| :--- | :--- | :--- | :--- |
| `/ws` | Chat events, tool approvals, text streaming | JSON envelopes (`kind`-based) | 30s ping/pong heartbeat (`attachWebSocketHeartbeat`) |
| `/shell` | Terminal PTY I/O & resize events | JSON envelopes (`init`, `input`, `resize`, `output`, `auth_url`) | Socket-level ping/pong |
| `/desktop-notifications` | Native desktop alert streaming | JSON notification envelopes | Managed by Electron preload bridge |
| `/plugin-ws/:pluginName` | Proxied WebSocket connection to plugin subprocesses | Raw binary / JSON passthrough | Subprocess port reverse proxy |

---

### 6.2 Chat Message Wire Format (`NormalizedMessage`)

Every frame emitted over `/ws` conforms to the `NormalizedMessage` envelope defined in [`server/shared/types.ts`](file:///c:/Users/W/Documents/GitHub/OpenRemote/ref/claudecodeui/server/shared/types.ts#L224-L282):

```typescript
export type NormalizedMessage = {
  id: string;                      // Unique message ID
  sessionId: string;               // Application-level session ID
  timestamp: string;               // ISO 8601 timestamp
  provider: 'claude' | 'cursor' | 'codex' | 'opencode';
  kind: MessageKind;               // 'stream_delta' | 'tool_use' | 'tool_result' | 
                                   // 'permission_request' | 'complete' | 'error' | ...
  seq?: number;                    // Monotonic run sequence number (for reconnect replay)
  role?: 'user' | 'assistant';
  content?: string;
  toolName?: string;
  toolInput?: unknown;
  toolResult?: { content?: string; isError?: boolean };
  requestId?: string;              // Used for interactive permission approvals
  tokenBudget?: {
    used: number;
    total: number;
    inputTokens: number;
    outputTokens: number;
  };
  parentToolUseId?: string;        // Subagent grouping hierarchy
};
```

---

### 6.3 REST API Endpoints Overview

| Route Prefix | Module | Authentication | Key Endpoints |
| :--- | :--- | :--- | :--- |
| `/api/auth` | [`auth`](file:///c:/Users/W/Documents/GitHub/OpenRemote/ref/claudecodeui/server/modules/auth/) | Public | `POST /login`, `POST /register`, `GET /status`, `POST /logout` |
| `/api/providers` | [`providers`](file:///c:/Users/W/Documents/GitHub/OpenRemote/ref/claudecodeui/server/modules/providers/) | Bearer JWT | `GET /sessions`, `POST /sessions`, `GET /models`, `GET /mcp` |
| `/api/file-tree` | [`file-tree`](file:///c:/Users/W/Documents/GitHub/OpenRemote/ref/claudecodeui/server/modules/file-tree/) | Bearer JWT | `GET /`, `POST /read`, `POST /write`, `POST /search` (Ripgrep) |
| `/api/git` | [`git`](file:///c:/Users/W/Documents/GitHub/OpenRemote/ref/claudecodeui/server/modules/git/) | Bearer JWT | `GET /status`, `POST /commit`, `POST /branch`, `GET /diff` |
| `/api/worktrees` | [`worktrees`](file:///c:/Users/W/Documents/GitHub/OpenRemote/ref/claudecodeui/server/modules/worktrees/) | Bearer JWT | `GET /`, `POST /create`, `POST /merge`, `POST /remove` |
| `/api/assets` | [`assets`](file:///c:/Users/W/Documents/GitHub/OpenRemote/ref/claudecodeui/server/modules/assets/) | Bearer JWT | `POST /images` (Uploads to `~/.cloudcli/assets`), `GET /:id` |
| `/api/browser-use`| [`browser-use`](file:///c:/Users/W/Documents/GitHub/OpenRemote/ref/claudecodeui/server/modules/browser-use/) | Bearer JWT | `GET /sessions`, `POST /action`, `GET /screenshot` |

---

## 7. Reliability, Fault Tolerance & Edge Cases

### 7.1 Live Stream Reconnection via Monotonic Sequence Numbers
When a client experiences temporary network disconnection during a long AI generation, [`chat-run-registry.service.ts`](file:///c:/Users/W/Documents/GitHub/OpenRemote/ref/claudecodeui/server/modules/websocket/services/chat-run-registry.service.ts) ensures zero lost events:
1. As each event is emitted to the WebSocket, `chatRunRegistry` assigns a monotonically increasing `seq` integer and stores the event in an in-memory run buffer.
2. Upon reconnecting, the client sends a `chat.subscribe` message with `{ sessions: [{ sessionId, lastSeq: 42 }] }`.
3. The server immediately replays all buffered events where $\text{seq} > \text{lastSeq}$, seamlessly continuing the stream.

### 7.2 Run Superseding & Zombie Prevention
If a user submits a new prompt on an existing session while an earlier generation or background hold is still running:
- [`claude-runtime.provider.js`](file:///c:/Users/W/Documents/GitHub/OpenRemote/ref/claudecodeui/server/modules/providers/list/claude/claude-runtime.provider.js#L279-L290) flags the old SDK query instance in a `supersededInstances` `WeakSet`.
- Calls `instance.interrupt()` on the old instance and invokes its `releaseInput()` to release the stdin hold.
- When the old generator unwinds in its `finally` block, it detects its membership in `supersededInstances` and remains silent, suppressing redundant `complete` or `error` events to the frontend.

### 7.3 Windows Shell Newline Truncation Protection
On Windows, executing CLI shims (like `cursor-agent.cmd` or `opencode.cmd`) via `cmd.exe` causes commands to truncate at the first newline character.
[`server/shared/utils.ts`](file:///c:/Users/W/Documents/GitHub/OpenRemote/ref/claudecodeui/server/shared/utils.ts) provides `flattenPromptForWindowsShell`, which normalizes multi-line prompts and attachment paths into safe single-line argument strings before process dispatch.

---

## 8. Security & Access Control

### 8.1 JWT Authentication & Half-Life Token Rotation
Authentication is enforced by [`auth.middleware.ts`](file:///c:/Users/W/Documents/GitHub/OpenRemote/ref/claudecodeui/server/modules/auth/auth.middleware.ts):
- Standard JWT signed with a secret persisted in `~/.cloudcli/auth.db` (`app_config` table).
- Tokens expire in 7 days (`expiresIn: '7d'`).
- **Auto-Rotation**: On every authenticated HTTP request, if the token has passed half its lifespan ($\text{now} > \text{iat} + \frac{\text{exp} - \text{iat}}{2}$), the server automatically generates a fresh token and returns it in the `X-Refreshed-Token` response header, preventing session timeouts during active development.

### 8.2 Attachment Path Traversal Protection
Because provider runtimes read attached image files directly from disk, client-supplied filepaths represent a potential local file inclusion (LFI) vulnerability.
[`chat-websocket.service.ts`](file:///c:/Users/W/Documents/GitHub/OpenRemote/ref/claudecodeui/server/modules/websocket/services/chat-websocket.service.ts#L34-L56) enforces a strict trust boundary:
```typescript
export function filterAttachmentsToUploadStore(attachments: unknown): ChatAttachmentDescriptor[] {
  const assetsRoot = path.resolve(getGlobalImageAssetsDir()); // ~/.cloudcli/assets
  return normalizeAttachmentDescriptors(attachments).filter((descriptor) => {
    const resolved = path.resolve(assetsRoot, descriptor.path);
    const relative = path.relative(assetsRoot, resolved);
    return (
      relative.length > 0 &&
      !relative.startsWith('..') &&
      !path.isAbsolute(relative) &&
      !relative.includes(path.sep) &&
      !relative.includes('/')
    );
  });
}
```
Any attachment referencing directories outside `~/.cloudcli/assets` or containing path traversal syntax (`../`) is stripped before reaching agent tools.

### 8.3 Subprocess Environment Variable Sanitization
When launching user plugins ([`plugin-process.service.ts`](file:///c:/Users/W/Documents/GitHub/OpenRemote/ref/claudecodeui/server/modules/plugins/plugin-process.service.ts#L25-L47)), the host process explicitly avoids leaking its full environment (which may contain `JWT_SECRET`, database passwords, or API keys). It constructs a minimal sanitized environment whitelist (`PATH`, `HOME`, `NODE_ENV`, and essential Windows bootstrap variables like `SystemRoot` and `APPDATA`).

---

## 9. Flaws, Antipatterns & Gotchas

> [!WARNING]
> ### 1. Startup Logic Bug in `server/index.ts`
> In [`server/index.ts`](file:///c:/Users/W/Documents/GitHub/OpenRemote/ref/claudecodeui/server/index.ts#L372):
> ```typescript
> server.listen(SERVER_PORT, HOST, async () => {
>   // ... async callback runs after listen binds
>   await initializeSessionsWatcher();
> });
> await closeSessionsWatcher(); // <-- CRITICAL BUG
> ```
> `await closeSessionsWatcher();` is placed immediately after `server.listen(...)` inside the synchronous `startServer()` body rather than inside a shutdown handler. When `startServer()` executes, it starts listening, immediately executes `closeSessionsWatcher()` (closing any watcher previously established), and then registers signal listeners. The file watcher is later created inside the `listen` callback, but this code placement is confusing and error-prone.

> [!CAUTION]
> ### 2. Unbounded Memory Usage in PTY Ring Buffers
> In [`shell-websocket.service.ts`](file:///c:/Users/W/Documents/GitHub/OpenRemote/ref/claudecodeui/server/modules/websocket/services/shell-websocket.service.ts#L435-L440), each PTY session stores up to 5,000 raw string chunks:
> ```typescript
> if (session.buffer.length < 5000) {
>   session.buffer.push(chunk);
> } else {
>   session.buffer.shift();
>   session.buffer.push(chunk);
> }
> ```
> If a process emits large bursts of output (e.g. `cat large_file.json` or massive build logs emitting 64KB chunks), a single session buffer can consume upwards of 300MB of RAM. The buffer should enforce a maximum **byte length** rather than an arbitrary chunk count.

> [!IMPORTANT]
> ### 3. Single-Node In-Memory State
> All active session registries (`activeSessions`, `pendingToolApprovals`, `ptySessionsMap`, `chatRunRegistry`) are stored in local Node.js `Map` and `Set` instances. Running multiple instances of the backend behind a load balancer without sticky sessions will result in broken WebSocket streams, dropped tool approvals, and orphaned PTY processes.

> [!NOTE]
> ### 4. Native Dependency (`node-pty`) Build Fragility
> Because `node-pty` compiles native C++ bindings for OS pseudoterminals, installation on developer machines frequently fails if Python, Visual Studio C++ Build Tools (Windows), or Xcode command line tools (macOS) are missing. The repo includes a custom workaround script ([`scripts/fix-node-pty.js`](file:///c:/Users/W/Documents/GitHub/OpenRemote/ref/claudecodeui/scripts/fix-node-pty.js)) to patch prebuilt binaries.

---

## 10. Actionable Lessons & Takeaways for OpenRemote

1. **Adopt Dual Interaction Surfaces (Chat + Terminal)**:
   Developers love conversational AI interfaces for complex task planning, but need raw terminal access when commands require manual intervention, curses interfaces, or interactive authentication. OpenRemote should maintain parallel Chat and PTY tabs, preserving component DOM state with CSS visibility toggling rather than unmounting components.
2. **Implement Async Prompt Streams for Agent Background Tasks**:
   When building agent execution runners, adopt the `createHeldPromptStream` pattern to keep the agent subprocess stdin stream alive. This allows background tools (`run_in_background`, schedulers, daemons) to continue running after a turn finishes and push asynchronous notifications back to the user.
3. **Use Monotonic Sequence Numbers for Resilient Telemetry**:
   Implement a `seq` counter in agent event streams. When clients reconnect after mobile standby or network dropouts, they can request `subscribe({ lastSeq })`, eliminating dropped messages and duplicate rendering.
4. **Deploy Compact Inline Diff Cards**:
   The `ToolDiffViewer` pattern (compact, line-by-line diff rendering inside the chat bubble with a direct link to the full editor) provides superior UX compared to full editor modals for standard file edits.
5. **Secure Local File Attachments with Strict Upload Store Boundaries**:
   Always sanitize and isolate uploaded chat attachments into a dedicated storage root (`~/.openremote/assets`), blocking relative path traversal (`..`) and absolute path escaping before passing paths to agent tools.

---

## 11. Key Code File Index

| Module / Area | File Path | Key Functions & Components | Description |
| :--- | :--- | :--- | :--- |
| **Server Entry** | [`server/index.ts`](file:///c:/Users/W/Documents/GitHub/OpenRemote/ref/claudecodeui/server/index.ts) | `startServer`, `createWebSocketServer` | Backend composition root, Express configuration, WebSocket routing, and shutdown handlers |
| **WebSocket Gateway** | [`server/modules/websocket/services/websocket-server.service.ts`](file:///c:/Users/W/Documents/GitHub/OpenRemote/ref/claudecodeui/server/modules/websocket/services/websocket-server.service.ts) | `createWebSocketServer`, `attachWebSocketHeartbeat` | Unified WebSocket router dispatching `/ws`, `/shell`, and `/plugin-ws/` |
| **Chat WebSocket** | [`server/modules/websocket/services/chat-websocket.service.ts`](file:///c:/Users/W/Documents/GitHub/OpenRemote/ref/claudecodeui/server/modules/websocket/services/chat-websocket.service.ts) | `handleChatConnection`, `handleChatSend`, `handleChatSubscribe` | Chat command dispatcher, session validation, attachment filtering, and permission response routing |
| **Run Registry** | [`server/modules/websocket/services/chat-run-registry.service.ts`](file:///c:/Users/W/Documents/GitHub/OpenRemote/ref/claudecodeui/server/modules/websocket/services/chat-run-registry.service.ts) | `startRun`, `replayEvents`, `completeRun` | Active run tracking, monotonic `seq` assignment, and event replay buffer management |
| **Shell PTY** | [`server/modules/websocket/services/shell-websocket.service.ts`](file:///c:/Users/W/Documents/GitHub/OpenRemote/ref/claudecodeui/server/modules/websocket/services/shell-websocket.service.ts) | `handleShellConnection`, `buildShellCommand`, `extractUrlsFromText` | `node-pty` lifecycle, 30m disconnect retention, ANSI stripping, and login URL auto-detection |
| **Claude Runtime** | [`server/modules/providers/list/claude/claude-runtime.provider.js`](file:///c:/Users/W/Documents/GitHub/OpenRemote/ref/claudecodeui/server/modules/providers/list/claude/claude-runtime.provider.js) | `queryClaudeSDK`, `createHeldPromptStream`, `waitForToolApproval` | Claude Agent SDK integration, interactive tool approval hooks, token budget extraction, and background hold |
| **Cursor Runtime** | [`server/modules/providers/list/cursor/cursor-runtime.provider.js`](file:///c:/Users/W/Documents/GitHub/OpenRemote/ref/claudecodeui/server/modules/providers/list/cursor/cursor-runtime.provider.js) | `spawnCursor`, `abortCursorSession` | `cursor-agent` CLI subprocess execution, stream-json parsing, and workspace trust auto-retry |
| **Codex Runtime** | [`server/modules/providers/list/codex/codex-runtime.provider.js`](file:///c:/Users/W/Documents/GitHub/OpenRemote/ref/claudecodeui/server/modules/providers/list/codex/codex-runtime.provider.js) | `codexRuntime.run`, `extractCodexTokenBudget` | OpenAI Codex SDK integration and message transformation |
| **OpenCode Runtime**| [`server/modules/providers/list/opencode/opencode-runtime.provider.js`](file:///c:/Users/W/Documents/GitHub/OpenRemote/ref/claudecodeui/server/modules/providers/list/opencode/opencode-runtime.provider.js) | `resolveOpenCodePermissionOptions`, `runOpenCode` | OpenCode CLI subprocess management and permission argument mapping |
| **Database Schema** | [`server/modules/database/schema.ts`](file:///c:/Users/W/Documents/GitHub/OpenRemote/ref/claudecodeui/server/modules/database/schema.ts) | `INIT_SCHEMA_SQL`, `SESSIONS_TABLE_SCHEMA_SQL` | SQLite table definitions for users, API keys, credentials, projects, and sessions |
| **Database Conn** | [`server/modules/database/connection.ts`](file:///c:/Users/W/Documents/GitHub/OpenRemote/ref/claudecodeui/server/modules/database/connection.ts) | `getConnection`, `migrateLegacyDatabase` | SQLite singleton connection manager and legacy `auth.db` migration |
| **Auth Middleware** | [`server/modules/auth/auth.middleware.ts`](file:///c:/Users/W/Documents/GitHub/OpenRemote/ref/claudecodeui/server/modules/auth/auth.middleware.ts) | `authenticateToken`, `validateApiKey`, `authenticateWebSocket` | JWT verification, auto-token refresh header emission, and API key validation |
| **Browser Use** | [`server/modules/browser-use/browser-use.service.ts`](file:///c:/Users/W/Documents/GitHub/OpenRemote/ref/claudecodeui/server/modules/browser-use/browser-use.service.ts) | `BrowserUseService`, `takeScreenshot` | Headless Playwright browser automation and live viewport streaming |
| **UI Main Layout** | [`src/components/main-content/view/MainContent.tsx`](file:///c:/Users/W/Documents/GitHub/OpenRemote/ref/claudecodeui/src/components/main-content/view/MainContent.tsx) | `MainContent` | Root workspace layout managing Chat, Files, Shell, Git, Tasks, and Browser tabs |
| **Chat Interface** | [`src/components/chat/view/ChatInterface.tsx`](file:///c:/Users/W/Documents/GitHub/OpenRemote/ref/claudecodeui/src/components/chat/view/ChatInterface.tsx) | `ChatInterface` | Main conversational UI handling message list, composer, and realtime event subscriptions |
| **Diff Viewer** | [`src/components/chat/tools/components/ToolDiffViewer.tsx`](file:///c:/Users/W/Documents/GitHub/OpenRemote/ref/claudecodeui/src/components/chat/tools/components/ToolDiffViewer.tsx) | `ToolDiffViewer` | Compact inline VS Code-style diff renderer for file changes |
| **Subagent Box** | [`src/components/chat/tools/components/SubagentContainer.tsx`](file:///c:/Users/W/Documents/GitHub/OpenRemote/ref/claudecodeui/src/components/chat/tools/components/SubagentContainer.tsx) | `SubagentContainer` | Collapsible tree view container for nested subagent tool executions |
| **Question Modal** | [`src/components/chat/tools/components/InteractiveRenderers/AskUserQuestionPanel.tsx`](file:///c:/Users/W/Documents/GitHub/OpenRemote/ref/claudecodeui/src/components/chat/tools/components/InteractiveRenderers/AskUserQuestionPanel.tsx) | `AskUserQuestionPanel` | Interactive modal prompting users for required inputs and approvals |
| **Terminal Hook** | [`src/components/shell/hooks/useShellTerminal.ts`](file:///c:/Users/W/Documents/GitHub/OpenRemote/ref/claudecodeui/src/components/shell/hooks/useShellTerminal.ts) | `useShellTerminal`, `oscClipboardProvider` | `xterm.js` initialization, WebGL addon loading, mobile selection manager, and OSC 52 copy provider |
| **Editor Sidebar** | [`src/components/code-editor/view/EditorSidebar.tsx`](file:///c:/Users/W/Documents/GitHub/OpenRemote/ref/claudecodeui/src/components/code-editor/view/EditorSidebar.tsx) | `EditorSidebar` | Resizable split-pane CodeMirror 6 code editor and two-way merge diff viewer |
