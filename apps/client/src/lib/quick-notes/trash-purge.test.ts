import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { QuickNotesTrashPurge } from "$lib/quick-notes/types";
import {
  createQuickNotesTrashPurgeScheduler,
  QUICK_NOTE_IMAGE_COLLECTION_INTERVAL_MS,
  quickNotesTrashPurgeDeadline,
} from "./trash-purge";

const START = Date.parse("2026-10-08T12:00:00.000Z");

describe("Quick notes trash purge deadline", () => {
  it("reads the next expiry as epoch milliseconds", () => {
    expect(quickNotesTrashPurgeDeadline({ purged: 0, nextPurgeAt: "2026-10-15T12:00:00.000Z" }))
      .toBe(Date.parse("2026-10-15T12:00:00.000Z"));
    expect(quickNotesTrashPurgeDeadline({ purged: 3, nextPurgeAt: null })).toBeNull();
  });

  it("rejects malformed native timestamps", () => {
    expect(() => quickNotesTrashPurgeDeadline({ purged: 0, nextPurgeAt: "next week" }))
      .toThrow("deadline is invalid");
  });
});

describe("Quick notes trash purge scheduler", () => {
  beforeEach(() => {
    vi.useFakeTimers({ now: START });
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it("purges on enable and again when the oldest trash expires", async () => {
    const results: QuickNotesTrashPurge[] = [
      { purged: 0, nextPurgeAt: new Date(START + 60_000).toISOString() },
      { purged: 2, nextPurgeAt: null },
    ];
    const purge = vi.fn(async () => results.shift() ?? { purged: 0, nextPurgeAt: null });
    const onPurged = vi.fn();
    const scheduler = createQuickNotesTrashPurgeScheduler({ purge, onPurged });

    scheduler.setEnabled(true);
    await vi.advanceTimersByTimeAsync(0);
    expect(purge).toHaveBeenCalledTimes(1);
    expect(onPurged).not.toHaveBeenCalled();

    await vi.advanceTimersByTimeAsync(59_999);
    expect(purge).toHaveBeenCalledTimes(1);
    await vi.advanceTimersByTimeAsync(1);
    expect(purge).toHaveBeenCalledTimes(2);
    expect(onPurged).toHaveBeenCalledWith(2);
    expect(scheduler.hasScheduledDeadline()).toBe(false);
    scheduler.dispose();
  });

  it("reruns after invalidation and retries failed passes", async () => {
    const purge = vi.fn<() => Promise<QuickNotesTrashPurge>>()
      .mockResolvedValueOnce({ purged: 0, nextPurgeAt: null })
      .mockRejectedValueOnce(new Error("busy"))
      .mockResolvedValue({ purged: 0, nextPurgeAt: null });
    const onError = vi.fn();
    const scheduler = createQuickNotesTrashPurgeScheduler({ purge, onPurged: vi.fn(), onError });

    scheduler.setEnabled(true);
    await vi.advanceTimersByTimeAsync(0);
    scheduler.invalidate();
    await vi.advanceTimersByTimeAsync(0);
    expect(purge).toHaveBeenCalledTimes(2);
    expect(onError).toHaveBeenCalledOnce();

    await vi.advanceTimersByTimeAsync(60_000);
    expect(purge).toHaveBeenCalledTimes(3);
    expect(scheduler.hasScheduledDeadline()).toBe(false);
    scheduler.dispose();
  });

  it("collects unused images at most once per interval", async () => {
    const purge = vi.fn(async (): Promise<QuickNotesTrashPurge> => ({ purged: 0, nextPurgeAt: null }));
    const collectImages = vi.fn(async () => 0);
    const scheduler = createQuickNotesTrashPurgeScheduler({ purge, onPurged: vi.fn(), collectImages });

    scheduler.setEnabled(true);
    await vi.advanceTimersByTimeAsync(0);
    scheduler.invalidate();
    await vi.advanceTimersByTimeAsync(0);
    expect(purge).toHaveBeenCalledTimes(2);
    expect(collectImages).toHaveBeenCalledOnce();
    expect(scheduler.hasScheduledDeadline()).toBe(true);

    await vi.advanceTimersByTimeAsync(QUICK_NOTE_IMAGE_COLLECTION_INTERVAL_MS);
    expect(collectImages).toHaveBeenCalledTimes(2);
    scheduler.dispose();
  });

  it("wakes for the earlier of the trash expiry and the next collection", async () => {
    const purge = vi.fn(async (): Promise<QuickNotesTrashPurge> => ({
      purged: 0,
      nextPurgeAt: new Date(START + 60_000).toISOString(),
    }));
    const collectImages = vi.fn(async () => 0);
    const scheduler = createQuickNotesTrashPurgeScheduler({ purge, onPurged: vi.fn(), collectImages });

    scheduler.setEnabled(true);
    await vi.advanceTimersByTimeAsync(60_000);
    expect(purge).toHaveBeenCalledTimes(2);
    expect(collectImages).toHaveBeenCalledOnce();
    scheduler.dispose();
  });

  it("reports a failed collection without retrying it on every purge", async () => {
    const purge = vi.fn(async (): Promise<QuickNotesTrashPurge> => ({ purged: 0, nextPurgeAt: null }));
    const failure = new Error("vault busy");
    const collectImages = vi.fn(async (): Promise<number> => {
      throw failure;
    });
    const onError = vi.fn();
    const scheduler = createQuickNotesTrashPurgeScheduler({ purge, onPurged: vi.fn(), onError, collectImages });

    scheduler.setEnabled(true);
    await vi.advanceTimersByTimeAsync(0);
    scheduler.invalidate();
    await vi.advanceTimersByTimeAsync(0);
    expect(onError).toHaveBeenCalledExactlyOnceWith(failure);
    expect(collectImages).toHaveBeenCalledOnce();
    scheduler.dispose();
  });
});
