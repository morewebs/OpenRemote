# opencode-remote: Architecture & Evolution Chronicle

## Repository Overview
- **Role**: Minimal React Native Expo mobile bridge to OpenCode `serve` API.
- **Total Commits**: 11
- **Lifespan**: 2026-02-25 to 2026-02-26
- **Primary Tech**: React Native, Expo, TypeScript, Server-Sent Events (SSE).

## Milestone Progression
1. **Epoch 1 (Commits 1-10)**: Core client implementation, SSE streaming, and human-in-the-loop interactive prompts (`permission.asked`, `question.asked`).
2. **Epoch 2 (Commit 11)**: Full architectural refactoring into decoupled streaming, networking, and request handler modules.

## Key Architectural Insights for OpenRemote
- High-resilience SSE stream handling with inline UTF-8 mojibake decoding.
- Composite-key tool update tracking to prevent chat list jitter.
- Interactive mobile sheet rendering for tool approvals and multi-choice agent questions.
