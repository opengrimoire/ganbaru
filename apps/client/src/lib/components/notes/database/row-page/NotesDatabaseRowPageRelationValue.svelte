<script lang="ts">
  import Plus from "@lucide/svelte/icons/plus";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { notesDatabaseTableRelationItems, type NotesDatabaseTableColumn } from "$lib/notes/database/table";
  import type { NotesPage } from "$lib/notes/types";
  import NotesDatabaseRowPageLink from "./NotesDatabaseRowPageLink.svelte";
  import NotesDatabaseRowPicker from "./NotesDatabaseRowPicker.svelte";

  /**
   * A relation value on a row page: linked pages open on click and unlink from their remove button, and the add
   * button opens the "Link or create a page" picker over the target source. Saves reject with a message ready to show.
   */
  let { row, column, rowIndex, columnIndex, disabled, sourceName, onSave, onCreate, onNavigate, onOpenPage }: {
    row: NotesPage;
    column: NotesDatabaseTableColumn;
    rowIndex: number;
    columnIndex: number;
    disabled: boolean;
    /** Name of the target source, shown in the picker. */
    sourceName: string;
    onSave: (relationIds: string[]) => Promise<void>;
    /** Creates a page in the target source and returns its id. */
    onCreate: (title: string) => Promise<string>;
    onNavigate: (event: KeyboardEvent, rowIndex: number, columnIndex: number) => void;
    onOpenPage: (pageId: string) => void;
  } = $props();

  const { t } = getLocalization();

  let addButton: HTMLButtonElement | undefined = $state();
  let pickerOpen = $state(false);
  let error = $state<string | null>(null);

  const items = $derived(notesDatabaseTableRelationItems(row, column));
  const relationIds = $derived(items.map((item) => item.id));

  function messageOf(caught: unknown): string {
    return caught instanceof Error ? caught.message : String(caught);
  }

  async function remove(pageId: string): Promise<void> {
    error = null;
    try {
      await onSave(relationIds.filter((id) => id !== pageId));
    } catch (caught) {
      error = messageOf(caught);
    }
  }

  /** Enter and Space open the picker; other keys move between values like the rest of the row page. */
  function keydown(event: KeyboardEvent): void {
    if (event.key === "Enter" || event.key === " ") {
      event.stopPropagation();
      return;
    }
    onNavigate(event, rowIndex, columnIndex);
  }
</script>

<div class="grid min-w-0 gap-0.5">
  <div class="flex min-h-8 min-w-0 flex-wrap items-center gap-x-1">
    {#each items as item (item.id)}
      <NotesDatabaseRowPageLink title={item.title} {disabled} removeLabel={t("notes.databaseRelationRemove", item.title || t("notes.untitled"))}
        onOpen={() => onOpenPage(item.id)} onRemove={() => { void remove(item.id); }} />
    {/each}
    {#if column.relationDataSourceId}
      <button bind:this={addButton} type="button"
        class="inline-flex h-7 min-w-0 cursor-pointer items-center gap-1 rounded-sm px-1.5 text-muted-foreground outline-none hover:bg-accent/60 hover:text-foreground focus-visible:bg-accent/60 focus-visible:text-foreground disabled:pointer-events-none disabled:opacity-50"
        {disabled} aria-expanded={pickerOpen} aria-label={t("notes.databaseRelationAdd")}
        data-table-cell="true" data-row-index={rowIndex} data-column-index={columnIndex}
        onkeydown={keydown} onclick={() => { error = null; pickerOpen = !pickerOpen; }}>
        {#if items.length === 0}
          <span class="truncate">{t("notes.databaseTableEmptyCell")}</span>
        {:else}
          <Plus class="size-3.5 shrink-0" strokeWidth={1.75} aria-hidden="true" />
        {/if}
      </button>
    {:else}
      <button type="button" class="inline-flex h-7 min-w-0 items-center rounded-sm px-1.5 text-left text-panel-detail text-muted-foreground outline-none focus-visible:bg-accent/60"
        data-table-cell="true" data-row-index={rowIndex} data-column-index={columnIndex} onkeydown={(event) => onNavigate(event, rowIndex, columnIndex)}>
        <span class="min-w-0 truncate">{t("notes.databaseRelationConfigureTarget")}</span>
      </button>
    {/if}
  </div>
  {#if error}<p class="px-1.5 text-panel-detail text-destructive" role="alert">{error}</p>{/if}
</div>

{#if pickerOpen && addButton && column.relationDataSourceId}
  <NotesDatabaseRowPicker anchor={addButton} dataSourceId={column.relationDataSourceId} {sourceName} exclude={relationIds}
    onPick={(target) => onSave([...relationIds, target.id])}
    onCreate={async (title) => { const id = await onCreate(title); await onSave([...relationIds, id]); }}
    onClose={() => { pickerOpen = false; }} />
{/if}
