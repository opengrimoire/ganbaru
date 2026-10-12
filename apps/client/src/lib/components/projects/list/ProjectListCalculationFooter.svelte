<script lang="ts">
  import CollectionRow from "$lib/components/collections/CollectionRow.svelte";
  import CollectionCell from "$lib/components/collections/CollectionCell.svelte";
  import Loading from "$lib/components/ui/Loading.svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { formatNumber } from "$lib/i18n/formatters";
  import type { ProjectTaskListColumn } from "$lib/projects/types";
  import type { ProjectTaskListResizableColumn } from "$lib/projects/list/view";
  import type { ProjectTaskQueryController } from "$lib/components/projects/task-query-controller.svelte";
  import { getProjectListTableContext } from "./table-context";
  import { projectListNumberLabel } from "$lib/projects/list/presentation";

  let { query, gridTemplate, gridMinWidth, columns, columnLabel }: {
    query: ProjectTaskQueryController;
    gridTemplate: string;
    gridMinWidth: string;
    columns: ProjectTaskListColumn[];
    columnLabel: (column: ProjectTaskListColumn) => string;
  } = $props();
  const localization = getLocalization();
  const { t } = localization;
  const context = getProjectListTableContext();
  const active = $derived(["name", ...columns].some((column) => (query.listPresentation.calculations[column as ProjectTaskListResizableColumn] ?? "none") !== "none"));

  /**
   * Read native reductions over the entire filtered query, including unloaded rows.
   * Returns `null` while the reduction for an active calculation is still pending.
   */
  function valueFor(column: ProjectTaskListResizableColumn): string | null {
    const calculation = query.listPresentation.calculations[column] ?? "none";
    if (calculation === "none") return "";
    const summary = query.columnCalculations?.find((entry) => entry.column === column);
    if (!summary) return null;
    const value = calculation === "count" ? summary.total : calculation === "filled" ? summary.filled
      : calculation === "empty" ? summary.total - summary.filled
      : calculation === "sum" ? summary.sum ?? 0 : summary[calculation];
    return t("projects.columns.calculationValue", t("projects.columns.calculation", calculation),
      value === undefined ? t("projects.customFields.emptyValue") : query.listPresentation.numberFormats[column] && ["sum", "average", "minimum", "maximum"].includes(calculation)
        ? projectListNumberLabel(value, query.listPresentation.numberFormats[column] ?? "number", localization.locale)
        : formatNumber(localization.locale, value, { maximumFractionDigits: 2 }));
  }
</script>

{#snippet calculationValue(column: ProjectTaskListResizableColumn)}
  {@const value = valueFor(column)}
  {#if value === null}<Loading layout="inline" />{:else}<span>{value}</span>{/if}
{/snippet}

{#if active && !query.loadError}
  <div aria-label={t("projects.columns.completeCalculations")} class="text-xs text-muted-foreground">
    <p class="mb-1 px-2">{t("projects.columns.completeCalculations")}</p>
    <CollectionRow template={gridTemplate} minWidth={gridMinWidth} gutter={context?.gutter} divider={false} role="row">
      <div class={context?.cellClass("selection")} style={context?.cellStyle("selection")}></div>
      <div class={context?.cellClass("expand")} style={context?.cellStyle("expand")}></div>
      <CollectionCell class={context?.cellClass("name")} style={context?.cellStyle("name")} aria-label={t("projects.list.name")}>{@render calculationValue("name")}</CollectionCell>
      {#each columns as column (column)}
        <CollectionCell class={context?.cellClass(column)} style={context?.cellStyle(column)} aria-label={columnLabel(column)}>{@render calculationValue(column)}</CollectionCell>
      {/each}
      <div></div>
    </CollectionRow>
  </div>
{/if}
