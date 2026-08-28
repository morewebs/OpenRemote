# remote-cli: Batch 02 (Commits 11-20)

## 1. Commit Log & Scope
- **Commit Range**: `61d212e7` -> `ac5cbdc3` (10 commits)
- **Author**: `mmoollee101-lab`
- **Date**: 2026-02-19

| Hash | Date | Subject | Key Changes |
| :--- | :--- | :--- | :--- |
| `61d212e7` | 2026-02-19 | `Fix Korean path encoding in runScriptSmart: use exec instead of spawn` | Windows code page (CP949/UTF-8) fix |
| `1b7e9fdc` | 2026-02-19 | `Kill GUI process after screenshot in /preview` | Process cleanup after visual snapshot |
| `433d7c44` | 2026-02-19 | `Bring window to foreground before screenshot in /preview` | Win32 `SetForegroundWindow` API |
| `c5baded8` | 2026-02-19 | `Fix GUI process kill: use taskkill /T /F to kill entire process tree` | Process tree termination |
| `168575ec` | 2026-02-19 | `Fix bringWindowToFront: replace broken here-string with MemberDefinition` | PowerShell P/Invoke reliability fix |
| `f54c1e4b` | 2026-02-19 | `Fix bringWindowToFront and add kill button for GUI preview` | Added Telegram button to terminate GUI |
| `41133457` | 2026-02-19 | `Add session resume, single-instance lock, UX improvements` | Mutex file locking; session search |
| `d2e22421` | 2026-02-19 | `Improve session resume: cross-project search, last message preview, active indicator` | Session picker UI in Telegram |
| `7b4cd9f6` | 2026-02-19 | `Fix Korean particle stripping in resolveDirectory` | Natural language path parsing |
| `ac5cbdc3` | 2026-02-19 | `Rewrite resolveDirectory: token matching instead of regex parsing` | Resilient directory name matcher |

---

## 2. Crucial Bug Fixes & Edge Cases Uncovered
- **Windows Process Tree Leaks**:
  - `child.kill()` on Windows leaves child subprocesses running. Fixed by invoking `taskkill /pid <PID> /T /F` to destroy entire process subtrees.
- **Win32 Window Capture Focus**:
  - Background windows capture blank or occluded bitmaps. Injected Win32 `ShowWindowAsync` and `SetForegroundWindow` prior to taking GDI+ screenshots.
