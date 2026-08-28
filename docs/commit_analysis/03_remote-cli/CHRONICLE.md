# remote-cli: Architecture & Evolution Chronicle

## Repository Overview
- **Role**: Multi-tier Telegram remote supervisor for Claude Code with Windows desktop tray integration.
- **Total Commits**: 50
- **Lifespan**: 2026-02-18 to 2026-04-08
- **Primary Tech**: Node.js, C# WinForms, PowerShell, Claude Agent SDK, Telegram Bot API.

## Milestone Progression
1. **Epoch 1 (Commits 1-10)**: Command injection fixes, directory sandboxing, SDK generator migration, and Windows tray integration.
2. **Epoch 2 (Commits 11-20)**: Win32 desktop GUI screenshot previews, `taskkill /T /F` process tree cleanup, and natural-language directory matching.
3. **Epoch 3 (Commits 21-30)**: Interactive question handler with "Other" text input state, photo follow-up state machines, and quick action keyboards.
4. **Epoch 4 & 5 (Commits 31-50)**: 2-step PIN authentication with message scrubbing, modular v2 architecture, Cloudflare Tunnel integration, and memory optimization.

## Key Architectural Insights for OpenRemote
- Complete Windows process tree termination via `taskkill /T /F`.
- Sensitive token/PIN message scrubbing from remote chat channels.
- State-machine prompt interception for interactive multi-choice selections.
