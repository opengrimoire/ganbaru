#!/bin/bash
# Usage: SCRATCHPAD=<dir> land.sh <branch> <worktree-dir-name> <milestone> <step> <title>
# Check the diff, versions, and migration order before running it.
set -e
repo=${GANBARU_REPO:-/home/victor/ganbaru}
cd "$repo"
git merge-base --is-ancestor feat/sync "$1"
git merge --no-ff -S "$1" -m "merge: $5 (sync milestone $3 step $4)" 2>&1 | tail -1
[ "$(git log -1 --format=%G?)" = G ] && echo signed
"$repo/goal/sync/handoff/tick.sh" "$3" "$4" "$SCRATCHPAD/milestone-$3-step-$4-progress.md" | head -1
git worktree remove -f -f ".claude/worktrees/$2"
git branch -D "$1" "worktree-$2" 2>/dev/null | tail -1 || true
git worktree prune
rm -rf "$HOME/.cache/ganbaru-targets/android-$2"
