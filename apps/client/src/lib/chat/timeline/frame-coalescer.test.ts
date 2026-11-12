import { describe, expect, it } from "vitest";
import { AsyncFrameKeyedCoalescer, type FrameScheduler } from "./frame-coalescer";

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

function unionCoalescer(
  frames: ReturnType<typeof controlledFrames>,
  consume: (key: string, value: string[]) => Promise<void>,
): AsyncFrameKeyedCoalescer<string, string[]> {
  return new AsyncFrameKeyedCoalescer<string, string[]>(
    consume,
    (current, next) => [...new Set([...current, ...next])],
    frames.schedule,
  );
}

describe("AsyncFrameKeyedCoalescer", () => {
  it("runs every key pushed in one frame, in arrival order", async () => {
    const frames = controlledFrames();
    const keys: string[] = [];
    const coalescer = unionCoalescer(frames, async (key) => {
      keys.push(key);
    });

    const first = coalescer.push("thread-a", []);
    const second = coalescer.push("thread-b", []);
    expect(frames.queued()).toBe(1);
    frames.runNext();
    await Promise.all([first, second]);

    expect(keys).toEqual(["thread-a", "thread-b"]);
  });

  it("runs a repeated key once with its values merged and resolves every caller", async () => {
    const frames = controlledFrames();
    const calls: Array<[string, string[]]> = [];
    const coalescer = unionCoalescer(frames, async (key, value) => {
      calls.push([key, value]);
    });

    const first = coalescer.push("thread-a", ["messages"]);
    const second = coalescer.push("thread-b", ["thread"]);
    const third = coalescer.push("thread-a", ["turns", "messages"]);
    frames.runNext();
    await Promise.all([first, second, third]);

    expect(calls).toEqual([
      ["thread-a", ["messages", "turns"]],
      ["thread-b", ["thread"]],
    ]);
  });

  it("rejects only the failing key's callers", async () => {
    const frames = controlledFrames();
    const keys: string[] = [];
    const coalescer = unionCoalescer(frames, async (key) => {
      if (key === "thread-a") throw new Error("refresh failed");
      keys.push(key);
    });

    const failing = coalescer.push("thread-a", []);
    const repeated = coalescer.push("thread-a", []);
    const passing = coalescer.push("thread-b", []);
    frames.runNext();

    await expect(failing).rejects.toThrow("refresh failed");
    await expect(repeated).rejects.toThrow("refresh failed");
    await expect(passing).resolves.toBeUndefined();
    expect(keys).toEqual(["thread-b"]);
  });

  it("runs a key pushed while a frame is running in the next frame", async () => {
    const frames = controlledFrames();
    let release: (() => void) | undefined;
    const calls: Array<[string, string[]]> = [];
    const coalescer = unionCoalescer(frames, async (key, value) => {
      calls.push([key, value]);
      if (calls.length === 1) await new Promise<void>((resolve) => { release = resolve; });
    });

    const first = coalescer.push("thread-a", ["messages"]);
    frames.runNext();
    const second = coalescer.push("thread-a", ["turns"]);
    const third = coalescer.push("thread-a", ["plans"]);
    expect(frames.queued()).toBe(0);
    release?.();
    await first;
    expect(frames.queued()).toBe(1);
    frames.runNext();
    await Promise.all([second, third]);

    expect(calls).toEqual([
      ["thread-a", ["messages"]],
      ["thread-a", ["turns", "plans"]],
    ]);
  });
});
