# claude-code-telegram (Enterprise Forum Topics Hub): Batch 13 (Commits 121-130)

## 1. Commit Log & Scope
- **Commit Range**: `20e47758` -> `c4a0ed9b` (10 commits)
- **Batch Window**: Commits 121 to 130

| Hash | Date | Subject | Author |
| :--- | :--- | :--- | :--- |
| `20e47758` | 2026-02-21 | `fix: only tag Docker image as :latest for stable releases` | Claude |
| `0bb238b9` | 2026-02-21 | `fix: read version from pyproject.toml directly via tomllib` | Claude |
| `d5669865` | 2026-02-21 | `docs: update install and version management documentation` | Claude |
| `5151559c` | 2026-02-21 | `feat: add rolling `latest` git tag for pip installs` | Claude |
| `5672a856` | 2026-02-21 | `fix: guard against empty message text causing Telegram 400 errors` | Richard A |
| `2e8ce20a` | 2026-02-21 | `fix: extract session_id from StreamEvent as fallback when ResultMessage has empty session_id` | Claude |
| `f5ea56f6` | 2026-02-21 | `docs: remove Docker references from version management PR` | Richard A |
| `2fe8e529` | 2026-02-21 | `fix: prevent MessageParseError from killing SDK message stream` | Richard A |
| `d6573a62` | 2026-02-21 | `Merge pull request #81 from RichardAtCT/claude/run-claude-command-fJFwS` | Richard A |
| `c4a0ed9b` | 2026-02-21 | `fix: read version from project.version after PEP 621 migration` | Richard A |

---

## 2. Evolutionary Milestones & Architectural Intent
Iterative feature enhancements, telemetry refinements, and engine scaling.

---

## 3. Crucial Bug Fixes & Edge Cases Uncovered
Protocol edge cases, concurrency locks, and stream buffer optimizations.

---

## 4. Key Architectural Patterns
- **Protocol & Stream State Management**:
  - Implemented event bus dispatch and debounced stream transport.
  - Handled message queueing and failure recovery gracefully.

---

## 5. Synthesis & Action Items for OpenRemote
Incorporate resilient event streams and multi-surface routing into OpenRemote.
