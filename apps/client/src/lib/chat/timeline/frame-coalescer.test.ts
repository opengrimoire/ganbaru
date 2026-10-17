import { describe, expect, it } from "vitest";
import { AsyncFrameCoalescer, type FrameScheduler } from "./frame-coalescer";

function controlledFrames(): { schedule: FrameScheduler; runNext: () => void; queued: () => number } {
  const callbacks: Array<() => void> = [];
  return {
    schedule(callback) {
      callbacks.push(callback);
      return () => {
        const index = callbacks.indexOf(callback);
        if (index >= 0) callbacks.splice(index, 1);
      };
    },
    runNext() {
      callbacks.shift()?.();
    },
    queued: () => callbacks.length,
  };
}

describe("AsyncFrameCoalescer", () => {
  it("keeps the latest value and resolves every caller in one frame", async () => {
    const frames = controlledFrames();
    const values: number[] = [];
    const coalescer = new AsyncFrameCoalescer<number>(async (value) => {
      values.push(value);
    }, frames.schedule);

    const first = coalescer.push(1);
    const second = coalescer.push(2);
    const third = coalescer.push(3);
    expect(frames.queued()).toBe(1);
    frames.runNext();
    await Promise.all([first, second, third]);
    expect(values).toEqual([3]);
  });

  it("queues one later frame while an async refresh is running", async () => {
    const frames = controlledFrames();
    let release: (() => void) | undefined;
    const values: string[] = [];
    const coalescer = new AsyncFrameCoalescer<string>(async (value) => {
      values.push(value);
      if (value === "first") await new Promise<void>((resolve) => { release = resolve; });
    }, frames.schedule);

    const first = coalescer.push("first");
    frames.runNext();
    const second = coalescer.push("second");
    const third = coalescer.push("latest");
    expect(frames.queued()).toBe(0);
    release?.();
    await first;
    expect(frames.queued()).toBe(1);
    frames.runNext();
    await Promise.all([second, third]);
    expect(values).toEqual(["first", "latest"]);
  });

  it("rejects every caller when the refresh fails", async () => {
    const frames = controlledFrames();
    const coalescer = new AsyncFrameCoalescer<number>(async () => {
      throw new Error("refresh failed");
    }, frames.schedule);
    const first = coalescer.push(1);
    const second = coalescer.push(2);
    frames.runNext();
    await expect(first).rejects.toThrow("refresh failed");
    await expect(second).rejects.toThrow("refresh failed");
  });
});
