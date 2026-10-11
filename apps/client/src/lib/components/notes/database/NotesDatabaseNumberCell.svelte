<script lang="ts">
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { notesDatabasePropertyDisplayText } from "$lib/notes/database/property-display";
  import { notesDatabaseTableCellText, type NotesDatabaseTableColumn } from "$lib/notes/database/table";
  import type { NotesPage } from "$lib/notes/types";
  import { cn } from "$lib/utils";

  let { row, column, rowIndex, columnIndex, mutating, onSave, onNavigate, onEditingChange = () => {}, variant = "cell" }: {
    row: NotesPage;
    column: NotesDatabaseTableColumn;
    rowIndex: number;
    columnIndex: number;
    mutating: boolean;
    onSave: (value: string) => Promise<boolean>;
    onNavigate: (event: KeyboardEvent, rowIndex: number, columnIndex: number) => void;
    onEditingChange?: (editing: boolean) => void;
    /** Cells fill a table column; page values align their text with the page and show a placeholder when empty. */
    variant?: "cell" | "page";
  } = $props();
  const localization = getLocalization();
  const { t } = localization;
  let focused = $state(false);
  let draft = $state("");
  let failed = $state(false);
  const raw = $derived(notesDatabaseTableCellText(row, column));
  const display = $derived(notesDatabasePropertyDisplayText(row, column, localization));

  /** Submit raw scalar text while retaining rejected edits for correction. */
  async function commit(): Promise<void> {
    onEditingChange(false);
    focused = false;
    failed = !(await onSave(draft));
  }
</script>

<input data-table-cell="true" data-collection-cell-primary data-row-index={rowIndex} data-column-index={columnIndex}
  class={cn("h-8 w-full min-w-0 rounded-sm border border-transparent bg-transparent px-1 text-foreground outline-none",
    variant === "page" && "cursor-pointer px-1.5 placeholder:text-muted-foreground focus:cursor-text")}
  value={focused || failed ? draft : display} inputmode="decimal" aria-label={column.name} disabled={mutating}
  placeholder={variant === "page" ? t("notes.databaseTableEmptyCell") : undefined}
  onfocus={() => { if (!failed) draft = raw; focused = true; onEditingChange(true); }}
  oninput={(event) => { draft = event.currentTarget.value; }} onblur={() => { void commit(); }}
  onkeydown={(event) => onNavigate(event, rowIndex, columnIndex)} />
