# oc-remote (Android Native): Batch 02 (Commits 11-20)

## 1. Commit Log & Scope
- **Commit Range**: `294a75ec` -> `d5c12906` (10 commits)
- **Batch Window**: Commits 11 to 20

| Hash | Date | Subject | Author |
| :--- | :--- | :--- | :--- |
| `294a75ec` | 2026-02-18 | `Add provider icons, agent color selector, CI/CD workflows, and Indonesian locale` | crim50n |
| `23a637e3` | 2026-02-18 | `Fix language switching, add About screen, and locale-aware service notifications` | crim50n |
| `4148e46e` | 2026-02-18 | `Add MIT License file to the repository` | crim50n |
| `b270e91c` | 2026-02-18 | `Add screenshots to README for better visual representation` | crim50n |
| `d76cec5f` | 2026-02-18 | `Fix release workflow: find APK by glob instead of hardcoded name` | crim50n |
| `a36afebd` | 2026-02-19 | `v1.0.1: multi-project sessions, revert-to-input, deep-link fixes` | crim50n |
| `368bad48` | 2026-02-19 | `Fix release workflow: trigger on tag push, handle existing releases` | crim50n |
| `6ef79f2c` | 2026-02-19 | `Release 1.0.2: unify AMOLED UI styling across screens` | crim50n |
| `cb5538a9` | 2026-02-20 | `Release 1.1.0: add server settings parity, AMOLED model picker, and Lokit config` | crim50n |
| `d5c12906` | 2026-02-21 | `Release 1.2.0: deliver Termux-like terminal UX and PTY reliability` | crim50n |

---

## 2. Evolutionary Milestones & Architectural Intent
Implemented in-tree VT100 Canvas terminal renderer in Kotlin/Compose.

---

## 3. Crucial Bug Fixes & Edge Cases Uncovered
ANSI color code rendering glitches and cursor positioning bugs on mobile screens.

---

## 4. Key Architectural Patterns
- **Protocol & Stream State Management**:
  - Maintained reliable connection state across network hops.
  - Implemented graceful error recovery and stream reconnection logic.

---

## 5. Synthesis & Action Items for OpenRemote
Use canvas-backed terminal rendering for high performance on low-end mobile devices.
