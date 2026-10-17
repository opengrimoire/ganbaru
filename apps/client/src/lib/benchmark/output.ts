/**
 * Markdown formatter for benchmark results.
 *
 * The shape is pinned so historical rows in `docs/performance/results.md` stay
 * comparable across builds. The vitest suite in `output.test.ts` locks the
 * format with golden assertions; do not change spacing or column order
 * without updating the tests and docs. Layout-only output changes do not bump
 * `HARNESS_VERSION`.
 *
 * See `docs/performance/harness.md` for the rationale.
 */
import type {
  BenchmarkMetric,
  BenchmarkResult,
  PhaseResult,
  SamplePoint,
  StartupBootSample,
} from "./types";

/**
 * Boot mark used for the usable paint startup column. The headline launch
 * stat measures process spawn through usable calendar paint.
 */
const USABLE_PAINT_MARK = "boot.usable-paint";

const STAT_DETAIL_KEYS = ["runs", "minMs", "medianMs", "p95Ms", "maxMs"] as const;
const CANONICAL_METRIC_LABELS_BY_SCENARIO = new Map<string, readonly string[]>([
  ["calendar-nav", []],
  ["calendar-panel-latency", [
    "click existing event avg",
    "click empty time slot avg",
  ]],
  ["calendar-import-ops", [
    "bulk import 1000 add",
    "bulk import 1000 update",
  ]],
]);
export interface BenchmarkRunMetadataRow {
  run: string;
  harness: string;
  anchorDate: string;
  buildRef: string;
  platform: string;
  notes: string;
}

export interface BenchmarkStartupRow {
  run: string;
  dataset: string;
  runs: string;
  usablePaintMedianMs: string;
  launchMedianMs: string;
  launchP95Ms: string;
}

export interface BenchmarkMemoryRow {
  run: string;
  dataset: string;
  statistic: string;
  backendMb: string;
  frontendMb: string;
  networkMb: string;
  totalMb: string;
}

export interface BenchmarkLatencyRow {
  run: string;
  dataset: string;
  metric: string;
  runs: string;
  median: string;
  p95: string;
}

export interface BenchmarkScalarMetricRow {
  run: string;
  dataset: string;
  metric: string;
  value: string;
  unit: string;
}

export type BenchmarkPreviewSection =
  | {
    id: string;
    title: string;
    kind: "startup";
    rows: BenchmarkStartupRow[];
  }
  | {
    id: string;
    title: string;
    kind: "memory";
    rows: BenchmarkMemoryRow[];
    latencyRows: BenchmarkLatencyRow[];
    scalarRows: BenchmarkScalarMetricRow[];
  }
  | {
    id: string;
    title: string;
    kind: "metrics";
    latencyRows: BenchmarkLatencyRow[];
    scalarRows: BenchmarkScalarMetricRow[];
  };

export interface BenchmarkSuitePreview {
  metadata: BenchmarkRunMetadataRow;
  sections: BenchmarkPreviewSection[];
}

interface BenchmarkOutputOptions {
  /** Override the date stamp; defaults to today's local YYYY-MM-DD. */
  date?: string;
  /** Optional environment hint, e.g. "Linux Ubuntu 25.10, WebKitGTK 2.46". */
  env?: string;
  /** Optional build identifier shown in run metadata. */
  build?: string;
  /** Optional run anchor. Defaults to the result anchor dates. */
  anchorDate?: string;
  /** Optional complete run id. Defaults to `${date}-ID`. */
  run?: string;
  /** Optional run notes. */
  notes?: string;
}

function formatMb(n: number): string {
  if (Number.isNaN(n) || !Number.isFinite(n)) return "n/a";
  return n.toFixed(1);
}

function formatTotalMb(n: number): string {
  if (Number.isNaN(n) || !Number.isFinite(n)) return "n/a";
  return Math.round(n).toString();
}

function formatMetricValue(metric: BenchmarkMetric | undefined): string {
  if (!metric) return "n/a";
  if (!Number.isFinite(metric.value)) return "n/a";
  if (metric.unit === "percent") return metric.value.toFixed(2);
  return Math.round(metric.value).toString();
}

function metricsByLabel(phase: PhaseResult): Map<string, BenchmarkMetric> {
  const map = new Map<string, BenchmarkMetric>();
  for (const metric of phase.metrics ?? []) {
    map.set(metric.label, metric);
  }
  return map;
}

function formatBootValue(value: number | undefined): string {
  return value === undefined ? "n/a" : Math.round(value).toString();
}

interface NumberStats {
  runs: number;
  min?: number;
  median?: number;
  p95?: number;
  max?: number;
}

function numberStats(values: Array<number | undefined>): NumberStats {
  const finite = values
    .filter((value): value is number => value !== undefined && Number.isFinite(value))
    .sort((a, b) => a - b);
  if (finite.length === 0) return { runs: 0 };
  return {
    runs: finite.length,
    min: finite[0],
    median: percentile(finite, 0.5),
    p95: percentile(finite, 0.95),
    max: finite[finite.length - 1],
  };
}

function percentile(sortedValues: number[], fraction: number): number {
  if (sortedValues.length === 0) return Number.NaN;
  const index = Math.min(sortedValues.length - 1, Math.max(0, Math.ceil(sortedValues.length * fraction) - 1));
  return sortedValues[index] ?? Number.NaN;
}

function fallbackStartupSample(phase: PhaseResult): StartupBootSample {
  return {
    startedAt: phase.startedAt,
    eventCountAtStart: phase.eventCountAtStart,
    boot: {
      ...phase.boot,
      launchTotalMs: phase.boot.launchTotalMs ?? phase.startupMs,
    },
  };
}

function startupSamples(phase: PhaseResult): StartupBootSample[] {
  return phase.startupSamples && phase.startupSamples.length > 0
    ? phase.startupSamples
    : [fallbackStartupSample(phase)];
}

function resultPhases(result: BenchmarkResult): PhaseResult[] {
  return [
    ...(result.phaseA ? [result.phaseA] : []),
    ...(result.datasetPhases && result.datasetPhases.length > 0
      ? result.datasetPhases
      : [result.phaseB]),
  ];
}

export function benchmarkDatasetLabel(result: BenchmarkResult, phase: PhaseResult): string {
  if (phase.phase === "A") {
    return `base-${phase.eventCountAtStart}`;
  }
  if (phase.datasetId) return phase.datasetId;
  const datasetVersion = result.datasetVersion.startsWith("v")
    ? result.datasetVersion
    : `v${result.datasetVersion}`;
  return `dense-${datasetVersion}-${phase.eventCountAtStart}`;
}

function buildStartupRows(result: BenchmarkResult, run: string): BenchmarkStartupRow[] {
  return canonicalPhases(result).map((phase) => {
    const samples = startupSamples(phase);
    const usablePaintStats = numberStats(samples.map((sample) => sample.boot.marks[USABLE_PAINT_MARK]));
    const launchStats = numberStats(samples.map((sample) => sample.boot.launchTotalMs));
    return {
      run,
      dataset: benchmarkDatasetLabel(result, phase),
      runs: launchStats.runs.toString(),
      usablePaintMedianMs: formatBootValue(usablePaintStats.median),
      launchMedianMs: formatBootValue(launchStats.median),
      launchP95Ms: formatBootValue(launchStats.p95),
    };
  });
}

function buildMemoryRows(result: BenchmarkResult, run: string): BenchmarkMemoryRow[] {
  const rows: BenchmarkMemoryRow[] = [];
  for (const phase of canonicalPhases(result)) {
    for (const statistic of ["Min", "Max", "End"] as const) {
      rows.push(formatMemoryRow(result, phase, run, statistic));
    }
  }
  return rows;
}

type MemoryStatistic = "Min" | "Max" | "End";
type MemoryBucket = "backendMb" | "frontendMb" | "networkMb" | "totalMb";

function formatMemoryRow(
  result: BenchmarkResult,
  phase: PhaseResult,
  run: string,
  statistic: MemoryStatistic,
): BenchmarkMemoryRow {
  return {
    run,
    dataset: benchmarkDatasetLabel(result, phase),
    statistic,
    backendMb: formatMemoryStatistic(phase.curve, statistic, "backendMb"),
    frontendMb: formatMemoryStatistic(phase.curve, statistic, "frontendMb"),
    networkMb: formatMemoryStatistic(phase.curve, statistic, "networkMb"),
    totalMb: formatMemoryStatistic(phase.curve, statistic, "totalMb"),
  };
}

function formatMemoryStatistic(
  samples: SamplePoint[],
  statistic: MemoryStatistic,
  bucket: MemoryBucket,
): string {
  const value = memoryStatisticValue(samples, statistic, bucket);
  if (value === undefined) return "n/a";
  return bucket === "totalMb" ? formatTotalMb(value) : formatMb(value);
}

function memoryStatisticValue(
  samples: SamplePoint[],
  statistic: MemoryStatistic,
  bucket: MemoryBucket,
): number | undefined {
  if (samples.length === 0) return undefined;
  if (statistic === "End") return samples[samples.length - 1]?.[bucket];
  const values = samples
    .map((sample) => sample[bucket])
    .filter((value) => Number.isFinite(value));
  if (values.length === 0) return undefined;
  return statistic === "Min" ? Math.min(...values) : Math.max(...values);
}

/**
 * Build the structured summary used by the readable in-app preview and the
 * canonical markdown formatter.
 */
export function buildBenchmarkSuitePreview(
  results: BenchmarkResult[],
  opts: BenchmarkOutputOptions = {},
): BenchmarkSuitePreview {
  const run = runId(opts);
  const buildRef = opts.build ?? unique(results.map((result) => result.buildRef).filter(isDefined)).join(", ");
  const platform = opts.env ?? unique(results.map((result) => result.platform)).join(", ");
  const harness = unique(results.map((result) => result.harnessVersion)).map(formatHarness).join(", ");
  const anchorDate = opts.anchorDate ?? unique(results.map((result) => result.anchorDate)).join(", ");
  const metadata: BenchmarkRunMetadataRow = {
    run,
    harness: harness || "n/a",
    anchorDate: anchorDate || "n/a",
    buildRef: buildRef || "n/a",
    platform: platform || "n/a",
    notes: opts.notes ?? "",
  };
  const sections = results
    .map((result) => buildSection(result, run))
    .filter(isDefined);
  return {
    metadata,
    sections,
  };
}

function buildSection(result: BenchmarkResult, run: string): BenchmarkPreviewSection | undefined {
  if (result.workload.kind === "startup") {
    return {
      id: result.scenarioId,
      title: sectionTitle(result),
      kind: "startup",
      rows: buildStartupRows(result, run),
    };
  }
  if (result.workload.memoryMode === "post-workload") {
    const metricRows = buildMetricRows(result, run);
    return {
      id: result.scenarioId,
      title: sectionTitle(result),
      kind: "memory",
      rows: buildMemoryRows(result, run),
      ...metricRows,
    };
  }
  return {
    id: result.scenarioId,
    title: sectionTitle(result),
    kind: "metrics",
    ...buildMetricRows(result, run),
  };
}

function buildMetricRows(
  result: BenchmarkResult,
  run: string,
): { latencyRows: BenchmarkLatencyRow[]; scalarRows: BenchmarkScalarMetricRow[] } {
  const phases = canonicalPhases(result);
  const metricLabels = [
    ...new Set([
      ...phases.flatMap((phase) => (phase.metrics ?? []).map((metric) => metric.label)),
    ]),
  ].filter((label) => shouldIncludeMetric(result, label));
  const phaseMetrics = phases.map((phase) => metricsByLabel(phase));
  const latencyRows: BenchmarkLatencyRow[] = [];
  const scalarRows: BenchmarkScalarMetricRow[] = [];
  for (const label of metricLabels) {
    const metrics = phaseMetrics.map((metricsForPhase) => metricsForPhase.get(label));
    const unit = metrics.find((metric) => metric)?.unit ?? "";
    if (metrics.some(hasStatDetails)) {
      for (const [index, phase] of phases.entries()) {
        latencyRows.push(buildLatencyRow(result, phase, run, label, metrics[index]));
      }
    } else {
      for (const [index, phase] of phases.entries()) {
        scalarRows.push(buildScalarMetricRow(result, phase, run, label, unit, metrics[index]));
      }
    }
  }
  return { latencyRows, scalarRows };
}

function buildLatencyRow(
  result: BenchmarkResult,
  phase: PhaseResult,
  run: string,
  label: string,
  metric: BenchmarkMetric | undefined,
): BenchmarkLatencyRow {
  return {
    run,
    dataset: benchmarkDatasetLabel(result, phase),
    metric: canonicalMetricLabel(label),
    runs: formatMetricDetail(metric, "runs"),
    median: formatMetricDetail(metric, "medianMs"),
    p95: formatMetricDetail(metric, "p95Ms"),
  };
}

function buildScalarMetricRow(
  result: BenchmarkResult,
  phase: PhaseResult,
  run: string,
  label: string,
  unit: string,
  metric: BenchmarkMetric | undefined,
): BenchmarkScalarMetricRow {
  return {
    run,
    dataset: benchmarkDatasetLabel(result, phase),
    metric: canonicalMetricLabel(label),
    value: formatMetricValue(metric),
    unit,
  };
}

function hasStatDetails(metric: BenchmarkMetric | undefined): boolean {
  if (!metric?.details) return false;
  return STAT_DETAIL_KEYS.some((key) => metric.details?.[key] !== undefined);
}

function canonicalPhases(result: BenchmarkResult): PhaseResult[] {
  return resultPhases(result);
}

function shouldIncludeMetric(result: BenchmarkResult, label: string): boolean {
  const canonicalLabels = CANONICAL_METRIC_LABELS_BY_SCENARIO.get(result.scenarioId);
  return canonicalLabels === undefined || canonicalLabels.includes(label);
}

function canonicalMetricLabel(label: string): string {
  return label.endsWith(" avg") ? label.slice(0, -" avg".length) : label;
}

function formatMetricDetail(metric: BenchmarkMetric | undefined, key: string): string {
  const value = metric?.details?.[key];
  if (value === undefined) return "";
  if (typeof value === "number") {
    return Number.isFinite(value) ? Math.round(value).toString() : "";
  }
  return value;
}

function sectionTitle(result: BenchmarkResult): string {
  if (result.scenarioId === "startup-boot") return "Startup boot";
  if (result.scenarioId === "idle-memory") return "Idle memory";
  if (result.scenarioId === "calendar-nav") return "Calendar held navigation memory";
  if (result.scenarioId === "calendar-panel-latency") return "Calendar panel latency";
  return result.scenarioLabel;
}

export function formatBenchmarkSuiteMarkdown(
  results: BenchmarkResult[],
  opts: BenchmarkOutputOptions = {},
): string {
  const preview = buildBenchmarkSuitePreview(results, opts);
  const lines: string[] = [];
  lines.push(`## Benchmark ${preview.metadata.run}`);
  lines.push("");
  appendRunMetadata(lines, preview.metadata);
  for (const section of preview.sections) {
    lines.push("");
    lines.push(`### ${section.title}`);
    lines.push("");
    if (section.kind === "startup") {
      appendStartupTable(lines, section.rows);
    } else if (section.kind === "memory") {
      appendMemoryTable(lines, section.rows);
      if (section.latencyRows.length > 0 || section.scalarRows.length > 0) {
        lines.push("");
        appendMetricTables(lines, section.id, section.latencyRows, section.scalarRows);
      }
    } else {
      appendMetricTables(lines, section.id, section.latencyRows, section.scalarRows);
    }
  }
  return lines.join("\n");
}

function appendRunMetadata(lines: string[], metadata: BenchmarkRunMetadataRow): void {
  lines.push("### Run metadata");
  lines.push("");
  lines.push("| Run | Harness | Anchor date | Build ref | Platform | Notes |");
  lines.push("|---|---|---|---|---|---|");
  lines.push(
    `| ${metadata.run} | ${metadata.harness} | ${metadata.anchorDate} | ${metadata.buildRef} | ${metadata.platform} | ${metadata.notes} |`,
  );
}

function appendStartupTable(lines: string[], rows: BenchmarkStartupRow[]): void {
  lines.push("| Run | Dataset | Runs | Usable paint median ms | Launch median ms | Launch P95 ms |");
  lines.push("|---|---|---:|---:|---:|---:|");
  for (const row of rows) {
    lines.push(
      `| ${row.run} | ${row.dataset} | ${row.runs} | ${row.usablePaintMedianMs} | ${row.launchMedianMs} | ${row.launchP95Ms} |`,
    );
  }
}

function appendMemoryTable(lines: string[], rows: BenchmarkMemoryRow[]): void {
  lines.push("| Run | Dataset | Statistic | Backend MB | Frontend MB | Network MB | Total MB |");
  lines.push("|---|---|---|---:|---:|---:|---:|");
  for (const row of rows) {
    lines.push(
      `| ${row.run} | ${row.dataset} | ${row.statistic} | ${row.backendMb} | ${row.frontendMb} | ${row.networkMb} | ${row.totalMb} |`,
    );
  }
}

function appendMetricTables(
  lines: string[],
  sectionId: string,
  latencyRows: BenchmarkLatencyRow[],
  scalarRows: BenchmarkScalarMetricRow[],
): void {
  if (latencyRows.length === 0 && scalarRows.length === 0) {
    lines.push("No primary metrics were captured.");
    return;
  }
  if (latencyRows.length > 0) {
    if (sectionId === "calendar-panel-latency") {
      appendPanelLatencyTable(lines, latencyRows);
    } else {
      appendLatencyTable(lines, latencyRows);
    }
  }
  if (scalarRows.length > 0) {
    if (latencyRows.length > 0) lines.push("");
    appendScalarMetricTable(lines, scalarRows);
  }
}

function appendPanelLatencyTable(lines: string[], rows: BenchmarkLatencyRow[]): void {
  lines.push("| Run | Dataset | Action | Median ms | P95 ms |");
  lines.push("|---|---|---|---:|---:|");
  for (const row of rows) {
    lines.push(
      `| ${row.run} | ${row.dataset} | ${row.metric} | ${row.median} | ${row.p95} |`,
    );
  }
}

function appendLatencyTable(lines: string[], rows: BenchmarkLatencyRow[]): void {
  lines.push("| Run | Dataset | Metric | Runs | Median ms | P95 ms |");
  lines.push("|---|---|---|---:|---:|---:|");
  for (const row of rows) {
    lines.push(
      `| ${row.run} | ${row.dataset} | ${row.metric} | ${row.runs} | ${row.median} | ${row.p95} |`,
    );
  }
}

function appendScalarMetricTable(lines: string[], rows: BenchmarkScalarMetricRow[]): void {
  if (usesUniformUnit(rows, "ms")) {
    lines.push("| Run | Dataset | Metric | Value ms |");
    lines.push("|---|---|---|---:|");
    for (const row of rows) {
      lines.push(
        `| ${row.run} | ${row.dataset} | ${row.metric} | ${row.value} |`,
      );
    }
    return;
  }
  lines.push("| Run | Dataset | Metric | Value | Unit |");
  lines.push("|---|---|---|---:|---|");
  for (const row of rows) {
    lines.push(
      `| ${row.run} | ${row.dataset} | ${row.metric} | ${row.value} | ${row.unit} |`,
    );
  }
}

function usesUniformUnit(rows: BenchmarkScalarMetricRow[], unit: string): boolean {
  return rows.length > 0 && rows.every((row) => row.unit === unit);
}

function runId(opts: BenchmarkOutputOptions): string {
  if (opts.run) return opts.run;
  return `${opts.date ?? today()}-ID`;
}

function formatHarness(version: string): string {
  if (!version) return "n/a";
  return version.startsWith("v") ? version : `v${version}`;
}

function unique(values: string[]): string[] {
  return [...new Set(values.filter((value) => value.length > 0))];
}

function isDefined<T>(value: T | undefined): value is T {
  return value !== undefined;
}

function today(): string {
  const d = new Date();
  return `${d.getFullYear()}-${pad2(d.getMonth() + 1)}-${pad2(d.getDate())}`;
}

function pad2(n: number): string {
  return n < 10 ? `0${n}` : `${n}`;
}
