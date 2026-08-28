# paseo: Architecture & Evolution Chronicle

## Repository Overview
- **Role**: Multi-workspace agent orchestrator, multi-agent driver engine, and cross-platform companion.
- **Total Commits**: 5,077
- **Lifespan**: 2025 to 2026
- **Primary Tech**: Node.js, TypeScript, React Native, Expo, Electron, node-pty, TweetNaCl, Git Worktrees.

## Master Architectural Insights for OpenRemote
1. **Worker-Thread ConPTY Isolation**: Traps Windows ConPTY C++ exceptions in a child worker process to prevent host daemon crashes.
2. **Binary Frame Multiplexing**: High-speed binary WebSocket framing `[Opcode, Slot, Payload]` for ultra-low latency terminal streaming.
3. **Ephemeral Git Worktrees**: Automatically provisions `git worktree add task/<hash>` directories for parallel agent tasks, completely isolating working states.
4. **Decoupled Workspace IDs vs Filesystem Paths**: Assigns opaque workspace IDs (`wks_<hex>`) so multiple independent sessions can target the same directory without state collisions.
5. **Zero-Knowledge E2EE Relay**: Cryptographic public-key encrypted relay protocol for seamless zero-port-forwarding ingress.
