<script lang="ts">
  import { untrack } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import Pin from "@lucide/svelte/icons/pin";
  import PinOff from "@lucide/svelte/icons/pin-off";
  import Copy from "@lucide/svelte/icons/copy";
  import Check from "@lucide/svelte/icons/check";
  import Play from "@lucide/svelte/icons/play";
  import ChevronDown from "@lucide/svelte/icons/chevron-down";
  import TriangleAlert from "@lucide/svelte/icons/triangle-alert";
  import CircleHelp from "@lucide/svelte/icons/circle-help";
  import { formatNumber } from "$lib/i18n/formatters";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { cn } from "$lib/utils";
  import { scrollEdgeFadeAction } from "$lib/utils/scroll-edge-fade";
  import { getBenchmarkStatus } from "$lib/stores/benchmark-status.svelte";
  import {
    BENCHMARK_SCENARIOS,
    BENCHMARK_SUITES,
    type BenchmarkSuiteMetadata,
  } from "$lib/benchmark/registry";
  import type { BenchmarkScenarioMetadata } from "$lib/benchmark/types";
  import { perfLog, type PerfLogEntry, clear as clearPerfLog, setTracking } from "$lib/stores/perf-log.svelte";
  import MemoryChart from "./MemoryChart.svelte";
  import type { MemorySample } from "$lib/diagnostics/memory-samples";
  import { SAMPLE_CAP, SAMPLE_INTERVAL_MS, samplesToCsv } from "$lib/diagnostics/memory-samples";
  import {
    memoryDisplayRows,
    type MemoryDisplayLabel,
    type MemoryDisplayRow,
    type MemoryReport,
    type StartupMemorySnapshot,
  } from "$lib/diagnostics/memory-report";

  let {
    shellStartupMs = null,
    startupMemorySnapshot = { status: "pending" },
    pinned = false,
    onPinnedChange = () => {},
    ensureBenchmarkOverlay = async () => {},
  }: {
    shellStartupMs: number | null;
    startupMemorySnapshot: StartupMemorySnapshot;
    pinned: boolean;
    onPinnedChange: (pinned: boolean) => void;
    ensureBenchmarkOverlay?: () => Promise<void>;
  } = $props();

  const benchmarkStatus = getBenchmarkStatus();
  const localization = getLocalization();
  const { t } = localization;
  const locale = $derived(localization.locale);
  const sectionDividerClass = "mx-0 my-3 h-px bg-border";
  const isDevMode = import.meta.env.DEV;

  let perfLive = $state(true);
  // One slot for the most recently copied button so each one can flash a
  // "Copied" confirmation without needing a separate flag per button.
  let copiedId = $state<string | null>(null);
  let memorySamples = $state<MemorySample[]>([]);
  // Logical sample index. Multiplying by SAMPLE_INTERVAL_MS yields a
  // drift-free `t` even when setInterval fires a few ms early or late, so
  // the chart ticks land on clean 5 s multiples instead of accumulating
  // event-loop slop.
  let pollIndex = 0;

  let liveReport = $state<MemoryReport | null>(null);
  let chartHoverSample = $state<MemorySample | null>(null);
  let expandedBenchmarkSuites = $state<string[]>([]);

  const displayReport = $derived.by<MemoryReport | null>(() => {
    // While the live trend chart is hovered, the per-process panel mirrors the
    // hovered sample instead of a duplicate tooltip below the plot. Otherwise
    // it shows the live report or the startup snapshot.
    if (perfLive && chartHoverSample && liveReport) {
      return {
        processes: chartHoverSample.processes.map((p) => ({ name: p.name, mb: p.mb })),
        total_mb: chartHoverSample.totalMb,
        platform: liveReport.platform,
        metric: liveReport.metric,
      };
    }
    if (perfLive) return liveReport;
    return startupMemorySnapshot.status === "ready" ? startupMemorySnapshot.report : null;
  });
  const memoryRows = $derived.by<MemoryDisplayRow[]>(() => memoryDisplayRows(displayReport));

  // Recolor the numbers while hovering to mark that the panel shows a past
  // sample instead of the live reading.
  const showingHoveredSample = $derived(perfLive && chartHoverSample !== null);

  $effect(() => {
    function update() {
      const t = untrack(() => {
        const next = pollIndex * SAMPLE_INTERVAL_MS;
        pollIndex++;
        return next;
      });
      invoke<MemoryReport>("memory_report")
        .then((report) => {
          liveReport = report;
          memorySamples.push({
            t,
            totalMb: report.total_mb,
            processes: report.processes.map((p) => ({ name: p.name, mb: p.mb })),
          });
          if (memorySamples.length > SAMPLE_CAP) memorySamples.shift();
        })
        .catch((e) => {
          console.warn("[perf] memory report failed:", e);
        });
    }
    update();
    const id = setInterval(update, SAMPLE_INTERVAL_MS);
    return () => clearInterval(id);
  });

  let launchExpanded = $state(false);
  /**
   * Boot timeline rows. Each entry shows the time spent reaching that
   * milestone from the previous one, so the column reads as "this step took
   * X ms." The synthetic `shell-startup` row at the top covers the gap
   * between process spawn and `boot.script-start`. `boot.script-start`
   * itself is the anchor and is omitted: its delta would be 0 by definition.
   * `boot.rawblocks-set` is also omitted because it fires immediately after
   * `boot.sql-children-done`, so its delta carries no information.
   */
  type BootRow = { label: string; deltaMs: number };
  const HIDDEN_BOOT_TAGS = new Set(["boot.rawblocks-set"]);
  const bootRows = $derived.by<BootRow[]>(() => {
    const rows: BootRow[] = [];
    if (shellStartupMs !== null) {
      rows.push({ label: "shell-startup", deltaMs: shellStartupMs });
    }
    let prev: PerfLogEntry | null = null;
    for (const entry of perfLog.entries) {
      if (!entry.tag.startsWith("boot.")) continue;
      if (HIDDEN_BOOT_TAGS.has(entry.tag)) continue;
      if (prev === null) {
        prev = entry;
        continue;
      }
      rows.push({
        label: entry.tag.replace(/^boot\./, ""),
        deltaMs: Math.round((entry.t - prev.t) * 10) / 10,
      });
      prev = entry;
    }
    return rows;
  });

  /**
   * Headline Launch time, derived as the sum of all visible boot row deltas.
   * Computing it client-side from the same data the table renders means the
   * number on top and the rows below are guaranteed to agree.
   */
  const launchMs = $derived.by(() => {
    if (bootRows.length === 0) return null;
    return Math.round(bootRows.reduce((sum, r) => sum + r.deltaMs, 0));
  });

  /**
   * Action chains: pair `<prefix>.start` with the next `<prefix>.paint-done`
   * of the same prefix and report the duration. In-flight chains are hidden
   * until they complete, so the list never shows partial values.
   */
  type ChainRow = {
    prefix: "nav" | "view" | "panel";
    action: string;
    durationMs: number;
    steps: { label: string; ms: number }[];
  };
  type PendingChain = { start: PerfLogEntry; steps: PerfLogEntry[] };

  function panelRequest(entry: PerfLogEntry): number | undefined {
    const value = entry.detail?.request;
    return typeof value === "number" ? value : undefined;
  }

  function panelStepLabel(tag: string): string {
    if (tag === "panel.module-ready") return "module";
    if (tag === "panel.details-ready") return "details";
    if (tag === "panel.state-open") return "state";
    if (tag === "panel.flush-done") return "flush";
    return tag.replace(/^panel\./, "");
  }

  function findPanelChainIndex(stack: PendingChain[], entry: PerfLogEntry): number {
    const request = panelRequest(entry);
    if (request === undefined) return stack.length - 1;
    for (let i = stack.length - 1; i >= 0; i--) {
      if (panelRequest(stack[i].start) === request) return i;
    }
    return -1;
  }

  const actionChains = $derived.by<ChainRow[]>(() => {
    const results: ChainRow[] = [];
    const stacks: Record<string, PendingChain[]> = {};
    for (const entry of perfLog.entries) {
      if (entry.tag === "nav.release-tail") {
        const ms = entry.detail?.ms;
        results.push({
          prefix: "nav",
          action: "release tail",
          durationMs: typeof ms === "number" ? ms : 0,
          steps: [],
        });
        continue;
      }
      const match = /^(nav|view|panel)\.(start|module-ready|details-ready|state-open|flush-done|paint-done)$/.exec(entry.tag);
      if (!match) continue;
      const prefix = match[1];
      const mark = match[2];
      if (mark === "start") {
        (stacks[prefix] ??= []).push({ start: entry, steps: [] });
        continue;
      }
      const stack = stacks[prefix];
      if (!stack || stack.length === 0) continue;
      const chainIndex = prefix === "panel" ? findPanelChainIndex(stack, entry) : stack.length - 1;
      if (chainIndex < 0) continue;
      const active = stack[chainIndex];
      if (prefix === "panel" && mark !== "paint-done") {
        active.steps.push(entry);
        continue;
      }
      const [chain] = stack.splice(chainIndex, 1);
      if (!chain) continue;
      const start = chain.start;
      const detail = start.detail ?? {};
      let action = "";
      if (prefix === "nav") action = String(detail.dir ?? "");
      else if (prefix === "view") action = `${detail.from} -> ${detail.to}`;
      else if (prefix === "panel") {
        const state = detail.state ? ` ${detail.state}` : "";
        const moduleState = detail.module ? `/${detail.module}` : "";
        action = `${String(detail.mode ?? "")}${state}${moduleState}`;
      }
      results.push({
        prefix: prefix as ChainRow["prefix"],
        action,
        durationMs: Math.round((entry.t - start.t) * 10) / 10,
        steps: chain.steps.map((step) => ({
          label: panelStepLabel(step.tag),
          ms: Math.round((step.t - start.t) * 10) / 10,
        })),
      });
    }
    return results;
  });
  const hasSpeedLogEntries = $derived(
    perfLog.entries.some((entry) => !entry.tag.startsWith("boot.")),
  );

  function memoryLabel(label: MemoryDisplayLabel): string {
    if (label === "Backend") return t("diagnostics.memory.backend");
    if (label === "Frontend") return t("diagnostics.memory.frontend");
    if (label === "Network") return t("diagnostics.memory.network");
    return t("diagnostics.memory.total");
  }

  function formatMemoryMb(value: number): string {
    return `${formatNumber(locale, value, {
      minimumFractionDigits: 1,
      maximumFractionDigits: 1,
    })} ${t("diagnostics.memory.unitMb")}`;
  }

  function formatWholeMilliseconds(value: number): string {
    return `${formatNumber(locale, value)} ms`;
  }

  function localizedBenchmarkSuiteLabel(id: string, fallback?: string): string {
    if (id === "core") return t("benchmark.suite.core.label");
    if (id === "backend") return t("benchmark.suite.backend.label");
    if (id === "all") return t("benchmark.suite.all.label");
    return fallback ?? id;
  }

  function localizedBenchmarkSuiteDescription(id: string, fallback: string): string {
    if (id === "core") return t("benchmark.suite.core.description");
    if (id === "backend") return t("benchmark.suite.backend.description");
    if (id === "all") return t("benchmark.suite.all.description");
    return fallback;
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

  function localizedBenchmarkScenarioDescription(id: string, fallback: string): string {
    if (id === "startup-boot") return t("benchmark.scenario.startupBoot.description");
    if (id === "idle-memory") return t("benchmark.scenario.idleMemory.description");
    if (id === "calendar-nav") return t("benchmark.scenario.calendarNav.description");
    if (id === "calendar-panel-latency") {
      return t("benchmark.scenario.calendarPanelLatency.description");
    }
    if (id === "calendar-import-ops") {
      return t("benchmark.scenario.calendarImportOps.description");
    }
    return fallback;
  }

  // Pin the chain list scroll to the bottom on new entries so the latest
  // action is always in view.
  let speedLogEl = $state<HTMLDivElement | undefined>(undefined);
  $effect(() => {
    void actionChains.length;
    if (speedLogEl) speedLogEl.scrollTop = speedLogEl.scrollHeight;
  });

  function formatBootRowText(row: BootRow): string {
    const value = formatNumber(locale, row.deltaMs, { maximumFractionDigits: 1 });
    return `  ${row.label.padEnd(20)} ${value} ms`;
  }

  function formatChainRowText(row: ChainRow): string {
    const steps = row.steps.length > 0
      ? ` (${row.steps.map((step) => {
        const value = formatNumber(locale, step.ms, { maximumFractionDigits: 1 });
        return `${step.label} ${value} ms`;
      }).join(", ")})`
      : "";
    const value = formatNumber(locale, row.durationMs, { maximumFractionDigits: 1 });
    return `  ${row.prefix.padEnd(6)} ${row.action.padEnd(16)} ${value} ms${steps}`;
  }

  function flashCopied(id: string) {
    copiedId = id;
    setTimeout(() => {
      if (copiedId === id) copiedId = null;
    }, 2000);
  }

  function copyToClipboard(id: string, text: string) {
    if (text.length === 0) return;
    navigator.clipboard.writeText(text);
    flashCopied(id);
  }

  function ramReportLines(report: MemoryReport, label: string): string[] {
    const lines: string[] = [`${label} (${report.platform})`, ""];
    const totalLabel = report.metric.slug === "unavailable"
      ? t("diagnostics.memory.total")
      : t("diagnostics.memory.totalMetric", report.metric.name);
    lines.push(`${t("diagnostics.metric")}: ${report.metric.name}`);
    lines.push("");
    lines.push(t("diagnostics.ramByProcess"));
    for (const p of report.processes) {
      lines.push(`  ${p.name}: ${formatMemoryMb(p.mb)}`);
    }
    lines.push(
      `  ${totalLabel}: ${formatNumber(locale, Math.round(report.total_mb))} ${t("diagnostics.memory.unitMb")}`,
    );
    return lines;
  }

  function totalMemoryTooltip(report: MemoryReport | null): string {
    if (!report) return t("diagnostics.memory.waiting");

    if (report.metric.slug === "pss" || report.metric.slug === "pss_rss") {
      const fallback = report.metric.slug === "pss_rss"
        ? t("diagnostics.memory.linuxRssFallback")
        : "";
      return t("diagnostics.memory.linuxPss", fallback);
    }
    if (report.metric.slug === "working_set") {
      return t("diagnostics.memory.windowsWorkingSet");
    }
    if (report.metric.slug === "unavailable") {
      return t("diagnostics.memory.macUnavailable");
    }

    return t("diagnostics.memory.metricDescription", report.metric.name, report.metric.description);
  }

  function speedLogLines(): string[] {
    if (actionChains.length === 0) return [];
    const lines: string[] = [t("diagnostics.speedLogCopyHeading", actionChains.length)];
    for (const row of actionChains) {
      lines.push(formatChainRowText(row));
    }
    return lines;
  }

  function launchTableLines(): string[] {
    if (launchMs === null) return [];
    const lines: string[] = [`${t("diagnostics.launchTime")}: ${formatWholeMilliseconds(launchMs)}`, ""];
    for (const row of bootRows) {
      lines.push(formatBootRowText(row));
    }
    return lines;
  }

  function copyLiveRam() {
    if (!liveReport) return;
    copyToClipboard("live-ram", ramReportLines(liveReport, t("diagnostics.liveRam")).join("\n"));
  }

  function copyStartupRam() {
    if (startupMemorySnapshot.status !== "ready") return;
    copyToClipboard(
      "startup-ram",
      ramReportLines(startupMemorySnapshot.report, t("diagnostics.startupRamSnapshot")).join("\n"),
    );
  }

  function copyChart() {
    copyToClipboard("chart", samplesToCsv(memorySamples, liveReport?.metric.slug));
  }

  function copySpeedLog() {
    copyToClipboard("speed-log", speedLogLines().join("\n"));
  }

  function copyLaunchTable() {
    const lines = launchTableLines();
    if (lines.length === 0) return;
    copyToClipboard("launch", lines.join("\n"));
  }

  async function requestBenchmark(scenarioId: string): Promise<void> {
    try {
      await ensureBenchmarkOverlay();
      const { getBenchmarkRunner } = await import("$lib/stores/benchmark-runner.svelte");
      getBenchmarkRunner().request(scenarioId);
    } catch (e) {
      console.error("benchmark overlay load failed", e);
    }
  }

  async function requestBenchmarkSuite(suite: BenchmarkSuiteMetadata): Promise<void> {
    try {
      await ensureBenchmarkOverlay();
      const { getBenchmarkRunner } = await import("$lib/stores/benchmark-runner.svelte");
      getBenchmarkRunner().requestSuite(suite.scenarioIds, suite.id);
    } catch (e) {
      console.error("benchmark overlay load failed", e);
    }
  }

  function scenariosForSuite(suite: BenchmarkSuiteMetadata): BenchmarkScenarioMetadata[] {
    const suiteIds = new Set(suite.scenarioIds);
    return BENCHMARK_SCENARIOS.filter((scenario) => suiteIds.has(scenario.id));
  }

  function suiteExpanded(suiteId: string): boolean {
    return expandedBenchmarkSuites.includes(suiteId);
  }

  function toggleBenchmarkSuite(suiteId: string): void {
    expandedBenchmarkSuites = suiteExpanded(suiteId)
      ? expandedBenchmarkSuites.filter((id) => id !== suiteId)
      : [...expandedBenchmarkSuites, suiteId];
  }
</script>

<!--
  Stop wheel propagation so scrolling inside the popover does not reach the
  title bar's tab-wheel handler.
-->
<!-- svelte-ignore a11y_no_static_element_interactions -->
<div
  class="surface-floating fixed z-50 flex w-floating flex-col overflow-hidden"
  style="top: calc(var(--titlebar-h) + 4px); right: 8px; max-height: calc(100dvh - var(--titlebar-h) - 12px);"
  onwheel={(e) => e.stopPropagation()}
>
  <div use:scrollEdgeFadeAction class="perf-scroll min-h-0 flex-1 overflow-y-auto overflow-x-hidden px-3 py-3">
    <div class="flex items-center justify-between gap-2">
      <div class="flex min-w-0 flex-1 items-center gap-1.5 text-[0.666667rem] uppercase tracking-wider">
        <button
          onclick={() => { perfLive = true; }}
          class={cn(
            "min-w-0 truncate transition-colors",
            perfLive ? "text-foreground" : "text-muted-foreground/40 hover:text-muted-foreground/70",
          )}
        >{t("diagnostics.liveRam")}</button>
        <span class="text-muted-foreground/30">|</span>
        <button
          onclick={() => { perfLive = false; }}
          class={cn(
            "min-w-0 truncate transition-colors",
            !perfLive ? "text-foreground" : "text-muted-foreground/40 hover:text-muted-foreground/70",
          )}
        >{t("diagnostics.startupRam")}</button>
      </div>
      <button
        onclick={() => onPinnedChange(!pinned)}
        class={cn(
          "flex h-5 w-5 items-center justify-center rounded transition-colors",
          pinned ? "text-foreground" : "text-muted-foreground/40 hover:text-muted-foreground",
        )}
        aria-label={pinned ? t("diagnostics.unpin") : t("diagnostics.pin")}
        data-app-tooltip-disabled="true"
      >
        {#if pinned}
          <Pin size={11} />
        {:else}
          <PinOff size={11} />
        {/if}
      </button>
    </div>

    {#if isDevMode}
      <div class="mt-2 flex w-full items-center justify-between gap-2 text-[0.666667rem] text-warning">
        <span class="flex min-w-0 items-center gap-1.5">
          <TriangleAlert size={11} class="shrink-0" />
          <span class="truncate">{t("diagnostics.devModeUsesMoreResources")}</span>
        </span>
      </div>
    {/if}

    {#if !perfLive && startupMemorySnapshot.status === "pending"}
      <div class="mt-2 text-xs text-muted-foreground">{t("diagnostics.startupSnapshotPending")}</div>
    {:else if !perfLive && startupMemorySnapshot.status === "failed"}
      <div class="mt-2 text-xs text-muted-foreground">{t("diagnostics.startupSnapshotUnavailable")}</div>
    {/if}

    <div class="mt-2 space-y-1.5" aria-busy={displayReport === null}>
      {#each memoryRows as row (row.label)}
        <div class="flex items-baseline justify-between">
          <span class="flex min-w-0 items-center gap-1 text-xs text-muted-foreground">
            <span>{memoryLabel(row.label)}</span>
          </span>
          {#if row.mb === null}
            <span class="min-w-14 text-right text-[0.733333rem] tabular-nums text-muted-foreground/50">...</span>
          {:else}
            <span class="flex min-w-0 items-baseline justify-end gap-1.5">
              {#if row.label === "Total"}
              <button
                type="button"
                class="inline-flex h-3.5 w-3.5 shrink-0 items-center justify-center rounded bg-transparent p-0 text-muted-foreground/70 transition-colors hover:text-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
                data-app-tooltip={totalMemoryTooltip(displayReport)}
                data-app-tooltip-keep-on-click="true"
                aria-label={totalMemoryTooltip(displayReport)}
              >
                <CircleHelp size={10} strokeWidth={2} />
              </button>
              {/if}
              <span class={cn("min-w-14 text-right text-[0.733333rem] tabular-nums text-foreground", showingHoveredSample && "italic")}>{formatMemoryMb(row.mb)}</span>
            </span>
          {/if}
        </div>
      {/each}
    </div>

    {#if perfLive}
      <div class="mt-3 flex justify-center">
        <MemoryChart
          samples={memorySamples}
          width={240}
          height={64}
          onHover={(s) => { chartHoverSample = s; }}
        />
      </div>
      <div class="mt-3 grid grid-cols-2 gap-1.5">
        <button
          onclick={copyLiveRam}
          disabled={liveReport === null}
          class="flex w-full items-center justify-center gap-1.5 rounded-md bg-primary px-2 py-1 text-[0.666667rem] font-medium uppercase tracking-wider text-primary-foreground transition-colors hover:bg-primary/90 disabled:cursor-not-allowed disabled:opacity-60"
        >
          {#if copiedId === "live-ram"}
            <Check size={11} />
            {t("diagnostics.copied")}
          {:else}
            <Copy size={11} />
            {t("diagnostics.copyLiveRam")}
          {/if}
        </button>
        <button
          onclick={copyChart}
          disabled={memorySamples.length === 0}
          class="flex w-full items-center justify-center gap-1.5 rounded-md bg-primary px-2 py-1 text-[0.666667rem] font-medium uppercase tracking-wider text-primary-foreground transition-colors hover:bg-primary/90 disabled:cursor-not-allowed disabled:opacity-60"
        >
          {#if copiedId === "chart"}
            <Check size={11} />
            {t("diagnostics.copied")}
          {:else}
            <Copy size={11} />
            {t("diagnostics.copyChart")}
          {/if}
        </button>
      </div>
    {:else if startupMemorySnapshot.status === "ready"}
      <button
        onclick={copyStartupRam}
        class="mt-3 flex w-full items-center justify-center gap-1.5 rounded-md bg-primary px-2 py-1 text-[0.666667rem] font-medium uppercase tracking-wider text-primary-foreground transition-colors hover:bg-primary/90"
      >
        {#if copiedId === "startup-ram"}
          <Check size={11} />
          {t("diagnostics.copied")}
        {:else}
          <Copy size={11} />
          {t("diagnostics.copyStartupRam")}
        {/if}
      </button>
    {/if}

    {#if launchMs !== null}
      <div class={sectionDividerClass}></div>
      <button
        onclick={() => (launchExpanded = !launchExpanded)}
        class="flex w-full items-center justify-between rounded text-left transition-colors hover:bg-accent"
      >
        <span class="text-[0.666667rem] uppercase tracking-wider text-foreground">{t("diagnostics.launchTime")}</span>
        <span class="flex items-center gap-1.5">
          <span class="text-[0.733333rem] tabular-nums text-foreground">{formatWholeMilliseconds(launchMs)}</span>
          <ChevronDown
            size={11}
            class={cn("text-muted-foreground transition-transform", launchExpanded && "rotate-180")}
          />
        </span>
      </button>
      {#if launchExpanded}
        {#if bootRows.length > 0}
          <div class="perf-scroll mt-1.5 max-h-48 overflow-y-auto rounded border border-border/50 bg-muted/30 px-2 py-1.5 text-[0.666667rem] leading-tight">
            {#each bootRows as row (row.label)}
              <div class="flex min-w-0 justify-between gap-2 text-muted-foreground tabular-nums">
                <span class="min-w-0 truncate">{row.label}</span>
                <span class="shrink-0">{formatNumber(locale, row.deltaMs, { maximumFractionDigits: 1 })} ms</span>
              </div>
            {/each}
          </div>
        {:else}
          <div class="mt-1.5 text-[0.666667rem] text-muted-foreground/60">{t("diagnostics.noBootMarksCaptured")}</div>
        {/if}
        <button
          onclick={copyLaunchTable}
          class="mt-1.5 flex w-full items-center justify-center gap-1.5 rounded-md bg-primary px-2 py-1 text-[0.666667rem] font-medium uppercase tracking-wider text-primary-foreground transition-colors hover:bg-primary/90"
        >
          {#if copiedId === "launch"}
            <Check size={11} />
            {t("diagnostics.copied")}
          {:else}
            <Copy size={11} />
            {t("diagnostics.copyLaunchTable")}
          {/if}
        </button>
      {/if}
    {/if}

    <div class={sectionDividerClass}></div>
    <div class="flex items-center justify-between">
      <span class="text-[0.666667rem] uppercase tracking-wider text-foreground">
        {t("diagnostics.speedLogHeading", actionChains.length)}
      </span>
      <div class="flex items-center gap-2">
        <button
          onclick={() => setTracking(!perfLog.tracking)}
          class={cn(
            "text-[0.666667rem] uppercase tracking-wider transition-colors",
            perfLog.tracking ? "text-foreground" : "text-muted-foreground/60 hover:text-foreground",
          )}
          title={perfLog.tracking
            ? t("diagnostics.stopTrackingTitle")
            : t("diagnostics.startTrackingTitle")}
        >{t("diagnostics.trackState", perfLog.tracking ? t("diagnostics.trackOn") : t("diagnostics.trackOff"))}</button>
        <button
          onclick={clearPerfLog}
          disabled={!hasSpeedLogEntries}
          class="text-[0.666667rem] uppercase tracking-wider text-muted-foreground/60 transition-colors hover:text-foreground disabled:cursor-not-allowed disabled:opacity-40 disabled:hover:text-muted-foreground/60"
        >{t("diagnostics.clear")}</button>
      </div>
    </div>
    {#if actionChains.length > 0}
      <div bind:this={speedLogEl} class="perf-scroll mt-1.5 max-h-48 overflow-y-auto rounded border border-border/50 bg-muted/30 px-2 py-1.5 text-[0.666667rem] leading-tight">
        {#each actionChains as row, i (i)}
          <div class="min-w-0 text-muted-foreground tabular-nums">
            <div class="flex min-w-0 justify-between gap-2">
              <span class="min-w-0 truncate"><span class="text-muted-foreground/60">{row.prefix}</span> {row.action}</span>
              <span class="shrink-0">{formatNumber(locale, row.durationMs, { maximumFractionDigits: 1 })} ms</span>
            </div>
            {#if row.steps.length > 0}
              <div class="truncate pl-6 text-muted-foreground/60">
                {row.steps.map((step) => {
                  const value = formatNumber(locale, step.ms, { maximumFractionDigits: 1 });
                  return `${step.label} ${value} ms`;
                }).join("  ")}
              </div>
            {/if}
          </div>
        {/each}
      </div>
    {/if}
    <button
      onclick={copySpeedLog}
      disabled={actionChains.length === 0}
      class="mt-1.5 flex w-full items-center justify-center gap-1.5 rounded-md bg-primary px-2 py-1 text-[0.666667rem] font-medium uppercase tracking-wider text-primary-foreground transition-colors hover:bg-primary/90 disabled:cursor-not-allowed disabled:opacity-60"
    >
      {#if copiedId === "speed-log"}
        <Check size={11} />
        {t("diagnostics.copied")}
      {:else}
        <Copy size={11} />
        {t("diagnostics.copySpeedLog")}
      {/if}
    </button>

    {#if BENCHMARK_SUITES.length > 0}
      <div class={sectionDividerClass}></div>
      <div class="flex items-center justify-between">
        <span class="text-[0.666667rem] uppercase tracking-wider text-foreground">{t("diagnostics.benchmarks")}</span>
        <span class="text-[0.666667rem] uppercase tracking-wider text-muted-foreground/60"
          >{t("diagnostics.restartsApp")}</span
        >
      </div>
      <div use:scrollEdgeFadeAction class="perf-scroll mt-1.5 flex max-h-56 flex-col gap-1 overflow-y-auto">
        {#each BENCHMARK_SUITES as suite (suite.id)}
          <div class="flex flex-col gap-1">
            <div class="flex gap-1">
              <button
                onclick={() => void requestBenchmarkSuite(suite)}
                disabled={benchmarkStatus.status !== "idle"}
                class="flex h-6 min-w-0 flex-1 items-center justify-start gap-1.5 rounded-md bg-primary px-2 text-[0.733333rem] font-semibold text-primary-foreground transition-colors hover:bg-primary/90 disabled:cursor-not-allowed disabled:opacity-60"
                title={localizedBenchmarkSuiteDescription(suite.id, suite.description)}
              >
                <Play size={12} class="shrink-0" />
                <span class="truncate">{t("diagnostics.runSuite", localizedBenchmarkSuiteLabel(suite.id, suite.label))}</span>
              </button>
              <button
                type="button"
                onclick={() => toggleBenchmarkSuite(suite.id)}
                class="flex h-6 w-6 shrink-0 items-center justify-center rounded-md border border-border bg-card text-muted-foreground transition-colors hover:bg-accent hover:text-foreground dark:bg-transparent"
                aria-label={suiteExpanded(suite.id)
                  ? t("diagnostics.hideSuite", localizedBenchmarkSuiteLabel(suite.id, suite.label))
                  : t("diagnostics.showSuite", localizedBenchmarkSuiteLabel(suite.id, suite.label))}
                data-app-tooltip-disabled="true"
                aria-expanded={suiteExpanded(suite.id)}
              >
                <ChevronDown
                  size={13}
                  class={cn("transition-transform", suiteExpanded(suite.id) && "rotate-180")}
                />
              </button>
            </div>
            {#if suiteExpanded(suite.id)}
              <div class="flex flex-col gap-1">
                {#each scenariosForSuite(suite) as scenario (scenario.id)}
                  <button
                    onclick={() => void requestBenchmark(scenario.id)}
                    disabled={benchmarkStatus.status !== "idle"}
                    class="flex h-6 w-full min-w-0 items-center justify-start gap-1.5 rounded-md bg-muted/70 px-2 text-[0.733333rem] font-medium text-foreground transition-colors hover:bg-accent disabled:cursor-not-allowed disabled:opacity-60"
                    title={localizedBenchmarkScenarioDescription(scenario.id, scenario.description)}
                  >
                    <Play size={11} class="shrink-0" />
                    <span class="truncate">{localizedBenchmarkScenarioLabel(scenario.id, scenario.label)}</span>
                  </button>
                {/each}
              </div>
            {/if}
          </div>
        {/each}
      </div>
    {/if}
  </div>
</div>
