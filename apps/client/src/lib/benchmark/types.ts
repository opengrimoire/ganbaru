/**
 * Type contracts for the in-app performance benchmark harness.
 *
 * The harness drives deterministic workloads against a baseline dataset and
 * one or more dense calendar datasets, then emits compact markdown ready for
 * `docs/performance/results.md`. Each scenario declares whether it measures startup,
 * memory, or feature latency so the runner avoids unrelated waits. Both
 * passes run after a cold restart against an isolated `benchmark.sqlite`,
 * so the user's vault and its database are never touched. The rationale lives in `docs/performance/harness.md`.
 */
import type { CalendarEvent } from "$lib/calendar/types";

/** Fixed duration for the real held-arrow navigation benchmark action. */
export const HELD_NAVIGATION_DURATION_MS = 3000;
/** Longest window the Chat timeline scroll benchmark scrolls before it stops short of the start. */
export const CHAT_TIMELINE_SCROLL_DURATION_MS = 20_000;
/** One memory reading per second over the post-state observation window. */
export const MEMORY_OBSERVATION_INTERVAL_MS = 1000;
/** Total duration of memory observation windows. */
export const MEMORY_OBSERVATION_DURATION_MS = 30_000;
export const MEMORY_OBSERVATION_SAMPLE_COUNT =
  MEMORY_OBSERVATION_DURATION_MS / MEMORY_OBSERVATION_INTERVAL_MS;
/** Number of process launches captured per dataset by the startup benchmark. */
export const STARTUP_SAMPLE_RUNS = 5;
/** Number of repeated opens captured per calendar panel benchmark action. */
export const PANEL_ACTION_RUNS = 50;
/** Closed-process wait before each startup benchmark relaunch sample. */
export const STARTUP_RELAUNCH_COOLDOWN_MS = 10_000;
/** Stale state older than this on boot is discarded silently. */
export const STATE_TTL_MS = 60 * 60 * 1000;
/**
 * Pending state exists only between an intentional benchmark restart and
 * the next boot claiming that pass. If it survives longer than this, treat
 * the relaunch as failed and fall back to the user's vault database.
 */
export const PENDING_STATE_TTL_MS = 2 * 60 * 1000;

/**
 * Bumped manually when a recorded run uses a measurement methodology,
 * sampling cadence, scenario workload, or dataset generator that invalidates
 * numeric cross-build comparison with the previous recorded harness.
 * Local iteration before a run is recorded, cosmetic markdown, wording, and
 * docs changes do not bump this value.
 * Stored on the persisted state file so baseline data captured by a
 * different build cannot feed the dense pass of the current build.
 *
 * v1 (2026-05-12): active baseline reset. Calendar benchmarks use a
 * run-persisted today anchor, skip normal current-week preload on benchmark
 * boots, and copy reduced canonical tables.
 */
export const HARNESS_VERSION = "1";

/**
 * Dense dataset shape version. Bumping this requires renaming the calendar
 * grouping and should only happen when a run with the new shape is recorded.
 */
export const DENSE_DATASET_VERSION = "v1";

export type DenseCalendarDetailProfile = "d1";

export interface DenseCalendarDatasetProfile {
  kind: "dense-calendar";
  version: typeof DENSE_DATASET_VERSION;
  /** Years before and after the run's persisted benchmark anchor. */
  yearRadius: number;
  /** Number of events created at the start of every hour. */
  stackCount: number;
  /** Detail payload shape for metadata, all-day events, and Pomodoro history. */
  detailProfile: DenseCalendarDetailProfile;
}

/**
 * Chat timeline dataset shape version. Edit it in place while the shape is unrecorded; bump it
 * only when a recorded Chat run would no longer compare with new rows.
 */
export const CHAT_TIMELINE_DATASET_VERSION = "v1";

/**
 * Chat timeline dataset shapes: `t5000` is one settled thread with 5000 rows (40 percent
 * messages, 50 percent tool activities, 10 percent plans), and `flood` is one empty thread the
 * stream flood replays into.
 */
export type ChatTimelineDatasetShape = "t5000" | "flood";

export interface ChatTimelineDatasetProfile {
  kind: "chat-timeline";
  version: typeof CHAT_TIMELINE_DATASET_VERSION;
  shape: ChatTimelineDatasetShape;
}

export type BenchmarkDatasetProfile = DenseCalendarDatasetProfile | ChatTimelineDatasetProfile;

export interface BenchmarkSeedHandle {
  /** Calendar grouping the dense Calendar seed created, or `null` for other datasets. */
  calendarId: string | null;
  /** Chat thread a Chat timeline seed created. */
  chatThreadId?: string;
  eventCount: number;
  datasetId: string;
  dataset: BenchmarkDatasetProfile;
}

/** Default dense-span dataset used by all benchmark scenarios. */
export const DEFAULT_BENCHMARK_DATASET = {
  kind: "dense-calendar",
  version: DENSE_DATASET_VERSION,
  yearRadius: 1,
  stackCount: 1,
  detailProfile: "d1",
} as const satisfies BenchmarkDatasetProfile;

/** Large dense-span dataset used by core benchmarks to expose history-range costs. */
export const LARGE_BENCHMARK_DATASET = {
  kind: "dense-calendar",
  version: DENSE_DATASET_VERSION,
  yearRadius: 10,
  stackCount: 1,
  detailProfile: "d1",
} as const satisfies BenchmarkDatasetProfile;

/** Dense datasets that core benchmarks must capture for the optimization gate. */
export const CORE_BENCHMARK_DATASETS = [
  DEFAULT_BENCHMARK_DATASET,
  LARGE_BENCHMARK_DATASET,
] as const;

/** Long settled Chat thread the timeline scroll benchmark reads. */
export const CHAT_LONG_THREAD_DATASET = {
  kind: "chat-timeline",
  version: CHAT_TIMELINE_DATASET_VERSION,
  shape: "t5000",
} as const satisfies BenchmarkDatasetProfile;

/** Empty Chat thread the stream flood benchmark replays into. */
export const CHAT_STREAM_FLOOD_DATASET = {
  kind: "chat-timeline",
  version: CHAT_TIMELINE_DATASET_VERSION,
  shape: "flood",
} as const satisfies BenchmarkDatasetProfile;

export function benchmarkDatasetId(dataset: BenchmarkDatasetProfile): string {
  if (dataset.kind === "chat-timeline") return `chat-${dataset.version}-${dataset.shape}`;
  return `dense-${dataset.version}-r${dataset.yearRadius}y-s${dataset.stackCount}-${dataset.detailProfile}`;
}

/** Narrows a dataset to a dense Calendar profile, for seeds that only build Calendar data. */
export function requireDenseCalendarDataset(dataset: BenchmarkDatasetProfile): DenseCalendarDatasetProfile {
  if (dataset.kind !== "dense-calendar") {
    throw new Error(`Dataset ${benchmarkDatasetId(dataset)} is not a dense Calendar dataset`);
  }
  return dataset;
}

export type SampleLabel = string;

export function formatOffsetLabel(ms: number): string {
  return `+${Math.round(ms / 1000)}s`;
}

export function resolveBenchmarkAnchorDate(now: Date = new Date()): string {
  const year = now.getFullYear();
  const month = String(now.getMonth() + 1).padStart(2, "0");
  const day = String(now.getDate()).padStart(2, "0");
  return `${year}-${month}-${day}`;
}

/** Single memory reading at one sample point. MB units, metric depends on platform. */
export interface SamplePoint {
  label: SampleLabel;
  /** Milliseconds since the current sampling window started. */
  tMs: number;
  totalMb: number;
  backendMb: number;
  frontendMb: number;
  networkMb: number;
}

export type BenchmarkQuestionKind =
  | "startup"
  | "idle-memory"
  | "stress-memory"
  | "interaction-latency"
  | "operation-latency";

export type BenchmarkMemoryMode = "none" | "post-workload";
export type BenchmarkRunMode = "baseline-and-dense" | "dense-only";

export interface BenchmarkWorkload {
  /** Primary question this scenario answers. Controls markdown output shape. */
  kind: BenchmarkQuestionKind;
  /** Human-readable question shown in docs and benchmark output. */
  question: string;
  /** Short phrase used in methodology output, e.g. "programmatic week-view navigation". */
  label: string;
  /**
   * Target wall time for fixed-window workloads. Interaction and operation
   * benchmarks can use `0` when they finish after their measured actions.
   */
  durationMs: number;
  /** Whether the runner should sample the post-action memory observation window. */
  memoryMode: BenchmarkMemoryMode;
}

export type BenchmarkMetricUnit = "ms" | "count" | "percent" | "bytes" | "per second";

export interface BenchmarkMetric {
  /** Stable row label within a scenario, e.g. `click existing event`. */
  label: string;
  unit: BenchmarkMetricUnit;
  value: number;
  /** Optional scalar breakdown, such as `moduleMs`, `detailsMs`, or row counts. */
  details?: Record<string, string | number>;
}

export interface BenchmarkScenarioContext {
  /** Local YYYY-MM-DD date fixed when the benchmark run starts. */
  anchorDate: string;
}

/**
 * Boot timing extracted from the perflog ring buffer. The marks the harness
 * cares about exist regardless of whether a benchmark is running, so the
 * runner's job is to filter the snapshot by these tags and compute deltas
 * relative to `boot.script-start` (the first mark fired).
 */
export interface BootTimings {
  /** Tag-keyed milliseconds from `boot.script-start`. Missing marks are omitted. */
  marks: Record<string, number>;
  /**
   * Process-spawn to usable calendar paint time, derived from the boot
   * baseline and `boot.usable-paint`, or `boot.first-paint` when the usable
   * paint mark is missing. Omitted when no paint mark or boot baseline exists.
   */
  launchTotalMs?: number;
}

export interface StartupBootSample {
  /** Wall-clock ISO 8601 string captured when the sample was collected. */
  startedAt: string;
  /** Total benchmark DB event count for this launch sample. */
  eventCountAtStart: number;
  /** Boot timings captured from process start to usable calendar paint. */
  boot: BootTimings;
}

/** One pass's complete result: boot, optional memory samples, metrics, plus context. */
export interface PhaseResult {
  /** "A" for the baseline run, "B" for the dense-seeded run. */
  phase: "A" | "B";
  /** Wall-clock ISO 8601 string captured at the start of the phase. */
  startedAt: string;
  /** Total ms the scenario workload ran. */
  workloadDurationMs: number;
  /** Memory observation samples for memory benchmarks. Empty for latency-only scenarios. */
  curve: SamplePoint[];
  /** Optional scenario-specific timings or counters captured during the workload. */
  metrics?: BenchmarkMetric[];
  /** Filtered slice of perflog entries scoped to boot of the run that produced this phase. */
  boot: BootTimings;
  /**
   * End-to-end launch time, in ms, derived from Rust process spawn to the
   * usable calendar paint mark. Omitted when boot timings lack a launch total.
   */
  startupMs?: number;
  /** Individual process-launch samples for startup benchmark phases. */
  startupSamples?: StartupBootSample[];
  /** Total benchmark DB event count at the moment the phase started. */
  eventCountAtStart: number;
  /** Dataset profile id for seeded phases, e.g. `dense-v1-r1y-s1-d1`. */
  datasetId?: string;
}

/** Fields every persisted benchmark stage carries. */
interface BenchmarkStateBase {
  scenarioId: string;
  /** ISO 8601 from when the run was confirmed. Drives the stale TTL check. */
  startedAt: string;
  /** ISO 8601 from the last state write. Drives the pending-restart TTL check. */
  updatedAt: string;
  /** Pinned `HARNESS_VERSION` at write time. Mismatch on read clears state. */
  harnessVersion: string;
  /** Dense dataset version pinned at seed time. Currently `v1`. */
  datasetVersion: string;
  /** Platform string at write time, used only for the markdown header. */
  platform: string;
  /** App version plus git ref at write time, e.g. `0.1.0+a7451de-dirty`. */
  buildRef?: string;
  /** Local YYYY-MM-DD anchor date used for calendar dataset generation and UI windows. */
  anchorDate: string;
  /**
   * Which DB the next boot should open. `"benchmark"` routes
   * `db.ts:resolveUrl()` to the isolated `benchmark.sqlite`; the
   * vault database stays untouched. Always `"benchmark"` for the current
   * harness; the field exists to make the boot path's decision explicit
   * and to leave room for future scenarios that need user-DB access.
   */
  vaultMode: "user" | "benchmark";
  /** Dense datasets to run after the baseline pass. Defaults to the scenario's default dataset. */
  benchmarkDatasets?: BenchmarkDatasetProfile[];
  /** Present when the user chose Run all instead of one scenario. */
  suite?: BenchmarkSuiteState;
  /** Accumulated repeated launch samples for the startup benchmark. */
  startupRuns?: StartupRunState;
}

/** The baseline stage: nothing is seeded yet. */
export interface BenchmarkPhaseAState extends BenchmarkStateBase {
  stage: "phase-a-pending" | "phase-a-running";
}

/** A dense stage: a dataset is seeded and the baseline result, when the scenario measures one, is captured. */
export interface BenchmarkPhaseBState extends BenchmarkStateBase {
  stage: "phase-b-pending" | "phase-b-running";
  /** Baseline result; absent for dense-only scenarios. */
  phaseA?: PhaseResult;
  /** What the seed of the pending or running dense pass created. */
  seedHandle: BenchmarkSeedHandle;
  /** Index into `benchmarkDatasets` for the pending or running dense pass. */
  datasetIndex?: number;
  /** Completed dense passes before the pending or running one. */
  datasetPhases?: PhaseResult[];
}

/**
 * Persisted across each restart in the benchmark sequence, discriminated by `stage`. Lives in
 * `app_config_dir/benchmark-state.json`, not the vault, so a `reset_database` call does not delete it mid-run and
 * vault backups never include it. Read it only through `parseBenchmarkState`.
 *
 * The harness writes this file before each intentional restart with a `*-pending` stage, then flips that stage to
 * `*-running` before the measured pass starts. A later boot that sees `*-running` means the process was killed
 * mid-pass, so the runner discards benchmark artifacts instead of resuming from a possibly dirty isolated DB.
 */
export type BenchmarkState = BenchmarkPhaseAState | BenchmarkPhaseBState;

/** Every persisted stage. Pending stages intentionally resume; running stages mean interrupted work. */
export type BenchmarkStage = BenchmarkState["stage"];

export interface BenchmarkSuiteState {
  scenarioIds: string[];
  index: number;
  results: BenchmarkResult[];
}

export interface StartupRunState {
  targetRuns: number;
  phaseA: PhaseResult[];
  phaseB: PhaseResult[];
}

interface BenchmarkStateTimeProbe {
  startedAt?: string;
  updatedAt?: string;
}

export type BenchmarkPendingStage = "phase-a-pending" | "phase-b-pending";

export function isBenchmarkPendingStage(stage: string | undefined): stage is BenchmarkPendingStage {
  return stage === "phase-a-pending" || stage === "phase-b-pending";
}

function timestampAgeMs(value: string | undefined, nowMs: number): number {
  const timestampMs = Date.parse(value ?? "");
  return Number.isFinite(timestampMs) ? nowMs - timestampMs : Number.NaN;
}

export function benchmarkTotalAgeMs(state: BenchmarkStateTimeProbe, nowMs = Date.now()): number {
  return timestampAgeMs(state.startedAt, nowMs);
}

export function benchmarkPendingAgeMs(state: BenchmarkStateTimeProbe, nowMs = Date.now()): number {
  return timestampAgeMs(state.updatedAt, nowMs);
}

export function isFreshBenchmarkTotalAge(state: BenchmarkStateTimeProbe, nowMs = Date.now()): boolean {
  const ageMs = benchmarkTotalAgeMs(state, nowMs);
  return Number.isFinite(ageMs) && ageMs >= 0 && ageMs <= STATE_TTL_MS;
}

export function isFreshBenchmarkPendingAge(state: BenchmarkStateTimeProbe, nowMs = Date.now()): boolean {
  const ageMs = benchmarkPendingAgeMs(state, nowMs);
  return Number.isFinite(ageMs) && ageMs >= 0 && ageMs <= PENDING_STATE_TTL_MS;
}

/**
 * Lightweight scenario data safe to import in normal UI paths. This must not
 * carry executable scenario code or imports that pull in benchmark workloads.
 */
export interface BenchmarkScenarioMetadata {
  /** Stable id used in the persisted state file and markdown output. */
  id: string;
  /** Human label rendered on the scenario Run button in the performance popover. */
  label: string;
  /** Short paragraph rendered as a tooltip on hover. Explain what the scenario stresses. */
  description: string;
  /** Workload shape used by the runner and markdown formatter. */
  workload: BenchmarkWorkload;
  /** Default dataset used by `seed()` for the dense calendar pass. */
  defaultDataset: BenchmarkDatasetProfile;
  /** Optional ordered dense datasets. Defaults to `[defaultDataset]`. */
  benchmarkDatasets?: BenchmarkDatasetProfile[];
  /**
   * Whether the runner should measure an empty baseline before seeding.
   * Practical user-window benchmarks use `dense-only` so hidden control
   * passes do not add runtime or misleading output.
   */
  runMode?: BenchmarkRunMode;
}

/**
 * Implemented by every scenario module. The runner is scenario-agnostic;
 * adding a new scenario means dropping a new module under
 * `lib/benchmark/scenarios/` and registering it in `registry.ts`.
 */
export interface BenchmarkScenario extends BenchmarkScenarioMetadata {
  /**
   * Configure the app into the precondition required by the stress phase.
   * Runs at the start of every phase (A and B).
   */
  setup(context: BenchmarkScenarioContext): Promise<void>;
  /**
   * Drive the deterministic workload. The scenario keeps the work loop
   * going for `workload.durationMs` only when it needs a fixed window. It
   * may return scenario-specific metrics for the markdown summary.
   */
  runWorkload(
    signal: AbortSignal,
    context: BenchmarkScenarioContext,
  ): Promise<void | BenchmarkMetric[]>;
  /**
   * Seed the dense dataset. Called once between the baseline pass and
   * the restart. Returns a handle that `cleanup()` can use to undo. Stops
   * between write batches with an `AbortError` once `signal` aborts.
   */
  seed(
    dataset: BenchmarkDatasetProfile,
    context: BenchmarkScenarioContext,
    signal: AbortSignal,
  ): Promise<BenchmarkSeedHandle>;
  /**
   * Undo whatever `seed()` created. Runs on summary close, on cancel, and
   * on stale-state cleanup at boot.
   */
  cleanup(seedHandle: BenchmarkSeedHandle): Promise<void>;
}

/** A whole benchmark run after both passes complete. */
export interface BenchmarkResult {
  scenarioId: string;
  scenarioLabel: string;
  workload: BenchmarkWorkload;
  datasetVersion: string;
  harnessVersion: string;
  platform: string;
  buildRef?: string;
  phaseA?: PhaseResult;
  phaseB: PhaseResult;
  /** All dense dataset passes. When absent, `phaseB` is the only dense pass. */
  datasetPhases?: PhaseResult[];
  /** Maximum total memory (MB) observed in any memory observation sample. */
  peakTotalMb?: number;
  /** Local YYYY-MM-DD anchor date used by calendar benchmark scenarios. */
  anchorDate: string;
}

/** Dense benchmark event shape produced by the v1 generator. */
export interface BenchmarkEventDraft {
  title: string;
  start: string;
  end: string;
  sourceUid: string;
  allDay?: boolean;
  description?: string;
  recurrence?: CalendarEvent["recurrence"];
  notifications?: CalendarEvent["notifications"];
  pomodoroConfig?: CalendarEvent["pomodoroConfig"];
  color?: CalendarEvent["color"];
  location?: CalendarEvent["location"];
  url?: CalendarEvent["url"];
  transparency?: CalendarEvent["transparency"];
  status?: CalendarEvent["status"];
  visibility?: CalendarEvent["visibility"];
  priority?: CalendarEvent["priority"];
  categories?: CalendarEvent["categories"];
  geo?: CalendarEvent["geo"];
  extendedProperties?: CalendarEvent["extendedProperties"];
  organizer?: CalendarEvent["organizer"];
  alarms?: CalendarEvent["alarms"];
  attendees?: CalendarEvent["attendees"];
  guestPermissions?: CalendarEvent["guestPermissions"];
}
