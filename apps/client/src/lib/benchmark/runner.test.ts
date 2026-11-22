import { invoke } from "@tauri-apps/api/core";
import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

import { clearPersistedState, persistBenchmarkState } from "./runner";
import type { BenchmarkState } from "./types";

interface Deferred {
  resolve: () => void;
  reject: (error: Error) => void;
}

const state: BenchmarkState = {
  scenarioId: "dense",
  startedAt: "2026-01-01T00:00:00.000Z",
  updatedAt: "2026-01-01T00:00:00.000Z",
  harnessVersion: "test",
  datasetVersion: "test",
  platform: "linux",
  anchorDate: "2026-01-01",
  vaultMode: "benchmark",
  stage: "phase-a-running",
};

describe("benchmark state file operations", () => {
  let pending: Deferred[];
  let log: string[];

  beforeEach(() => {
    pending = [];
    log = [];
    vi.mocked(invoke).mockImplementation(
      (command: string) =>
        new Promise<void>((resolve, reject) => {
          log.push(`start ${command}`);
          pending.push({
            resolve: () => {
              log.push(`end ${command}`);
              resolve();
            },
            reject,
          });
        }),
    );
  });

  it("starts a clear only after a write that was already in flight lands", async () => {
    const write = persistBenchmarkState(state);
    const clear = clearPersistedState();
    await Promise.resolve();
    expect(log).toEqual(["start write_benchmark_state"]);

    pending[0]?.resolve();
    await write;
    await vi.waitFor(() => expect(pending).toHaveLength(2));
    pending[1]?.resolve();
    await clear;

    expect(log).toEqual([
      "start write_benchmark_state",
      "end write_benchmark_state",
      "start clear_benchmark_state",
      "end clear_benchmark_state",
    ]);
  });

  it("still clears after an earlier write fails, and reports the write failure to its caller", async () => {
    const write = persistBenchmarkState(state);
    const clear = clearPersistedState();
    await Promise.resolve();

    pending[0]?.reject(new Error("disk full"));
    await expect(write).rejects.toThrow("disk full");
    await vi.waitFor(() => expect(pending).toHaveLength(2));
    pending[1]?.resolve();
    await expect(clear).resolves.toBeUndefined();
    expect(log.at(-1)).toBe("end clear_benchmark_state");
  });
});
