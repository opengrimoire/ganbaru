<script lang="ts">
  import CollectionMenu from "$lib/components/collections/CollectionMenu.svelte";
  import { formatList } from "$lib/i18n/formatters";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { notesDatabaseFilterPredicates, notesDatabaseIsFilterPredicate, type NotesDatabaseQueryProperty } from "$lib/notes/database/query-controls";
  import type { NotesDatabaseTableFilter, NotesDatabaseTableSort } from "$lib/notes/types";
  import NotesDatabaseFilterControls from "./NotesDatabaseFilterControls.svelte";
  import NotesDatabaseSortControls from "./NotesDatabaseSortControls.svelte";

  let { properties, filters, sorts, pending = false, onFiltersChange, onSortsChange }: {
    properties: readonly NotesDatabaseQueryProperty[];
    filters: readonly NotesDatabaseTableFilter[];
    sorts: readonly NotesDatabaseTableSort[];
    pending?: boolean;
    onFiltersChange: (filters: NotesDatabaseTableFilter[]) => void;
    onSortsChange: (sorts: NotesDatabaseTableSort[]) => void;
  } = $props();

  const localization = getLocalization();
  const { t } = localization;
  const sortNames = $derived(sorts.flatMap((sort) => {
    const name = properties.find((property) => property.id === sort.property_id)?.name.trim();
    return name ? [name] : [];
  }));
  const predicates = $derived(notesDatabaseFilterPredicates(filters));
  const grouped = $derived(filters.some((filter) => !notesDatabaseIsFilterPredicate(filter)));
  const filterProperties = $derived([...new Set(predicates.map((filter) => filter.property_id))]
    .flatMap((propertyId) => {
      const name = properties.find((property) => property.id === propertyId)?.name.trim();
      return name ? [{ id: propertyId, name, count: predicates.filter((filter) => filter.property_id === propertyId).length }] : [];
    }));
  const sortLabel = $derived(formatList(localization.locale, sortNames) || t("notes.databaseTableSorts"));
  const filterLabel = $derived(formatList(localization.locale, filterProperties.map((property) => property.name)) || t("notes.databaseTableFilters"));
</script>

{#if filters.length || sorts.length}
  <div class="flex min-w-0 flex-wrap items-center gap-1 py-0.5 font-normal" data-notes-database-query-bar>
    {#if sorts.length}
      <CollectionMenu label={sortLabel} ariaLabel={`${t("notes.databaseTableSorts")}: ${sortLabel}`} kind="sort" disabled={pending}
        triggerClass="h-7 rounded-full bg-primary/8 text-primary hover:bg-primary/12 font-normal">
        <NotesDatabaseSortControls {properties} {sorts} {pending} onChange={onSortsChange} />
      </CollectionMenu>
    {/if}
    {#if grouped}
      <CollectionMenu label={filterLabel} ariaLabel={`${t("notes.databaseTableFilters")}: ${filterLabel}`} kind="filter" disabled={pending}
        activeCount={predicates.length} triggerClass="h-7 rounded-full bg-primary/8 text-primary hover:bg-primary/12 font-normal">
        <NotesDatabaseFilterControls {properties} {filters} {pending} onChange={onFiltersChange} />
      </CollectionMenu>
    {:else}
    {#each filterProperties as property (property.id)}
      <CollectionMenu label={property.name} ariaLabel={`${t("collections.property.filter")}: ${property.name}`} kind="filter" disabled={pending}
        activeCount={property.count > 1 ? property.count : 0}
        triggerClass="h-7 rounded-full bg-primary/8 text-primary hover:bg-primary/12 font-normal">
        <NotesDatabaseFilterControls {properties} {filters} propertyId={property.id} {pending} onChange={onFiltersChange} />
      </CollectionMenu>
    {/each}
    {/if}
  </div>
{/if}
