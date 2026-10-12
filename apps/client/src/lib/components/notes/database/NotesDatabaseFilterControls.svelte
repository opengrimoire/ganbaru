<script lang="ts">
  import Plus from "@lucide/svelte/icons/plus";
  import X from "@lucide/svelte/icons/x";
  import Select from "$lib/components/ui/Select.svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import {
    NOTES_DATABASE_QUERY_MAX_FILTERS, NOTES_DATABASE_QUERY_MAX_FILTER_GROUPS, NOTES_DATABASE_QUERY_MAX_FILTER_DEPTH,
    notesDatabaseAppendFilter, notesDatabaseFilterConditions, notesDatabaseFilterCount, notesDatabaseFilterGroupCount,
    notesDatabaseIsDateFilterProperty, notesDatabaseFilterNeedsValue, notesDatabaseIsFilterPredicate,
    notesDatabaseNewFilter, notesDatabaseTransformFilter, notesDatabaseUpdatedFilters,
    type NotesDatabaseQueryProperty,
  } from "$lib/notes/database/query-controls";
  import { notesDatabaseIsFilterDateValue } from "$lib/notes/database/filters";
  import type { NotesDatabaseTableFilter, NotesDatabaseTableFilterCondition, NotesDatabaseTableFilterPredicate } from "$lib/notes/types";

  let { properties, filters, pending = false, propertyId = null, onChange }: {
    properties: readonly NotesDatabaseQueryProperty[];
    filters: readonly NotesDatabaseTableFilter[];
    pending?: boolean;
    propertyId?: string | null;
    onChange: (filters: NotesDatabaseTableFilter[]) => void;
  } = $props();

  const { t } = getLocalization();
  const available = $derived(properties.filter((property) => notesDatabaseFilterConditions(property).length));
  const target = $derived(propertyId ? available.find((property) => property.id === propertyId) : available[0]);
  const filterCount = $derived(notesDatabaseFilterCount(filters));
  const groupCount = $derived(notesDatabaseFilterGroupCount(filters));

  /** Persist a checked scalar edit through the owning layout. */
  function update(path: number[], patch: Partial<NotesDatabaseTableFilterPredicate>): void {
    if (!pending) onChange(notesDatabaseUpdatedFilters(filters, path, patch, properties));
  }

  /** Add a complete predicate or group within the shared native bounds. */
  function add(path: number[], isGroup = false): void {
    if (pending || !target || filterCount >= NOTES_DATABASE_QUERY_MAX_FILTERS || (isGroup && groupCount >= NOTES_DATABASE_QUERY_MAX_FILTER_GROUPS)) return;
    const predicate = notesDatabaseNewFilter(target);
    onChange(notesDatabaseAppendFilter(filters, path, isGroup ? { type: "and", filters: [predicate] } : predicate));
  }

  /** Persist only complete numbers and dates; leave invalid drafts available for correction. */
  function commitValue(input: HTMLInputElement, filter: NotesDatabaseTableFilterPredicate, property: NotesDatabaseQueryProperty | undefined, path: number[]): void {
    if (!property || pending || input.value === String(filter.value ?? "")) return;
    let value: string | number = input.value;
    if (property.type === "number") {
      if (!value.trim() || !Number.isFinite(Number(value))) { input.setCustomValidity(t("notes.databaseTableFilterInvalidNumber")); input.reportValidity(); return; }
      value = Number(value);
    } else if (notesDatabaseIsDateFilterProperty(property) && !notesDatabaseIsFilterDateValue(value)) {
      input.setCustomValidity(t("notes.databaseTableFilterInvalidDate")); input.reportValidity(); return;
    }
    input.setCustomValidity("");
    update(path, { value });
  }

  /** Translate the stable native condition identifiers. */
  function conditionLabel(condition: NotesDatabaseTableFilterCondition): string {
    switch (condition) {
      case "contains": return t("notes.databaseTableFilterCondition.contains");
      case "equals": return t("notes.databaseTableFilterCondition.equals");
      case "not_equals": return t("notes.databaseTableFilterCondition.notEquals");
      case "greater_than": return t("notes.databaseTableFilterCondition.greaterThan");
      case "greater_than_or_equal": return t("notes.databaseTableFilterCondition.greaterThanOrEqual");
      case "less_than": return t("notes.databaseTableFilterCondition.lessThan");
      case "less_than_or_equal": return t("notes.databaseTableFilterCondition.lessThanOrEqual");
      case "before": return t("notes.databaseTableFilterCondition.before");
      case "on_or_before": return t("notes.databaseTableFilterCondition.onOrBefore");
      case "after": return t("notes.databaseTableFilterCondition.after");
      case "on_or_after": return t("notes.databaseTableFilterCondition.onOrAfter");
      case "is_empty": return t("notes.databaseTableFilterCondition.isEmpty");
      case "is_not_empty": return t("notes.databaseTableFilterCondition.isNotEmpty");
      case "checked": return t("notes.databaseTableFilterCondition.checked");
      case "unchecked": return t("notes.databaseTableFilterCondition.unchecked");
    }
  }
</script>

{#snippet renderNodes(nodes: readonly NotesDatabaseTableFilter[], parentPath: number[], depth: number)}
  {#each nodes as filter, index}
    {@const path = [...parentPath, index]}
    {#if notesDatabaseIsFilterPredicate(filter)}
      {#if propertyId === null || filter.property_id === propertyId}
        {@const property = properties.find((candidate) => candidate.id === filter.property_id)}
        <div class="grid gap-1.5" data-notes-filter-predicate>
          <div class={propertyId === null ? "grid min-w-0 grid-cols-[minmax(0,1fr)_minmax(0,1fr)_auto] gap-1" : "grid min-w-0 grid-cols-[minmax(0,1fr)_auto] gap-1"}>
            {#if propertyId === null}
              <Select textSize="collection" inline appearance="quiet" contentAlign="start" class="w-full min-w-0"
                ariaLabel={t("notes.databaseTableFilterProperty")} value={filter.property_id} disabled={pending}
                options={available.map((candidate) => ({ value: candidate.id, label: candidate.name }))}
                onChange={(property_id) => update(path, { property_id })} />
            {/if}
            <Select textSize="collection" inline appearance="quiet" contentAlign="start" class="w-full min-w-0"
              ariaLabel={t("notes.databaseTableFilterConditionLabel")} value={filter.condition} disabled={pending || !property}
              options={property ? notesDatabaseFilterConditions(property).map((condition) => ({ value: condition, label: conditionLabel(condition) })) : []}
              onChange={(value) => {
                const condition = property && notesDatabaseFilterConditions(property).find((candidate) => candidate === value);
                if (condition) update(path, { condition });
              }} />
            <button type="button" class="inline-flex size-7 items-center justify-center rounded-md text-muted-foreground hover:bg-accent hover:text-foreground"
              disabled={pending} aria-label={t("notes.databaseTableRemoveFilter")}
              onclick={() => { if (!pending) onChange(notesDatabaseTransformFilter(filters, path, () => null)); }}>
              <X class="size-3.5" aria-hidden="true" />
            </button>
          </div>
          {#if notesDatabaseFilterNeedsValue(filter.condition)}
            <input class="field min-w-0 text-foreground"
              type={property?.type === "number" ? "number" : property && notesDatabaseIsDateFilterProperty(property) && String(filter.value ?? "").length === 10 ? "date" : "text"}
              step={property?.type === "number" ? "any" : undefined}
              value={String(filter.value ?? "")} disabled={pending} aria-label={t("notes.databaseTableFilterValue")}
              oninput={(event) => event.currentTarget.setCustomValidity("")}
              onblur={(event) => commitValue(event.currentTarget, filter, property, path)}
              onkeydown={(event) => { event.stopPropagation(); if (event.key === "Enter") event.currentTarget.blur(); }} />
          {/if}
        </div>
      {/if}
    {:else if propertyId === null || notesDatabaseFilterCount(filter.filters, propertyId)}
      <div class="grid min-w-0 gap-2 rounded-md border border-border/60 p-2" data-notes-filter-group={filter.type}>
        <div class="flex min-w-0 items-center gap-1">
          <Select textSize="collection" inline appearance="quiet" class="min-w-0 flex-1" ariaLabel={t("notes.databaseTableFilterGroupOperator")}
            value={filter.type} disabled={pending} options={[{ value: "and", label: t("notes.databaseTableFilterMatchAll") }, { value: "or", label: t("notes.databaseTableFilterMatchAny") }]}
            onChange={(value) => { if (!pending && (value === "and" || value === "or")) onChange(notesDatabaseTransformFilter(filters, path, () => ({ type: value, filters: filter.filters }))); }} />
          <button type="button" class="inline-flex size-7 items-center justify-center rounded-md text-muted-foreground hover:bg-accent hover:text-foreground"
            disabled={pending} aria-label={t("notes.databaseTableRemoveFilterGroup")}
            onclick={() => { if (!pending) onChange(notesDatabaseTransformFilter(filters, path, () => null)); }}><X class="size-3.5" aria-hidden="true" /></button>
        </div>
        {@render renderNodes(filter.filters, path, depth + 1)}
        {@render addButtons(path, depth + 1)}
      </div>
    {/if}
  {/each}
{/snippet}

{#snippet addButtons(path: number[], depth: number)}
  <div class="flex flex-wrap gap-1">
    <button type="button" class="flex min-h-7 items-center gap-1.5 rounded-md px-2 text-left text-muted-foreground hover:bg-accent hover:text-foreground"
      disabled={pending || !target || filterCount >= NOTES_DATABASE_QUERY_MAX_FILTERS} onclick={() => add(path)}>
      <Plus class="size-3.5 shrink-0" aria-hidden="true" />{t("notes.databaseTableAddFilter")}
    </button>
    {#if propertyId === null}
      <button type="button" class="flex min-h-7 items-center gap-1.5 rounded-md px-2 text-left text-muted-foreground hover:bg-accent hover:text-foreground"
        disabled={pending || !target || filterCount >= NOTES_DATABASE_QUERY_MAX_FILTERS || groupCount >= NOTES_DATABASE_QUERY_MAX_FILTER_GROUPS || depth >= NOTES_DATABASE_QUERY_MAX_FILTER_DEPTH}
        onclick={() => add(path, true)}><Plus class="size-3.5 shrink-0" aria-hidden="true" />{t("notes.databaseTableAddFilterGroup")}</button>
    {/if}
  </div>
{/snippet}

<div class="grid gap-2 font-normal">
  {#if filters.length > 1 && propertyId === null}<p class="text-muted-foreground">{t("notes.databaseTableFilterMatchAll")}</p>{/if}
  {@render renderNodes(filters, [], 0)}
  {@render addButtons([], 0)}
</div>
