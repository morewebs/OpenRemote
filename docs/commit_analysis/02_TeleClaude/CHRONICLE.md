# TeleClaude: Architecture & Evolution Chronicle

## Repository Overview
- **Role**: Lightweight, feature-complete Telegram Bot bridge for Claude Code CLI.
- **Total Commits**: 24
- **Lifespan**: 2026-03-04
- **Primary Tech**: Python 3.11, `python-telegram-bot`, `asyncio`, Groq Whisper, APScheduler.

## Milestone Progression
1. **Epoch 1 (Commits 1-10)**: CLI wrapper (`claude -p`), session resumption (`--resume`), LaunchAgent macOS daemon, and skill command dispatcher.
2. **Epoch 2 (Commits 11-20)**: Progressive message streaming (1.5s debouncing), Telegram HTML conversion, auto-document export, and auto-file delivery.
3. **Epoch 3 (Commits 21-24)**: Voice STT transcription, background cron jobs, and permission mode toggles.

## Key Architectural Insights for OpenRemote
- Debounced live message streaming with rate-limit backoff.
- Auto-detection of newly generated files via `st_mtime >= run_start` timestamp comparison.
- Environment sanitization (`unset CLAUDECODE`) to enable nested agent subprocess execution.
