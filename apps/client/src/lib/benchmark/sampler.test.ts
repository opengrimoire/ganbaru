import { beforeEach, describe, expect, it, vi } from "vitest";
import { sampleMemoryObservation } from "./sampler";
import {
  MEMORY_OBSERVATION_INTERVAL_MS,
  MEMORY_OBSERVATION_SAMPLE_COUNT,
} from "./types";

const invokeMock = vi.fn();

vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => invokeMock(...args),
}));

vi.mock("$lib/stores/perf-log.svelte", () => ({
  perfLog: { entries: [], tracking: false },
  snapshot: () => [],
}));

function memoryReport(totalMb: number) {
  return {
    processes: [
      { name: "backend", mb: 10 },
      { name: "frontend", mb: totalMb - 15 },
      { name: "network", mb: 5 },
    ],
    total_mb: totalMb,
    platform: "Linux",
    metric: {
      name: "PSS",
      slug: "pss",
      description: "Proportional Set Size",
    },
  };
}

describe("sampleMemoryObservation", () => {
  beforeEach(() => {
    invokeMock.mockReset();
  });

  it("samples once per second across the full observation window", async () => {
    const originalSetTimeout = globalThis.setTimeout;
    const originalClearTimeout = globalThis.clearTimeout;
    let nextTimerId = 1;
    const delays: number[] = [];

    const guardedSetTimeout = function (
      this: typeof globalThis,
      callback: () => void,
      delayMs?: number,
    ): ReturnType<typeof setTimeout> {
      if (this !== globalThis) throw new TypeError("wrong setTimeout receiver");
      delays.push(delayMs ?? 0);
      const timerId = nextTimerId++ as ReturnType<typeof setTimeout>;
      void Promise.resolve().then(callback);
      return timerId;
    };

    const guardedClearTimeout = function (
      this: typeof globalThis,
      _timer: ReturnType<typeof setTimeout>,
    ): void {
      if (this !== globalThis) throw new TypeError("wrong clearTimeout receiver");
    };

    invokeMock.mockImplementation(() => Promise.resolve(memoryReport(80 + invokeMock.mock.calls.length)));
    globalThis.setTimeout = guardedSetTimeout as typeof globalThis.setTimeout;
    globalThis.clearTimeout = guardedClearTimeout as typeof globalThis.clearTimeout;
    try {
      const samples = await sampleMemoryObservation({ signal: new AbortController().signal });
      expect(samples).toHaveLength(MEMORY_OBSERVATION_SAMPLE_COUNT);
      expect(samples[0]).toMatchObject({ label: "+1s", totalMb: 81 });
      expect(samples[samples.length - 1]).toMatchObject({ label: "+30s", totalMb: 110 });
      expect(delays[0]).toBeGreaterThan(MEMORY_OBSERVATION_INTERVAL_MS - 5);
    } finally {
      globalThis.setTimeout = originalSetTimeout;
      globalThis.clearTimeout = originalClearTimeout;
    }
  });

  it("reports progress after every captured sample", async () => {
    const originalSetTimeout = globalThis.setTimeout;
    const progress: string[] = [];

    const immediateSetTimeout = function (
      this: typeof globalThis,
      callback: () => void,
      _delayMs?: number,
    ): ReturnType<typeof setTimeout> {
      if (this !== globalThis) throw new TypeError("wrong setTimeout receiver");
      void Promise.resolve().then(callback);
      return 1 as ReturnType<typeof setTimeout>;
    };

    invokeMock.mockResolvedValue(memoryReport(64));
    globalThis.setTimeout = immediateSetTimeout as typeof globalThis.setTimeout;
    try {
      await sampleMemoryObservation({
        signal: new AbortController().signal,
        onProgress: (label, total, samples) => {
          progress.push(`${samples.length}/${total}:${label}`);
        },
      });
      expect(progress).toHaveLength(MEMORY_OBSERVATION_SAMPLE_COUNT);
      expect(progress[0]).toBe(`1/${MEMORY_OBSERVATION_SAMPLE_COUNT}:+1s`);
      expect(progress[progress.length - 1]).toBe(`${MEMORY_OBSERVATION_SAMPLE_COUNT}/${MEMORY_OBSERVATION_SAMPLE_COUNT}:+30s`);
    } finally {
      globalThis.setTimeout = originalSetTimeout;
    }
  });

  it("clears observation timers with the global receiver on abort", async () => {
    const originalSetTimeout = globalThis.setTimeout;
    const originalClearTimeout = globalThis.clearTimeout;
    const timeoutId = 1 as ReturnType<typeof setTimeout>;
    let cleared = false;

    const guardedSetTimeout = function (
      this: typeof globalThis,
      _callback: () => void,
      _delayMs?: number,
    ): ReturnType<typeof setTimeout> {
      if (this !== globalThis) throw new TypeError("wrong setTimeout receiver");
      return timeoutId;
    };
    const guardedClearTimeout = function (
      this: typeof globalThis,
      timer: ReturnType<typeof setTimeout>,
    ): void {
      if (this !== globalThis) throw new TypeError("wrong clearTimeout receiver");
      expect(timer).toBe(timeoutId);
      cleared = true;
    };

    const controller = new AbortController();
    globalThis.setTimeout = guardedSetTimeout as typeof globalThis.setTimeout;
    globalThis.clearTimeout = guardedClearTimeout as typeof globalThis.clearTimeout;
    try {
      const samplesPromise = sampleMemoryObservation({ signal: controller.signal });
      controller.abort();
      await expect(samplesPromise).resolves.toEqual([]);
      expect(cleared).toBe(true);
    } finally {
      globalThis.setTimeout = originalSetTimeout;
      globalThis.clearTimeout = originalClearTimeout;
    }
  });

  it("fails when a memory sample fails", async () => {
    const originalSetTimeout = globalThis.setTimeout;

    const immediateSetTimeout = function (
      this: typeof globalThis,
      callback: () => void,
      _delayMs?: number,
    ): ReturnType<typeof setTimeout> {
      if (this !== globalThis) throw new TypeError("wrong setTimeout receiver");
      void Promise.resolve().then(callback);
      return 1 as ReturnType<typeof setTimeout>;
    };

    invokeMock.mockRejectedValueOnce(new Error("memory unavailable"));
    globalThis.setTimeout = immediateSetTimeout as typeof globalThis.setTimeout;
    try {
      await expect(sampleMemoryObservation({ signal: new AbortController().signal }))
        .rejects.toThrow("Memory observation sampling failed at +1s: memory unavailable");
    } finally {
      globalThis.setTimeout = originalSetTimeout;
    }
  });
});
