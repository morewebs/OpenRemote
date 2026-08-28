# paseo: Milestone Epoch 05 (React Native / Expo Cross-Platform App & Canvas Terminal)

## 1. Commit Scope & Focus
- **Milestone Range**: `Commits 3201-4200`
- **Epoch Theme**: React Native / Expo Cross-Platform App & Canvas Terminal

---

## 2. Evolutionary Milestones & Architectural Intent
Unified Expo client (iOS, Android, Web, Desktop) with virtualized split diff viewer, interactive permission dialogs, and native audio STT.

---

## 3. Crucial Bug Fixes & Edge Cases Uncovered
- **Root Cause & Fix**:
  Large unified diff files freezing mobile UI; implemented windowed virtual DOM list renderer.

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
Virtualize diff views and tool outputs across Web and Mobile clients.
