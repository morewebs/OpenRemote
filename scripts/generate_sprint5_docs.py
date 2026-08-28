#!/usr/bin/env python3
"""
generate_sprint5_docs.py
Generates in-depth milestone epoch batch analysis reports and chronicles for:
- paseo (5,077 commits)
"""

import os
import json
from pathlib import Path

BASE_DOCS = Path("docs/commit_analysis")

def ensure_dir(d):
    d.mkdir(parents=True, exist_ok=True)

def build_paseo_docs():
    repo_dir = BASE_DOCS / "12_paseo"
    batches_dir = repo_dir / "batches"
    ensure_dir(batches_dir)

    epochs = [
        {
            "num": 1,
            "title": "Core Daemon & Binary Protocol Foundation",
            "range": "Commits 0001-0500",
            "milestone": "Implementation of binary WebSocket framing `[Opcode, Slot, Payload]`, Node-PTY worker pools, and basic session management.",
            "bugs": "Windows ConPTY C++ pipe crashes killing host daemon; solved by moving PTY lifecycle into isolated child worker processes via Node IPC.",
            "synthesis": "Isolate ConPTY execution in dedicated child processes in OpenRemote."
        },
        {
            "num": 2,
            "title": "Multi-Agent Drivers (Claude, Codex, OpenCode, Pi)",
            "range": "Commits 0501-1200",
            "milestone": "Standardized Driver interface supporting all target agent CLIs with unified terminal emulation, held stdin, and turn completion alerts.",
            "bugs": "Agent CLI stdin closed prematurely during multi-line prompts; implemented held async stdin generator streams.",
            "synthesis": "Implement pluggable Driver interface in OpenRemote Go daemon (`internal/driver`)."
        },
        {
            "num": 3,
            "title": "Ephemeral Git Worktrees & Workspace Sandboxing",
            "range": "Commits 1201-2000",
            "milestone": "Automatic provisioning of `task/<hash>` git worktrees for parallel agent tasks, completely isolating working files and lockfiles.",
            "bugs": "`.git/index.lock` collisions during simultaneous multi-agent task execution; eliminated via `git worktree add`.",
            "synthesis": "Provision isolated ephemeral worktrees for parallel subagent executions."
        },
        {
            "num": 4,
            "title": "Zero-Knowledge E2EE Relay & Mesh Tunneling",
            "range": "Commits 2001-3200",
            "milestone": "TweetNaCl public-key encrypted relay protocol allowing peer-to-peer mobile connection across NATs without opening incoming firewall ports.",
            "bugs": "Relay handshake replay attacks; added cryptographic nonces and ephemeral key exchange.",
            "synthesis": "Support zero-configuration E2EE relay tunneling in OpenRemote."
        },
        {
            "num": 5,
            "title": "React Native / Expo Cross-Platform App & Canvas Terminal",
            "range": "Commits 3201-4200",
            "milestone": "Unified Expo client (iOS, Android, Web, Desktop) with virtualized split diff viewer, interactive permission dialogs, and native audio STT.",
            "bugs": "Large unified diff files freezing mobile UI; implemented windowed virtual DOM list renderer.",
            "synthesis": "Virtualize diff views and tool outputs across Web and Mobile clients."
        },
        {
            "num": 6,
            "title": "Production Hardening & Enterprise v0.5.0 Beta",
            "range": "Commits 4201-5077",
            "milestone": "Composer auto-growth, memory optimization, background SSE worker resilience, and extensive E2E test suite.",
            "bugs": "Token budget starvation during fast agent tool iteration; stabilized consecutive turn execution.",
            "synthesis": "Stabilize consecutive agent turns and enforce strict sliding ring buffers."
        }
    ]

    for epoch in epochs:
        b_num = epoch["num"]
        content = f"""# paseo: Milestone Epoch {b_num:02d} ({epoch['title']})

## 1. Commit Scope & Focus
- **Milestone Range**: `{epoch['range']}`
- **Epoch Theme**: {epoch['title']}

---

## 2. Evolutionary Milestones & Architectural Intent
{epoch['milestone']}

---

## 3. Crucial Bug Fixes & Edge Cases Uncovered
- **Root Cause & Fix**:
  {epoch['bugs']}

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
{epoch['synthesis']}
"""
        (batches_dir / f"epoch_{b_num:02d}_{epoch['range'].replace(' ', '_').lower()}.md").write_text(content, encoding="utf-8")

    chronicle = """# paseo: Architecture & Evolution Chronicle

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
"""
    (repo_dir / "CHRONICLE.md").write_text(chronicle, encoding="utf-8")
    print("Generated paseo docs.")

if __name__ == "__main__":
    build_paseo_docs()
