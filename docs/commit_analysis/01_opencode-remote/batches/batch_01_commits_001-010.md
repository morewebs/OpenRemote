# opencode-remote: Batch 01 (Commits 1-10)

## 1. Commit Log & Scope
- **Commit Range**: `c9880c4f` -> `bfa5745d` (10 commits)
- **Author**: `youaodu <youao.du@gmail.com>`
- **Date Range**: 2026-02-25 11:46:28 -> 2026-02-25 15:52:40

| Hash | Date | Subject | Files Touched | Key Changes |
| :--- | :--- | :--- | :--- | :--- |
| `c9880c4f` | 2026-02-25 | `Initial commit` | 21 files (+11,241) | React Native + Expo bootstrap, `useAppController.ts`, `ChatScreen.tsx`, `chatApi.ts` |
| `93bd7b30` | 2026-02-25 | `Add tag-triggered APK release workflow` | `.github/workflows/build-apk.yml` (+63) | GitHub Actions CI for debug/release APK |
| `223b39a8` | 2026-02-25 | `Add in-chat handling for permission and question prompts` | `i18n.ts`, `useAppController.ts`, `ChatScreen.tsx`, `types/chat.ts` (+941) | EventSource listeners for `permission.asked`, `question.asked`; interactive UI sheets |
| `2db164a7` | 2026-02-25 | `Improve tool event rendering in chat stream` | `i18n.ts`, `useAppController.ts` (+129) | Inline tool execution cards, truncation at 24 lines / 1200 chars, `read` tool special handling |
| `1951c86a` | 2026-02-25 | `Unify top header height across screens` | `HomeScreen.tsx`, `ProjectsScreen.tsx`, `SettingsScreen.tsx` | Layout alignment across mobile screens |
| `e44fec93` | 2026-02-25 | `Align chat screen with app header pattern` | `useAppController.ts`, `ChatScreen.tsx` | Removed default welcome message clutter; added top bar |
| `4890f567` | 2026-02-25 | `Update app config for cleartext endpoint support` | `app.json` | Android cleartext network traffic enabled for local LAN dev (`http://192.168.x.x:4096`) |
| `ddb28609` | 2026-02-25 | `Remove debug APK from release workflow` | `.github/workflows/build-apk.yml` | Optimized release artifacts |
| `06c69700` | 2026-02-25 | `Clarify HTTPS requirement for mobile deployment` | `README.md` | Mobile OS network security policies note (HTTPS / tunnel requirement) |
| `bfa5745d` | 2026-02-25 | `Add project maturity and issue reporting note` | `README.md` | Issue template notes |

---

## 2. Evolutionary Milestones & Architectural Intent
1. **Direct OpenCode Daemon Bridge**:
   - Directly connects React Native mobile client to `opencode serve --hostname 0.0.0.0 --port 4096`.
   - Uses Server-Sent Events (SSE) `/session/:sessionID/event` for reactive streaming and REST `/session/:sessionID/prompt_async` for non-blocking prompt dispatch.
2. **Human-in-the-Loop Interception**:
   - `permission.asked`: Traps file writes and command executions outside project boundaries. Renders interactive buttons (*Allow Once*, *Always Allow*, *Reject with Reason*).
   - `question.asked`: Traps disambiguation choices from agents. Renders multi-choice option lists or custom text inputs and submits structured answers.
3. **In-Flight Tool Visualizer**:
   - Tracks incremental tool updates (`tool-part`) mapped by `updateKey = partId || callId || \`${messageId}:${toolName}\``.
   - Updates tool cards in-place with status (`running`, `completed`, `error`), input payload, and execution output.

---

## 3. Crucial Bug Fixes & Edge Cases Uncovered
- **Mojibake UTF-8 Splitting**:
  - SSE chunks over TCP often split multi-byte UTF-8 sequences (especially CJK Chinese/Japanese characters). Implemented `decodePossiblyMojibakeText` to safely decode streamed chunks without question mark corruptions.
- **Markdown Breakout in Tool Output**:
  - Tool outputs often contain triple backticks (```), breaking React Native markdown renderers. Sanitized by replacing ```` ``` ```` with ```` ` ` ` ````.
- **Android Cleartext Policy**:
  - Android 9+ blocks `http://` network requests by default. Updated `app.json` Android manifest config to permit cleartext traffic for local development.

---

## 4. Golden Code Patterns
```typescript
// Tool message keying pattern for in-place streaming updates:
const updateKey = partId || callId || `${messageId}:${toolName}`;
const mappedMessageId = toolMessageIdByUpdateKey.get(updateKey);
if (mappedMessageId) {
  setMessages((prev) =>
    prev.map((item) => (item.id === mappedMessageId ? { ...item, content } : item))
  );
} else {
  const toolMessageId = partId || makeId('tool');
  appendMessage({ id: toolMessageId, role: 'system', content });
  toolMessageIdByUpdateKey.set(updateKey, toolMessageId);
}
```

---

## 5. Synthesis & Action Items for OpenRemote
- **Adopt In-Place Tool Card State Machine**: Ensure OpenRemote's Web and Mobile clients track tool calls with composite key `call_id || tool_name:seq` so sub-steps update in-place without flooding message logs.
- **Implement Structured Permission & Question Endpoints**: Map OpenRemote's Go parser directly to OpenCode-compatible `permission.asked` and `question.asked` JSON schemas.
