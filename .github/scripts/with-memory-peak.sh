#!/usr/bin/env bash
# Run a command while sampling host memory in use, then report the peak.
# Hosted runners do not expose a readable cgroup peak, so this samples /proc/meminfo.
set -euo pipefail

peak_file="$(mktemp)"
echo 0 > "$peak_file"

sample_memory() {
  local peak=0 used
  while true; do
    used="$(awk '/^MemTotal:/ { total = $2 } /^MemAvailable:/ { available = $2 } END { print total - available }' /proc/meminfo)"
    if [ "$used" -gt "$peak" ]; then
      peak="$used"
      echo "$peak" > "$peak_file"
    fi
    sleep 2
  done
}

sample_memory &
sampler=$!

report_memory_peak() {
  kill "$sampler" 2>/dev/null || true
  awk '{ printf "Runner memory in use peak: %.2f GiB\n", $1 / 1048576 }' "$peak_file"
  rm -f "$peak_file"
}
trap report_memory_peak EXIT

"$@"
