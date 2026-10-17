/**
 * Memory and boot-mark sampling for the benchmark harness.
 *
 * Wraps `memory_report` (Tauri command in `src-tauri/app/src/benchmark/memory.rs`) and the
 * `lib/stores/perf-log.svelte.ts` ring buffer. Scenarios never call this
 * directly: the runner orchestrates the post-state memory observation
 * schedule after `runWorkload`.
 */
import { invoke } from "@tauri-apps/api/core";
import { perfLog, snapshot as perfSnapshot, type PerfLogEntry } from "$lib/stores/perf-log.svelte";
import { categorizeMemoryProcessName } from "$lib/diagnostics/memory-report";
import type { BootTimings, SampleLabel, SamplePoint } from "./types";
import {
  MEMORY_OBSERVATION_INTERVAL_MS,
  MEMORY_OBSERVATION_SAMPLE_COUNT,
  formatOffsetLabel,
} from "./types";

interface MemoryReportProcess {
  name: string;
  mb: number;
}

interface MemoryReport {
  processes: MemoryReportProcess[];
  total_mb: number;
  platform: string;
  metric: {
    name: string;
    slug: string;
    description: string;
  };
}

type TimeoutId = ReturnType<typeof setTimeout>;

function setSampleTimeout(callback: () => void, delayMs: number): TimeoutId {
  return globalThis.setTimeout(callback, delayMs);
}

function clearSampleTimeout(id: TimeoutId): void {
  globalThis.clearTimeout(id);
}

/**
 * Read one memory snapshot from the backend. Maps the Rust report into the
 * `SamplePoint` shape (backend / frontend / network split). Process names
 * come from `benchmark/memory.rs`, while category mapping is shared with the live
 * diagnostics panel so both surfaces agree.
 */
export async function readMemorySample(
  label: SampleLabel,
  tMs: number,
): Promise<SamplePoint> {
  const report = await invoke<MemoryReport>("memory_report");
  let backend = 0;
  let frontend = 0;
  let network = 0;
  for (const p of report.processes) {
    const category = categorizeMemoryProcessName(p.name);
    if (category === "Backend") backend += p.mb;
    else if (category === "Network") network += p.mb;
    else frontend += p.mb;
  }
  return {
    label,
    tMs,
    totalMb: report.total_mb,
    backendMb: backend,
    frontendMb: frontend,
    networkMb: network,
  };
}

/**
 * Sample memory once per second over the observation window. A failed sample
 * fails the benchmark pass because copied output must not hide partial data.
 * Aborts cleanly via `signal`: the pending timer is cleared and the promise
 * resolves with whatever samples were captured so far.
 */
export function sampleMemoryObservation(opts: {
  signal: AbortSignal;
  onProgress?: (label: SampleLabel, total: number, samples: SamplePoint[]) => void;
}): Promise<SamplePoint[]> {
  return new Promise((resolve, reject) => {
    const samples: SamplePoint[] = [];
    const startedAt = performance.now();
    let sampleIndex = 1;
    let timer: TimeoutId | undefined;
    let settled = false;

    function finish(value: SamplePoint[]): void {
      if (settled) return;
      settled = true;
      if (timer !== undefined) clearSampleTimeout(timer);
      opts.signal.removeEventListener("abort", onAbort);
      resolve(value);
    }

    function fail(error: unknown): void {
      if (settled) return;
      settled = true;
      if (timer !== undefined) clearSampleTimeout(timer);
      opts.signal.removeEventListener("abort", onAbort);
      reject(error);
    }

    function onAbort(): void {
      finish(samples);
    }

    function scheduleNext() {
      if (opts.signal.aborted || sampleIndex > MEMORY_OBSERVATION_SAMPLE_COUNT) {
        finish(samples);
        return;
      }
      const targetMs = sampleIndex * MEMORY_OBSERVATION_INTERVAL_MS;
      const elapsed = performance.now() - startedAt;
      const wait = Math.max(0, targetMs - elapsed);
      timer = setSampleTimeout(async () => {
        if (opts.signal.aborted) {
          finish(samples);
          return;
        }
        const label = formatOffsetLabel(targetMs);
        try {
          const s = await readMemorySample(label, targetMs);
          if (!opts.signal.aborted) {
            samples.push(s);
            opts.onProgress?.(label, MEMORY_OBSERVATION_SAMPLE_COUNT, samples);
          }
        } catch (error: unknown) {
          const detail = error instanceof Error ? `: ${error.message}` : "";
          fail(new Error(`Memory observation sampling failed at ${label}${detail}`));
          return;
        }
        sampleIndex++;
        scheduleNext();
      }, wait);
    }

    opts.signal.addEventListener("abort", onAbort, { once: true });
    scheduleNext();
  });
}

/**
 * Tags the harness lifts out of the perflog ring buffer.
 *
 * The harness omits `boot.sql-children-done` and `boot.rawblocks-set`
 * because both are intermediate implementation marks. `boot.usable-paint`
 * is the startup target: the calendar has loaded data and rendered a frame.
 */
const BOOT_MARKS_OF_INTEREST = new Set<string>([
  "boot.script-start",
  "boot.app-mount",
  "boot.sql-start",
  "boot.sql-main-done",
  "boot.maprow-done",
  "boot.first-paint",
  "boot.usable-paint",
]);

/**
 * Lift the boot marks from the perflog snapshot, expressed as ms relative
 * to `boot.script-start` (the first mark fired in `App.svelte`). If
 * `boot.script-start` is missing, falls back to the first mark
 * in the buffer so deltas stay consistent within the run.
 */
export function captureBootTimings(): BootTimings {
  const entries = perfSnapshot();
  const baseMs = findBaseMs(entries);
  const marks: Record<string, number> = {};
  let firstPaintMs: number | undefined;
  let usablePaintMs: number | undefined;
  for (const e of entries) {
    if (BOOT_MARKS_OF_INTEREST.has(e.tag)) {
      // First write wins so a re-emitted mark does not overwrite the boot value.
      if (!(e.tag in marks)) {
        marks[e.tag] = Math.max(0, e.t - baseMs);
      }
      if (e.tag === "boot.first-paint" && firstPaintMs === undefined) {
        firstPaintMs = e.t;
      }
      if (e.tag === "boot.usable-paint" && usablePaintMs === undefined) {
        usablePaintMs = e.t;
      }
    }
  }
  const launchTargetMs = usablePaintMs ?? firstPaintMs;
  const launchTotalMs = launchTargetMs === undefined || perfLog.shellStartupMs === null
    ? undefined
    : Math.max(0, perfLog.shellStartupMs + launchTargetMs - baseMs);
  return launchTotalMs === undefined ? { marks } : { marks, launchTotalMs };
}

function findBaseMs(entries: readonly PerfLogEntry[]): number {
  for (const e of entries) {
    if (e.tag === "boot.script-start") return e.t;
  }
  return entries[0]?.t ?? 0;
}
