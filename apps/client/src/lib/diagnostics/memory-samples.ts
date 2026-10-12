/**
 * Live RAM time series for the performance popover chart.
 *
 * Samples are pushed by the polling effect in `PerformancePopover.svelte` once every
 * `SAMPLE_INTERVAL_MS`. The chart and the CSV exporter both read from the
 * same buffer so what the user copies matches what they see.
 */

export interface ProcessMemoryEntry {
  name: string;
  mb: number;
}

export interface MemorySample {
  /** Milliseconds since the first sample of the session. */
  t: number;
  /** Sum of the per-process memory metric values in MB. */
  totalMb: number;
  /** Per-process memory breakdown. Names match what the Rust side emits. */
  processes: ProcessMemoryEntry[];
}

/** Polling cadence for the live memory ring buffer. */
export const SAMPLE_INTERVAL_MS = 5_000;

/**
 * Cap on retained samples: an hour of polling at the default cadence. Older
 * points drop off the front. Sized so the SVG render stays cheap and the
 * CSV stays under a few hundred KB.
 */
export const SAMPLE_CAP = (60 * 60 * 1000) / SAMPLE_INTERVAL_MS;

/**
 * Format a duration in milliseconds as a compact `Xh Ym Zs` string. Zero
 * components are dropped so short spans read as `5s` or `2m 15s` instead
 * of `0h 0m 5s`. The empty case still renders as `0s` so callers always
 * have a label to draw.
 */
export function formatElapsed(ms: number): string {
  const totalSec = Math.max(0, Math.round(ms / 1000));
  const h = Math.floor(totalSec / 3600);
  const m = Math.floor((totalSec % 3600) / 60);
  const s = totalSec % 60;
  const parts: string[] = [];
  if (h > 0) parts.push(`${h}h`);
  if (m > 0) parts.push(`${m}m`);
  if (s > 0 || parts.length === 0) parts.push(`${s}s`);
  return parts.join(" ");
}

/**
 * Pick evenly spaced tick offsets for the x-axis. Always includes both
 * ends. Tick count adapts to available width: a 200 px chart gets three
 * ticks, anything wider gets four.
 */
export function pickTicks(tMin: number, tMax: number, innerWidth: number): number[] {
  if (tMax <= tMin) return [tMin];
  const count = innerWidth < 200 ? 3 : 4;
  const step = (tMax - tMin) / (count - 1);
  const ticks: number[] = [];
  for (let i = 0; i < count; i++) ticks.push(tMin + step * i);
  return ticks;
}

/**
 * Render the buffered samples as CSV. Columns are `t_ms`, a metric-specific
 * total column, and one column per process name observed across the buffer
 * (sorted, lowercased to match the snake_case style of the fixed columns).
 * Missing values emit as empty cells so spreadsheet imports keep alignment.
 */
export function samplesToCsv(samples: MemorySample[], metricSlug?: string): string {
  if (samples.length === 0) return "";
  const procNames = new Set<string>();
  for (const s of samples) {
    for (const p of s.processes) procNames.add(p.name);
  }
  // ASCII sort (uppercase before lowercase) so column order is locale-independent.
  const names = Array.from(procNames).sort();
  const totalColumn = metricSlug
    ? `total_${metricSlug.replace(/[^a-z0-9_]+/gi, "_").toLowerCase()}_mb`
    : "total_mb";
  const header = ["t_ms", totalColumn, ...names.map((n) => n.toLowerCase())].join(",");
  const rows = samples.map((s) => {
    const map = new Map<string, number>();
    for (const p of s.processes) map.set(p.name, p.mb);
    const cells = [
      Math.round(s.t).toString(),
      s.totalMb.toFixed(2),
      ...names.map((n) => {
        const v = map.get(n);
        return v === undefined ? "" : v.toFixed(2);
      }),
    ];
    return cells.join(",");
  });
  return [header, ...rows].join("\n");
}
