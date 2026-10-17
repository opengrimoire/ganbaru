import { getCalendarNavHandle } from "$lib/calendar/nav-handle.svelte";
import { requireScenarioMetadata } from "../registry";
import {
  type BenchmarkMetric,
  type BenchmarkDatasetProfile,
  type BenchmarkScenario,
  type BenchmarkScenarioContext,
  type BenchmarkSeedHandle,
} from "../types";
import {
  parseCalendarBenchmarkAnchor,
  loadCalendarBenchmarkWindow,
  seedCalendarDataset,
  waitForFrames,
} from "./calendar-utils";

export const idleMemoryScenario: BenchmarkScenario = {
  ...requireScenarioMetadata("idle-memory"),

  async setup(context: BenchmarkScenarioContext): Promise<void> {
    const handle = getCalendarNavHandle();
    if (!handle.available) {
      throw new Error("Calendar view is not mounted; cannot run idle-memory benchmark");
    }
    handle.setViewMode("week");
    handle.setAnchorDate(parseCalendarBenchmarkAnchor(context.anchorDate));
    await loadCalendarBenchmarkWindow(context.anchorDate, "week");
    await waitForFrames(1);
  },

  async runWorkload(_signal: AbortSignal): Promise<BenchmarkMetric[]> {
    return [];
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
