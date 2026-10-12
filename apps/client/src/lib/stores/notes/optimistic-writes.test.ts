import { describe, expect, it } from "vitest";
import { createNotesOptimisticWriteTracker } from "./optimistic-writes";

describe("notes optimistic write tracker", () => {
  it("keeps the newer write pending when an older write finishes out of order", async () => {
    let finishOlder!: () => void;
    let finishNewer!: () => void;
    const older = new Promise<void>((resolve) => { finishOlder = resolve; });
    const newer = new Promise<void>((resolve) => { finishNewer = resolve; });
    const tracker = createNotesOptimisticWriteTracker();

    tracker.track(["block-a"], older);
    tracker.track(["block-a"], newer);
    finishOlder();
    await older;
    await Promise.resolve();

    expect(tracker.pending("block-a")).toBe(newer);
    finishNewer();
    await tracker.flush();
    expect(tracker.pending("block-a")).toBeNull();
  });
});
