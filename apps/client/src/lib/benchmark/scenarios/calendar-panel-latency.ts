import { getCalendarNavHandle } from "$lib/calendar/nav-handle.svelte";
import {
  clear as clearPerfLog,
  perfLog,
  setTracking,
  snapshot as perfSnapshot,
  type PerfLogEntry,
} from "$lib/stores/perf-log.svelte";
import { requireScenarioMetadata } from "../registry";
import {
  PANEL_ACTION_RUNS,
  type BenchmarkDatasetProfile,
  type BenchmarkMetric,
  type BenchmarkScenario,
  type BenchmarkScenarioContext,
  type BenchmarkSeedHandle,
} from "../types";
import {
  loadCalendarBenchmarkWindow,
  parseCalendarBenchmarkAnchor,
  seedCalendarDataset,
  timingStatsMetric,
  waitForFrames,
} from "./calendar-utils";

type PanelMode = "edit" | "create";

function panelRequest(entry: PerfLogEntry): number | undefined {
  const request = entry.detail?.request;
  return typeof request === "number" ? request : undefined;
}

function latestPanelOpenMs(mode: PanelMode): number {
  const entries = perfSnapshot();
  const starts = entries.filter((entry) =>
    entry.tag === "panel.start" && entry.detail?.mode === mode,
  );
  const start = starts[starts.length - 1];
  if (!start) return Number.NaN;
  const request = panelRequest(start);
  const done = entries.find((entry) =>
    entry.t >= start.t
    && entry.tag === "panel.paint-done"
    && (request === undefined || panelRequest(entry) === request),
  );
  return done ? Math.max(0, done.t - start.t) : Number.NaN;
}

async function measurePanelOpen(
  mode: PanelMode,
  action: () => Promise<boolean>,
): Promise<number> {
  const wasTracking = perfLog.tracking;
  clearPerfLog();
  setTracking(true);
  try {
    const opened = await action();
    await waitForFrames(2);
    return opened ? latestPanelOpenMs(mode) : Number.NaN;
  } finally {
    clearPerfLog();
    setTracking(wasTracking);
  }
}

function throwIfAborted(signal: AbortSignal): void {
  if (signal.aborted) throw new DOMException("aborted", "AbortError");
}

function existingEventIndex(runIndex: number, visibleEventCount: number): number {
  if (visibleEventCount >= PANEL_ACTION_RUNS) {
    return Math.floor((runIndex * visibleEventCount) / PANEL_ACTION_RUNS);
  }
  return runIndex % visibleEventCount;
}

function pad2(n: number): string {
  return n < 10 ? `0${n}` : `${n}`;
}

function localDateString(date: Date): string {
  return `${date.getFullYear()}-${pad2(date.getMonth() + 1)}-${pad2(date.getDate())}`;
}

function addDays(date: Date, days: number): Date {
  const next = new Date(date);
  next.setDate(date.getDate() + days);
  return next;
}

function createPanelTimes(anchorDate: string, runIndex: number) {
  const base = parseCalendarBenchmarkAnchor(anchorDate);
  const day = addDays(base, runIndex % 7);
  const hour = 6 + (runIndex % 16);
  return {
    start: `${localDateString(day)} ${pad2(hour)}:30`,
    end: `${localDateString(day)} ${pad2(hour + 1)}:00`,
  };
}

function metric(label: string, samples: number[]): BenchmarkMetric {
  return timingStatsMetric(label, samples);
}

export const calendarPanelLatencyScenario: BenchmarkScenario = {
  ...requireScenarioMetadata("calendar-panel-latency"),

  async setup(context: BenchmarkScenarioContext): Promise<void> {
    const handle = getCalendarNavHandle();
    if (!handle.available) {
      throw new Error("Calendar view is not mounted; cannot run panel benchmark");
    }
    handle.setViewMode("week");
    handle.setAnchorDate(parseCalendarBenchmarkAnchor(context.anchorDate));
    await loadCalendarBenchmarkWindow(context.anchorDate, "week");
    await waitForFrames(2);
    await handle.closePanel();
  },

  async runWorkload(
    signal: AbortSignal,
    context: BenchmarkScenarioContext,
  ): Promise<BenchmarkMetric[]> {
    const handle = getCalendarNavHandle();
    const visibleEventCount = handle.getVisibleEventCount();
    if (visibleEventCount <= 0) {
      throw new Error("Calendar panel benchmark could not find visible events");
    }

    const existingOpenMs: number[] = [];
    const createOpenMs: number[] = [];

    for (let i = 0; i < PANEL_ACTION_RUNS; i++) {
      throwIfAborted(signal);
      await handle.closePanel();
      const index = existingEventIndex(i, visibleEventCount);
      existingOpenMs.push(
        await measurePanelOpen("edit", () => handle.openVisibleEvent(index)),
      );
    }

    for (let i = 0; i < PANEL_ACTION_RUNS; i++) {
      throwIfAborted(signal);
      await handle.closePanel();
      const times = createPanelTimes(context.anchorDate, i);
      createOpenMs.push(
        await measurePanelOpen("create", () => handle.openCreatePanel(times.start, times.end, false)),
      );
    }

    await handle.closePanel();

    return [
      metric("click existing event avg", existingOpenMs),
      metric("click empty time slot avg", createOpenMs),
    ];
  },

  async seed(
    dataset: BenchmarkDatasetProfile,
    context: BenchmarkScenarioContext,
  ): Promise<BenchmarkSeedHandle> {
    return seedCalendarDataset(dataset, context);
  },

  async cleanup(_seedHandle: { calendarId: string }): Promise<void> {
    // The isolated benchmark DB is deleted after the run.
  },
};
