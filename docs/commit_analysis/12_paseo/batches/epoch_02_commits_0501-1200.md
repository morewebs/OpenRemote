# paseo: Milestone Epoch 02 (Multi-Agent Drivers (Claude, Codex, OpenCode, Pi))

## 1. Commit Scope & Focus
- **Milestone Range**: `Commits 0501-1200`
- **Epoch Theme**: Multi-Agent Drivers (Claude, Codex, OpenCode, Pi)

---

## 2. Evolutionary Milestones & Architectural Intent
Standardized Driver interface supporting all target agent CLIs with unified terminal emulation, held stdin, and turn completion alerts.

---

## 3. Crucial Bug Fixes & Edge Cases Uncovered
- **Root Cause & Fix**:
  Agent CLI stdin closed prematurely during multi-line prompts; implemented held async stdin generator streams.

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
Implement pluggable Driver interface in OpenRemote Go daemon (`internal/driver`).
