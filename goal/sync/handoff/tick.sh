#!/bin/bash
# Usage: tick.sh <milestone> <step> <progress-note-file>
# Replaces the "- [ ] Step K:" line of goal/sync/milestone_M.md with the progress note.
set -e
repo=${GANBARU_REPO:-/home/victor/ganbaru}
f=$repo/goal/sync/milestone_$1.md
before=$(wc -l < "$f")
awk -v step="- [ ] Step $2:" -v ins="$3" 'index($0, step)==1 && !done {while((getline l < ins)>0) print l; done=1; next} {print}' "$f" > "$f.tmp"
after=$(wc -l < "$f.tmp")
if [ "$after" -lt "$before" ]; then echo "refusing: $f would shrink ($before -> $after)"; rm -f "$f.tmp"; exit 1; fi
mv "$f.tmp" "$f"
grep -n -A1 "Step $2:" "$f" | head -3
