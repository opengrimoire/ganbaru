import { beforeEach, describe, expect, it, vi } from "vitest";

import {
  DEFAULT_BENCHMARK_DATASET,
  DENSE_DATASET_VERSION,
  HARNESS_VERSION,
  type BenchmarkScenario,
  type BenchmarkSeedHandle,
  type BenchmarkState,
} from "$lib/benchmark/types";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn(async () => undefined) }));
vi.mock("$lib/build-info", () => ({ BUILD_REF: "test-build" }));
vi.mock("./calendar.svelte", () => ({ getCalendar: () => ({ eventCount: 0 }) }));
vi.mock("$lib/benchmark/runner", () => ({
  createRunner: vi.fn(),
  persistPhaseAPending: vi.fn(async () => undefined),
  persistPhaseBPending: vi.fn(async () => undefined),
  persistBenchmarkState: vi.fn(async () => undefined),
  restartApp: vi.fn(),
  restartAppAfterDelay: vi.fn(),
  loadPersistedState: vi.fn(),
  clearPersistedState: vi.fn(async () => undefined),
}));
vi.mock("$lib/benchmark/registry", () => ({
  BENCHMARK_SCENARIOS: [],
  getScenarioMetadataById: vi.fn(),
  loadScenarioById: vi.fn(),
}));

const runner = await import("$lib/benchmark/runner");
const registry = await import("$lib/benchmark/registry");

function deferred<T>() {
  let resolve: (value: T) => void = () => {};
  const promise = new Promise<T>((accept) => {
    resolve = accept;
  });
  return { promise, resolve };
}

const seedHandle: BenchmarkSeedHandle = {
  calendarId: "benchmark-calendar",
  eventCount: 10,
  datasetId: "dense-v1-r1y-s1-d1",
  dataset: DEFAULT_BENCHMARK_DATASET,
};

function denseOnlyScenario(seed: BenchmarkScenario["seed"]): BenchmarkScenario {
  return {
    id: "dense-only-test",
    label: "Dense only test",
    description: "",
    workload: { kind: "interaction-latency", question: "", label: "", durationMs: 0, memoryMode: "none" },
    defaultDataset: DEFAULT_BENCHMARK_DATASET,
    runMode: "dense-only",
    setup: async () => {},
    runWorkload: async () => {},
    seed,
    cleanup: async () => {},
  };
}

function phaseAPendingState(scenarioId: string): BenchmarkState {
  const now = new Date().toISOString();
  return {
    scenarioId,
    startedAt: now,
    updatedAt: now,
    harnessVersion: HARNESS_VERSION,
    datasetVersion: DENSE_DATASET_VERSION,
    platform: "linux",
    anchorDate: "2026-11-22",
    vaultMode: "benchmark",
    stage: "phase-a-pending",
  };
}

describe("benchmark runner cancellation", () => {
  beforeEach(() => {
    vi.resetModules();
    vi.clearAllMocks();
  });

  it("aborts the seed and never persists or restarts into the next stage after a cancel", async () => {
    const seedDone = deferred<BenchmarkSeedHandle>();
    let seedSignal: AbortSignal | undefined;
    const scenario = denseOnlyScenario(async (_dataset, _context, signal) => {
      seedSignal = signal;
      return seedDone.promise;
    });
    vi.mocked(registry.getScenarioMetadataById).mockReturnValue(scenario);
    vi.mocked(registry.loadScenarioById).mockResolvedValue(scenario);
    vi.mocked(runner.loadPersistedState).mockResolvedValue(phaseAPendingState(scenario.id));

    const { getBenchmarkRunner } = await import("./benchmark-runner.svelte");
    const store = getBenchmarkRunner();
    const resume = store.checkAndResume();
    await vi.waitFor(() => expect(seedSignal).toBeDefined());

    await store.cancel();
    expect(seedSignal?.aborted).toBe(true);
    vi.mocked(runner.restartApp).mockClear();

    seedDone.resolve(seedHandle);
    await resume;

    expect(runner.persistPhaseBPending).not.toHaveBeenCalled();
    expect(runner.restartApp).not.toHaveBeenCalled();
    expect(store.status).toBe("idle");
  });

  it("does not report a seed aborted by a cancel as a failure", async () => {
    let seedSignal: AbortSignal | undefined;
    const scenario = denseOnlyScenario((_dataset, _context, signal) => {
      seedSignal = signal;
      return new Promise<BenchmarkSeedHandle>((_resolve, reject) => {
        signal.addEventListener("abort", () => reject(signal.reason));
      });
    });
    vi.mocked(registry.getScenarioMetadataById).mockReturnValue(scenario);
    vi.mocked(registry.loadScenarioById).mockResolvedValue(scenario);
    vi.mocked(runner.loadPersistedState).mockResolvedValue(phaseAPendingState(scenario.id));

    const { getBenchmarkRunner } = await import("./benchmark-runner.svelte");
    const store = getBenchmarkRunner();
    const resume = store.checkAndResume();
    await vi.waitFor(() => expect(seedSignal).toBeDefined());
    await store.cancel();
    await resume;

    expect(store.status).toBe("idle");
    expect(store.errorMessage).toBeUndefined();
  });

  it("persists the dense stage and restarts when the seed finishes uncancelled", async () => {
    const scenario = denseOnlyScenario(async () => seedHandle);
    vi.mocked(registry.getScenarioMetadataById).mockReturnValue(scenario);
    vi.mocked(registry.loadScenarioById).mockResolvedValue(scenario);
    vi.mocked(runner.loadPersistedState).mockResolvedValue(phaseAPendingState(scenario.id));

    const { getBenchmarkRunner } = await import("./benchmark-runner.svelte");
    await getBenchmarkRunner().checkAndResume();

    expect(runner.persistPhaseBPending).toHaveBeenCalledWith(expect.objectContaining({ seedHandle }));
    expect(runner.restartApp).toHaveBeenCalledOnce();
  });
});
