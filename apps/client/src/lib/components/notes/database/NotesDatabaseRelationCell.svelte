<script lang="ts">
  import Select from "$lib/components/ui/Select.svelte";
  import Notice from "$lib/components/ui/Notice.svelte";
  import { listNotesDataSourceRowPages } from "$lib/api/notes";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import {
    notesDatabaseRowTitle,
    notesDatabaseTableRelationItems,
    type NotesDatabaseTableColumn,
    type NotesDatabaseTableEditValue,
  } from "$lib/notes/database/table";
  import type { NotesPage } from "$lib/notes/types";
  import Plus from "@lucide/svelte/icons/plus";
  import X from "@lucide/svelte/icons/x";

  let {
    row,
    column,
    rowIndex,
    columnIndex,
    mutating,
    onSave,
    onNavigate,
  }: {
    row: NotesPage;
    column: NotesDatabaseTableColumn;
    rowIndex: number;
    columnIndex: number;
    mutating: boolean;
    onSave: (value: NotesDatabaseTableEditValue) => Promise<void> | void;
    onNavigate: (event: KeyboardEvent, rowIndex: number, columnIndex: number) => void;
  } = $props();

  const { t } = getLocalization();

  let targetRows = $state<NotesPage[]>([]);
  let loadedDataSourceId = $state<string | null>(null);
  let loading = $state(false);
  let error = $state<string | null>(null);
  let selectedRowId = $state("");

  const targetDataSourceId = $derived(column.relationDataSourceId);
  const relationItems = $derived(notesDatabaseTableRelationItems(row, column));
  const relationIds = $derived(relationItems.map((item) => item.id));
  const availableRows = $derived(
    targetRows.filter((target) => !relationIds.includes(target.id)),
  );

  function rowTitle(target: NotesPage): string {
    return notesDatabaseRowTitle(target, t("notes.untitled"));
  }

  async function loadTargets(): Promise<void> {
    if (!targetDataSourceId || loadedDataSourceId === targetDataSourceId || loading) return;
    loading = true;
    error = null;
    try {
      targetRows = await listNotesDataSourceRowPages(targetDataSourceId);
      loadedDataSourceId = targetDataSourceId;
    } catch (caught) {
      error = caught instanceof Error ? caught.message : String(caught);
    } finally {
      loading = false;
    }
  }

  /** Reloads relation targets after a failure, rejecting while they are still unavailable. */
  async function retryTargets(): Promise<void> {
    await loadTargets();
    if (error) throw new Error(error);
  }

  function addRelation(targetId: string): void {
    if (!targetId || relationIds.includes(targetId)) return;
    selectedRowId = "";
    void onSave([...relationIds, targetId]);
  }

  function removeRelation(targetId: string): void {
    void onSave(relationIds.filter((id) => id !== targetId));
  }
</script>

<div class="grid min-w-0 gap-1">
  <div class="flex min-h-8 w-full min-w-0 flex-wrap items-center gap-1 rounded-sm border border-transparent bg-transparent px-1 py-0.5">
    {#if relationItems.length === 0}
      <span class="min-w-0 truncate text-muted-foreground">{t("notes.databaseTableEmptyCell")}</span>
    {:else}
      {#each relationItems as item (item.id)}
        <span
          class="inline-flex max-w-full items-center gap-1 rounded-sm bg-muted px-1.5 py-0.5 text-[0.733333rem] text-foreground"
        >
          <span class="min-w-0 truncate">{item.title || item.id}</span>
          <button
            type="button"
            class="relative z-10 inline-flex size-4 shrink-0 items-center justify-center rounded-sm hover:bg-accent disabled:pointer-events-none disabled:opacity-50"
            disabled={mutating}
            aria-label={t("notes.databaseRelationRemove", item.title || item.id)}
            title={t("notes.databaseRelationRemove", item.title || item.id)}
            onclick={(event) => {
              event.stopPropagation();
              removeRelation(item.id);
            }}
          >
            <X class="size-3" aria-hidden="true" />
          </button>
        </span>
      {/each}
    {/if}
  </div>

  {#if targetDataSourceId}
    <div class="flex min-w-0 items-center gap-1">
      <Plus class="size-3.5 shrink-0 text-muted-foreground" aria-hidden="true" />
      <Select textSize="collection"
        inline
        appearance="quiet"
        contentAlign="start"
        class="w-full min-w-0"
        ariaLabel={t("notes.databaseRelationAdd")}
        value={String(selectedRowId ?? "")}
        disabled={mutating}
        options={[{ value: "", label: String(loading ? t("notes.databaseRelationLoading") : t("notes.databaseRelationAdd")) },
          ...(availableRows).map((target) => ({ value: String(target.id), label: String(rowTitle(target)) }))]}
        onChange={(nextValue) => {
          addRelation(nextValue);
        }}
        triggerProps={{ "data-table-cell": "true", "data-collection-cell-primary": "", "data-row-index": rowIndex, "data-column-index": columnIndex, "onfocus": () => {
              void loadTargets();
          }, "onkeydown": (event) => onNavigate(event, rowIndex, columnIndex) }}
      />
    </div>
  {:else}
    <button
      data-table-cell="true"
      data-row-index={rowIndex}
      data-column-index={columnIndex}
      type="button"
      class="flex h-7 min-w-0 items-center rounded-sm px-1 text-left text-[0.733333rem] text-muted-foreground outline-none focus-visible:ring-2 focus-visible:ring-ring"
      onkeydown={(event) => onNavigate(event, rowIndex, columnIndex)}
    >
      <span class="min-w-0 truncate">{t("notes.databaseRelationConfigureTarget")}</span>
    </button>
  {/if}

  {#if error}
    <Notice class="text-[0.733333rem]" message={t("notes.databaseRelationLoadFailed", error)} copy={error} onRetry={retryTargets} />
  {/if}
</div>
