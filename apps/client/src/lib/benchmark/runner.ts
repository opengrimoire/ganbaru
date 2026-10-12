/**
 * Pass orchestration for the benchmark harness.
 *
 * Public surface:
 * - `createRunner(...)`: returns `runPhaseA` and `runPhaseB`, which drive the workload for one pass.
 * - `persistPhaseAPending(...)`: write state file before the first restart.
 * - `persistPhaseBPending(...)`: write state file between the two passes.
 * - `persistBenchmarkState(...)`: update the state file without changing the payload.
 * - `loadPersistedState()` / `clearPersistedState()`: state file plumbing.
 *
 * The scenario module owns `setup()`, `runWorkload()`, `seed()`, and
 * `cleanup()`, so the runner stays independent of what each scenario
 * measures.
 */
import { invoke } from "@tauri-apps/api/core";
import {
  HARNESS_VERSION,
  DENSE_DATASET_VERSION,
  resolveBenchmarkAnchorDate,
  isBenchmarkPendingStage,
  isFreshBenchmarkPendingAge,
  isFreshBenchmarkTotalAge,
  type BenchmarkDatasetProfile,
  type BenchmarkScenarioContext,
  type BenchmarkScenario,
  type BenchmarkSeedHandle,
  type BenchmarkSuiteState,
  type BenchmarkState,
  type PhaseResult,
  type SampleLabel,
  type StartupRunState,
} from "./types";
import {
  sampleMemoryObservation,
  captureBootTimings,
} from "./sampler";

/**
 * Run one pass. The scenario provides `setup()` and `runWorkload()`;
 * the runner adds boot timing and, only for memory scenarios, a post-action
 * memory observation window.
 */
async function runPhase(opts: {
  phase: "A" | "B";
  scenario: BenchmarkScenario;
  signal: AbortSignal;
  getEventCount: () => number;
  context: BenchmarkScenarioContext;
  datasetId?: string;
  onCurveProgress?: (label: SampleLabel, total: number, completed: number) => void;
}): Promise<PhaseResult> {
  await opts.scenario.setup(opts.context);
  if (opts.signal.aborted) throw new DOMException("aborted", "AbortError");

  const startedAt = new Date().toISOString();
  const eventCountAtStart = opts.getEventCount();
  const shouldSampleMemory = opts.scenario.workload.memoryMode === "post-workload";

  const workloadStarted = performance.now();
  let maybeMetrics: PhaseResult["metrics"] | void;
  let workloadError: unknown;
  try {
    maybeMetrics = await opts.scenario.runWorkload(opts.signal, opts.context);
  } catch (error: unknown) {
    workloadError = error;
  }
  const workloadDurationMs = performance.now() - workloadStarted;

  if (workloadError) throw workloadError;
  if (opts.signal.aborted) throw new DOMException("aborted", "AbortError");

  let curve = [] as PhaseResult["curve"];
  if (shouldSampleMemory) {
    curve = await sampleMemoryObservation({
      signal: opts.signal,
      onProgress: (label, total, samples) => {
        opts.onCurveProgress?.(label, total, samples.length);
      },
    });
  }

  const boot = captureBootTimings();
  const startupSamples = opts.scenario.workload.kind === "startup"
    ? [{ startedAt, eventCountAtStart, boot }]
    : undefined;

  return {
    phase: opts.phase,
    startedAt,
    workloadDurationMs,
    curve,
    metrics: maybeMetrics ?? undefined,
    boot,
    startupMs: boot.launchTotalMs,
    startupSamples,
    eventCountAtStart,
    datasetId: opts.datasetId,
  };
}

export function createRunner(getEventCount: () => number) {
  return {
    async runPhaseA(opts: {
      scenario: BenchmarkScenario;
      signal: AbortSignal;
      anchorDate: string;
      onCurveProgress?: (label: SampleLabel, total: number, completed: number) => void;
    }): Promise<PhaseResult> {
      return runPhase({
        phase: "A",
        scenario: opts.scenario,
        signal: opts.signal,
        getEventCount,
        context: { anchorDate: opts.anchorDate },
        onCurveProgress: opts.onCurveProgress,
      });
    },

    async runPhaseB(opts: {
      scenario: BenchmarkScenario;
      signal: AbortSignal;
      anchorDate: string;
      datasetId?: string;
      onCurveProgress?: (label: SampleLabel, total: number, completed: number) => void;
    }): Promise<PhaseResult> {
      return runPhase({
        phase: "B",
        scenario: opts.scenario,
        signal: opts.signal,
        getEventCount,
        context: { anchorDate: opts.anchorDate },
        datasetId: opts.datasetId,
        onCurveProgress: opts.onCurveProgress,
      });
    },
  };
}

/**
 * Persist `phase-a-pending` state, written right after the user confirms.
 * The next cold boot opens the isolated benchmark DB (because
 * `vaultMode === "benchmark"`) and the benchmark runner store resumes the
 * baseline pass from `checkAndResume`. The state file lives in
 * `app_config_dir/benchmark-state.json`, not the vault.
 */
export async function persistPhaseAPending(opts: {
  scenarioId: string;
  platform: string;
  buildRef?: string;
  startedAt?: string;
  anchorDate?: string;
  suite?: BenchmarkSuiteState;
  benchmarkDatasets?: BenchmarkDatasetProfile[];
  startupRuns?: StartupRunState;
}): Promise<void> {
  const now = new Date().toISOString();
  const state: BenchmarkState = {
    scenarioId: opts.scenarioId,
    startedAt: opts.startedAt ?? now,
    updatedAt: now,
    harnessVersion: HARNESS_VERSION,
    datasetVersion: DENSE_DATASET_VERSION,
    platform: opts.platform,
    buildRef: opts.buildRef,
    anchorDate: opts.anchorDate ?? resolveBenchmarkAnchorDate(),
    vaultMode: "benchmark",
    stage: "phase-a-pending",
    suite: opts.suite,
    benchmarkDatasets: opts.benchmarkDatasets,
    startupRuns: opts.startupRuns,
  };
  await invoke("write_benchmark_state", { json: JSON.stringify(state) });
}

/**
 * Persist `phase-b-pending` state, written after the baseline pass and
 * before the second restart. Carries the captured baseline result and the
 * seed handle so the dense pass can reference both after the next cold
 * boot.
 */
export async function persistPhaseBPending(opts: {
  scenarioId: string;
  platform: string;
  buildRef?: string;
  startedAt: string;
  anchorDate: string;
  phaseA?: PhaseResult;
  seedHandle: BenchmarkSeedHandle;
  suite?: BenchmarkSuiteState;
  benchmarkDatasets?: BenchmarkDatasetProfile[];
  datasetIndex?: number;
  datasetPhases?: PhaseResult[];
  startupRuns?: StartupRunState;
}): Promise<void> {
  const now = new Date().toISOString();
  const state: BenchmarkState = {
    scenarioId: opts.scenarioId,
    startedAt: opts.startedAt,
    updatedAt: now,
    harnessVersion: HARNESS_VERSION,
    datasetVersion: DENSE_DATASET_VERSION,
    platform: opts.platform,
    buildRef: opts.buildRef,
    anchorDate: opts.anchorDate,
    vaultMode: "benchmark",
    stage: "phase-b-pending",
    phaseA: opts.phaseA,
    seedHandle: opts.seedHandle,
    suite: opts.suite,
    benchmarkDatasets: opts.benchmarkDatasets,
    datasetIndex: opts.datasetIndex,
    datasetPhases: opts.datasetPhases,
    startupRuns: opts.startupRuns,
  };
  await invoke("write_benchmark_state", { json: JSON.stringify(state) });
}

export async function persistBenchmarkState(state: BenchmarkState): Promise<void> {
  await invoke("write_benchmark_state", {
    json: JSON.stringify({ ...state, updatedAt: new Date().toISOString() }),
  });
}

/** Trigger the relaunch. The Rust command never returns; do not await. */
export function restartApp(): void {
  void invoke("restart_app").catch(() => {
    // The IPC can fail when the process exits before replying. Nothing
    // useful can be done here.
  });
}

/** Trigger a relaunch after the current app process has fully exited. */
export function restartAppAfterDelay(delayMs: number): void {
  void invoke("restart_app_after_delay", { delayMs }).catch(() => {
    // If the helper cannot spawn, there is no useful in-process recovery:
    // benchmark state has already been written for the next boot.
  });
}

/**
 * Read the persisted state file from `app_config_dir/benchmark-state.json`.
 * Validates harness version, dataset version, and TTL; if any check fails,
 * deletes the benchmark DB and clears the file silently before returning
 * `null`. A stale state file usually implies a stale benchmark DB beside the
 * vault database, and both must be gone before the next normal boot.
 */
export async function loadPersistedState(): Promise<BenchmarkState | null> {
  const json = await invoke<string | null>("read_benchmark_state");
  if (!json) return null;
  let parsed: BenchmarkState;
  try {
    parsed = JSON.parse(json) as BenchmarkState;
  } catch {
    await discardStaleArtifacts();
    return null;
  }
  if (parsed.harnessVersion !== HARNESS_VERSION) {
    await discardStaleArtifacts();
    return null;
  }
  if (parsed.datasetVersion !== DENSE_DATASET_VERSION) {
    await discardStaleArtifacts();
    return null;
  }
  if (parsed.stage === "phase-a-running" || parsed.stage === "phase-b-running") {
    await discardStaleArtifacts();
    return null;
  }
  if (!isBenchmarkPendingStage(parsed.stage)) {
    await discardStaleArtifacts();
    return null;
  }
  if (!isFreshBenchmarkTotalAge(parsed) || !isFreshBenchmarkPendingAge(parsed)) {
    await discardStaleArtifacts();
    return null;
  }
  return parsed;
}

async function discardStaleArtifacts(): Promise<void> {
  try {
    await invoke("teardown_benchmark_db");
  } catch (e) {
    console.error("benchmark teardown (stale) failed", e);
  }
  await clearPersistedState();
}

export async function clearPersistedState(): Promise<void> {
  await invoke("clear_benchmark_state");
}
