import { describe, it, expect } from "vitest";
import {
  countDenseCalendarEvents,
  denseCalendarDateRange,
  generateDenseCalendarEvents,
} from "./dense";
import { DENSE_TIMED_POMODORO_CONFIG } from "./pomodoro-history";
import {
  benchmarkDatasetId,
  DEFAULT_BENCHMARK_DATASET,
  LARGE_BENCHMARK_DATASET,
} from "./types";
import { PALETTE_SIZE } from "$lib/calendar/types";

const ANCHOR = new Date(2026, 3, 30);

describe("benchmark dense calendar datasets", () => {
  it("formats dataset ids by version, year radius, stack count, and detail profile", () => {
    expect(benchmarkDatasetId(DEFAULT_BENCHMARK_DATASET)).toBe("dense-v1-r1y-s1-d1");
    expect(benchmarkDatasetId(LARGE_BENCHMARK_DATASET)).toBe("dense-v1-r10y-s1-d1");
  });

  it("counts the 1-year and 10-year dense spans around the provided anchor", () => {
    expect(denseCalendarDateRange(DEFAULT_BENCHMARK_DATASET, ANCHOR)).toEqual({
      start: "2025-04-30",
      endExclusive: "2027-04-30",
      days: 730,
    });
    expect(countDenseCalendarEvents(DEFAULT_BENCHMARK_DATASET, ANCHOR)).toBe(19_710);
    expect(denseCalendarDateRange(LARGE_BENCHMARK_DATASET, ANCHOR)).toEqual({
      start: "2016-04-30",
      endExclusive: "2036-04-30",
      days: 7305,
    });
    expect(countDenseCalendarEvents(LARGE_BENCHMARK_DATASET, ANCHOR)).toBe(197_235);
  });

  it("places one detailed one-hour Pomodoro event at the start of every hour", () => {
    const events = generateDenseCalendarEvents({
      dataset: DEFAULT_BENCHMARK_DATASET,
      anchor: ANCHOR,
      count: 4,
    });

    expect(events.map((event) => event.start)).toEqual([
      "2025-04-30 00:00",
      "2025-04-30 01:00",
      "2025-04-30 02:00",
      "2025-04-30 03:00",
    ]);
    expect(events.map((event) => event.end)).toEqual([
      "2025-04-30 01:00",
      "2025-04-30 02:00",
      "2025-04-30 03:00",
      "2025-04-30 04:00",
    ]);
    expect(events.every((event) => event.pomodoroConfig === DENSE_TIMED_POMODORO_CONFIG)).toBe(true);
  });

  it("adds three all-day events after each day's timed events", () => {
    const events = generateDenseCalendarEvents({
      dataset: DEFAULT_BENCHMARK_DATASET,
      anchor: ANCHOR,
      offset: 24,
      count: 4,
    });

    expect(events.slice(0, 3).map((event) => ({
      sourceUid: event.sourceUid,
      allDay: event.allDay,
      start: event.start,
      end: event.end,
    }))).toEqual([
      {
        sourceUid: "dense-v1-s1-d1-2025-04-30-allday1",
        allDay: true,
        start: "2025-04-30 00:00",
        end: "2025-04-30 00:00",
      },
      {
        sourceUid: "dense-v1-s1-d1-2025-04-30-allday2",
        allDay: true,
        start: "2025-04-30 00:00",
        end: "2025-04-30 00:00",
      },
      {
        sourceUid: "dense-v1-s1-d1-2025-04-30-allday3",
        allDay: true,
        start: "2025-04-30 00:00",
        end: "2025-04-30 00:00",
      },
    ]);
    expect(events[3]?.start).toBe("2025-05-01 00:00");
    expect(events[3]?.allDay).toBeUndefined();
  });

  it("uses every current palette slot across dense timed and all-day events", () => {
    const eventsPerDay = 24 * DEFAULT_BENCHMARK_DATASET.stackCount + 3;
    const events = generateDenseCalendarEvents({
      dataset: DEFAULT_BENCHMARK_DATASET,
      anchor: ANCHOR,
      count: PALETTE_SIZE * eventsPerDay,
    });
    const timedColors = new Set<number>();
    const allDayColors = new Set<number>();

    for (const event of events) {
      expect(typeof event.color).toBe("number");
      if (typeof event.color !== "number") continue;
      expect(Number.isInteger(event.color)).toBe(true);
      expect(event.color).toBeGreaterThanOrEqual(0);
      expect(event.color).toBeLessThan(PALETTE_SIZE);
      if (event.allDay) {
        allDayColors.add(event.color);
      } else {
        timedColors.add(event.color);
      }
    }

    const expected = Array.from({ length: PALETTE_SIZE }, (_, index) => index);
    expect([...timedColors].sort((a, b) => a - b)).toEqual(expected);
    expect([...allDayColors].sort((a, b) => a - b)).toEqual(expected);
  });

  it("keeps overlapping source UIDs stable when the year radius changes", () => {
    const defaultIndexForAnchor = 365 * (24 * DEFAULT_BENCHMARK_DATASET.stackCount + 3);
    const largeIndexForAnchor = 3652 * (24 * LARGE_BENCHMARK_DATASET.stackCount + 3);
    const defaultEvent = generateDenseCalendarEvents({
      dataset: DEFAULT_BENCHMARK_DATASET,
      anchor: ANCHOR,
      offset: defaultIndexForAnchor,
      count: 1,
    })[0];
    const largeEvent = generateDenseCalendarEvents({
      dataset: LARGE_BENCHMARK_DATASET,
      anchor: ANCHOR,
      offset: largeIndexForAnchor,
      count: 1,
    })[0];

    expect(defaultEvent?.start).toBe("2026-04-30 00:00");
    expect(defaultEvent?.sourceUid).toBe(largeEvent?.sourceUid);
  });

  it("adds detail profile fields to practical benchmark events", () => {
    const event = generateDenseCalendarEvents({
      dataset: DEFAULT_BENCHMARK_DATASET,
      anchor: ANCHOR,
      count: 1,
    })[0];

    expect(event).toMatchObject({
      start: "2025-04-30 00:00",
      end: "2025-04-30 01:00",
      sourceUid: "dense-v1-s1-d1-2025-04-30T00-stack1",
      notifications: [10],
      location: expect.stringContaining("Home office"),
      url: "https://benchmark.local/dense-v1-r1y-s1-d1/2025-04-30/00/1",
      pomodoroConfig: DENSE_TIMED_POMODORO_CONFIG,
      organizer: {
        name: "Benchmark Calendar",
        email: "owner@benchmark.local",
      },
      guestPermissions: {
        canModify: false,
        canInviteOthers: true,
        canSeeOtherGuests: true,
      },
    });
    expect(event?.description).toContain("Preparation:");
    expect(event?.alarms).toHaveLength(1);
    expect(event?.attendees).toHaveLength(1);
    expect(event?.categories).toContain("benchmark");
    expect(event?.extendedProperties?.["X-BENCHMARK-DATASET"]).toBe("dense-v1-r1y-s1-d1");
  });

  it("supports deterministic chunked slices", () => {
    const whole = generateDenseCalendarEvents({
      dataset: DEFAULT_BENCHMARK_DATASET,
      anchor: ANCHOR,
      count: 12,
    });
    const chunk = generateDenseCalendarEvents({
      dataset: DEFAULT_BENCHMARK_DATASET,
      anchor: ANCHOR,
      offset: 6,
      count: 3,
    });

    expect(chunk).toEqual(whole.slice(6, 9));
  });
});
