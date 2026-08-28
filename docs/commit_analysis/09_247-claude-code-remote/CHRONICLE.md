# 247-claude-code-remote (24/7 Mobile PWA Shell): Architecture & Evolution Chronicle

## Repository Overview
- **Role**: 24/7 autonomous mobile PWA shell for Claude Code.
- **Tech**: Next.js 16, React 19, @xterm/xterm (Canvas), node-pty, WebSockets.

## Milestone Progression
- **Total Commits Analyzed**: 420
- **Total Batches**: 42
- **Lifespan & Evolution**: Enterprise terminal virtualization, ConPTY worker isolation, mobile touch SGR translation, and monotonic event replay.

## Master Architectural Insights for OpenRemote
1. **Alternate Screen Touch Translation**: Translate touch swipe gestures to SGR mouse scroll escape sequences (`\x1b[<64;1;1M`) for tmux/vim navigation on mobile devices.
2. **Monotonic Sequence Event Replay (`seq`)**: Tag all daemon events with monotonic integers for gapless, duplicate-free client reconnection.
3. **Held Stdin Prompt Streams**: Maintain stdin handles open via async generator streams so background subprocesses survive turn transitions.
4. **Android Native SSE Worker**: Use background foreground service with infinite read timeout to prevent Android Doze connection drops.
