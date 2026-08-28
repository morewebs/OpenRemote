# 247-claude-code-remote (24/7 Mobile PWA Shell): Batch 20 (Commits 191-200)

## 1. Commit Log & Scope
- **Commit Range**: `ee2bc885` -> `72f55682` (10 commits)
- **Batch Window**: Commits 191 to 200

| Hash | Date | Subject | Author |
| :--- | :--- | :--- | :--- |
| `ee2bc885` | 2026-01-12 | `fix(cloud-agent): build shared package before agent in Dockerfile` | StanGirard |
| `55512b43` | 2026-01-12 | `fix(cloud-agent): copy root tsconfig.json for shared package build` | StanGirard |
| `a36e06fa` | 2026-01-12 | `fix(cloud-agent): fix Dockerfile build issues` | StanGirard |
| `83ae0e7e` | 2026-01-12 | `fix(provisioning): use correct ghcr.io org for cloud-agent image` | StanGirard |
| `7c4b2626` | 2026-01-12 | `feat(cloud-agent): rename user to quivr and use port 4678` | StanGirard |
| `4d726083` | 2026-01-12 | `feat(cloud-agent): add deployment step for 247-agent and update Dockerfile to copy standalone agent` | StanGirard |
| `d23ec241` | 2026-01-12 | `fix(cloud-agent): copy dist folder from builder stage to include compiled code` | StanGirard |
| `30a10d2f` | 2026-01-12 | `fix(agent): add missing .js extension to init-script import` | StanGirard |
| `fb39c4eb` | 2026-01-12 | `chore(release): 2.9.0` | StanGirard |
| `72f55682` | 2026-01-12 | `feat(cloud-agent): add fly.toml for proper Fly.io port mapping` | StanGirard |

---

## 2. Evolutionary Milestones & Architectural Intent
Progressive scaling of agent execution pipelines, terminal rendering optimizations, and mobile touch adaptations.

---

## 3. Crucial Bug Fixes & Edge Cases Uncovered
PTY pipe stability, ANSI color sequence boundary fixes, and websocket reconnect deduplication.

---

## 4. Key Architectural Patterns
- **High-Fidelity Virtual Terminal & Stream Architecture**:
  - Implemented non-blocking PTY pipe multiplexing and sliding ring buffer replay.
  - Adapted touch gestures to terminal mouse protocol escape sequences.

---

## 5. Synthesis & Action Items for OpenRemote
Apply advanced streaming, touch translation, and event replay patterns to OpenRemote.
