<script lang="ts">
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { scrollEdgeFadeAction } from "$lib/utils/scroll-edge-fade";
  import {
    roundTripCategoryCounts,
    visibleRoundTripCounts,
    type NotesRoundTripCountItem,
    type NotesRoundTripDiagnosticCategory,
    type NotesRoundTripDiagnosticItem,
  } from "$lib/notes/round-trip-diagnostics";

  let {
    counts = [],
    diagnostics = [],
  }: {
    counts?: NotesRoundTripCountItem[];
    diagnostics?: NotesRoundTripDiagnosticItem[];
  } = $props();

  const { t } = getLocalization();
  const visibleCounts = $derived(visibleRoundTripCounts(counts));
  const categoryCounts = $derived(roundTripCategoryCounts(diagnostics));

  function categoryLabel(category: NotesRoundTripDiagnosticCategory, count: number): string {
    if (category === "preserved") return t("notes.roundTripCategoryPreserved", count);
    if (category === "approximated") return t("notes.roundTripCategoryApproximated", count);
    if (category === "skipped") return t("notes.roundTripCategorySkipped", count);
    if (category === "unsupported") return t("notes.roundTripCategoryUnsupported", count);
    if (category === "error") return t("notes.roundTripCategoryError", count);
    if (category === "warning") return t("notes.roundTripCategoryWarning", count);
    return t("notes.roundTripCategoryInfo", count);
  }

  function severityLabel(severity: NotesRoundTripDiagnosticItem["severity"]): string {
    if (severity === "error") return t("notes.roundTripSeverityError");
    if (severity === "warning") return t("notes.roundTripSeverityWarning");
    return t("notes.roundTripSeverityInfo");
  }
</script>

<div class="grid gap-2 rounded-md border border-border bg-background/55 px-3 py-2">
  <div class="text-[0.8rem] font-medium text-foreground">
    {t("notes.roundTripDiagnosticsTitle")}
  </div>

  {#if visibleCounts.length > 0}
    <div class="grid gap-1">
      <div class="text-[0.733333rem] font-medium text-muted-foreground">
        {t("notes.roundTripObjectCounts")}
      </div>
      <dl class="notes-round-trip-counts grid gap-1">
        {#each visibleCounts as count (count.id)}
          <div class="min-w-0 rounded border border-border bg-muted/35 px-2 py-1">
            <dt class="truncate text-[0.666667rem] text-muted-foreground">{count.label}</dt>
            <dd class="text-[0.866667rem] font-semibold text-foreground">{count.value}</dd>
          </div>
        {/each}
      </dl>
    </div>
  {/if}

  <div class="grid gap-1">
    <div class="text-[0.733333rem] font-medium text-muted-foreground">
      {t("notes.roundTripIssueCounts")}
    </div>
    {#if categoryCounts.length > 0}
      <div class="flex flex-wrap gap-1">
        {#each categoryCounts as entry (`${entry.category}-${entry.count}`)}
          <span class="rounded border border-border bg-muted/45 px-1.5 py-0.5 text-[0.7rem] text-muted-foreground">
            {categoryLabel(entry.category, entry.count)}
          </span>
        {/each}
      </div>
    {:else}
      <p class="text-[0.733333rem] text-muted-foreground">
        {t("notes.roundTripDiagnosticsEmpty")}
      </p>
    {/if}
  </div>

  {#if diagnostics.length > 0}
    <ul class="grid max-h-36 gap-1 overflow-auto text-[0.733333rem] text-muted-foreground" use:scrollEdgeFadeAction>
      {#each diagnostics as diagnostic, index (`${diagnostic.code}-${index}`)}
        <li class="grid gap-0.5 rounded border border-border/70 bg-muted/25 px-2 py-1">
          <div class="flex min-w-0 flex-wrap items-center gap-1">
            <span class="rounded bg-muted px-1.5 py-0.5 text-[0.666667rem] text-muted-foreground">
              {severityLabel(diagnostic.severity)}
            </span>
            <span class="rounded bg-muted px-1.5 py-0.5 text-[0.666667rem] text-muted-foreground">
              {categoryLabel(diagnostic.category, 1)}
            </span>
            {#if diagnostic.sourceHref && diagnostic.sourceLabel}
              <a
                class="min-w-0 truncate text-primary underline-offset-2 hover:underline"
                href={diagnostic.sourceHref}
                onclick={(event) => event.stopPropagation()}
              >
                {diagnostic.sourceLabel}
              </a>
            {:else if diagnostic.sourceLabel}
              <span class="min-w-0 truncate text-muted-foreground">{diagnostic.sourceLabel}</span>
            {/if}
          </div>
          <div class="text-foreground/85">{diagnostic.message}</div>
        </li>
      {/each}
    </ul>
  {/if}
</div>

<style>
  .notes-round-trip-counts {
    grid-template-columns: repeat(auto-fit, minmax(6rem, 1fr));
  }
</style>
