import { describe, expect, it } from "vitest";
import { frameBudgetMetrics, frameDurations, streamFloodMetrics } from "./chat-metrics";

describe("frameDurations", () => {
  it("returns the gap between consecutive frames", () => {
    expect(frameDurations([0, 16, 40, 48])).toEqual([16, 24, 8]);
  });

  it("returns no gaps for fewer than two frames", () => {
    expect(frameDurations([])).toEqual([]);
    expect(frameDurations([12])).toEqual([]);
  });
});

describe("frameBudgetMetrics", () => {
  it("counts frames over the 60 Hz and 120 Hz budgets separately", () => {
    const [timing, over60, over120] = frameBudgetMetrics("timeline scroll", [8, 8.4, 16.6, 16.8, 33, Number.NaN]);

    expect(timing.label).toBe("timeline scroll frame time");
    expect(timing.details?.runs).toBe(5);
    expect(timing.details?.maxMs).toBe(33);
    expect(over60).toMatchObject({ label: "timeline scroll frames over 16.7 ms", unit: "count", value: 2 });
    expect(over120).toMatchObject({ label: "timeline scroll frames over 8.3 ms", unit: "count", value: 4 });
  });

  it("reports a missing timing row and zero misses without frames", () => {
    const [timing, over60] = frameBudgetMetrics("stream flood", []);

    expect(Number.isNaN(timing.value)).toBe(true);
    expect(over60.value).toBe(0);
  });
});

describe("streamFloodMetrics", () => {
  it("reports the notification rate over the replay and the refresh read sizes", () => {
    const [rate, mean, max, replay] = streamFloodMetrics({
      notifications: 130,
      emittedNotifications: 132,
      replayMs: 10_000,
      refreshReadBytes: [100, 300, 200],
    });

    expect(rate).toMatchObject({ unit: "per second", value: 13, details: { received: 130, emitted: 132 } });
    expect(mean).toMatchObject({ unit: "bytes", value: 200, details: { reads: 3, totalBytes: 600 } });
    expect(max).toMatchObject({ unit: "bytes", value: 300 });
    expect(replay).toMatchObject({ unit: "ms", value: 10_000 });
  });

  it("marks the rate and size missing when nothing was measured", () => {
    const [rate, mean, max] = streamFloodMetrics({
      notifications: 0,
      emittedNotifications: 0,
      replayMs: 0,
      refreshReadBytes: [],
    });

    expect(Number.isNaN(rate.value)).toBe(true);
    expect(Number.isNaN(mean.value)).toBe(true);
    expect(Number.isNaN(max.value)).toBe(true);
  });
});
