# opencode-remote-android (Local-First Android TaskDesk): Architecture & Evolution Chronicle

## Repository Overview
- **Role**: Local-first Android TaskDesk harness with native SSE service and ACP driver.
- **Tech**: Capacitor, React, Java HttpURLConnection, Git Worktrees, ACP v1.

## Milestone Progression
- **Total Commits Analyzed**: 590
- **Total Batches**: 59
- **Lifespan & Evolution**: Enterprise terminal virtualization, ConPTY worker isolation, mobile touch SGR translation, and monotonic event replay.

## Master Architectural Insights for OpenRemote
1. **Alternate Screen Touch Translation**: Translate touch swipe gestures to SGR mouse scroll escape sequences (`\x1b[<64;1;1M`) for tmux/vim navigation on mobile devices.
2. **Monotonic Sequence Event Replay (`seq`)**: Tag all daemon events with monotonic integers for gapless, duplicate-free client reconnection.
3. **Held Stdin Prompt Streams**: Maintain stdin handles open via async generator streams so background subprocesses survive turn transitions.
4. **Android Native SSE Worker**: Use background foreground service with infinite read timeout to prevent Android Doze connection drops.
