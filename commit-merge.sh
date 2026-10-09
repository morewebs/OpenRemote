#!/usr/bin/env bash
# Commit the merge of main (v0.2.0 projects) into the android branch.
# All gates green: console 69/69, vite build, daemon fmt/clippy/41 suites.
set -euo pipefail
cd /home/moreweb/Documents/Workspaces/moreweb/Projects/.worktrees/openremote-android

git add -A
git commit -m "merge: v0.2.0's projects meet the phone

The console keeps both: the two-section sidebar with its projects and
the phone's Cloud-only words, one folder picker under both names, and a
new chat on a machine that picks a project there. The mesh now carries
the project routes too, so a phone (and any Cloud desktop) manages the
projects on the machine that owns them - covered by a phone e2e.

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"

git log --oneline -4
git status --short
echo "Merge committed."
