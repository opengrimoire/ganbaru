import { describe, expect, it, vi } from "vitest";
import { ActivityDetailCache, activityDetailPreviewKeepsTail } from "./activity-detail";

describe("activity detail cache", () => {
  it("shares one read for the same activity version and rereads a changed version", async () => {
    const read = vi.fn((_threadId: string, activityId: string) => Promise.resolve(`whole ${activityId}`));
    const cache = new ActivityDetailCache(read, 4);

    const first = cache.load("thread-1", { id: "activity-1", updatedAt: "2026-11-11T10:00:00.000Z" });
    const second = cache.load("thread-1", { id: "activity-1", updatedAt: "2026-11-11T10:00:00.000Z" });
    expect(await first).toBe("whole activity-1");
    expect(await second).toBe("whole activity-1");
    expect(read).toHaveBeenCalledTimes(1);

    await cache.load("thread-1", { id: "activity-1", updatedAt: "2026-11-11T10:00:01.000Z" });
    expect(read).toHaveBeenCalledTimes(2);
  });

  it("keeps a bounded number of details and lets the least recently opened leave first", async () => {
    const read = vi.fn((_threadId: string, activityId: string) => Promise.resolve(activityId));
    const cache = new ActivityDetailCache(read, 2);

    await cache.load("thread-1", { id: "one" });
    await cache.load("thread-1", { id: "two" });
    await cache.load("thread-1", { id: "one" });
    await cache.load("thread-1", { id: "three" });
    expect(cache.size).toBe(2);
    expect(read).toHaveBeenCalledTimes(3);

    await cache.load("thread-1", { id: "one" });
    expect(read).toHaveBeenCalledTimes(3);
    await cache.load("thread-1", { id: "two" });
    expect(read).toHaveBeenCalledTimes(4);
  });

  it("forgets a failed read so the next opening tries again", async () => {
    const read = vi.fn()
      .mockRejectedValueOnce(new Error("offline"))
      .mockResolvedValueOnce("whole");
    const cache = new ActivityDetailCache(read, 4);

    await expect(cache.load("thread-1", { id: "activity-1" })).rejects.toThrow("offline");
    expect(cache.size).toBe(0);
    expect(await cache.load("thread-1", { id: "activity-1" })).toBe("whole");
  });

  it("keeps the same activity of different threads apart", async () => {
    const read = vi.fn((threadId: string) => Promise.resolve(threadId));
    const cache = new ActivityDetailCache(read, 4);

    expect(await cache.load("thread-1", { id: "activity-1" })).toBe("thread-1");
    expect(await cache.load("thread-2", { id: "activity-1" })).toBe("thread-2");
  });
});

describe("activity detail preview direction", () => {
  it("keeps the end of command output and reasoning summaries and the start of other detail", () => {
    expect(activityDetailPreviewKeepsTail({ activityKind: "command_execution" })).toBe(true);
    expect(activityDetailPreviewKeepsTail({ activityKind: "command_output" })).toBe(true);
    expect(activityDetailPreviewKeepsTail({ activityKind: "reasoning_summary" })).toBe(true);
    expect(activityDetailPreviewKeepsTail({ activityKind: "dynamic_tool_call" })).toBe(false);
    expect(activityDetailPreviewKeepsTail({ activityKind: "notice" })).toBe(false);
  });
});
