/**
 * Week-view forward-nav scenario. Dispatches the same initial ArrowRight
 * keydown, native repeat keydown, and keyup events used by a physical held
 * key, then observes CalendarView's real held-navigation controller. This
 * keeps the memory stress representative of a user holding the right arrow
 * instead of a benchmark-only controller.
 *
 * Seeding lays down a versioned dense calendar in the isolated
 * benchmark DB so the dense dataset compares across builds. `cleanup` is a no-op:
 * the entire benchmark DB file is deleted on summary close, so
 * per-calendar deletion would be redundant.
 */
import { getCalendarNavHandle } from "$lib/calendar/nav-handle.svelte";
import { NAV_HOLD_DELAY_MS } from "$lib/calendar/held-navigation";
import { requireScenarioMetadata } from "../registry";
import {
  HELD_NAVIGATION_DURATION_MS,
  type BenchmarkMetric,
  type BenchmarkDatasetProfile,
  type BenchmarkScenario,
  type BenchmarkScenarioContext,
  type BenchmarkSeedHandle,
} from "../types";
import {
  loadCalendarBenchmarkWindow,
  parseCalendarBenchmarkAnchor,
  seedCalendarDataset,
  waitForMs,
} from "./calendar-utils";

export const calendarNavScenario: BenchmarkScenario = {
  ...requireScenarioMetadata("calendar-nav"),

  async setup(context: BenchmarkScenarioContext): Promise<void> {
    const handle = getCalendarNavHandle();
    if (!handle.available) {
      throw new Error("Calendar view is not mounted; cannot run calendar benchmark");
    }
    handle.setViewMode("week");
    handle.setAnchorDate(parseCalendarBenchmarkAnchor(context.anchorDate));
    await loadCalendarBenchmarkWindow(context.anchorDate, "week");
    // One frame for the view to settle before the held navigation action starts.
    await new Promise((r) => requestAnimationFrame(() => r(undefined)));
  },

  async runWorkload(signal: AbortSignal): Promise<BenchmarkMetric[]> {
    const handle = getCalendarNavHandle();
    let moves = 0;
    let repeats = 0;
    let skippedTicks = 0;

    const stopObserving = handle.observeHeldNavigation((event) => {
      if (event.key !== "ArrowRight") return;
      if (event.type === "hold-start") {
        moves++;
      } else if (event.type === "repeat") {
        moves++;
        repeats++;
      } else if (event.type === "repeat-skip") {
        skippedTicks++;
      }
    });

    dispatchRightArrow("keydown");
    if (moves === 0) {
      stopObserving();
      throw new Error("Calendar benchmark ArrowRight keydown did not start held navigation");
    }

    try {
      await waitForMs(NAV_HOLD_DELAY_MS, signal);
      dispatchRightArrow("keydown", true);
      await waitForMs(Math.max(0, HELD_NAVIGATION_DURATION_MS - NAV_HOLD_DELAY_MS), signal);
    } finally {
      dispatchRightArrow("keyup");
      stopObserving();
    }

    return [
      { label: "held navigation moves", unit: "count", value: moves },
      { label: "held navigation repeats", unit: "count", value: repeats },
      { label: "held navigation skipped ticks", unit: "count", value: skippedTicks },
    ];
  },

  async seed(
    dataset: BenchmarkDatasetProfile,
    context: BenchmarkScenarioContext,
  ): Promise<BenchmarkSeedHandle> {
    return seedCalendarDataset(dataset, context);
  },

  async cleanup(_seedHandle: { calendarId: string }): Promise<void> {
    // The isolated benchmark DB file is deleted on summary close, so
    // per-calendar deletion is unnecessary.
  },
};

function dispatchRightArrow(type: "keydown" | "keyup", repeat = false): void {
  window.dispatchEvent(new KeyboardEvent(type, {
    key: "ArrowRight",
    code: "ArrowRight",
    repeat,
    bubbles: true,
    cancelable: true,
  }));
}
