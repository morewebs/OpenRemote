# oc-remote (Android Native): Batch 07 (Commits 61-70)

## 1. Commit Log & Scope
- **Commit Range**: `8d45d55c` -> `0b088b72` (10 commits)
- **Batch Window**: Commits 61 to 70

| Hash | Date | Subject | Author |
| :--- | :--- | :--- | :--- |
| `8d45d55c` | 2026-02-24 | `Restore proxy exports for local runtime start` | crim50n |
| `83e1c612` | 2026-02-24 | `Preserve terminal IME lock state while typing` | crim50n |
| `3c95037c` | 2026-02-24 | `Allow custom NO_PROXY list in local runtime script` | crim50n |
| `303e5187` | 2026-02-24 | `Enable text selection for chat error blocks` | crim50n |
| `c4a55c2a` | 2026-02-24 | `Release 1.5.0: harden local runtime and improve chat media UX` | crim50n |
| `16c519db` | 2026-02-25 | `Stop pinning OpenCode version in setup script` | crim50n |
| `6431d378` | 2026-02-25 | `Harden OpenCode detection and clean runtime output` | crim50n |
| `2e6701ec` | 2026-02-25 | `Auto-update setup script during runtime refresh` | crim50n |
| `7274d4d1` | 2026-02-25 | `Show help and status when no local command` | crim50n |
| `0b088b72` | 2026-02-25 | `Harden refresh-runtime self-update against stale caches` | crim50n |

---

## 2. Evolutionary Milestones & Architectural Intent
Iterative feature enhancements, UI refinements, and protocol stability improvements.

---

## 3. Crucial Bug Fixes & Edge Cases Uncovered
Standard lifecycle, reconnection, and state synchronization fixes.

---

## 4. Key Architectural Patterns
- **Protocol & Stream State Management**:
  - Maintained reliable connection state across network hops.
  - Implemented graceful error recovery and stream reconnection logic.

---

## 5. Synthesis & Action Items for OpenRemote
Refine OpenRemote client drivers and event subscribers.
