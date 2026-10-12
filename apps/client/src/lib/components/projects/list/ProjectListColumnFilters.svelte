<script lang="ts">
  import Check from "@lucide/svelte/icons/check";
  import CollectionMenu from "$lib/components/collections/CollectionMenu.svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { customFieldIdFromTaskListColumn } from "$lib/projects/tasks/list-columns";
  import type { ProjectCustomFieldFilter, ProjectTaskDueFilter, ProjectTaskStatusFilter } from "$lib/projects/types";
  import type { ProjectTaskListResizableColumn } from "$lib/projects/list/view";
  import type { ProjectTaskQueryController } from "$lib/components/projects/task-query-controller.svelte";
  import { getProjects } from "$lib/stores/projects.svelte";

  let { query, column }: { query: ProjectTaskQueryController; column: ProjectTaskListResizableColumn } = $props();
  const { t } = getLocalization();
  const projects = getProjects();
  const field = $derived(column === "name" ? undefined : query.customFields.find((entry) => entry.id === customFieldIdFromTaskListColumn(column)));
  const current = $derived(field ? query.customFieldFilters.find((entry) => entry.fieldId === field.id) : undefined);
  const statusOptions: ProjectTaskStatusFilter[] = ["all", "open", "blocked", "done"];
  const dueOptions: ProjectTaskDueFilter[] = ["all", "overdue", "today", "week", "none"];

  /** Replace the canonical property's filter so toolbar controls and saved views stay synchronized. */
  function setCustom(filter?: ProjectCustomFieldFilter): void {
    if (!field) return;
    query.customFieldFilters = [...query.customFieldFilters.filter((entry) => entry.fieldId !== field.id), ...(filter ? [filter] : [])];
  }

  /** Return the localized label for a supported due-date filter. */
  function dueLabel(value: ProjectTaskDueFilter): string {
    if (value === "all") return t("projects.filters.allDueDates");
    if (value === "overdue") return t("projects.filters.overdue");
    if (value === "today") return t("projects.filters.today");
    if (value === "week") return t("projects.filters.thisWeek");
    return t("projects.filters.noDueDate");
  }
</script>

{#snippet choice(label: string, selected: boolean, select: () => void)}
  <button type="button" class="menu-item min-w-0 justify-between" aria-pressed={selected} onclick={select}><span class="min-w-0 truncate">{label}</span>{#if selected}<Check class="size-3.5 shrink-0" aria-hidden="true" />{/if}</button>
{/snippet}

{#if column === "name" || column === "status" || column === "priority" || column === "due" || column === "scheduled" || field}
  <CollectionMenu label={t("collections.property.filter")} kind="filter" fullWidth>
    {#if column === "name"}
      <input class="field w-full" aria-label={t("projects.header.searchPlaceholder")} bind:value={query.search} />
    {:else if column === "status"}
      {#each statusOptions as option}
        {@render choice(option === "all" ? t("projects.filters.allStatuses") : option === "open" ? t("projects.filters.open") : option === "blocked" ? t("projects.filters.blocked") : t("projects.filters.done"), query.statusFilter === option, () => { query.statusFilter = option; })}
      {/each}
    {:else if column === "priority"}
      {@render choice(t("projects.filters.allPriorities"), query.priorityFilter === "all", () => { query.priorityFilter = "all"; })}
      {#each query.priorities as priority}{@render choice(priority.name, query.priorityFilter === priority.id, () => { query.priorityFilter = priority.id; })}{/each}
    {:else if column === "due"}
      {#each dueOptions as option}{@render choice(dueLabel(option), query.dueFilter === option, () => { query.dueFilter = option; })}{/each}
    {:else if column === "scheduled"}
      {@render choice(t("projects.filters.allSchedule"), query.scheduleFilter === "all", () => { query.scheduleFilter = "all"; })}
      {@render choice(t("projects.filters.scheduled"), query.scheduleFilter === "scheduled", () => { query.scheduleFilter = "scheduled"; })}
      {@render choice(t("projects.filters.unscheduled"), query.scheduleFilter === "unscheduled", () => { query.scheduleFilter = "unscheduled"; })}
    {:else if field}
      {@render choice(t("projects.toolbar.none"), !current, () => setCustom())}
      {@render choice(t("projects.filters.filled"), current?.mode === "filled", () => setCustom({ fieldId: field.id, mode: "filled" }))}
      {@render choice(t("projects.filters.empty"), current?.mode === "empty", () => setCustom({ fieldId: field.id, mode: "empty" }))}
      {#if field.fieldType === "checkbox"}
        {@render choice(t("projects.customFields.checked"), current?.mode === "checkbox" && current.checked, () => setCustom({ fieldId: field.id, mode: "checkbox", checked: true }))}
        {@render choice(t("projects.customFields.unchecked"), current?.mode === "checkbox" && !current.checked, () => setCustom({ fieldId: field.id, mode: "checkbox", checked: false }))}
      {:else if field.fieldType === "select" || field.fieldType === "multi_select" || field.fieldType === "status"}
        {#each projects.customFieldOptionsForField(field.id) as option}
          {@render choice(option.name, current?.mode === "option" && current.optionId === option.id, () => setCustom({ fieldId: field.id, mode: "option", optionId: option.id }))}
        {/each}
      {/if}
    {/if}
  </CollectionMenu>
{/if}
