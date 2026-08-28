# paseo: Milestone Epoch 01 (Core Daemon & Binary Protocol Foundation)

## 1. Commit Scope & Focus
- **Milestone Range**: `Commits 0001-0500`
- **Epoch Theme**: Core Daemon & Binary Protocol Foundation

---

## 2. Evolutionary Milestones & Architectural Intent
Implementation of binary WebSocket framing `[Opcode, Slot, Payload]`, Node-PTY worker pools, and basic session management.

---

## 3. Crucial Bug Fixes & Edge Cases Uncovered
- **Root Cause & Fix**:
  Windows ConPTY C++ pipe crashes killing host daemon; solved by moving PTY lifecycle into isolated child worker processes via Node IPC.

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
Isolate ConPTY execution in dedicated child processes in OpenRemote.
