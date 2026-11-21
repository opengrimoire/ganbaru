import type { BenchmarkMetric } from "../types";
import { timingStatsMetric } from "./calendar-utils";

/** Frame budget of a 60 Hz display. */
export const FRAME_BUDGET_60HZ_MS = 1000 / 60;
/** Frame budget of a 120 Hz display. */
export const FRAME_BUDGET_120HZ_MS = 1000 / 120;

/**
 * Converts animation frame timestamps into the gaps between consecutive frames.
 *
 * @param timestamps `requestAnimationFrame` timestamps in increasing order.
 */
export function frameDurations(timestamps: readonly number[]): number[] {
  const durations: number[] = [];
  for (let index = 1; index < timestamps.length; index++) {
    durations.push(timestamps[index] - timestamps[index - 1]);
  }
  return durations;
}

/**
 * Reports frame time statistics plus how many frames missed the 60 Hz and 120 Hz budgets.
 *
 * @param label Row label prefix, such as `timeline scroll`.
 * @param durations Gaps between consecutive frames in milliseconds.
 */
export function frameBudgetMetrics(label: string, durations: readonly number[]): BenchmarkMetric[] {
  const valid = durations.filter(Number.isFinite);
  return [
    timingStatsMetric(`${label} frame time`, valid),
    {
      label: `${label} frames over 16.7 ms`,
      unit: "count",
      value: valid.filter((duration) => duration > FRAME_BUDGET_60HZ_MS).length,
      details: { frames: valid.length },
    },
    {
      label: `${label} frames over 8.3 ms`,
      unit: "count",
      value: valid.filter((duration) => duration > FRAME_BUDGET_120HZ_MS).length,
      details: { frames: valid.length },
    },
  ];
}

/** What the stream flood scenario observed while the replay ran. */
export interface StreamFloodObservation {
  /** Change notifications the webview received for the flood thread. */
  notifications: number;
  /** Notifications the backend reported emitting. */
  emittedNotifications: number;
  /** Wall time of the replay as the backend measured it. */
  replayMs: number;
  /** UTF-8 sizes of the live refresh reads the timeline made. */
  refreshReadBytes: readonly number[];
}

/**
 * Reports notification rate and refresh read sizes for one stream flood replay.
 */
export function streamFloodMetrics(observation: StreamFloodObservation): BenchmarkMetric[] {
  const reads = observation.refreshReadBytes;
  const totalBytes = reads.reduce((sum, bytes) => sum + bytes, 0);
  const seconds = observation.replayMs / 1000;
  return [
    {
      label: "change notifications per second",
      unit: "per second",
      value: seconds > 0 ? observation.notifications / seconds : Number.NaN,
      details: {
        received: observation.notifications,
        emitted: observation.emittedNotifications,
      },
    },
    {
      label: "refresh read size mean",
      unit: "bytes",
      value: reads.length > 0 ? totalBytes / reads.length : Number.NaN,
      details: { reads: reads.length, totalBytes },
    },
    {
      label: "refresh read size max",
      unit: "bytes",
      value: reads.length > 0 ? Math.max(...reads) : Number.NaN,
    },
    {
      label: "stream flood replay",
      unit: "ms",
      value: observation.replayMs,
    },
  ];
}

/** Collects animation frame timestamps until stopped. */
export interface FrameSampler {
  /** Stops sampling and returns the gaps between the sampled frames. */
  stop(): number[];
}

/** Starts recording `requestAnimationFrame` timestamps. */
export function startFrameSampler(): FrameSampler {
  const timestamps: number[] = [];
  let handle = 0;
  let running = true;
  const sample = (timestamp: number) => {
    if (!running) return;
    timestamps.push(timestamp);
    handle = requestAnimationFrame(sample);
  };
  handle = requestAnimationFrame(sample);
  return {
    stop() {
      running = false;
      cancelAnimationFrame(handle);
      return frameDurations(timestamps);
    },
  };
}
