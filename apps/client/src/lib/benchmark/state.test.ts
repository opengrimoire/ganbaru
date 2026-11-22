import { describe, expect, it } from "vitest";

import { parseBenchmarkState } from "./state";
import {
  CHAT_LONG_THREAD_DATASET,
  DEFAULT_BENCHMARK_DATASET,
  DENSE_DATASET_VERSION,
  HARNESS_VERSION,
  type PhaseResult,
} from "./types";

const phase: PhaseResult = {
  phase: "A",
  startedAt: "2026-11-22T10:00:00.000Z",
  workloadDurationMs: 1200,
  curve: [],
  boot: { marks: { "boot.script-start": 0 } },
  eventCountAtStart: 0,
};

const base = {
  scenarioId: "calendar-nav",
  startedAt: "2026-11-22T10:00:00.000Z",
  updatedAt: "2026-11-22T10:01:00.000Z",
  harnessVersion: HARNESS_VERSION,
  datasetVersion: DENSE_DATASET_VERSION,
  platform: "linux",
  anchorDate: "2026-11-22",
  vaultMode: "benchmark",
};

const seedHandle = {
  calendarId: "calendar-1",
  eventCount: 8760,
  datasetId: "dense-v1-r1y-s1-d1",
  dataset: DEFAULT_BENCHMARK_DATASET,
};

function parse(value: unknown) {
  return parseBenchmarkState(JSON.stringify(value));
}

describe("parseBenchmarkState", () => {
  it("reads a baseline stage with its suite and datasets", () => {
    const state = parse({
      ...base,
      stage: "phase-a-pending",
      benchmarkDatasets: [DEFAULT_BENCHMARK_DATASET, CHAT_LONG_THREAD_DATASET],
      suite: { scenarioIds: ["calendar-nav", "idle-memory"], index: 0, results: [] },
    });
    expect(state?.stage).toBe("phase-a-pending");
    expect(state?.benchmarkDatasets).toHaveLength(2);
  });

  it("reads a dense stage with the seed handle and earlier passes", () => {
    const state = parse({
      ...base,
      stage: "phase-b-pending",
      phaseA: phase,
      seedHandle,
      datasetIndex: 1,
      datasetPhases: [{ ...phase, phase: "B", datasetId: "dense-v1-r1y-s1-d1" }],
    });
    expect(state?.stage).toBe("phase-b-pending");
    if (state?.stage !== "phase-b-pending") return;
    expect(state.seedHandle).toEqual(seedHandle);
    expect(state.datasetPhases).toHaveLength(1);
  });

  it("rejects a dense stage without a seed handle", () => {
    expect(parse({ ...base, stage: "phase-b-pending", phaseA: phase })).toBeNull();
  });

  it("rejects an unknown stage or vault mode", () => {
    expect(parse({ ...base, stage: "phase-c-pending" })).toBeNull();
    expect(parse({ ...base, stage: "phase-a-pending", vaultMode: "other" })).toBeNull();
  });

  it("rejects malformed nested values that the runner would read", () => {
    expect(parse({ ...base, stage: "phase-a-pending", suite: { scenarioIds: ["a"], index: 3, results: [] } })).toBeNull();
    expect(parse({ ...base, stage: "phase-a-pending", startupRuns: { targetRuns: 5, phaseA: [{}], phaseB: [] } }))
      .toBeNull();
    expect(parse({ ...base, stage: "phase-a-pending", benchmarkDatasets: [{ kind: "dense-calendar" }] })).toBeNull();
    expect(parse({ ...base, stage: "phase-b-pending", seedHandle: { ...seedHandle, eventCount: "many" } })).toBeNull();
    expect(parse({ ...base, stage: "phase-b-pending", seedHandle, datasetIndex: -1 })).toBeNull();
  });

  it("rejects missing required fields, non-object JSON, and invalid JSON", () => {
    const { scenarioId: _scenarioId, ...withoutScenario } = base;
    expect(parse({ ...withoutScenario, stage: "phase-a-pending" })).toBeNull();
    expect(parse([base])).toBeNull();
    expect(parse(null)).toBeNull();
    expect(parseBenchmarkState("{not json")).toBeNull();
  });

  it("keeps an outdated harness version for the callers to discard", () => {
    expect(parse({ ...base, harnessVersion: "0", stage: "phase-a-pending" })?.harnessVersion).toBe("0");
  });
});
