import { beforeEach, describe, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("$lib/api/db", () => ({ ensureDbUrl: vi.fn() }));
vi.mock("./distractions.svelte", () => ({
  getDistractions: () => { throw new Error("Native usage reads must not depend on frontend configuration"); },
}));

function projection() {
  return { vaultId: "vault", localDate: "2026-10-02", weekStartLocalDate: "2026-09-28",
    updatedAt: "2026-10-02T12:00:00.000Z",
    foregroundStatus: { available: false, appName: null, processName: null, processId: null, matchNames: [], reason: "Wayland foreground unavailable" },
    totals: [{
      limitId: "habit", period: "week", windowStartLocalDate: "2026-09-28", windowEndLocalDate: "2026-10-02",
      usedSeconds: 120, limitSeconds: 600, remainingSeconds: 480, exhausted: false,
      entries: [{ entryId: "website", usedSeconds: 100 }, { entryId: "application", usedSeconds: 20 }],
    }] };
}

describe("Distractions native totals presentation", () => {
  beforeEach(async () => {
    vi.resetModules();
    vi.clearAllMocks();
    vi.unstubAllGlobals();
    const { setActiveVaultIdentity } = await import("$lib/vault/active-vault");
    setActiveVaultIdentity("vault");
  });

  it("renders native budgets and allocations with one read and no sample or exhaustion writes", async () => {
    vi.mocked(invoke).mockResolvedValue(projection());
    const { getDistractionsUsage } = await import("./distractions-usage.svelte");
    const usage = getDistractionsUsage();
    await usage.refresh();
    expect(invoke).toHaveBeenCalledExactlyOnceWith("distractions_load_usage_projection");
    expect(usage.localDate).toBe("2026-10-02");
    expect(usage.weekStartLocalDate).toBe("2026-09-28");
    expect(usage.totalsFor("habit")).toEqual(projection().totals);
    expect(usage.entryTotalsFor("habit", "week")).toEqual(projection().totals[0].entries);
    expect(usage.entryTotalsFor("habit", "day")).toEqual([]);
    expect(usage.foregroundStatus).toEqual(projection().foregroundStatus);
  });

  it("renders Android totals through the shared reader without submitting Guardian rules or samples", async () => {
    vi.stubGlobal("__GANBARU_AI_BUILD_PLATFORM__", "android");
    vi.mocked(invoke).mockResolvedValue(projection());
    const { getDistractionsUsage } = await import("./distractions-usage.svelte");
    const usage = getDistractionsUsage();
    await usage.refresh();
    expect(invoke).toHaveBeenCalledExactlyOnceWith("distractions_mobile_load_usage_projection");
    expect(usage.totals).toEqual(projection().totals);
    expect(usage.entryTotalsFor("habit", "week")).toEqual(projection().totals[0].entries);
  });

  it("handles malformed results explicitly and permits a later native retry", async () => {
    const warning = vi.spyOn(console, "warn").mockImplementation(() => {});
    try {
      vi.mocked(invoke).mockResolvedValueOnce({ ...projection(), totals: [{ ...projection().totals[0], remainingSeconds: 0 }] })
        .mockResolvedValueOnce(projection());
      const { getDistractionsUsage } = await import("./distractions-usage.svelte");
      const usage = getDistractionsUsage();
      await usage.refresh();
      expect(warning).toHaveBeenCalledWith("Failed to refresh distraction usage limits:", expect.any(Error));
      expect(usage.totals).toEqual([]);
      await usage.refresh();
      expect(usage.totals).toEqual(projection().totals);
      expect(invoke).toHaveBeenCalledTimes(2);
    } finally {
      warning.mockRestore();
    }
  });

  it("clears old totals and discards a response delivered after the active vault changes", async () => {
    const { getDistractionsUsage } = await import("./distractions-usage.svelte");
    const { setActiveVaultIdentity } = await import("$lib/vault/active-vault");
    const usage = getDistractionsUsage();
    vi.mocked(invoke).mockResolvedValueOnce(projection());
    await usage.refresh();
    let resolve!: (value: ReturnType<typeof projection>) => void;
    vi.mocked(invoke).mockReturnValueOnce(new Promise((accept) => { resolve = accept; }));
    const refresh = usage.refresh();
    setActiveVaultIdentity("different-vault");
    expect(usage.totals).toEqual([]);
    resolve(projection());
    await refresh;
    expect(usage.totals).toEqual([]);
    vi.mocked(invoke).mockResolvedValueOnce({ ...projection(), vaultId: "different-vault" });
    await usage.refresh();
    expect(usage.totals).toEqual(projection().totals);
  });

  it("admits a refresh for the new vault while an old vault read is still in flight", async () => {
    const { getDistractionsUsage } = await import("./distractions-usage.svelte");
    const { setActiveVaultIdentity } = await import("$lib/vault/active-vault");
    const usage = getDistractionsUsage();
    let resolveOld!: (value: ReturnType<typeof projection>) => void;
    vi.mocked(invoke).mockReturnValueOnce(new Promise((accept) => { resolveOld = accept; }));
    const oldRefresh = usage.refresh();
    setActiveVaultIdentity("different-vault");
    vi.mocked(invoke).mockResolvedValueOnce({ ...projection(), vaultId: "different-vault" });
    await usage.refresh();
    expect(invoke).toHaveBeenCalledTimes(2);
    expect(usage.totals).toEqual(projection().totals);
    resolveOld(projection());
    await oldRefresh;
    expect(usage.totals).toEqual(projection().totals);
  });
});
