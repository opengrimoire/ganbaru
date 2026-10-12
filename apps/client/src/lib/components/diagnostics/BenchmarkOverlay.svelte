<script lang="ts">
  import LoaderCircle from "@lucide/svelte/icons/loader-circle";
  import Copy from "@lucide/svelte/icons/copy";
  import Check from "@lucide/svelte/icons/check";
  import { getBenchmarkRunner } from "$lib/stores/benchmark-runner.svelte";
  import { buildBenchmarkSuitePreview, formatBenchmarkSuiteMarkdown } from "$lib/benchmark/output";
  import type { BenchmarkScalarMetricRow } from "$lib/benchmark/output";
  import ConfirmDialog from "$lib/components/ui/ConfirmDialog.svelte";
  import Notice from "$lib/components/ui/Notice.svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { scrollEdgeFadeAction } from "$lib/utils/scroll-edge-fade";

  const runner = getBenchmarkRunner();
  const localization = getLocalization();
  const { t } = localization;
  const tableWrapClass = "overflow-x-auto rounded-md border border-border";
  const tableClass = "min-w-full border-collapse text-left text-[0.8rem]";
  const thClass = "sticky top-0 whitespace-nowrap border-b border-border bg-muted px-3 py-2 font-medium text-muted-foreground";
  const tdClass = "whitespace-nowrap border-b border-border/70 px-3 py-2 text-foreground";

  let copied = $state(false);
  let copyTimer: ReturnType<typeof setTimeout> | undefined;

  const summaryPreview = $derived.by(() => {
    if (runner.status !== "summary" || runner.results.length === 0) return null;
    return buildBenchmarkSuitePreview(runner.results, {});
  });

  // Build the summary markdown only once the run reaches the summary state.
  const summaryMarkdown = $derived.by(() => {
    if (runner.status !== "summary" || runner.results.length === 0) return "";
    return formatBenchmarkSuiteMarkdown(runner.results, {});
  });

  const confirmTitle = $derived.by(() => {
    if (runner.pendingMode !== "suite") return t("benchmark.runBenchmarkTitle");
    return t(
      "benchmark.runSuiteTitle",
      localizedBenchmarkSuiteLabel(runner.pendingSuiteLabel),
    );
  });
  const confirmMessage = $derived.by(() => {
    const labels = localizedPendingScenarioLabels();
    const prefix = runner.pendingMode === "suite"
      ? t("benchmark.suiteIntro", labels.length)
      : t("benchmark.singleIntro");
    const listed = runner.pendingMode === "suite"
      ? `\n\n${t("benchmark.scenariosHeading")}\n${labels.map((label) => `- ${label}`).join("\n")}`
      : "";
    return `${prefix}\n${t("benchmark.realCalendarUntouched")} ${t("benchmark.completionNotification")}${listed}`;
  });

  function copyMarkdown() {
    if (!summaryMarkdown) return;
    void navigator.clipboard.writeText(summaryMarkdown);
    copied = true;
    if (copyTimer) clearTimeout(copyTimer);
    copyTimer = setTimeout(() => {
      copied = false;
    }, 2000);
  }

  function runningDatasetLabel(phase: "A" | "B", datasetId: string | undefined): string {
    return phase === "A"
      ? t("benchmark.datasetBase")
      : datasetId ?? t("benchmark.datasetDenseFallback");
  }

  function scalarRowsUseUnitColumn(rows: BenchmarkScalarMetricRow[]): boolean {
    return rows.length === 0 || rows.some((row) => row.unit !== "ms");
  }

  function scalarValueHeading(rows: BenchmarkScalarMetricRow[]): string {
    return scalarRowsUseUnitColumn(rows)
      ? t("benchmark.table.value")
      : t("benchmark.table.valueMs");
  }

  function localizedPendingScenarioLabels(): string[] {
    const ids = runner.pendingRequest?.scenarioIds ?? [];
    if (ids.length === 0) return runner.pendingScenarioLabels;
    return ids.map((id) => localizedBenchmarkScenarioLabel(id, id));
  }

  function localizedBenchmarkSuiteLabel(idOrLabel: string | undefined): string {
    const normalized = idOrLabel?.toLowerCase();
    if (normalized === "core" || normalized === "core benchmarks") {
      return t("benchmark.suite.core.label");
    }
    if (normalized === "backend" || normalized === "backend benchmarks") {
      return t("benchmark.suite.backend.label");
    }
    if (normalized === "all" || normalized === "all benchmarks") {
      return t("benchmark.suite.all.label");
    }
    return idOrLabel ?? t("benchmark.suite.fallbackPlural");
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

  function localizedBenchmarkSectionTitle(id: string, fallback: string): string {
    if (id === "startup-boot") return t("benchmark.scenario.startupBoot.sectionTitle");
    if (id === "idle-memory") return t("benchmark.scenario.idleMemory.sectionTitle");
    if (id === "calendar-nav") return t("benchmark.scenario.calendarNav.sectionTitle");
    if (id === "calendar-panel-latency") {
      return t("benchmark.scenario.calendarPanelLatency.sectionTitle");
    }
    if (id === "calendar-import-ops") return t("benchmark.scenario.calendarImportOps.sectionTitle");
    return fallback;
  }

  function localizedWorkloadLabel(label: string): string {
    if (label === "calendar startup launch samples") {
      return t("benchmark.workload.calendarStartupLaunchSamples");
    }
    if (label === "idle calendar baseline") {
      return t("benchmark.workload.idleCalendarBaseline");
    }
    if (label === "held right-arrow week-view navigation") {
      return t("benchmark.workload.heldRightArrowWeekViewNavigation");
    }
    if (label === "scripted calendar panel open actions") {
      return t("benchmark.workload.scriptedCalendarPanelOpenActions");
    }
    if (label === "scripted calendar bulk import commands") {
      return t("benchmark.workload.scriptedCalendarBulkImportCommands");
    }
    return label;
  }

  function localizedRunningStep(step: string): string {
    if (step === "Setting up") return t("benchmark.step.settingUp");
    if (step === "Memory observation") return t("benchmark.step.memoryObservation");
    if (step === "Preparing memory observation") {
      return t("benchmark.step.preparingMemoryObservation");
    }
    if (step === "Restarting to seed dense dataset") {
      return t("benchmark.step.restartingToSeedDenseDataset");
    }
    if (step === "Restarting for baseline dataset") {
      return t("benchmark.step.restartingForBaselineDataset");
    }
    if (step === "Restarting for dense dataset") {
      return t("benchmark.step.restartingForDenseDataset");
    }
    if (step === "Restarting for next benchmark") {
      return t("benchmark.step.restartingForNextBenchmark");
    }
    if (step === "Restarting for next dense dataset") {
      return t("benchmark.step.restartingForNextDenseDataset");
    }

    const startupCooldown = /^Closing for (\d+) s startup cooldown$/.exec(step);
    if (startupCooldown) {
      return t("benchmark.step.startupCooldown", Number.parseInt(startupCooldown[1], 10));
    }

    const baselineCooldown =
      /^Closing for (\d+) s cooldown before baseline launch (\d+)\/(\d+)$/.exec(step);
    if (baselineCooldown) {
      return t(
        "benchmark.step.baselineCooldown",
        Number.parseInt(baselineCooldown[1], 10),
        Number.parseInt(baselineCooldown[2], 10),
        Number.parseInt(baselineCooldown[3], 10),
      );
    }

    const denseCooldown =
      /^Closing for (\d+) s cooldown before dense launch (\d+)\/(\d+)$/.exec(step);
    if (denseCooldown) {
      return t(
        "benchmark.step.denseCooldown",
        Number.parseInt(denseCooldown[1], 10),
        Number.parseInt(denseCooldown[2], 10),
        Number.parseInt(denseCooldown[3], 10),
      );
    }

    const launchSample = /^Launch sample (\d+)\/(\d+)$/.exec(step);
    if (launchSample) {
      return t(
        "benchmark.step.launchSample",
        Number.parseInt(launchSample[1], 10),
        Number.parseInt(launchSample[2], 10),
      );
    }

    const seeding = /^Seeding (.+)$/.exec(step);
    if (seeding) return t("benchmark.step.seeding", seeding[1]);

    const timedWorkload = /^(.+): (\d+) s$/.exec(step);
    if (timedWorkload) {
      return t(
        "benchmark.step.timedWorkload",
        localizedWorkloadLabel(timedWorkload[1]),
        Number.parseInt(timedWorkload[2], 10),
      );
    }

    return localizedWorkloadLabel(step);
  }
</script>

{#if runner.status === "confirming"}
  <ConfirmDialog
    title={confirmTitle}
    message={confirmMessage}
    confirmLabel={t("benchmark.confirmRun")}
    cancelLabel={t("common.cancel")}
    onConfirm={() => void runner.confirm()}
    onCancel={() => runner.cancelConfirm()}
  />
{/if}

{#if runner.status === "running" && runner.running}
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div class="fixed inset-0 z-75 flex items-center justify-center">
    <div class="surface-backdrop absolute inset-0 cursor-not-allowed"></div>
    <div
      class="relative z-10 flex w-[min(520px,90vw)] flex-col gap-4 px-6 py-5 surface-dialog"
    >
      <div class="flex items-center gap-2.5">
        <LoaderCircle size={16} strokeWidth={2.25} class="animate-spin text-primary" />
        <h2 class="text-[0.933333rem] font-semibold text-foreground">
          {t(
            "benchmark.runningTitle",
            localizedBenchmarkScenarioLabel(runner.running.scenarioId, runner.running.scenarioLabel),
          )}
        </h2>
      </div>

      <div class="text-[0.866667rem] text-foreground">
        {#if runner.running.suite}
          {t(
            "benchmark.runningSuiteProgress",
            runner.running.suite.index + 1,
            runner.running.suite.total,
          )}
        {/if}
        {t(
          "benchmark.dataset",
          runningDatasetLabel(runner.running.phase, runner.running.datasetId),
        )} {localizedRunningStep(runner.running.step)}
        {#if runner.running.curve}
          <span class="text-muted-foreground"
            >({runner.running.curve.done}/{runner.running.curve.total})</span
          >
        {/if}
      </div>

      <p class="text-[0.8rem] text-muted-foreground">
        {t("benchmark.runningWarning")}
      </p>

      <div class="flex justify-end">
        <button
          type="button"
          onclick={() => void runner.cancel()}
          class="rounded-md border border-border bg-card px-3 py-1.5 text-[0.8rem] font-medium text-foreground transition-colors hover:bg-accent dark:bg-transparent"
        >
          {t("common.cancel")}
        </button>
      </div>
    </div>
  </div>
{/if}

{#if runner.status === "summary" && runner.result}
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div class="fixed inset-0 z-75 flex items-center justify-center">
    <div class="surface-backdrop absolute inset-0 cursor-not-allowed"></div>
    <div
      class="relative z-10 flex h-[80vh] w-[min(1040px,94vw)] flex-col surface-dialog"
    >
      <header class="flex items-center justify-between border-b border-border px-5 py-3">
        <div>
          <h2 class="text-[0.933333rem] font-semibold text-foreground">{t("benchmark.completeTitle")}</h2>
          <p class="mt-0.5 text-[0.8rem] text-muted-foreground">
            {t("benchmark.completeDescription")}
          </p>
        </div>
        <button
          type="button"
          onclick={copyMarkdown}
          class="flex items-center gap-1.5 rounded-md border border-border bg-primary px-3 py-1.5 text-[0.8rem] font-medium text-primary-foreground transition-colors hover:bg-primary/90"
        >
          {#if copied}
            <Check size={12} strokeWidth={2.25} />
            <span>{t("benchmark.copied")}</span>
          {:else}
            <Copy size={12} strokeWidth={2.25} />
            <span>{t("benchmark.copyMarkdown")}</span>
          {/if}
        </button>
      </header>

      <div use:scrollEdgeFadeAction class="flex-1 space-y-5 overflow-y-auto px-5 py-4">
        {#if summaryPreview}
          <section class="space-y-2">
            <h3 class="text-[0.866667rem] font-semibold text-foreground">{t("benchmark.runMetadata")}</h3>
            <div class={tableWrapClass}>
              <table class={tableClass}>
                <thead>
                  <tr>
                    <th class={thClass}>{t("benchmark.table.run")}</th>
                    <th class={thClass}>{t("benchmark.table.harness")}</th>
                    <th class={thClass}>{t("benchmark.table.anchorDate")}</th>
                    <th class={thClass}>{t("benchmark.table.buildRef")}</th>
                    <th class={thClass}>{t("benchmark.table.platform")}</th>
                    <th class={thClass}>{t("benchmark.table.notes")}</th>
                  </tr>
                </thead>
                <tbody>
                  <tr>
                    <td class={tdClass}>{summaryPreview.metadata.run}</td>
                    <td class={tdClass}>{summaryPreview.metadata.harness}</td>
                    <td class={tdClass}>{summaryPreview.metadata.anchorDate}</td>
                    <td class={tdClass}>{summaryPreview.metadata.buildRef}</td>
                    <td class={tdClass}>{summaryPreview.metadata.platform}</td>
                    <td class={tdClass}>{summaryPreview.metadata.notes}</td>
                  </tr>
                </tbody>
              </table>
            </div>
          </section>

          {#each summaryPreview.sections as section, sectionIndex (section.id + sectionIndex)}
            <section class="space-y-2">
              <h3 class="text-[0.866667rem] font-semibold text-foreground">{localizedBenchmarkSectionTitle(section.id, section.title)}</h3>
              <div class={section.kind === "startup" ? tableWrapClass : "space-y-3"}>
                {#if section.kind === "startup"}
                  <table class={tableClass}>
                    <thead>
                      <tr>
                        <th class={thClass}>{t("benchmark.table.run")}</th>
                        <th class={thClass}>{t("benchmark.table.dataset")}</th>
                        <th class={thClass}>{t("benchmark.table.runs")}</th>
                        <th class={thClass}>{t("benchmark.table.usablePaintMedianMs")}</th>
                        <th class={thClass}>{t("benchmark.table.launchMedianMs")}</th>
                        <th class={thClass}>{t("benchmark.table.launchP95Ms")}</th>
                      </tr>
                    </thead>
                    <tbody>
                      {#each section.rows as row}
                        <tr>
                          <td class={tdClass}>{row.run}</td>
                          <td class={tdClass}>{row.dataset}</td>
                          <td class={tdClass}>{row.runs}</td>
                          <td class={tdClass}>{row.usablePaintMedianMs}</td>
                          <td class={tdClass}>{row.launchMedianMs}</td>
                          <td class={tdClass}>{row.launchP95Ms}</td>
                        </tr>
                      {/each}
                    </tbody>
                  </table>
                {:else if section.kind === "memory"}
                  <div class={tableWrapClass}>
                    <table class={tableClass}>
                      <thead>
                        <tr>
                          <th class={thClass}>{t("benchmark.table.run")}</th>
                          <th class={thClass}>{t("benchmark.table.dataset")}</th>
                          <th class={thClass}>{t("benchmark.table.statistic")}</th>
                          <th class={thClass}>{t("benchmark.table.backendMb")}</th>
                          <th class={thClass}>{t("benchmark.table.frontendMb")}</th>
                          <th class={thClass}>{t("benchmark.table.networkMb")}</th>
                          <th class={thClass}>{t("benchmark.table.totalMb")}</th>
                        </tr>
                      </thead>
                      <tbody>
                        {#each section.rows as row}
                          <tr>
                            <td class={tdClass}>{row.run}</td>
                            <td class={tdClass}>{row.dataset}</td>
                            <td class={tdClass}>{row.statistic}</td>
                            <td class={tdClass}>{row.backendMb}</td>
                            <td class={tdClass}>{row.frontendMb}</td>
                            <td class={tdClass}>{row.networkMb}</td>
                            <td class={tdClass}>{row.totalMb}</td>
                          </tr>
                        {/each}
                      </tbody>
                    </table>
                  </div>
                  {#if section.scalarRows.length > 0}
                    <div class={tableWrapClass}>
                      <table class={tableClass}>
                        <thead>
                          <tr>
                            <th class={thClass}>{t("benchmark.table.run")}</th>
                            <th class={thClass}>{t("benchmark.table.dataset")}</th>
                            <th class={thClass}>{t("benchmark.table.metric")}</th>
                            <th class={thClass}>{scalarValueHeading(section.scalarRows)}</th>
                            {#if scalarRowsUseUnitColumn(section.scalarRows)}
                              <th class={thClass}>{t("benchmark.table.unit")}</th>
                            {/if}
                          </tr>
                        </thead>
                        <tbody>
                          {#each section.scalarRows as row}
                            <tr>
                              <td class={tdClass}>{row.run}</td>
                              <td class={tdClass}>{row.dataset}</td>
                              <td class={tdClass}>{row.metric}</td>
                              <td class={tdClass}>{row.value}</td>
                              {#if scalarRowsUseUnitColumn(section.scalarRows)}
                                <td class={tdClass}>{row.unit}</td>
                              {/if}
                            </tr>
                          {/each}
                        </tbody>
                      </table>
                    </div>
                  {/if}
                {:else}
                  {#if section.latencyRows.length > 0}
                    <div class={tableWrapClass}>
                      <table class={tableClass}>
                        <thead>
                          <tr>
                            <th class={thClass}>{t("benchmark.table.run")}</th>
                            <th class={thClass}>{t("benchmark.table.dataset")}</th>
                            <th class={thClass}>{section.id === "calendar-panel-latency" ? t("benchmark.table.action") : t("benchmark.table.metric")}</th>
                            {#if section.id !== "calendar-panel-latency"}
                              <th class={thClass}>{t("benchmark.table.runs")}</th>
                            {/if}
                            <th class={thClass}>{t("benchmark.table.medianMs")}</th>
                            <th class={thClass}>{t("benchmark.table.p95Ms")}</th>
                          </tr>
                        </thead>
                        <tbody>
                          {#each section.latencyRows as row}
                            <tr>
                              <td class={tdClass}>{row.run}</td>
                              <td class={tdClass}>{row.dataset}</td>
                              <td class={tdClass}>{row.metric}</td>
                              {#if section.id !== "calendar-panel-latency"}
                                <td class={tdClass}>{row.runs}</td>
                              {/if}
                              <td class={tdClass}>{row.median}</td>
                              <td class={tdClass}>{row.p95}</td>
                            </tr>
                          {/each}
                        </tbody>
                      </table>
                    </div>
                  {/if}
                  {#if section.scalarRows.length > 0}
                    <div class={tableWrapClass}>
                      <table class={tableClass}>
                        <thead>
                          <tr>
                            <th class={thClass}>{t("benchmark.table.run")}</th>
                            <th class={thClass}>{t("benchmark.table.dataset")}</th>
                            <th class={thClass}>{t("benchmark.table.metric")}</th>
                            <th class={thClass}>{scalarValueHeading(section.scalarRows)}</th>
                            {#if scalarRowsUseUnitColumn(section.scalarRows)}
                              <th class={thClass}>{t("benchmark.table.unit")}</th>
                            {/if}
                          </tr>
                        </thead>
                        <tbody>
                          {#each section.scalarRows as row}
                            <tr>
                              <td class={tdClass}>{row.run}</td>
                              <td class={tdClass}>{row.dataset}</td>
                              <td class={tdClass}>{row.metric}</td>
                              <td class={tdClass}>{row.value}</td>
                              {#if scalarRowsUseUnitColumn(section.scalarRows)}
                                <td class={tdClass}>{row.unit}</td>
                              {/if}
                            </tr>
                          {/each}
                        </tbody>
                      </table>
                    </div>
                  {/if}
                  {#if section.latencyRows.length === 0 && section.scalarRows.length === 0}
                    <p class="text-[0.8rem] text-muted-foreground">{t("benchmark.noPrimaryMetrics")}</p>
                  {/if}
                {/if}
              </div>
            </section>
          {/each}
        {/if}
      </div>

      <footer class="flex justify-end border-t border-border px-5 py-3">
        <button
          type="button"
          onclick={() => runner.closeSummary()}
          class="rounded-md border border-border bg-card px-3 py-1.5 text-[0.8rem] font-medium text-foreground transition-colors hover:bg-accent dark:bg-transparent"
        >
          {t("benchmark.returnToData")}
        </button>
      </footer>
    </div>
  </div>
{/if}

{#if runner.status === "error"}
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div class="fixed inset-0 z-75 flex items-center justify-center">
    <div class="surface-backdrop absolute inset-0 cursor-not-allowed"></div>
    <Notice
      layout="floating"
      class="text-[0.8rem]"
      title={t("benchmark.failedTitle")}
      message={runner.errorMessage ?? t("benchmark.unknownError")}
      onDismiss={() => runner.closeSummary()}
    />
  </div>
{/if}
