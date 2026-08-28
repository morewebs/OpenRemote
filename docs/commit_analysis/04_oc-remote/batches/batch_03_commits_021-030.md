# oc-remote (Android Native): Batch 03 (Commits 21-30)

## 1. Commit Log & Scope
- **Commit Range**: `55e41722` -> `ff66d8b8` (10 commits)
- **Batch Window**: Commits 21 to 30

| Hash | Date | Subject | Author |
| :--- | :--- | :--- | :--- |
| `55e41722` | 2026-02-21 | `Add release notes for v1.2.0` | crim50n |
| `a3644876` | 2026-02-21 | `Update README for terminal mode and release workflow` | crim50n |
| `9fd4877c` | 2026-02-21 | `Rename branding to OC Remote and remove test workflow` | crim50n |
| `6d14d19f` | 2026-02-22 | `Release 1.2.1: harden terminal reconnect and command path handling` | crim50n |
| `d66aea81` | 2026-02-22 | `Adjust 1.2.1 notes: remove unresolved tilde fix claim` | crim50n |
| `1c4e7f51` | 2026-02-22 | `Release 1.2.2: add session terminal entry and paste menu support` | crim50n |
| `c9e818f8` | 2026-02-22 | `Stabilize terminal rendering and PTY resize flow` | crim50n |
| `5cf35b71` | 2026-02-22 | `Fix terminal hardware keyboard input handling` | crim50n |
| `90333e9e` | 2026-02-22 | `Add server auto-connect and terminal font preferences` | crim50n |
| `ff66d8b8` | 2026-02-22 | `Release 1.3.0: summarize terminal and server improvements` | crim50n |

---

## 2. Evolutionary Milestones & Architectural Intent
Added Base64 image offloading to disk storage to eliminate ART heap Out-Of-Memory crashes.

---

## 3. Crucial Bug Fixes & Edge Cases Uncovered
Multimodal agent image responses caused GC churn and UI freezes. Extracted base64 strings to cache files.

---

## 4. Key Architectural Patterns
- **Protocol & Stream State Management**:
  - Maintained reliable connection state across network hops.
  - Implemented graceful error recovery and stream reconnection logic.

---

## 5. Synthesis & Action Items for OpenRemote
OpenRemote daemon should stream large image artifacts via separate binary file endpoints rather than inline JSON strings.
