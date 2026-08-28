# claudecodeui (Multi-Agent Web IDE & Shell): Batch 64 (Commits 631-640)

## 1. Commit Log & Scope
- **Commit Range**: `d9e9df18` -> `ef2fd48b` (10 commits)
- **Batch Window**: Commits 631 to 640

| Hash | Date | Subject | Author |
| :--- | :--- | :--- | :--- |
| `d9e9df18` | 2026-06-02 | `fix: plugin svg icon sanitization (#817)` | Haile |
| `f082cdc6` | 2026-06-04 | `fix(websocket): reset unmountedRef on each effect re-run so token refresh reconnects (#721)` | Peter Buchegger |
| `96b16b42` | 2026-06-04 | `fix(vite): proxy /plugin-ws WebSocket requests to the backend in dev (#757)` | ehsanmim |
| `2edfef2e` | 2026-06-04 | `fix(websocket): add 30s server-side heartbeat to prevent proxy idle disconnects (#770)` | Vojtech |
| `fa9eaf55` | 2026-06-04 | `feat(chat): auto-detect text direction for RTL languages (#729)` | Reza Moghaddam |
| `c667b6a1` | 2026-06-04 | `Update model version in OPTIONS description` | Simos Mikelatos |
| `9e608b84` | 2026-06-05 | `Fixes/minor fixes (#832)` | Haile |
| `94785bfa` | 2026-06-05 | `chore: update Claude fallback models` | Haileyesus |
| `cdcac182` | 2026-06-05 | `fix: load claude models directly from provider` | Haileyesus |
| `ef2fd48b` | 2026-06-05 | `fix(shell): disconnect and restart buttons (#831)` | Haile |

---

## 2. Evolutionary Milestones & Architectural Intent
Progressive scaling of agent execution pipelines, terminal rendering optimizations, and mobile touch adaptations.

---

## 3. Crucial Bug Fixes & Edge Cases Uncovered
PTY pipe stability, ANSI color sequence boundary fixes, and websocket reconnect deduplication.

---

## 4. Key Architectural Patterns
- **High-Fidelity Virtual Terminal & Stream Architecture**:
  - Implemented non-blocking PTY pipe multiplexing and sliding ring buffer replay.
  - Adapted touch gestures to terminal mouse protocol escape sequences.

---

## 5. Synthesis & Action Items for OpenRemote
Apply advanced streaming, touch translation, and event replay patterns to OpenRemote.
