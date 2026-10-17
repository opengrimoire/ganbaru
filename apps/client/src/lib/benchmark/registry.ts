/**
 * Registry of installed benchmark scenarios. Adding a scenario means adding
 * lightweight metadata here and a dynamic loader for the matching module.
 * UI paths read only the metadata list so normal app startup never pulls in
 * scenario implementation modules.
 *
 * Order is rendered order; keep startup and memory baselines first so
 * pasted suite output is easy to place in docs/performance/results.md.
 */
import {
  CORE_BENCHMARK_DATASETS,
  DEFAULT_BENCHMARK_DATASET,
  HELD_NAVIGATION_DURATION_MS,
  type BenchmarkScenario,
  type BenchmarkScenarioMetadata,
} from "./types";

type BenchmarkScenarioLoader = () => Promise<BenchmarkScenario>;

export interface BenchmarkSuiteMetadata {
  id: "core" | "backend" | "all";
  label: string;
  description: string;
  scenarioIds: string[];
}

const CORE_BENCHMARK_SCENARIO_IDS = [
  "startup-boot",
  "idle-memory",
  "calendar-nav",
  "calendar-panel-latency",
];

const BACKEND_BENCHMARK_SCENARIO_IDS = ["calendar-import-ops"];

export const BENCHMARK_SCENARIOS: BenchmarkScenarioMetadata[] = [
  {
    id: "startup-boot",
    label: "Startup boot",
    description:
      "Captures repeated process launch samples to usable calendar paint without adding a memory settling window. Use this for startup-time regressions.",
    workload: {
      kind: "startup",
      question: "How fast does the app launch into the calendar?",
      label: "calendar startup launch samples",
      durationMs: 0,
      memoryMode: "none",
    },
    defaultDataset: DEFAULT_BENCHMARK_DATASET,
    benchmarkDatasets: [...CORE_BENCHMARK_DATASETS],
  },
  {
    id: "idle-memory",
    label: "Idle memory",
    description:
      "Loads the fixed anchored week, performs no interaction for the workload window, and reports memory.",
    workload: {
      kind: "idle-memory",
      question: "How much memory does the calendar hold while idle?",
      label: "idle calendar baseline",
      durationMs: 0,
      memoryMode: "post-workload",
    },
    defaultDataset: DEFAULT_BENCHMARK_DATASET,
    benchmarkDatasets: [...CORE_BENCHMARK_DATASETS],
  },
  {
    id: "calendar-nav",
    label: "Calendar week-view nav",
    description:
      "Dispatches initial and repeated ArrowRight keydown events plus keyup for a 3-second hold, using the same window keyboard handler and held-navigation controller as a physical right-arrow hold. It runs against the default dense calendar dataset.",
    workload: {
      kind: "stress-memory",
      question: "How much memory does repeated week navigation use?",
      label: "held right-arrow week-view navigation",
      durationMs: HELD_NAVIGATION_DURATION_MS,
      memoryMode: "post-workload",
    },
    defaultDataset: DEFAULT_BENCHMARK_DATASET,
    runMode: "dense-only",
  },
  {
    id: "calendar-panel-latency",
    label: "Calendar panel latency",
    description:
      "Measures the two calendar panel open actions with 50 runs each: clicking varied existing events and clicking deterministic time slots for create.",
    workload: {
      kind: "interaction-latency",
      question: "How quickly does the calendar panel open from user actions?",
      label: "scripted calendar panel open actions",
      durationMs: 0,
      memoryMode: "none",
    },
    defaultDataset: DEFAULT_BENCHMARK_DATASET,
    runMode: "dense-only",
  },
  {
    id: "calendar-import-ops",
    label: "Calendar import operations",
    description:
      "Measures the Rust calendar_bulk_import command for repeated 100-event imports and one 1000-event add/update pass.",
    workload: {
      kind: "operation-latency",
      question: "How quickly does Rust apply typed calendar import payloads?",
      label: "scripted calendar bulk import commands",
      durationMs: 0,
      memoryMode: "none",
    },
    defaultDataset: DEFAULT_BENCHMARK_DATASET,
    runMode: "dense-only",
  },
];

export const BENCHMARK_SUITES: BenchmarkSuiteMetadata[] = [
  {
    id: "core",
    label: "Core benchmarks",
    description: "Startup, memory, and user-visible interaction latency.",
    scenarioIds: CORE_BENCHMARK_SCENARIO_IDS,
  },
  {
    id: "backend",
    label: "Backend benchmarks",
    description: "Rust-backed calendar import latency.",
    scenarioIds: BACKEND_BENCHMARK_SCENARIO_IDS,
  },
  {
    id: "all",
    label: "All benchmarks",
    description: "Complete core and backend benchmark suite.",
    scenarioIds: [...CORE_BENCHMARK_SCENARIO_IDS, ...BACKEND_BENCHMARK_SCENARIO_IDS],
  },
];

const SCENARIO_LOADERS: Record<string, BenchmarkScenarioLoader> = {
  "startup-boot": () => import("./scenarios/startup-boot").then((module) => module.startupBootScenario),
  "idle-memory": () => import("./scenarios/idle-memory").then((module) => module.idleMemoryScenario),
  "calendar-nav": () => import("./scenarios/calendar-nav").then((module) => module.calendarNavScenario),
  "calendar-panel-latency": () => import("./scenarios/calendar-panel-latency")
    .then((module) => module.calendarPanelLatencyScenario),
  "calendar-import-ops": () => import("./scenarios/calendar-import-ops")
    .then((module) => module.calendarImportOpsScenario),
};

export function getScenarioMetadataById(id: string): BenchmarkScenarioMetadata | undefined {
  return BENCHMARK_SCENARIOS.find((scenario) => scenario.id === id);
}

/**
 * Return registered metadata for a scenario module, so each module shares the registry's single
 * definition of its label, description, workload, and datasets.
 *
 * @throws Error when the id is not registered.
 */
export function requireScenarioMetadata(id: string): BenchmarkScenarioMetadata {
  const metadata = getScenarioMetadataById(id);
  if (!metadata) throw new Error(`Benchmark scenario ${id} is not registered`);
  return metadata;
}

export async function loadScenarioById(id: string): Promise<BenchmarkScenario | undefined> {
  return SCENARIO_LOADERS[id]?.();
}
