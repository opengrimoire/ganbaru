<script lang="ts">
  import Eraser from "@lucide/svelte/icons/eraser";
  import SquareCheck from "@lucide/svelte/icons/square-check";
  import Square from "@lucide/svelte/icons/square";
  import Check from "@lucide/svelte/icons/check";
  import CollectionMenu from "$lib/components/collections/CollectionMenu.svelte";
  import CollectionMenuItem from "$lib/components/collections/CollectionMenuItem.svelte";
  import CollectionMenuSeparator from "$lib/components/collections/CollectionMenuSeparator.svelte";
  import CollectionMenuTextField from "$lib/components/collections/CollectionMenuTextField.svelte";
  import CollectionPropertyIcon from "$lib/components/collections/CollectionPropertyIcon.svelte";
  import { listNotesDataSourceRowPages } from "$lib/api/notes";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { notesDatabaseBulkToggledItem, notesDatabaseBulkValue } from "$lib/notes/database/bulk-edit";
  import type { NotesDatabaseDateValue } from "$lib/notes/database/date";
  import { notesDatabaseDateDisplay } from "$lib/notes/database/property-display";
  import { notesDatabaseRowTitle, type NotesDatabaseTableColumn, type NotesDatabaseTableEditValue } from "$lib/notes/database/table";
  import { notesDateMentionValueWithDates } from "$lib/notes/rich-text/date-mention-edit";
  import type { NotesPage } from "$lib/notes/types";
  import NotesDatabaseOptionBadge from "./NotesDatabaseOptionBadge.svelte";
  import NotesDateEditorPanel from "./NotesDateEditorPanel.svelte";
  import NotesDateReminderOptions from "./NotesDateReminderOptions.svelte";
  import { notesPropertyKind } from "./property-kinds";

  /**
   * One property segment of the database selection toolbar. It shows the value the selected rows share, or starts
   * empty when they differ, and every choice sets that value on all of them at once.
   */
  let { column, rows, disabled, onApply }: {
    column: NotesDatabaseTableColumn;
    rows: readonly NotesPage[];
    disabled: boolean;
    /** Set the value on every selected row; resolves to whether it was saved. */
    onApply: (value: NotesDatabaseTableEditValue) => Promise<boolean>;
  } = $props();

  const localization = getLocalization();
  const { t } = localization;
  const pickerId = $props.id();
  const kind = $derived(notesPropertyKind(column.type));
  const shared = $derived(notesDatabaseBulkValue(rows, column));
  const listValue = $derived(Array.isArray(shared.value) ? shared.value : []);
  const hasValue = $derived(shared.mixed || (Array.isArray(shared.value) ? shared.value.length > 0 : shared.value !== "" && shared.value !== null && shared.value !== false));
  const textValue = $derived(typeof shared.value === "string" ? shared.value : "");
  const clearValue = $derived<NotesDatabaseTableEditValue>(column.type === "multi_select" || column.type === "relation" ? [] : column.type === "rich_text" ? "" : null);
  /** Single choices close the menu; list choices keep it open so several options can be toggled. */
  const singleChoice = $derived(column.type === "checkbox" || column.type === "select" || column.type === "status");
  const segmentClass = "h-8 rounded-none px-2.5 hover:bg-accent/60";

  let targetRows = $state<NotesPage[]>([]);
  let targetsLoading = $state(false);
  let targetsError = $state<string | null>(null);

  async function loadTargets(open: boolean): Promise<void> {
    const targetId = column.relationDataSourceId;
    if (!open || !targetId || targetsLoading) return;
    targetsLoading = true;
    targetsError = null;
    try {
      targetRows = await listNotesDataSourceRowPages(targetId);
    } catch (caught) {
      targetsError = caught instanceof Error ? caught.message : String(caught);
    } finally {
      targetsLoading = false;
    }
  }

  let dateTrigger: HTMLButtonElement | undefined = $state();
  let dateOpen = $state(false);
  let dateDraft = $state.raw<NotesDatabaseDateValue | null>(null);
  let dateSaves: Promise<unknown> = Promise.resolve();
  const sharedDate = $derived(column.type === "date" && !shared.mixed && typeof shared.value === "object" && !Array.isArray(shared.value) ? shared.value : null);

  function closeDate(refocus: boolean): void {
    dateOpen = false;
    if (refocus) dateTrigger?.focus({ preventScroll: true });
  }

  /** Save picks in order, keeping a reminder the rows already share. */
  function commitDate(next: NotesDatabaseDateValue | null): void {
    dateDraft = next === null || dateDraft === null ? next : notesDateMentionValueWithDates(dateDraft, next);
    const value = dateDraft;
    dateSaves = dateSaves.then(() => onApply(value));
  }

  function commitReminder(next: NotesDatabaseDateValue | null): void {
    dateDraft = next;
    dateSaves = dateSaves.then(() => onApply(next));
  }

  function boundaryText(boundary: string): string {
    return notesDatabaseDateDisplay({ start: boundary, end: null, time_zone: dateDraft?.time_zone ?? null }, localization, column.displayFormat);
  }
</script>

{#snippet propertyIcon()}<CollectionPropertyIcon icon={column.icon ?? null} {kind} class="size-3.5 shrink-0 text-muted-foreground" />{/snippet}

{#if column.type === "date"}
  <button bind:this={dateTrigger} type="button"
    class="collection-selection-segment inline-flex h-8 shrink-0 items-center gap-1.5 px-2.5 text-collection whitespace-nowrap text-muted-foreground transition-colors hover:bg-accent/60 hover:text-foreground focus-visible:bg-accent focus-visible:outline-none disabled:cursor-not-allowed disabled:opacity-50"
    disabled={disabled && !dateOpen} aria-haspopup="dialog" aria-expanded={dateOpen} aria-controls={dateOpen ? pickerId : undefined}
    onclick={() => {
      if (dateOpen) { closeDate(false); return; }
      dateDraft = sharedDate;
      dateOpen = true;
    }}>
    {@render propertyIcon()}
    <span>{column.name}</span>
  </button>
  {#if dateOpen}
    <NotesDateEditorPanel anchor={dateTrigger} id={pickerId} label={column.name} value={dateDraft} formatBoundary={boundaryText}
      onCommit={commitDate} onClose={closeDate}>
      {#snippet extras()}
        {#if shared.mixed}<p class="px-1 text-muted-foreground">{t("collections.selection.mixedHint")}</p>{/if}
        {#if dateDraft}<NotesDateReminderOptions value={dateDraft} onChange={commitReminder} />{/if}
      {/snippet}
    </NotesDateEditorPanel>
  {/if}
{:else}
  <CollectionMenu label={column.name} kind="property" leading={propertyIcon} triggerClass={segmentClass} {disabled}
    dismissOnAction={singleChoice} onOpenChange={column.type === "relation" ? (open) => { void loadTargets(open); } : undefined}>
    <div class="grid gap-0">
      {#if shared.mixed}
        <p class="px-2 pb-1 text-muted-foreground">{t("collections.selection.mixedHint")}</p>
      {/if}
      {#if column.type === "checkbox"}
        <CollectionMenuItem icon={SquareCheck} label={t("collections.selection.checked")} checked={!shared.mixed && shared.value === true} onclick={() => { void onApply(true); }} />
        <CollectionMenuItem icon={Square} label={t("collections.selection.unchecked")} checked={!shared.mixed && shared.value === false} onclick={() => { void onApply(false); }} />
      {:else if column.type === "select" || column.type === "status" || column.type === "multi_select"}
        {#each column.options as option (option.id)}
          {@const selected = column.type === "multi_select" ? listValue.includes(option.name) : textValue === option.name}
          <button type="button" class="menu-item justify-between" aria-pressed={selected}
            onclick={() => { void onApply(column.type === "multi_select" ? notesDatabaseBulkToggledItem(listValue, option.name) : option.name); }}>
            <NotesDatabaseOptionBadge label={option.name} color={option.color} />
            {#if selected}<Check class="size-3.5 shrink-0" aria-hidden="true" />{/if}
          </button>
        {:else}
          <p class="px-2 py-1 text-muted-foreground">{t("collections.property.noOptions")}</p>
        {/each}
      {:else if column.type === "relation"}
        {#if targetsError}
          <p class="px-2 py-1 text-destructive">{t("notes.databaseRelationLoadFailed", targetsError)}</p>
        {:else if targetsLoading && targetRows.length === 0}
          <p class="px-2 py-1 text-muted-foreground">{t("notes.databaseRelationLoading")}</p>
        {:else if !column.relationDataSourceId}
          <p class="px-2 py-1 text-muted-foreground">{t("notes.databaseRelationConfigureTarget")}</p>
        {/if}
        {#each targetRows as target (target.id)}
          <CollectionMenuItem label={notesDatabaseRowTitle(target, t("notes.untitled"))} checked={listValue.includes(target.id)}
            onclick={() => { void onApply(notesDatabaseBulkToggledItem(listValue, target.id)); }} />
        {/each}
      {:else}
        <CollectionMenuTextField label={column.name} hideLabel initialFocus value={textValue}
          placeholder={shared.mixed ? t("collections.selection.mixed") : t("notes.databaseTableEmptyCell")} onCommit={(value) => { void onApply(value); }} />
      {/if}
      {#if hasValue && column.type !== "checkbox"}
        <CollectionMenuSeparator />
        <CollectionMenuItem icon={Eraser} label={t("collections.selection.clearValue")} onclick={() => { void onApply(clearValue); }} />
      {/if}
    </div>
  </CollectionMenu>
{/if}

<style>
  @media (any-pointer: coarse) and (any-hover: none) {
    .collection-selection-segment {
      min-height: var(--panel-row-height);
      min-width: 2.75rem;
    }
  }
</style>
