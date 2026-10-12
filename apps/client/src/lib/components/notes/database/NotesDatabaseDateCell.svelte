<script lang="ts">
  import Bell from "@lucide/svelte/icons/bell";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import type { NotesDatabaseDateValue } from "$lib/notes/database/date";
  import { notesDatabaseDateDisplay } from "$lib/notes/database/property-display";
  import { notesDatabaseTableDateValue, type NotesDatabaseTableColumn } from "$lib/notes/database/table";
  import { notesDateMentionValueWithDates } from "$lib/notes/rich-text/date-mention-edit";
  import type { NotesPage } from "$lib/notes/types";
  import { cn } from "$lib/utils";
  import NotesDateEditorPanel from "./NotesDateEditorPanel.svelte";
  import NotesDateReminderOptions from "./NotesDateReminderOptions.svelte";

  /**
   * Edits a date property with the calendar, time, and range controls Projects uses for task dates.
   *
   * Every pick saves at once. A range picks its start and then its end, and a failed save restores the saved value.
   */
  let { row, column, rowIndex, columnIndex, mutating, onSave, onNavigate, onEditingChange = () => {}, variant = "cell" }: {
    row: NotesPage;
    column: NotesDatabaseTableColumn;
    rowIndex: number;
    columnIndex: number;
    mutating: boolean;
    onSave: (value: NotesDatabaseDateValue | null) => Promise<boolean>;
    onNavigate: (event: KeyboardEvent, rowIndex: number, columnIndex: number) => void;
    onEditingChange?: (editing: boolean) => void;
    /** Cells fill a table column; page values align their text with the page and point like links. */
    variant?: "cell" | "page";
  } = $props();

  const localization = getLocalization();
  const { t } = localization;
  const pickerId = $props.id();
  const saved = $derived(notesDatabaseTableDateValue(row, column));
  let trigger: HTMLButtonElement | undefined = $state();
  let open = $state(false);
  let draft = $state.raw<NotesDatabaseDateValue | null>(null);
  let error = $state<string | null>(null);
  let pendingSaves = $state(0);
  let latestSave = 0;
  let saveChain: Promise<void> = Promise.resolve();

  /** The value on screen: the draft while the editor is open or saving, otherwise the saved value. */
  const shown = $derived(open || pendingSaves > 0 ? draft : saved);
  const text = $derived(notesDatabaseDateDisplay(shown, localization, column.displayFormat));

  function show(): void {
    if (open || mutating) return;
    if (pendingSaves === 0) draft = saved;
    error = null;
    open = true;
    onEditingChange(true);
  }

  /** Close the editor, returning focus to the value when the close came from the keyboard. */
  function close(refocus: boolean): void {
    if (!open) return;
    open = false;
    onEditingChange(false);
    if (refocus) trigger?.focus({ preventScroll: true });
  }

  /**
   * Show a new value and save it after any earlier save of this cell, so saves land in the order they were picked.
   * Only the newest save reports a failure and restores the saved value.
   */
  function commit(next: NotesDatabaseDateValue | null): void {
    draft = next;
    error = null;
    const sequence = ++latestSave;
    pendingSaves += 1;
    saveChain = saveChain.then(async () => {
      let succeeded = false;
      try {
        succeeded = await onSave(next);
      } catch (caught: unknown) {
        console.warn("Save database date failed", caught);
      }
      if (!succeeded && sequence === latestSave) {
        error = t("notes.databaseDateSaveFailed");
        draft = saved;
      }
    }).finally(() => { pendingSaves -= 1; });
  }

  /** Save dates picked in the editor, keeping the reminder the value already has. */
  function commitDates(next: NotesDatabaseDateValue | null): void {
    commit(next === null || shown === null ? next : notesDateMentionValueWithDates(shown, next));
  }

  /** The text of one boundary, formatted like the value itself. */
  function boundaryText(boundary: string): string {
    return notesDatabaseDateDisplay({ start: boundary, end: null, time_zone: shown?.time_zone ?? null }, localization, column.displayFormat);
  }
</script>

<button bind:this={trigger} data-table-cell="true" data-collection-cell-primary data-row-index={rowIndex} data-column-index={columnIndex}
  type="button" class={cn("flex w-full min-w-0 items-center rounded-sm text-left outline-none disabled:cursor-not-allowed",
    variant === "page" ? "min-h-8 cursor-pointer px-1.5" : "h-8 px-1", text ? "text-foreground" : "text-muted-foreground")}
  disabled={mutating && !open} aria-label={column.name} aria-haspopup="dialog" aria-expanded={open}
  aria-controls={open ? pickerId : undefined} title={text}
  onclick={() => { if (open) close(false); else show(); }}
  onkeydown={(event) => { if (event.key !== "Enter" && event.key !== " ") onNavigate(event, rowIndex, columnIndex); }}>
  <span class="min-w-0 truncate">{text || t("notes.databaseTableEmptyCell")}</span>
  {#if shown?.ganbaru_reminder?.enabled}
    <Bell size={12} strokeWidth={1.75} class="ml-1 shrink-0 text-muted-foreground" aria-label={t("notes.dateMentionRemind")} />
  {/if}
</button>

{#if open}
  <NotesDateEditorPanel anchor={trigger} id={pickerId} label={column.name} value={shown} formatBoundary={boundaryText}
    {error} onCommit={commitDates} onClose={close}>
    {#snippet extras()}
      {#if shown}
        <NotesDateReminderOptions value={shown} onChange={commit} />
      {/if}
    {/snippet}
  </NotesDateEditorPanel>
{/if}
