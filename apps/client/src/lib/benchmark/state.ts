/**
 * Validating reader for the persisted benchmark state file. The file survives restarts outside the vault, so every
 * reader treats it as untrusted input and gets either a well-formed `BenchmarkState` or `null`.
 */
import {
  CHAT_TIMELINE_DATASET_VERSION,
  DENSE_DATASET_VERSION,
  type BenchmarkDatasetProfile,
  type BenchmarkPhaseAState,
  type BenchmarkPhaseBState,
  type BenchmarkResult,
  type BenchmarkSeedHandle,
  type BenchmarkState,
  type BenchmarkSuiteState,
  type PhaseResult,
  type StartupRunState,
} from "./types";

type UnknownRecord = Readonly<Record<string, unknown>>;

function isRecord(value: unknown): value is UnknownRecord {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function isFiniteNumber(value: unknown): value is number {
  return typeof value === "number" && Number.isFinite(value);
}

function isNonNegativeInteger(value: unknown): value is number {
  return Number.isInteger(value) && typeof value === "number" && value >= 0;
}

function isOptional<T>(value: unknown, check: (value: unknown) => value is T): value is T | undefined {
  return value === undefined || check(value);
}

function isString(value: unknown): value is string {
  return typeof value === "string";
}

function isArrayOf<T>(value: unknown, check: (item: unknown) => item is T): value is T[] {
  return Array.isArray(value) && value.every(check);
}

function isDatasetProfile(value: unknown): value is BenchmarkDatasetProfile {
  if (!isRecord(value)) return false;
  if (value.kind === "dense-calendar") {
    return value.version === DENSE_DATASET_VERSION
      && isNonNegativeInteger(value.yearRadius)
      && isNonNegativeInteger(value.stackCount)
      && value.detailProfile === "d1";
  }
  if (value.kind === "chat-timeline") {
    return value.version === CHAT_TIMELINE_DATASET_VERSION && (value.shape === "t5000" || value.shape === "flood");
  }
  return false;
}

function isSeedHandle(value: unknown): value is BenchmarkSeedHandle {
  return isRecord(value)
    && (value.calendarId === null || isString(value.calendarId))
    && isOptional(value.chatThreadId, isString)
    && isFiniteNumber(value.eventCount)
    && isString(value.datasetId)
    && isDatasetProfile(value.dataset);
}

/**
 * Checks the fields the runner and markdown output read from a phase. Samples and metrics stay unchecked beyond
 * their container shape because the harness writes them itself and only formats them.
 */
function isPhaseResult(value: unknown): value is PhaseResult {
  return isRecord(value)
    && (value.phase === "A" || value.phase === "B")
    && isString(value.startedAt)
    && isFiniteNumber(value.workloadDurationMs)
    && Array.isArray(value.curve)
    && isOptional(value.metrics, Array.isArray)
    && isRecord(value.boot)
    && isRecord(value.boot.marks)
    && isOptional(value.startupMs, isFiniteNumber)
    && isOptional(value.startupSamples, Array.isArray)
    && isFiniteNumber(value.eventCountAtStart)
    && isOptional(value.datasetId, isString);
}

function isBenchmarkResult(value: unknown): value is BenchmarkResult {
  return isRecord(value)
    && isString(value.scenarioId)
    && isString(value.scenarioLabel)
    && isRecord(value.workload)
    && isString(value.datasetVersion)
    && isString(value.harnessVersion)
    && isString(value.platform)
    && isOptional(value.buildRef, isString)
    && isOptional(value.phaseA, isPhaseResult)
    && isPhaseResult(value.phaseB)
    && isOptional(value.datasetPhases, (item): item is PhaseResult[] => isArrayOf(item, isPhaseResult))
    && isOptional(value.peakTotalMb, isFiniteNumber)
    && isString(value.anchorDate);
}

function isSuiteState(value: unknown): value is BenchmarkSuiteState {
  return isRecord(value)
    && isArrayOf(value.scenarioIds, isString)
    && isNonNegativeInteger(value.index)
    && value.index < value.scenarioIds.length
    && isArrayOf(value.results, isBenchmarkResult);
}

function isStartupRunState(value: unknown): value is StartupRunState {
  return isRecord(value)
    && isNonNegativeInteger(value.targetRuns)
    && isArrayOf(value.phaseA, isPhaseResult)
    && isArrayOf(value.phaseB, isPhaseResult);
}

function isDatasetList(value: unknown): value is BenchmarkDatasetProfile[] {
  return isArrayOf(value, isDatasetProfile);
}

function isPhaseList(value: unknown): value is PhaseResult[] {
  return isArrayOf(value, isPhaseResult);
}

/** The fields every stage shares. */
type BenchmarkStateBaseFields = Omit<BenchmarkPhaseAState, "stage">;

function readStateBase(value: UnknownRecord): BenchmarkStateBaseFields | null {
  const {
    scenarioId,
    startedAt,
    updatedAt,
    harnessVersion,
    datasetVersion,
    platform,
    buildRef,
    anchorDate,
    vaultMode,
    benchmarkDatasets,
    suite,
    startupRuns,
  } = value;
  if (
    !isString(scenarioId)
    || !isString(startedAt)
    || !isString(updatedAt)
    || !isString(harnessVersion)
    || !isString(datasetVersion)
    || !isString(platform)
    || !isOptional(buildRef, isString)
    || !isString(anchorDate)
    || (vaultMode !== "user" && vaultMode !== "benchmark")
    || !isOptional(benchmarkDatasets, isDatasetList)
    || !isOptional(suite, isSuiteState)
    || !isOptional(startupRuns, isStartupRunState)
  ) {
    return null;
  }
  return {
    scenarioId,
    startedAt,
    updatedAt,
    harnessVersion,
    datasetVersion,
    platform,
    buildRef,
    anchorDate,
    vaultMode,
    benchmarkDatasets,
    suite,
    startupRuns,
  };
}

/**
 * Validates parsed JSON as a benchmark state. Version and freshness checks stay with the callers, which decide
 * whether a well-formed but outdated state routes to the benchmark database.
 *
 * @param value - The parsed JSON.
 * @returns The state, or `null` when any field is missing or malformed.
 */
export function readBenchmarkState(value: unknown): BenchmarkState | null {
  if (!isRecord(value)) return null;
  const base = readStateBase(value);
  if (!base) return null;
  const { stage, phaseA, seedHandle, datasetIndex, datasetPhases } = value;
  if (stage === "phase-a-pending" || stage === "phase-a-running") {
    return { ...base, stage };
  }
  if (stage !== "phase-b-pending" && stage !== "phase-b-running") return null;
  if (
    !isSeedHandle(seedHandle)
    || !isOptional(phaseA, isPhaseResult)
    || !isOptional(datasetIndex, isNonNegativeInteger)
    || !isOptional(datasetPhases, isPhaseList)
  ) {
    return null;
  }
  const state: BenchmarkPhaseBState = { ...base, stage, phaseA, seedHandle, datasetIndex, datasetPhases };
  return state;
}

/**
 * Parses and validates the persisted benchmark state file.
 *
 * @param json - The file contents.
 * @returns The state, or `null` when the file is not JSON or not a well-formed state.
 */
export function parseBenchmarkState(json: string): BenchmarkState | null {
  let parsed: unknown;
  try {
    parsed = JSON.parse(json);
  } catch {
    return null;
  }
  return readBenchmarkState(parsed);
}
