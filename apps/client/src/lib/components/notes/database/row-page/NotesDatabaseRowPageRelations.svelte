<script lang="ts">
  import CornerLeftUp from "@lucide/svelte/icons/corner-left-up";
  import ListTree from "@lucide/svelte/icons/list-tree";
  import Plus from "@lucide/svelte/icons/plus";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import type { NotesDatabaseRowHierarchyMetadata } from "$lib/notes/database/row-hierarchy";
  import { notesDatabaseParentExclusions, notesDatabaseRowCanAddSubitems, notesDatabaseSubitemExclusions } from "$lib/notes/database/row-page-properties";
  import { notesDatabaseRowTitle } from "$lib/notes/database/table";
  import type { NotesPage } from "$lib/notes/types";
  import NotesDatabaseRowPageLink from "./NotesDatabaseRowPageLink.svelte";
  import NotesDatabaseRowPicker from "./NotesDatabaseRowPicker.svelte";

  /**
   * The Relations section of a row page: its parent item and its sub-items, each opening on click. Sub-items are
   * linked or created through the "Link or create a page" picker, and the parent item is chosen the same way.
   * Every save rejects with a message ready to show.
   */
  let {
    dataSourceId,
    sourceName,
    row,
    hierarchy,
    parent,
    subitems,
    onOpenPage,
    onSetParent,
    onCreateParent,
    onLinkSubitem,
    onUnlinkSubitem,
    onCreateSubitem,
  }: {
    dataSourceId: string;
    sourceName: string;
    row: NotesPage;
    hierarchy: NotesDatabaseRowHierarchyMetadata;
    parent: NotesPage | null;
    subitems: readonly NotesPage[];
    onOpenPage: (pageId: string) => void;
    onSetParent: (parent: NotesPage | null) => Promise<void>;
    onCreateParent: (title: string) => Promise<void>;
    onLinkSubitem: (subitem: NotesPage) => Promise<void>;
    onUnlinkSubitem: (subitem: NotesPage) => Promise<void>;
    onCreateSubitem: (title: string) => Promise<void>;
  } = $props();

  type PickerMode = "subitems" | "parent";

  const { t } = getLocalization();

  let picker = $state<{ mode: PickerMode; anchor: HTMLElement } | null>(null);
  let pending = $state(false);
  let error = $state<string | null>(null);

  const canAddSubitems = $derived(notesDatabaseRowCanAddSubitems(hierarchy));
  const relatives = $derived({ rowId: row.id, hierarchy, childIds: subitems.map((subitem) => subitem.id) });

  function titleOf(page: NotesPage): string {
    return notesDatabaseRowTitle(page, "");
  }

  function messageOf(caught: unknown): string {
    return caught instanceof Error ? caught.message : String(caught);
  }

  /** Run a save from outside the picker, showing a failure below the section. */
  async function run(task: () => Promise<void>): Promise<void> {
    if (pending) return;
    pending = true;
    error = null;
    try {
      await task();
    } catch (caught) {
      error = messageOf(caught);
    } finally {
      pending = false;
    }
  }

  function togglePicker(mode: PickerMode, anchor: HTMLElement): void {
    error = null;
    picker = picker?.mode === mode && picker.anchor === anchor ? null : { mode, anchor };
  }
</script>

<div class="mt-1 grid min-w-0 gap-0.5 border-t border-border pt-2" role="group" aria-label={t("notes.databaseRowPageRelations")}>
  <div class="flex min-w-0 items-center justify-between gap-2">
    <h3 class="truncate px-1.5 text-panel-detail font-medium text-muted-foreground">{t("notes.databaseRowPageRelations")}</h3>
    <!-- The wrapper carries the limit tooltip, since a disabled button receives no pointer events. -->
    <span class="inline-flex min-w-0" data-app-tooltip={canAddSubitems ? undefined : t("notes.databaseRowPageSubitemLimit")}>
      <button type="button" class="row-page-relations-button"
        disabled={!canAddSubitems} aria-expanded={picker?.mode === "subitems"}
        onclick={(event) => togglePicker("subitems", event.currentTarget)}>
        <Plus class="size-3.5 shrink-0" strokeWidth={1.75} aria-hidden="true" />
        <span class="truncate">{t("notes.databaseRowPageAddSubitems")}</span>
      </button>
    </span>
  </div>

  <div class="grid min-w-0 grid-cols-1 items-start @sm:grid-cols-[minmax(6rem,10rem)_minmax(0,1fr)] @sm:gap-2">
    <div class="flex h-8 min-w-0 items-center gap-1.5 px-1.5 text-muted-foreground">
      <CornerLeftUp class="size-3.5 shrink-0" strokeWidth={1.75} aria-hidden="true" />
      <span class="truncate">{t("notes.databaseRowPageParentItem")}</span>
    </div>
    <div class="flex min-h-8 min-w-0 flex-wrap items-center gap-1">
      {#if parent}
        {@const parentPage = parent}
        <NotesDatabaseRowPageLink title={titleOf(parentPage)} icon={parentPage.icon} disabled={pending}
          removeLabel={t("notes.databaseRowPageRemoveParent", titleOf(parentPage) || t("notes.untitled"))}
          onOpen={() => onOpenPage(parentPage.id)} onRemove={() => { void run(() => onSetParent(null)); }} />
      {/if}
      <button type="button" class="row-page-relations-button" aria-expanded={picker?.mode === "parent"}
        aria-label={t("notes.databaseRowPageSetParent")} onclick={(event) => togglePicker("parent", event.currentTarget)}>
        {#if parent}
          <Plus class="size-3.5 shrink-0" strokeWidth={1.75} aria-hidden="true" />
        {:else}
          <span class="truncate">{t("notes.databaseTableEmptyCell")}</span>
        {/if}
      </button>
    </div>
  </div>

  <div class="grid min-w-0 grid-cols-1 items-start @sm:grid-cols-[minmax(6rem,10rem)_minmax(0,1fr)] @sm:gap-2">
    <div class="flex h-8 min-w-0 items-center gap-1.5 px-1.5 text-muted-foreground">
      <ListTree class="size-3.5 shrink-0" strokeWidth={1.75} aria-hidden="true" />
      <span class="truncate">{t("notes.databaseRowPageSubitems")}</span>
    </div>
    <div class="flex min-h-8 min-w-0 flex-col items-start">
      {#each subitems as subitem (subitem.id)}
        <NotesDatabaseRowPageLink title={titleOf(subitem)} icon={subitem.icon} disabled={pending}
          removeLabel={t("notes.databaseRowPageUnlinkSubitem", titleOf(subitem) || t("notes.untitled"))}
          onOpen={() => onOpenPage(subitem.id)} onRemove={() => { void run(() => onUnlinkSubitem(subitem)); }} />
      {:else}
        <button type="button" class="row-page-relations-button" disabled={!canAddSubitems}
          aria-label={t("notes.databaseRowPageAddSubitems")} onclick={(event) => togglePicker("subitems", event.currentTarget)}>
          <span class="truncate">{t("notes.databaseTableEmptyCell")}</span>
        </button>
      {/each}
    </div>
  </div>

  {#if error}<p class="px-1.5 py-1 text-panel-detail text-destructive" role="alert">{error}</p>{/if}
</div>

{#if picker?.mode === "subitems"}
  <NotesDatabaseRowPicker anchor={picker.anchor} {dataSourceId} {sourceName} exclude={notesDatabaseSubitemExclusions(relatives)}
    onPick={onLinkSubitem} onCreate={onCreateSubitem} onClose={() => { picker = null; }} />
{:else if picker?.mode === "parent"}
  <NotesDatabaseRowPicker anchor={picker.anchor} {dataSourceId} {sourceName} exclude={notesDatabaseParentExclusions(relatives)} closeOnPick
    onPick={onSetParent} onCreate={onCreateParent} onClose={() => { picker = null; }} />
{/if}

<style>
  .row-page-relations-button {
    display: inline-flex;
    min-width: 0;
    height: 1.75rem;
    align-items: center;
    gap: 0.375rem;
    border-radius: 0.25rem;
    padding-inline: 0.375rem;
    color: var(--muted-foreground);
  }

  .row-page-relations-button:hover:not(:disabled),
  .row-page-relations-button:focus-visible {
    background-color: color-mix(in oklab, var(--accent) 60%, transparent);
    color: var(--foreground);
    outline: none;
  }

  .row-page-relations-button:disabled {
    cursor: not-allowed;
    opacity: 0.5;
  }

  @media (any-pointer: coarse) and (any-hover: none) {
    .row-page-relations-button {
      min-height: var(--panel-row-height);
    }
  }
</style>
