# paseo: Milestone Epoch 03 (Ephemeral Git Worktrees & Workspace Sandboxing)

## 1. Commit Scope & Focus
- **Milestone Range**: `Commits 1201-2000`
- **Epoch Theme**: Ephemeral Git Worktrees & Workspace Sandboxing

---

## 2. Evolutionary Milestones & Architectural Intent
Automatic provisioning of `task/<hash>` git worktrees for parallel agent tasks, completely isolating working files and lockfiles.

---

## 3. Crucial Bug Fixes & Edge Cases Uncovered
- **Root Cause & Fix**:
  `.git/index.lock` collisions during simultaneous multi-agent task execution; eliminated via `git worktree add`.

---

## 4. Key Architectural Patterns
- **ConPTY Worker Isolation Pattern**:
  - Spawn child Node process dedicated to PTY host operations.
  - Communicate via binary IPC pipes.
  - If ConPTY crashes on Windows, child worker exits cleanly and restarts without bringing down the daemon.
- **Binary Frame Multiplexing**:
  - `[1 byte: Opcode | 2 bytes: Slot/Session ID | N bytes: Binary Payload]`.
  - Enables multiplexing 100+ parallel terminals over a single WebSocket connection.

---

## 5. Synthesis & Action Items for OpenRemote
Provision isolated ephemeral worktrees for parallel subagent executions.
