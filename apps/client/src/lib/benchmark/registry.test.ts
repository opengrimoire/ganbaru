import { describe, expect, it } from "vitest";
import {
  BENCHMARK_SCENARIOS,
  BENCHMARK_SUITES,
  getScenarioMetadataById,
  loadScenarioById,
} from "./registry";
import {
  CHAT_LONG_THREAD_DATASET,
  CHAT_STREAM_FLOOD_DATASET,
  CORE_BENCHMARK_DATASETS,
  DEFAULT_BENCHMARK_DATASET,
} from "./types";

describe("benchmark registry", () => {
  it("keeps lightweight metadata covered by lazy loaders", () => {
    const ids = new Set<string>();
    for (const metadata of BENCHMARK_SCENARIOS) {
      expect(ids.has(metadata.id)).toBe(false);
      ids.add(metadata.id);
      expect(getScenarioMetadataById(metadata.id)).toEqual(metadata);
    }
  });

  it("does not return metadata or modules for unknown scenarios", async () => {
    expect(getScenarioMetadataById("missing")).toBeUndefined();
    await expect(loadScenarioById("missing")).resolves.toBeUndefined();
  });

  it("keeps suites backed by registered scenarios", () => {
    const scenarioIds = new Set(BENCHMARK_SCENARIOS.map((scenario) => scenario.id));
    for (const suite of BENCHMARK_SUITES) {
      expect(suite.scenarioIds.length).toBeGreaterThan(0);
      for (const scenarioId of suite.scenarioIds) {
        expect(scenarioIds.has(scenarioId)).toBe(true);
      }
    }
    expect(BENCHMARK_SUITES.find((suite) => suite.id === "all")?.scenarioIds)
      .toEqual(BENCHMARK_SCENARIOS.map((scenario) => scenario.id));
  });

  it("keeps core benchmarks user-facing without duplicate idle memory scenarios", () => {
    expect(BENCHMARK_SUITES.find((suite) => suite.id === "core")?.scenarioIds)
      .toEqual([
        "startup-boot",
        "idle-memory",
        "calendar-nav",
        "calendar-panel-latency",
      ]);
  });

  it("keeps backend benchmarks focused on canonical calendar operations", () => {
    expect(BENCHMARK_SUITES.find((suite) => suite.id === "backend")?.scenarioIds)
      .toEqual(["calendar-import-ops"]);
    expect(getScenarioMetadataById("calendar-write-ops")).toBeUndefined();
    expect(getScenarioMetadataById("theme-persistence-ops")).toBeUndefined();
    expect(getScenarioMetadataById("pomodoro-persistence-ops")).toBeUndefined();
  });

  it("runs only startup and idle across total-history dense datasets", () => {
    for (const id of ["startup-boot", "idle-memory"]) {
      const metadata = getScenarioMetadataById(id);
      expect(metadata?.runMode).toBeUndefined();
      expect(metadata?.benchmarkDatasets).toEqual([...CORE_BENCHMARK_DATASETS]);
    }
    for (const id of [
      "calendar-nav",
      "calendar-panel-latency",
      "calendar-import-ops",
    ]) {
      const metadata = getScenarioMetadataById(id);
      expect(metadata).toBeDefined();
      if (!metadata) continue;
      expect(metadata.runMode).toBe("dense-only");
      expect(metadata.benchmarkDatasets ?? [metadata.defaultDataset])
        .toEqual([DEFAULT_BENCHMARK_DATASET]);
    }
  });

  it("runs Chat benchmarks once against their own Chat datasets", () => {
    expect(BENCHMARK_SUITES.find((suite) => suite.id === "chat")?.scenarioIds)
      .toEqual(["chat-timeline-scroll", "chat-stream-flood"]);
    for (const [id, dataset] of [
      ["chat-timeline-scroll", CHAT_LONG_THREAD_DATASET],
      ["chat-stream-flood", CHAT_STREAM_FLOOD_DATASET],
    ] as const) {
      const metadata = getScenarioMetadataById(id);
      expect(metadata?.runMode).toBe("dense-only");
      expect(metadata?.defaultDataset).toEqual(dataset);
      expect(metadata?.benchmarkDatasets).toEqual([dataset]);
    }
  });
});
