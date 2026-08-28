# claudecodeui (Multi-Agent Web IDE & Shell): Architecture & Evolution Chronicle

## Repository Overview
- **Role**: Multi-agent Web IDE & shell with held stdin streams and monotonic event replay.
- **Tech**: Node.js, Express, Next.js 15, React 18, @modelcontextprotocol, xterm.js.

## Milestone Progression
- **Total Commits Analyzed**: 780
- **Total Batches**: 78
- **Lifespan & Evolution**: Enterprise terminal virtualization, ConPTY worker isolation, mobile touch SGR translation, and monotonic event replay.

## Master Architectural Insights for OpenRemote
1. **Alternate Screen Touch Translation**: Translate touch swipe gestures to SGR mouse scroll escape sequences (`\x1b[<64;1;1M`) for tmux/vim navigation on mobile devices.
2. **Monotonic Sequence Event Replay (`seq`)**: Tag all daemon events with monotonic integers for gapless, duplicate-free client reconnection.
3. **Held Stdin Prompt Streams**: Maintain stdin handles open via async generator streams so background subprocesses survive turn transitions.
4. **Android Native SSE Worker**: Use background foreground service with infinite read timeout to prevent Android Doze connection drops.
