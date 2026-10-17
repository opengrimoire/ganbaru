<script lang="ts">
  import Plus from "@lucide/svelte/icons/plus";
  import X from "@lucide/svelte/icons/x";
  import Select from "$lib/components/ui/Select.svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { NOTES_DATABASE_QUERY_MAX_SORTS, type NotesDatabaseQueryProperty } from "$lib/notes/database/query-controls";
  import type { NotesDatabaseTableSort } from "$lib/notes/types";

  let { properties, sorts, pending = false, onChange }: {
    properties: readonly NotesDatabaseQueryProperty[];
    sorts: readonly NotesDatabaseTableSort[];
    pending?: boolean;
    onChange: (sorts: NotesDatabaseTableSort[]) => void;
  } = $props();

  const { t } = getLocalization();
  const unusedProperty = $derived(properties.find((property) => !sorts.some((sort) => sort.property_id === property.id)));

  /** Retain existing sort priority and prevent repeated property identities. */
  function update(index: number, patch: Partial<NotesDatabaseTableSort>): void {
    if (pending || (patch.property_id && sorts.some((sort, sortIndex) => sortIndex !== index && sort.property_id === patch.property_id))) return;
    onChange(sorts.map((sort, sortIndex) => sortIndex === index ? { ...sort, ...patch } : { ...sort }));
  }

  /** Add the first remaining property within the native sort bound. */
  function add(): void {
    if (pending || !unusedProperty || sorts.length >= NOTES_DATABASE_QUERY_MAX_SORTS) return;
    onChange([...sorts, { property_id: unusedProperty.id, direction: "ascending" }]);
  }
</script>

<div class="grid gap-2 font-normal">
  {#each sorts as sort, index}
    <div class="grid min-w-0 grid-cols-[minmax(0,1fr)_minmax(0,1fr)_auto] gap-1">
      <Select textSize="collection" inline appearance="quiet" contentAlign="start" class="w-full min-w-0"
        ariaLabel={t("notes.databaseTableSortProperty")} value={sort.property_id} disabled={pending}
        options={properties.filter((property) => !sorts.some((candidate, sortIndex) => sortIndex !== index && candidate.property_id === property.id))
          .map((property) => ({ value: property.id, label: property.name }))}
        onChange={(property_id) => update(index, { property_id })} triggerProps={{ "onkeydown": (event) => event.stopPropagation() }} />
      <Select textSize="collection" inline appearance="quiet" contentAlign="start" class="w-full min-w-0"
        ariaLabel={t("notes.databaseTableSortDirection")} value={sort.direction} disabled={pending}
        options={[{ value: "ascending", label: t("notes.databaseTableSortAscending") }, { value: "descending", label: t("notes.databaseTableSortDescending") }]}
        onChange={(direction) => update(index, { direction: direction === "descending" ? "descending" : "ascending" })}
        triggerProps={{ "onkeydown": (event) => event.stopPropagation() }} />
      <button type="button" class="inline-flex size-7 items-center justify-center rounded-md text-muted-foreground hover:bg-accent hover:text-foreground"
        disabled={pending} aria-label={t("notes.databaseTableRemoveSort")} onclick={() => onChange(sorts.filter((_, sortIndex) => sortIndex !== index))}>
        <X class="size-3.5" aria-hidden="true" />
      </button>
    </div>
  {/each}
  <button type="button" class="flex min-h-7 items-center gap-1.5 rounded-md px-2 text-left text-muted-foreground hover:bg-accent hover:text-foreground"
    disabled={pending || !unusedProperty || sorts.length >= NOTES_DATABASE_QUERY_MAX_SORTS} onclick={add}>
    <Plus class="size-3.5 shrink-0" aria-hidden="true" />{t("notes.databaseTableAddSort")}
  </button>
</div>
