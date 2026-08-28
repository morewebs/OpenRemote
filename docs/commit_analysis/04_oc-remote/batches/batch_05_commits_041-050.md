# oc-remote (Android Native): Batch 05 (Commits 41-50)

## 1. Commit Log & Scope
- **Commit Range**: `2eb1de10` -> `6570271b` (10 commits)
- **Batch Window**: Commits 41 to 50

| Hash | Date | Subject | Author |
| :--- | :--- | :--- | :--- |
| `2eb1de10` | 2026-02-23 | `Auto-enable Termux external commands during setup` | crim50n |
| `c4c253d5` | 2026-02-23 | `Switch local runtime from Alpine to Debian for bun-pty glibc compatibility` | crim50n |
| `4eb2e693` | 2026-02-23 | `Use GitHub CDN for Debian rootfs download instead of slow easycli.sh` | crim50n |
| `bd9bb8eb` | 2026-02-23 | `Polish setup script output: spinner, colors, step numbers, quiet apt` | crim50n |
| `7882f352` | 2026-02-23 | `Fix spinner animation and show live output for package installs` | crim50n |
| `886ef603` | 2026-02-23 | `Auto-select fastest Termux mirror before package install` | crim50n |
| `16473f74` | 2026-02-23 | `Use apt-get instead of pkg for package install` | crim50n |
| `37c508fa` | 2026-02-23 | `Add Russian mirror (repository.su, Nizhny Novgorod)` | crim50n |
| `0fa8cfc6` | 2026-02-23 | `Add Moscow HTTP mirror (mirror.mephi.ru)` | crim50n |
| `6570271b` | 2026-02-23 | `Use /bin/bash instead of /bin/sh inside proot` | crim50n |

---

## 2. Evolutionary Milestones & Architectural Intent
Interactive permission and question dialogs with custom reason inputs.

---

## 3. Crucial Bug Fixes & Edge Cases Uncovered
Dismissing permission modal without response hung agent execution indefinitely; added explicit reject on cancel.

---

## 4. Key Architectural Patterns
- **Protocol & Stream State Management**:
  - Maintained reliable connection state across network hops.
  - Implemented graceful error recovery and stream reconnection logic.

---

## 5. Synthesis & Action Items for OpenRemote
Daemon must enforce permission timeout with auto-reject or cancel notification.
