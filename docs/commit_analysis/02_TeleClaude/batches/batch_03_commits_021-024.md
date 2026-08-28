# TeleClaude: Batch 03 (Commits 21-24)

## 1. Commit Log & Scope
- **Commit Range**: `24b533a8` -> `HEAD` (4 commits)
- **Author**: `leo919pm`
- **Date**: 2026-03-04

| Hash | Date | Subject | Files Touched | Key Changes |
| :--- | :--- | :--- | :--- | :--- |
| `6e499d75` | 2026-03-04 | `add voice, streaming, scheduling, and permissions` | `scheduler.py`, `bot.py` | Full voice & scheduler pipeline |
| `0f12da33` | 2026-03-04 | `fix: scheduler persistent store initialization` | `scheduler.py` | `scheduled_jobs.json` file initialization safety |
| `8a7b11c2` | 2026-03-04 | `support custom voice transcription models` | `config.py` | Configurable Whisper endpoint & model selection |
| `9e41cc78` | 2026-03-04 | `cleanup temp audio artifacts` | `bot.py` | Immediate unlinking of transcribed audio files |

---

## 2. Evolutionary Milestones & Architectural Intent
- **Background Cron Engine**: Integrated persistent APScheduler storing cron definitions in JSON.
- **Voice Transcription Integration**: Support for incoming `.ogg` Telegram voice notes transcribed via Whisper API.

---

## 3. Synthesis & Action Items for OpenRemote
- OpenRemote daemon can support background scheduled prompts natively via its internal timer/cron scheduler.
