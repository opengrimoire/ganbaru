import { getCalendarNavHandle } from "$lib/calendar/nav-handle.svelte";
import { requireScenarioMetadata } from "../registry";
import {
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

export const startupBootScenario: BenchmarkScenario = {
  ...requireScenarioMetadata("startup-boot"),

  async setup(context: BenchmarkScenarioContext): Promise<void> {
    const handle = getCalendarNavHandle();
    if (!handle.available) {
      throw new Error("Calendar view is not mounted; cannot run startup benchmark");
    }
    handle.setViewMode("week");
    handle.setAnchorDate(parseCalendarBenchmarkAnchor(context.anchorDate));
    await loadCalendarBenchmarkWindow(context.anchorDate, "week");
    await waitForFrames(1);
  },

  async runWorkload(): Promise<void> {
    await waitForFrames(1);
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
