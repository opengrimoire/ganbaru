import { describe, expect, it, vi } from "vitest";

import { createActiveUsageTracker } from "../../../../extensions/chromium/active-usage.js";

function deferred() {
  let resolve = () => {};
  const promise = new Promise((accept) => {
    resolve = accept;
  });
  return { promise, resolve };
}

function trackerWith({ stored = null, target = null, now = 10_000 } = {}) {
  const storage = { usage: stored };
  const recorded = [];
  const deps = {
    readActiveUsage: vi.fn(async () => storage.usage),
    writeActiveUsage: vi.fn(async (usage) => {
      storage.usage = usage;
    }),
    clearActiveUsage: vi.fn(async () => {
      storage.usage = null;
    }),
    queryTarget: vi.fn(async () => target),
    recordInterval: vi.fn(async (usage, endedAtMs) => {
      recorded.push({ host: usage.host, startedAtMs: usage.startedAtMs, endedAtMs });
    }),
    now: () => now,
  };
  return { tracker: createActiveUsageTracker(deps), deps, storage, recorded };
}

describe("Chromium extension active usage tracking", () => {
  it("starts an interval for a trackable page when none is stored", async () => {
    const { tracker, storage, recorded } = trackerWith({ target: { tabId: 1, host: "example.com" } });
    await tracker.refresh();
    expect(storage.usage).toEqual({ tabId: 1, host: "example.com", startedAtMs: 10_000 });
    expect(recorded).toEqual([]);
  });

  it("keeps the running interval while the focused host is unchanged", async () => {
    const usage = { tabId: 1, host: "example.com", startedAtMs: 1_000 };
    const { tracker, storage, recorded } = trackerWith({ stored: usage, target: { tabId: 2, host: "example.com" } });
    await tracker.refresh();
    expect(storage.usage).toBe(usage);
    expect(recorded).toEqual([]);
  });

  it("reports the finished interval and starts the next one when the host changes", async () => {
    const { tracker, storage, recorded } = trackerWith({
      stored: { tabId: 1, host: "example.com", startedAtMs: 1_000 },
      target: { tabId: 2, host: "news.example" },
    });
    await tracker.refresh();
    expect(recorded).toEqual([{ host: "example.com", startedAtMs: 1_000, endedAtMs: 10_000 }]);
    expect(storage.usage).toEqual({ tabId: 2, host: "news.example", startedAtMs: 10_000 });
  });

  it("clears the interval after reporting it when no page is trackable", async () => {
    const { tracker, storage, recorded } = trackerWith({ stored: { tabId: 1, host: "example.com", startedAtMs: 1_000 } });
    await tracker.refresh({ forceFlush: true });
    expect(recorded).toHaveLength(1);
    expect(storage.usage).toBeNull();
  });

  it("reports one interval once when several listeners refresh at the same moment", async () => {
    const { tracker, recorded } = trackerWith({
      stored: { tabId: 1, host: "example.com", startedAtMs: 1_000 },
      target: { tabId: 2, host: "news.example" },
    });
    await Promise.all([tracker.refresh(), tracker.refresh(), tracker.refresh({ forceFlush: true })]);
    expect(recorded.filter((entry) => entry.startedAtMs === 1_000)).toHaveLength(1);
  });

  it("skips reporting when the stored interval changed while the focused page was read", async () => {
    const pending = deferred();
    const { tracker, deps, storage, recorded } = trackerWith({
      stored: { tabId: 1, host: "example.com", startedAtMs: 1_000 },
    });
    deps.queryTarget.mockImplementationOnce(async () => {
      await pending.promise;
      return { tabId: 2, host: "news.example" };
    });
    const refresh = tracker.refresh();
    await vi.waitFor(() => expect(deps.queryTarget).toHaveBeenCalled());
    storage.usage = { tabId: 3, host: "example.com", startedAtMs: 5_000 };
    pending.resolve();
    await refresh;
    expect(recorded).toEqual([]);
    expect(storage.usage).toEqual({ tabId: 3, host: "example.com", startedAtMs: 5_000 });
  });

  it("keeps later refreshes running after one fails", async () => {
    const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
    const { tracker, deps, storage } = trackerWith({ target: { tabId: 1, host: "example.com" } });
    deps.readActiveUsage.mockRejectedValueOnce(new Error("storage unavailable"));
    await expect(tracker.refresh()).resolves.toBeUndefined();
    await tracker.refresh();
    expect(storage.usage).toEqual({ tabId: 1, host: "example.com", startedAtMs: 10_000 });
    expect(warn).toHaveBeenCalledOnce();
    warn.mockRestore();
  });
});
