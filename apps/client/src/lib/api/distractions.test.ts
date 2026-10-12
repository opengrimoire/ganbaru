import { beforeEach, describe, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { loadDistractionsUsageProjection, parseDistractionsUsageProjection } from "./distractions";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

function projection() {
  return {
    vaultId: "vault", localDate: "2026-10-02", weekStartLocalDate: "2026-09-28",
    updatedAt: "2026-10-02T12:00:00.000Z",
    foregroundStatus: { available: false, appName: null, processName: null, processId: null, matchNames: [], reason: "Wayland foreground unavailable" },
    totals: [{
      limitId: "habit", period: "day", windowStartLocalDate: "2026-10-02", windowEndLocalDate: "2026-10-02",
      usedSeconds: 70, limitSeconds: 60, remainingSeconds: 0, exhausted: true,
      entries: [{ entryId: "web", usedSeconds: 40 }, { entryId: "app", usedSeconds: 30 }],
    }],
  };
}

describe("native Distractions budget boundary", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.unstubAllGlobals();
  });

  it("reads canonical budgets without supplying frontend samples, configuration, or clock values", async () => {
    vi.mocked(invoke).mockResolvedValue(projection());
    expect(await loadDistractionsUsageProjection()).toEqual(projection());
    expect(invoke).toHaveBeenCalledExactlyOnceWith("distractions_load_usage_projection");
  });

  it("reads the same canonical budget contract from the Android native publisher", async () => {
    vi.stubGlobal("__GANBARU_AI_BUILD_PLATFORM__", "android");
    vi.mocked(invoke).mockResolvedValue(projection());
    expect(await loadDistractionsUsageProjection()).toEqual(projection());
    expect(invoke).toHaveBeenCalledExactlyOnceWith("distractions_mobile_load_usage_projection");
  });

  it("retains separate daily and weekly allocations", () => {
    const value = projection();
    value.totals.push({ ...value.totals[0], period: "week", windowStartLocalDate: "2026-09-28" });
    expect(parseDistractionsUsageProjection(value).totals.map((total) => total.period)).toEqual(["day", "week"]);
  });

  it.each([
    (value: ReturnType<typeof projection>) => ({ ...value, localDate: "2026-02-30" }),
    (value: ReturnType<typeof projection>) => ({ ...value, weekStartLocalDate: "2026-09-27" }),
    (value: ReturnType<typeof projection>) => ({ ...value, totals: [value.totals[0], value.totals[0]] }),
    (value: ReturnType<typeof projection>) => ({ ...value, totals: [{ ...value.totals[0], exhausted: false }] }),
    (value: ReturnType<typeof projection>) => ({ ...value, totals: [{ ...value.totals[0], usedSeconds: Number.MAX_SAFE_INTEGER + 1 }] }),
    (value: ReturnType<typeof projection>) => ({ ...value, totals: [{ ...value.totals[0], remainingSeconds: 1 }] }),
    (value: ReturnType<typeof projection>) => ({ ...value, totals: [{ ...value.totals[0], windowEndLocalDate: "2026-10-03" }] }),
    (value: ReturnType<typeof projection>) => ({ ...value, totals: [{ ...value.totals[0], entries: [{ entryId: "web", usedSeconds: 69 }] }] }),
    (value: ReturnType<typeof projection>) => ({ ...value, totals: [{ ...value.totals[0], entries: [{ entryId: "web", usedSeconds: 35 }, { entryId: "web", usedSeconds: 35 }] }] }),
  ])("rejects inconsistent native totals before rendering them", (corrupt) => {
    expect(() => parseDistractionsUsageProjection(corrupt(projection()))).toThrow();
  });

  it("rejects oversized budget and entry results", () => {
    expect(() => parseDistractionsUsageProjection({ ...projection(), totals: Array.from({ length: 513 }, () => projection().totals[0]) })).toThrow();
    expect(() => parseDistractionsUsageProjection({ ...projection(), totals: [{ ...projection().totals[0], entries: Array.from({ length: 2001 }, (_, index) => ({ entryId: `entry-${index}`, usedSeconds: 0 })) }] })).toThrow();
  });

  it.each([
    { available: true, appName: null, processName: null, processId: null, matchNames: [], reason: null },
    { available: false, appName: "Game", processName: null, processId: null, matchNames: [], reason: null },
    { available: true, appName: "Game", processName: "game", processId: 0, matchNames: [], reason: null },
    { available: true, appName: "Game", processName: "game", processId: 42, matchNames: [null], reason: null },
    { available: false, appName: null, processName: null, processId: null, matchNames: [], reason: "x".repeat(513) },
  ])("rejects malformed native adapter status", (foregroundStatus) => {
    expect(() => parseDistractionsUsageProjection({ ...projection(), foregroundStatus })).toThrow();
  });
});
