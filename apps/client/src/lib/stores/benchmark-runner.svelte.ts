/**
 * Cross-component runner state for the in-app benchmark harness.
 *
 * `App.svelte` mounts the benchmark overlay and resumes pending runs at boot.
 * Settings panels call `request()` to ask for a run; the overlay reacts to
 * status transitions.
 *
 * Flow (cold-cold against an isolated benchmark DB):
 *
 *   idle
 *     -> user clicks Run
 *   confirming
 *     -> user confirms
 *     -> prepare_benchmark_db (idempotent delete of any prior file)
 *     -> persistPhaseAPending(...)
 *     -> restart app, delayed for startup samples            # restart 1
 *   [boot, vaultMode=benchmark, stage=phase-a-pending]
 *     -> Rust database layer opens benchmark.sqlite (empty)
 *     -> stage becomes phase-a-running
 *     -> checkAndResume runs the baseline pass, or skips it for dense-only scenarios
 *     -> scenario.seed(dense dataset)
 *     -> persistPhaseBPending(...)
 *     -> restart app, delayed for startup samples            # restart 2
 *   [boot, vaultMode=benchmark, stage=phase-b-pending]
 *     -> Rust database layer opens benchmark.sqlite (now seeded)
 *     -> stage becomes phase-b-running
 *     -> checkAndResume runs the dense pass
 *     -> if more dense datasets exist, seed the next dataset and repeat phase B
 *     -> teardown_benchmark_db (delete files)
 *     -> clear_benchmark_state
 *     -> show summary
 *   summary
 *     -> user clicks Close
 *     -> restart_app                                         # restart 3
 *   [boot, no state, default]
 *     -> Rust database layer opens user DB (untouched throughout)
 *   idle
 *
 * A later boot that sees a `*-running` stage treats the previous process as
 * killed mid-pass, discards the benchmark DB, clears state, and opens the
 * user's real DB. The user's real DB and vault are never opened during an
 * intentional run.
 */
import { invoke } from "@tauri-apps/api/core";
import { BUILD_REF } from "$lib/build-info";
import { formatNumber } from "$lib/i18n/formatters";
import { getLocalization } from "$lib/i18n/translator.svelte";
import { getCalendar } from "./calendar.svelte";
import { getBenchmarkStatus, type BenchmarkStatus } from "./benchmark-status.svelte";
import {
  STARTUP_RELAUNCH_COOLDOWN_MS,
  STARTUP_SAMPLE_RUNS,
  benchmarkDatasetId,
  type BootTimings,
  type BenchmarkDatasetProfile,
  type BenchmarkResult,
  type BenchmarkSeedHandle,
  type BenchmarkSuiteState,
  type BenchmarkState,
  type BenchmarkScenario,
  type BenchmarkScenarioMetadata,
  type PhaseResult,
  type SampleLabel,
  type StartupBootSample,
  type StartupRunState,
} from "$lib/benchmark/types";
import {
  createRunner,
  persistPhaseAPending,
  persistPhaseBPending,
  persistBenchmarkState,
  restartApp,
  restartAppAfterDelay,
  loadPersistedState,
  clearPersistedState,
} from "$lib/benchmark/runner";
import {
  BENCHMARK_SCENARIOS,
  getScenarioMetadataById,
  loadScenarioById,
} from "$lib/benchmark/registry";

const localization = getLocalization();
const { t } = localization;

type PendingRequest =
  | { mode: "single"; scenarioIds: string[] }
  | { mode: "suite"; scenarioIds: string[]; suiteLabel: string };

export interface BenchmarkRunProgress {
  phase: "A" | "B";
  scenarioId: string;
  scenarioLabel: string;
  suite?: { index: number; total: number };
  /** Short user-facing label for the current step. */
  step: string;
  /** Current dataset id when phase B is running. */
  datasetId?: string;
  /** Set during memory observation so the overlay can show progress. */
  curve?: { done: number; total: number; label: SampleLabel };
}

class BenchmarkRunnerStore {
  #benchmarkStatus = getBenchmarkStatus();

  get status(): BenchmarkStatus {
    return this.#benchmarkStatus.status;
  }

  set status(status: BenchmarkStatus) {
    this.#benchmarkStatus.setStatus(status);
  }

  running = $state<BenchmarkRunProgress | undefined>(undefined);
  result = $state<BenchmarkResult | null>(null);
  results = $state<BenchmarkResult[]>([]);
  errorMessage = $state<string | undefined>(undefined);
  pendingRequest = $state<PendingRequest | undefined>(undefined);

  #abort: AbortController | null = null;

  /** Open the confirmation dialog for `scenarioId`. */
  request(scenarioId: string) {
    if (this.status !== "idle") return;
    this.pendingRequest = { mode: "single", scenarioIds: [scenarioId] };
    this.status = "confirming";
  }

  /** Open the confirmation dialog for every registered scenario. */
  requestAll() {
    this.requestSuite(
      BENCHMARK_SCENARIOS.map((scenario) => scenario.id),
      "all benchmarks",
    );
  }

  /** Open the confirmation dialog for an arbitrary benchmark suite. */
  requestSuite(scenarioIds: string[], suiteLabel: string) {
    if (this.status !== "idle") return;
    if (scenarioIds.length === 0) return;
    this.pendingRequest = {
      mode: "suite",
      scenarioIds: [...scenarioIds],
      suiteLabel,
    };
    this.status = "confirming";
  }

  cancelConfirm() {
    if (this.status !== "confirming") return;
    this.pendingRequest = undefined;
    this.status = "idle";
  }

  get pendingMode(): "single" | "suite" | undefined {
    return this.pendingRequest?.mode;
  }

  get pendingSuiteLabel(): string | undefined {
    return this.pendingRequest?.mode === "suite" ? this.pendingRequest.suiteLabel : undefined;
  }

  get pendingScenarioLabels(): string[] {
    return (this.pendingRequest?.scenarioIds ?? [])
      .map((id) => getScenarioMetadataById(id)?.label ?? id);
  }

  /**
   * The user accepted the dialog. Wipe any stale benchmark DB, write the
   * `phase-a-pending` marker, and restart. The baseline pass itself runs
   * after the restart against the freshly-opened isolated DB.
   */
  async confirm() {
    if (this.status !== "confirming") return;
    const pending = this.pendingRequest;
    const scenarioId = pending?.scenarioIds[0];
    if (!pending || !scenarioId) return;
    const scenario = getScenarioMetadataById(scenarioId);
    if (!scenario) {
      this.errorMessage = `Unknown scenario: ${scenarioId}`;
      this.status = "error";
      return;
    }
    this.pendingRequest = undefined;

    this.status = "running";
    this.running = {
      phase: "A",
      scenarioId: scenario.id,
      scenarioLabel: scenario.label,
      suite: pending.mode === "suite" ? { index: 0, total: pending.scenarioIds.length } : undefined,
      step: isStartupScenario(scenario)
        ? `Closing for ${startupCooldownSeconds()} s startup cooldown`
        : isDenseOnlyScenario(scenario)
          ? "Restarting to seed dense dataset"
          : "Restarting for baseline dataset",
    };

    try {
      await invoke("prepare_benchmark_db");
      const platform = await this.#detectPlatform();
      const benchmarkDatasets = benchmarkDatasetsForScenario(scenario);
      await persistPhaseAPending({
        scenarioId: scenario.id,
        platform,
        buildRef: BUILD_REF,
        benchmarkDatasets,
        suite: pending.mode === "suite"
          ? { scenarioIds: pending.scenarioIds, index: 0, results: [] }
          : undefined,
      });
    } catch (e) {
      this.#failWith(`Setup failed: ${this.#describeError(e)}`);
      return;
    }

    restartForScenario(scenario);
  }

  /**
   * Aborts whatever is running, deletes the benchmark DB, clears state,
   * and restarts the app so the user lands back on their real DB. Safe to
   * call from any non-idle status.
   */
  async cancel() {
    this.#abort?.abort();
    try {
      await invoke("teardown_benchmark_db");
    } catch (e) {
      console.error("benchmark teardown failed", e);
    }
    try {
      await clearPersistedState();
    } catch (e) {
      console.error("benchmark clear state failed", e);
    }
    this.#reset();
    // Restart so the next boot opens the user DB. The database URL is
    // resolved once per process, so this session would otherwise keep
    // targeting the deleted benchmark DB.
    restartApp();
  }

  /**
   * The summary is a final-state UI; closing it tears down the benchmark
   * DB and restarts so the user resumes against their real DB.
   */
  closeSummary() {
    if (this.status !== "summary" && this.status !== "error") return;
    void this.#finishAndRestart();
  }

  async #finishAndRestart() {
    try {
      await invoke("teardown_benchmark_db");
    } catch (e) {
      console.error("benchmark teardown failed", e);
    }
    try {
      await clearPersistedState();
    } catch (e) {
      console.error("benchmark clear state failed", e);
    }
    this.#reset();
    restartApp();
  }

  /**
   * Boot-time entry point. Reads the persisted state file and dispatches
   * on `state.stage`. Stale or invalid state is silently cleaned up; if
   * no state exists, this is a no-op and the app behaves as a normal boot.
   */
  async checkAndResume(): Promise<boolean> {
    if (this.status !== "idle") return false;
    let state: BenchmarkState | null;
    try {
      state = await loadPersistedState();
    } catch (e) {
      console.error("benchmark loadPersistedState failed", e);
      return false;
    }
    if (!state) return false;

    // `loadPersistedState` already wipes the benchmark DB and clears state
    // on TTL, version mismatch, or an interrupted `*-running` stage.
    // Anything that reaches here has a fresh state file pointing at a valid
    // pending stage. Still guard against an unregistered scenario and
    // against a `phase-b-pending` state that is missing the data
    // needed to run it.
    const scenarioMetadata = getScenarioMetadataById(state.scenarioId);
    if (!scenarioMetadata) {
      await this.#abandonStale();
      return false;
    }
    let scenario: BenchmarkScenario | undefined;
    try {
      scenario = await loadScenarioById(state.scenarioId);
    } catch (e) {
      this.#failWith(`Loading scenario failed: ${this.#describeError(e)}`);
      return true;
    }
    if (!scenario) {
      await this.#abandonStale();
      return false;
    }

    if (state.stage === "phase-a-pending") {
      await this.#runPhaseAThenSeed(scenario, state);
      return true;
    } else if (state.stage === "phase-b-pending") {
      if (!state.seedHandle || (!state.phaseA && !isDenseOnlyScenario(scenario))) {
        await this.#abandonStale();
        return false;
      }
      await this.#runPhaseB(scenario, state, state.phaseA, state.seedHandle);
      return true;
    } else {
      await this.#abandonStale();
      return false;
    }
  }

  async #abandonStale() {
    try {
      await invoke("teardown_benchmark_db");
    } catch (e) {
      console.error("benchmark teardown failed", e);
    }
    try {
      await clearPersistedState();
    } catch (e) {
      console.error("benchmark clear state failed", e);
    }
  }

  async #runPhaseAThenSeed(scenario: BenchmarkScenario, state: BenchmarkState) {
    this.status = "running";
    this.errorMessage = undefined;
    this.running = {
      phase: "A",
      scenarioId: scenario.id,
      scenarioLabel: scenario.label,
      suite: state.suite
        ? { index: state.suite.index, total: state.suite.scenarioIds.length }
        : undefined,
      step: "Setting up",
    };
    this.#abort = new AbortController();
    try {
      await persistBenchmarkState({ ...state, stage: "phase-a-running" });
    } catch (e) {
      this.#failWith(`Persisting state failed: ${this.#describeError(e)}`);
      return;
    }

    let phaseA: PhaseResult | undefined;
    let startupRuns = state.startupRuns;
    if (!isDenseOnlyScenario(scenario)) {
      const calendarStore = getCalendar();
      const runner = createRunner(() => calendarStore.eventCount);
      try {
        this.#updateStep(this.#workloadStep(scenario, state, "A"));
        phaseA = await runner.runPhaseA({
          scenario,
          signal: this.#abort.signal,
          anchorDate: state.anchorDate,
          onCurveProgress: (label, total, done) => {
            this.#updateCurve({ label, total, done });
          },
        });
      } catch (e) {
        if (this.#isAbort(e)) return;
        this.#failWith(`Baseline dataset failed: ${this.#describeError(e)}`);
        return;
      }
      if (this.#abort?.signal.aborted) return;

      if (isStartupScenario(scenario)) {
        startupRuns = appendStartupPhaseRun(startupRuns, "phaseA", phaseA);
        if (startupRuns.phaseA.length < startupRuns.targetRuns) {
          const nextSample = startupRuns.phaseA.length + 1;
          this.#updateStep(
            `Closing for ${startupCooldownSeconds()} s cooldown before baseline launch ${nextSample}/${startupRuns.targetRuns}`,
            true,
          );
          try {
            await persistPhaseAPending({
              scenarioId: scenario.id,
              platform: state.platform,
              buildRef: state.buildRef ?? BUILD_REF,
              startedAt: state.startedAt,
              anchorDate: state.anchorDate,
              suite: state.suite,
              benchmarkDatasets: state.benchmarkDatasets,
              startupRuns,
            });
          } catch (e) {
            this.#failWith(`Persisting state failed: ${this.#describeError(e)}`);
            return;
          }
          restartForScenario(scenario);
          return;
        }
        phaseA = aggregateStartupPhase("A", startupRuns.phaseA);
      }
    }

    const benchmarkDatasets = state.benchmarkDatasets ?? benchmarkDatasetsForScenario(scenario);
    const dataset = benchmarkDatasets[0] ?? scenario.defaultDataset;
    this.#updateStep(`Seeding ${benchmarkDatasetId(dataset)}`, /* clearCurve */ true);
    let seedHandle: BenchmarkSeedHandle;
    try {
      seedHandle = await scenario.seed(dataset, { anchorDate: state.anchorDate });
    } catch (e) {
      this.#failWith(`Seeding failed: ${this.#describeError(e)}`);
      return;
    }
    if (this.#abort?.signal.aborted) return;

    this.#updateStep(
      isStartupScenario(scenario)
        ? `Closing for ${startupCooldownSeconds()} s cooldown before dense launch 1/${STARTUP_SAMPLE_RUNS}`
        : "Restarting for dense dataset",
    );
    try {
      await persistPhaseBPending({
        scenarioId: scenario.id,
        platform: state.platform,
        buildRef: state.buildRef ?? BUILD_REF,
        startedAt: state.startedAt,
        anchorDate: state.anchorDate,
        phaseA,
        seedHandle,
        suite: state.suite,
        benchmarkDatasets,
        datasetIndex: 0,
        datasetPhases: [],
        startupRuns,
      });
    } catch (e) {
      this.#failWith(`Persisting state failed: ${this.#describeError(e)}`);
      return;
    }

    restartForScenario(scenario);
  }

  async #runPhaseB(
    scenario: BenchmarkScenario,
    state: BenchmarkState,
    phaseA: PhaseResult | undefined,
    seedHandle: BenchmarkSeedHandle,
  ) {
    this.status = "running";
    this.errorMessage = undefined;
    this.running = {
      phase: "B",
      scenarioId: scenario.id,
      scenarioLabel: scenario.label,
      suite: state.suite
        ? { index: state.suite.index, total: state.suite.scenarioIds.length }
        : undefined,
      step: "Setting up",
      datasetId: seedHandle.datasetId,
    };
    this.#abort = new AbortController();
    const calendarStore = getCalendar();
    const runner = createRunner(() => calendarStore.eventCount);
    try {
      await persistBenchmarkState({ ...state, stage: "phase-b-running" });
    } catch (e) {
      this.#failWith(`Persisting state failed: ${this.#describeError(e)}`);
      return;
    }

    let phaseB: PhaseResult;
    try {
      this.#updateStep(this.#workloadStep(scenario, state, "B"));
      phaseB = await runner.runPhaseB({
        scenario,
        signal: this.#abort.signal,
        anchorDate: state.anchorDate,
        datasetId: seedHandle.datasetId,
        onCurveProgress: (label, total, done) => {
          this.#updateCurve({ label, total, done });
        },
      });
    } catch (e) {
      if (this.#isAbort(e)) return;
      this.#failWith(`Dense dataset failed: ${this.#describeError(e)}`);
      return;
    }
    if (this.#abort?.signal.aborted) return;

    const benchmarkDatasets = state.benchmarkDatasets ?? benchmarkDatasetsForScenario(scenario);
    const datasetIndex = state.datasetIndex ?? 0;
    let finalPhaseA = phaseA;
    let startupRuns = state.startupRuns;
    if (isStartupScenario(scenario)) {
      if (!phaseA) {
        this.#failWith("Startup benchmark is missing the baseline phase");
        return;
      }
      startupRuns = appendStartupPhaseRun(startupRuns, "phaseB", phaseB);
      if (startupRuns.phaseB.length < startupRuns.targetRuns) {
        const nextSample = startupRuns.phaseB.length + 1;
        this.#updateStep(
          `Closing for ${startupCooldownSeconds()} s cooldown before dense launch ${nextSample}/${startupRuns.targetRuns}`,
          true,
        );
        try {
          await persistPhaseBPending({
            scenarioId: scenario.id,
            platform: state.platform,
            buildRef: state.buildRef ?? BUILD_REF,
            startedAt: state.startedAt,
            anchorDate: state.anchorDate,
            phaseA,
            seedHandle,
            suite: state.suite,
            benchmarkDatasets,
            datasetIndex,
            datasetPhases: state.datasetPhases,
            startupRuns,
          });
        } catch (e) {
          this.#failWith(`Persisting state failed: ${this.#describeError(e)}`);
          return;
        }
        restartForScenario(scenario);
        return;
      }
      finalPhaseA = aggregateStartupPhase("A", startupRuns.phaseA);
      phaseB = {
        ...aggregateStartupPhase("B", startupRuns.phaseB),
        datasetId: seedHandle.datasetId,
      };
    }

    const datasetPhases = [...(state.datasetPhases ?? []), phaseB];
    if (datasetIndex < benchmarkDatasets.length - 1) {
      const nextDatasetIndex = datasetIndex + 1;
      const nextDataset = benchmarkDatasets[nextDatasetIndex];
      if (nextDataset === undefined) {
        this.#failWith("Benchmark dataset is missing");
        return;
      }
      this.#updateStep(`Seeding ${benchmarkDatasetId(nextDataset)}`, /* clearCurve */ true);
      let nextSeedHandle: BenchmarkSeedHandle;
      try {
        nextSeedHandle = await scenario.seed(nextDataset, { anchorDate: state.anchorDate });
      } catch (e) {
        this.#failWith(`Seeding failed: ${this.#describeError(e)}`);
        return;
      }
      if (this.#abort?.signal.aborted) return;

      this.#updateStep(
        isStartupScenario(scenario)
          ? `Closing for ${startupCooldownSeconds()} s cooldown before dense launch 1/${STARTUP_SAMPLE_RUNS}`
          : "Restarting for next dense dataset",
      );
      try {
        await persistPhaseBPending({
          scenarioId: scenario.id,
          platform: state.platform,
          buildRef: state.buildRef ?? BUILD_REF,
          startedAt: state.startedAt,
          anchorDate: state.anchorDate,
          phaseA: finalPhaseA,
          seedHandle: nextSeedHandle,
          suite: state.suite,
          benchmarkDatasets,
          datasetIndex: nextDatasetIndex,
          datasetPhases,
          startupRuns: isStartupScenario(scenario)
            ? resetDenseStartupRuns(startupRuns)
            : startupRuns,
        });
      } catch (e) {
        this.#failWith(`Persisting state failed: ${this.#describeError(e)}`);
        return;
      }
      restartForScenario(scenario);
      return;
    }

    const observedTotals = [
      ...(finalPhaseA ? [finalPhaseA] : []),
      ...datasetPhases,
    ]
      .flatMap((phase) => phase.curve)
      .map((s) => s.totalMb);
    const peakTotalMb = observedTotals.length > 0 ? Math.max(...observedTotals) : undefined;

    const result: BenchmarkResult = {
      scenarioId: scenario.id,
      scenarioLabel: scenario.label,
      workload: scenario.workload,
      datasetVersion: state.datasetVersion,
      harnessVersion: state.harnessVersion,
      platform: state.platform,
      buildRef: state.buildRef ?? BUILD_REF,
      phaseA: finalPhaseA,
      phaseB: datasetPhases[0] ?? phaseB,
      datasetPhases,
      peakTotalMb,
      anchorDate: state.anchorDate,
    };

    const suite = state.suite;
    if (suite && suite.index < suite.scenarioIds.length - 1) {
      await this.#continueSuite(
        suite,
        result,
        state.platform,
        state.buildRef ?? BUILD_REF,
        state.anchorDate,
      );
      return;
    }

    const finalResults = suite ? [...suite.results, result] : [result];
    this.result = result;
    this.results = finalResults;
    await this.#teardownFinishedBenchmark();
    this.running = undefined;
    this.status = "summary";
    notifyBenchmarkDone(
      suite
        ? t("benchmark.notification.suiteCompleteTitle")
        : t("benchmark.notification.completeTitle"),
      suite
        ? t("benchmark.notification.suiteCompleteBody", this.results.length)
        : this.#completionMessage(scenario, result),
    );
  }

  async #continueSuite(
    suite: BenchmarkSuiteState,
    result: BenchmarkResult,
    platform: string,
    buildRef: string,
    anchorDate: string,
  ): Promise<void> {
    const nextIndex = suite.index + 1;
    const nextScenarioId = suite.scenarioIds[nextIndex];
    const nextScenario = getScenarioMetadataById(nextScenarioId);
    if (!nextScenario) {
      this.#failWith(`Unknown suite scenario: ${nextScenarioId}`);
      return;
    }
    const nextSuite: BenchmarkSuiteState = {
      scenarioIds: suite.scenarioIds,
      index: nextIndex,
      results: [...suite.results, result],
    };
    this.result = result;
    this.results = nextSuite.results;
    this.running = {
      phase: "A",
      scenarioId: nextScenario.id,
      scenarioLabel: nextScenario.label,
      suite: { index: nextIndex, total: suite.scenarioIds.length },
      step: isStartupScenario(nextScenario)
        ? `Closing for ${startupCooldownSeconds()} s startup cooldown`
        : isDenseOnlyScenario(nextScenario)
          ? "Restarting to seed dense dataset"
          : "Restarting for next benchmark",
    };
    try {
      await invoke("prepare_benchmark_db");
      await persistPhaseAPending({
        scenarioId: nextScenario.id,
        platform,
        buildRef,
        anchorDate,
        benchmarkDatasets: benchmarkDatasetsForScenario(nextScenario),
        suite: nextSuite,
      });
    } catch (e) {
      this.#failWith(`Preparing next benchmark failed: ${this.#describeError(e)}`);
      return;
    }
    restartForScenario(nextScenario);
  }

  #updateStep(step: string, clearCurve = false) {
    if (!this.running) return;
    this.running = clearCurve
      ? { ...this.running, step, curve: undefined }
      : { ...this.running, step };
  }

  #updateCurve(curve: { done: number; total: number; label: SampleLabel }) {
    if (!this.running) return;
    this.running = { ...this.running, step: "Memory observation", curve };
  }

  #workloadStep(
    scenario: BenchmarkScenario,
    state: BenchmarkState,
    phase: "A" | "B",
  ): string {
    if (isStartupScenario(scenario)) {
      const runs = normalizeStartupRunState(state.startupRuns);
      const done = phase === "A" ? runs.phaseA.length : runs.phaseB.length;
      return `Launch sample ${done + 1}/${runs.targetRuns}`;
    }
    if (scenario.workload.memoryMode === "post-workload" && scenario.workload.durationMs > 0) {
      const seconds = Math.round(scenario.workload.durationMs / 1000);
      return `${scenario.workload.label}: ${seconds} s`;
    }
    return scenario.workload.memoryMode === "post-workload"
      ? "Preparing memory observation"
      : scenario.workload.label;
  }

  #completionMessage(scenario: BenchmarkScenario, result: BenchmarkResult): string {
    const label = localizedBenchmarkScenarioLabel(scenario.id, scenario.label);
    const datasetPhases = resultDatasetPhases(result);
    const largestDataset = datasetPhases[datasetPhases.length - 1] ?? result.phaseB;
    if (result.workload.kind === "startup") {
      const baseMs = formatOptionalMs(result.phaseA?.startupMs);
      const denseMs = formatOptionalMs(largestDataset.startupMs);
      return t("benchmark.notification.startupCompleteBody", label, baseMs, denseMs);
    }
    if (result.peakTotalMb !== undefined) {
      const mb = `${formatNumber(localization.locale, Math.round(result.peakTotalMb))} MB`;
      return t("benchmark.notification.memoryCompleteBody", label, mb);
    }
    const metricCount = [
      ...(result.phaseA?.metrics ?? []),
      ...datasetPhases.flatMap((phase) => phase.metrics ?? []),
    ].length;
    return t("benchmark.notification.metricsCompleteBody", label, metricCount);
  }

  async #teardownFinishedBenchmark(): Promise<void> {
    try {
      await invoke("teardown_benchmark_db");
    } catch (e) {
      console.error("benchmark teardown after summary failed", e);
    }
    try {
      await clearPersistedState();
    } catch (e) {
      console.error("benchmark clear state after summary failed", e);
    }
  }

  #failWith(message: string) {
    this.errorMessage = message;
    this.running = undefined;
    this.status = "error";
    notifyBenchmarkDone(t("benchmark.notification.failedTitle"), message);
  }

  #reset() {
    this.status = "idle";
    this.running = undefined;
    this.result = null;
    this.results = [];
    this.errorMessage = undefined;
    this.pendingRequest = undefined;
    this.#abort = null;
  }

  #isAbort(e: unknown): boolean {
    return e instanceof DOMException && e.name === "AbortError";
  }

  #describeError(e: unknown): string {
    return e instanceof Error ? e.message : String(e);
  }

  async #detectPlatform(): Promise<string> {
    try {
      const report = await invoke<{ platform: string }>("memory_report");
      return report.platform;
    } catch {
      return "unknown";
    }
  }
}

let store: BenchmarkRunnerStore | null = null;

export function getBenchmarkRunner(): BenchmarkRunnerStore {
  if (!store) store = new BenchmarkRunnerStore();
  return store;
}

function localizedBenchmarkScenarioLabel(id: string, fallback: string): string {
  if (id === "startup-boot") return t("benchmark.scenario.startupBoot.label");
  if (id === "idle-memory") return t("benchmark.scenario.idleMemory.label");
  if (id === "calendar-nav") return t("benchmark.scenario.calendarNav.label");
  if (id === "calendar-panel-latency") {
    return t("benchmark.scenario.calendarPanelLatency.label");
  }
  if (id === "calendar-import-ops") return t("benchmark.scenario.calendarImportOps.label");
  return fallback;
}

/** Fires the OS desktop notification used by terminal benchmark states. */
function notifyBenchmarkDone(title: string, body: string): void {
  invoke("show_benchmark_notification", { title, body }).catch((e) => {
    console.warn("Benchmark notification failed:", e);
  });
}

function isStartupScenario(scenario: BenchmarkScenarioMetadata): boolean {
  return scenario.workload.kind === "startup";
}

function isDenseOnlyScenario(scenario: BenchmarkScenarioMetadata): boolean {
  return scenario.runMode === "dense-only";
}

function restartForScenario(scenario: BenchmarkScenarioMetadata): void {
  if (isStartupScenario(scenario)) {
    restartAppAfterDelay(STARTUP_RELAUNCH_COOLDOWN_MS);
    return;
  }
  restartApp();
}

function startupCooldownSeconds(): number {
  return Math.round(STARTUP_RELAUNCH_COOLDOWN_MS / 1000);
}

function normalizeStartupRunState(state: StartupRunState | undefined): StartupRunState {
  return {
    targetRuns: state?.targetRuns && state.targetRuns > 0 ? state.targetRuns : STARTUP_SAMPLE_RUNS,
    phaseA: state?.phaseA ?? [],
    phaseB: state?.phaseB ?? [],
  };
}

function resetDenseStartupRuns(state: StartupRunState | undefined): StartupRunState {
  const current = normalizeStartupRunState(state);
  return {
    targetRuns: current.targetRuns,
    phaseA: current.phaseA,
    phaseB: [],
  };
}

function benchmarkDatasetsForScenario(scenario: BenchmarkScenarioMetadata): BenchmarkDatasetProfile[] {
  const datasets = scenario.benchmarkDatasets && scenario.benchmarkDatasets.length > 0
    ? scenario.benchmarkDatasets
    : [scenario.defaultDataset];
  const byId = new Map<string, BenchmarkDatasetProfile>();
  for (const dataset of datasets) {
    byId.set(benchmarkDatasetId(dataset), dataset);
  }
  const normalized = [...byId.values()]
    .filter((dataset) =>
      Number.isInteger(dataset.yearRadius)
      && dataset.yearRadius > 0
      && Number.isInteger(dataset.stackCount)
      && dataset.stackCount > 0,
    )
    .sort((a, b) => a.yearRadius - b.yearRadius || a.stackCount - b.stackCount);
  return normalized.length > 0 ? normalized : [scenario.defaultDataset];
}

function resultDatasetPhases(result: BenchmarkResult): PhaseResult[] {
  return result.datasetPhases && result.datasetPhases.length > 0
    ? result.datasetPhases
    : [result.phaseB];
}

function appendStartupPhaseRun(
  state: StartupRunState | undefined,
  phaseKey: "phaseA" | "phaseB",
  phaseResult: PhaseResult,
): StartupRunState {
  const current = normalizeStartupRunState(state);
  return {
    targetRuns: current.targetRuns,
    phaseA: phaseKey === "phaseA" ? [...current.phaseA, phaseResult] : current.phaseA,
    phaseB: phaseKey === "phaseB" ? [...current.phaseB, phaseResult] : current.phaseB,
  };
}

function aggregateStartupPhase(phase: "A" | "B", phases: PhaseResult[]): PhaseResult {
  const fallbackPhase = phases[0];
  const startupSamples = phases.flatMap(startupSamplesFromPhase);
  const boot = aggregateBootTimings(startupSamples);
  return {
    phase,
    startedAt: startupSamples[0]?.startedAt ?? fallbackPhase?.startedAt ?? new Date().toISOString(),
    workloadDurationMs: average(phases.map((sample) => sample.workloadDurationMs)),
    curve: [],
    metrics: undefined,
    boot,
    startupMs: boot.launchTotalMs,
    startupSamples,
    eventCountAtStart: startupSamples[0]?.eventCountAtStart ?? fallbackPhase?.eventCountAtStart ?? 0,
  };
}

function startupSamplesFromPhase(phase: PhaseResult): StartupBootSample[] {
  if (phase.startupSamples && phase.startupSamples.length > 0) {
    return phase.startupSamples;
  }
  return [{
    startedAt: phase.startedAt,
    eventCountAtStart: phase.eventCountAtStart,
    boot: {
      ...phase.boot,
      launchTotalMs: phase.boot.launchTotalMs ?? phase.startupMs,
    },
  }];
}

function aggregateBootTimings(samples: StartupBootSample[]): BootTimings {
  const markKeys = new Set<string>();
  for (const sample of samples) {
    for (const mark of Object.keys(sample.boot.marks)) {
      markKeys.add(mark);
    }
  }
  const marks: Record<string, number> = {};
  for (const mark of markKeys) {
    const value = median(samples.map((sample) => sample.boot.marks[mark]));
    if (value !== undefined) marks[mark] = value;
  }
  const launchTotalMs = median(samples.map((sample) => sample.boot.launchTotalMs));
  return launchTotalMs === undefined ? { marks } : { marks, launchTotalMs };
}

function median(values: Array<number | undefined>): number | undefined {
  const finite = values
    .filter((value): value is number => value !== undefined && Number.isFinite(value))
    .sort((a, b) => a - b);
  if (finite.length === 0) return undefined;
  const mid = Math.floor(finite.length / 2);
  if (finite.length % 2 === 1) return finite[mid];
  const low = finite[mid - 1];
  const high = finite[mid];
  if (low === undefined || high === undefined) return undefined;
  return (low + high) / 2;
}

function average(values: number[]): number {
  const finite = values.filter((value) => Number.isFinite(value));
  if (finite.length === 0) return 0;
  return finite.reduce((sum, value) => sum + value, 0) / finite.length;
}

function formatOptionalMs(value: number | undefined): string {
  return value === undefined || !Number.isFinite(value)
    ? "n/a"
    : `${formatNumber(localization.locale, Math.round(value))} ms`;
}
