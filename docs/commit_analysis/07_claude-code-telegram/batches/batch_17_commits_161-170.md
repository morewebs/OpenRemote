# claude-code-telegram (Enterprise Forum Topics Hub): Batch 17 (Commits 161-170)

## 1. Commit Log & Scope
- **Commit Range**: `39e40875` -> `5e3c234b` (10 commits)
- **Batch Window**: Commits 161 to 170

| Hash | Date | Subject | Author |
| :--- | :--- | :--- | :--- |
| `39e40875` | 2026-02-23 | `Add max_budget_usd per-request cost cap to SDK options` | Richard A |
| `85cc4c30` | 2026-02-23 | `Add outbound image support: auto-detect and send images to Telegram` | Guillaume Gay |
| `bf23e4e5` | 2026-02-23 | `Add MCP send_image_to_user tool for outbound image delivery` | Guillaume Gay |
| `89844d55` | 2026-02-25 | `Fix bot stopping mid-response when progress message deletion fails` | Guillaume Gay |
| `3ae7d8ae` | 2026-02-24 | `Add voice message support via Mistral API (Voxtral)` | Guillaume Gay |
| `22c88bed` | 2026-02-25 | `Add OpenAI Whisper as alternative voice transcription provider` | Guillaume Gay |
| `ddecc0ed` | 2026-02-25 | `fix(orchestrator): route General topic messages in forum supergroups` | F1orian |
| `26254e64` | 2026-02-25 | `Fix CI lint failure and add voice transcription documentation` | Guillaume Gay |
| `e00a36eb` | 2026-02-25 | `feat(config): add REPLY_QUOTE setting to control message quoting` | F1orian |
| `5e3c234b` | 2026-02-25 | `fix(reply-quote): add do_quote parameter to all reply_text calls` | F1orian |

---

## 2. Evolutionary Milestones & Architectural Intent
Iterative feature enhancements, telemetry refinements, and engine scaling.

---

## 3. Crucial Bug Fixes & Edge Cases Uncovered
Protocol edge cases, concurrency locks, and stream buffer optimizations.

---

## 4. Key Architectural Patterns
- **Protocol & Stream State Management**:
  - Implemented event bus dispatch and debounced stream transport.
  - Handled message queueing and failure recovery gracefully.

---

## 5. Synthesis & Action Items for OpenRemote
Incorporate resilient event streams and multi-surface routing into OpenRemote.
