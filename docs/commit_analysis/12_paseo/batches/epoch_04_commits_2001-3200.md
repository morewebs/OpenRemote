# paseo: Milestone Epoch 04 (Zero-Knowledge E2EE Relay & Mesh Tunneling)

## 1. Commit Scope & Focus
- **Milestone Range**: `Commits 2001-3200`
- **Epoch Theme**: Zero-Knowledge E2EE Relay & Mesh Tunneling

---

## 2. Evolutionary Milestones & Architectural Intent
TweetNaCl public-key encrypted relay protocol allowing peer-to-peer mobile connection across NATs without opening incoming firewall ports.

---

## 3. Crucial Bug Fixes & Edge Cases Uncovered
- **Root Cause & Fix**:
  Relay handshake replay attacks; added cryptographic nonces and ephemeral key exchange.

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
Support zero-configuration E2EE relay tunneling in OpenRemote.
