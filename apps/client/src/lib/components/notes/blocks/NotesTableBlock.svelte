<script lang="ts">
  import { tick } from "svelte";
  import { notesInlineClipboardContent, setNotesClipboardData } from "$lib/notes/clipboard/export";
  import { notesClipboardPasteHtml } from "$lib/notes/clipboard/paste";
  import { notesInlineClipboardRichText } from "$lib/notes/rich-text/paste";
  import { createTextRichText, replaceRichTextRange, richTextPlainText, richTextRangeSlice } from "$lib/notes/rich-text/core";
  import Plus from "@lucide/svelte/icons/plus";
  import Trash2 from "@lucide/svelte/icons/trash-2";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import {
    notesPlainTextFromEditableRoot,
    notesTextSelectionFromEditableRoot,
    restoreNotesEditableSelection,
    type NotesTextSelection,
  } from "$lib/notes/editor/selection";
  import {
    notesTableCanAddColumn,
    notesTableCanAddRow,
    notesTableCanRemoveColumn,
    notesTableCanRemoveRow,
    notesTableCellRichText,
    notesTableCellRichTextFromPlainTextEdit,
    notesTableVisibleWidth,
    planNotesTableCellNavigation,
    type NotesTableCellCoordinate,
  } from "$lib/notes/block-types/table";
  import {
    planNotesKeyboardAction,
    type NotesKeyboardAction,
  } from "$lib/notes/blocks/keyboard";
  import { notesUndoShortcutAction } from "$lib/notes/history/undo-history";
  import type {
    NotesBlockType,
    NotesRichText,
    NotesTableBlock as NotesTableBlockType,
    NotesTableRowBlock,
  } from "$lib/notes/types";
  import NotesRichTextInline from "$lib/components/notes/rich-text/NotesRichTextInline.svelte";

  let {
    block,
    tableRows,
    previousBlockType,
    isOnlyBlock,
    focusBlockId,
    focusRequestId,
    onKeyboardAction,
    onUndo,
    onRedo,
    onTableCellRichTextChange,
    onAddTableRow,
    onRemoveTableRow,
    onAddTableColumn,
    onRemoveTableColumn,
  }: {
    block: NotesTableBlockType;
    tableRows: NotesTableRowBlock[];
    previousBlockType: NotesBlockType | null;
    isOnlyBlock: boolean;
    focusBlockId: string | null;
    focusRequestId: number;
    onKeyboardAction: (blockId: string, action: NotesKeyboardAction) => void;
    onUndo: () => Promise<void> | void;
    onRedo: () => Promise<void> | void;
    onTableCellRichTextChange: (
      rowBlockId: string,
      columnIndex: number,
      richText: readonly NotesRichText[],
    ) => Promise<void> | void;
    onAddTableRow: (tableBlockId: string, afterRowIndex: number) => Promise<void> | void;
    onRemoveTableRow: (tableBlockId: string, rowBlockId: string) => Promise<void> | void;
    onAddTableColumn: (tableBlockId: string, afterColumnIndex: number) => Promise<void> | void;
    onRemoveTableColumn: (tableBlockId: string, columnIndex: number) => Promise<void> | void;
  } = $props();

  const { t } = getLocalization();
  let emptyTableButton: HTMLButtonElement | null = $state(null);
  const cellEditors = new Map<string, HTMLDivElement>();
  const tableWidth = $derived(notesTableVisibleWidth(block, tableRows));
  const tableColumnIndexes = $derived(
    Array.from({ length: tableWidth }, (_, index) => index),
  );
  const canAddRow = $derived(notesTableCanAddRow(tableRows.length));
  const canRemoveRow = $derived(notesTableCanRemoveRow(tableRows.length));
  const canAddColumn = $derived(notesTableCanAddColumn(tableWidth));
  const canRemoveColumn = $derived(notesTableCanRemoveColumn(tableWidth));

  $effect(() => {
    const _focusRequestId = focusRequestId;
    if (focusBlockId !== block.id) return;
    void tick().then(() => {
      if (tableRows.length > 0) {
        void focusTableCell({ rowIndex: 0, columnIndex: 0 });
        return;
      }
      emptyTableButton?.focus();
    });
  });

  function cellKey(coordinate: NotesTableCellCoordinate): string {
    return `${coordinate.rowIndex}:${coordinate.columnIndex}`;
  }

  function tableCellEditor(
    node: HTMLDivElement,
    coordinate: NotesTableCellCoordinate,
  ): { update: (nextCoordinate: NotesTableCellCoordinate) => void; destroy: () => void } {
    let currentCoordinate = coordinate;
    cellEditors.set(cellKey(currentCoordinate), node);
    return {
      update(nextCoordinate) {
        cellEditors.delete(cellKey(currentCoordinate));
        currentCoordinate = nextCoordinate;
        cellEditors.set(cellKey(currentCoordinate), node);
      },
      destroy() {
        cellEditors.delete(cellKey(currentCoordinate));
      },
    };
  }

  async function focusTableCell(
    coordinate: NotesTableCellCoordinate,
    selection?: NotesTextSelection,
  ): Promise<void> {
    await tick();
    const editor = cellEditors.get(cellKey(coordinate));
    if (!editor) return;
    editor.focus();
    const textLength = notesPlainTextFromEditableRoot(editor).length;
    restoreNotesEditableSelection(
      editor,
      selection ?? { start: textLength, end: textLength },
    );
  }

  function handleEmptyTableKeydown(event: KeyboardEvent): void {
    const undoAction = notesUndoShortcutAction(event);
    if (undoAction) {
      event.preventDefault();
      void Promise.resolve(undoAction === "undo" ? onUndo() : onRedo());
      return;
    }
    const action = planNotesKeyboardAction({
      key: event.key,
      shiftKey: event.shiftKey,
      ctrlKey: event.ctrlKey,
      metaKey: event.metaKey,
      altKey: event.altKey,
      text: "",
      selectionStart: 0,
      selectionEnd: 0,
      blockType: block.type,
      previousBlockType,
      isOnlyBlock,
    });
    if (
      action.type === "none"
      || action.type === "insert_newline"
      || action.type === "open_slash_menu"
    ) {
      return;
    }
    if (action.preventDefault) event.preventDefault();
    onKeyboardAction(block.id, action);
  }

  function handleCellInput(
    event: Event,
    row: NotesTableRowBlock,
    coordinate: NotesTableCellCoordinate,
    existingRichText: readonly NotesRichText[],
  ): void {
    const target = event.currentTarget;
    if (!(target instanceof HTMLElement)) return;
    const plainText = notesPlainTextFromEditableRoot(target);
    const selection = notesTextSelectionFromEditableRoot(target);
    const nextRichText = notesTableCellRichTextFromPlainTextEdit(
      existingRichText,
      plainText,
    );
    void Promise.resolve(
      onTableCellRichTextChange(row.id, coordinate.columnIndex, nextRichText),
    ).then(() => {
      if (selection) void focusTableCell(coordinate, selection);
    });
  }

  /** Copy and cut cell selections from the model so external apps receive semantic formatting. */
  function handleCellCopy(
    event: ClipboardEvent,
    row: NotesTableRowBlock,
    coordinate: NotesTableCellCoordinate,
    richText: readonly NotesRichText[],
  ): void {
    if (!(event.currentTarget instanceof HTMLElement) || !event.clipboardData) return;
    const selection = notesTextSelectionFromEditableRoot(event.currentTarget);
    if (!selection || selection.start === selection.end) return;
    event.preventDefault();
    try {
      setNotesClipboardData(event.clipboardData, notesInlineClipboardContent(
        richTextRangeSlice(richText, selection.start, selection.end),
      ));
      if (event.type === "cut") {
        void Promise.resolve(onTableCellRichTextChange(row.id, coordinate.columnIndex,
          replaceRichTextRange(richText, selection.start, selection.end, [])))
          .then(() => focusTableCell(coordinate, { start: selection.start, end: selection.start }))
          .catch((error: unknown) => console.warn("Notes table clipboard cut failed", error));
      }
    } catch (error) {
      console.warn("Notes table clipboard copy failed", error);
    }
  }

  function handleCellPaste(
    event: ClipboardEvent,
    row: NotesTableRowBlock,
    coordinate: NotesTableCellCoordinate,
    existingRichText: readonly NotesRichText[],
  ): void {
    const target = event.currentTarget;
    if (!(target instanceof HTMLElement)) return;
    const pastedText = event.clipboardData?.getData("text/plain") ?? "";
    const html = notesClipboardPasteHtml(pastedText, event.clipboardData?.getData("text/html") ?? "");
    const inserted = notesInlineClipboardRichText(html) ?? (pastedText ? [createTextRichText(pastedText)] : []);
    if (!inserted.length) return;
    event.preventDefault();
    const currentText = notesPlainTextFromEditableRoot(target);
    const selection = notesTextSelectionFromEditableRoot(target)
      ?? { start: currentText.length, end: currentText.length };
    const offset = selection.start + richTextPlainText(inserted).length;
    const nextSelection = { start: offset, end: offset };
    const nextRichText = replaceRichTextRange(existingRichText, selection.start, selection.end, inserted);
    void Promise.resolve(
      onTableCellRichTextChange(row.id, coordinate.columnIndex, nextRichText),
    ).then(() => {
      void focusTableCell(coordinate, nextSelection);
    });
  }

  function handleCellKeydown(
    event: KeyboardEvent,
    coordinate: NotesTableCellCoordinate,
  ): void {
    const undoAction = notesUndoShortcutAction(event);
    if (undoAction) {
      event.preventDefault();
      void Promise.resolve(undoAction === "undo" ? onUndo() : onRedo());
      return;
    }
    const target = event.currentTarget;
    if (!(target instanceof HTMLElement)) return;
    const textLength = notesPlainTextFromEditableRoot(target).length;
    const selection = notesTextSelectionFromEditableRoot(target)
      ?? { start: textLength, end: textLength };
    const plan = planNotesTableCellNavigation({
      key: event.key,
      code: event.code,
      shiftKey: event.shiftKey,
      ctrlKey: event.ctrlKey,
      metaKey: event.metaKey,
      altKey: event.altKey,
      rowIndex: coordinate.rowIndex,
      columnIndex: coordinate.columnIndex,
      rowCount: tableRows.length,
      columnCount: tableWidth,
      selectionStart: selection.start,
      selectionEnd: selection.end,
      textLength,
    });
    if (plan.type !== "focus_cell") return;
    if (plan.preventDefault) event.preventDefault();
    void focusTableCell({ rowIndex: plan.rowIndex, columnIndex: plan.columnIndex });
  }

  async function addRowAfter(rowIndex: number): Promise<void> {
    if (!canAddRow) return;
    await Promise.resolve(onAddTableRow(block.id, rowIndex));
    const nextRowIndex = Math.min(rowIndex + 1, tableRows.length);
    await focusTableCell({ rowIndex: nextRowIndex, columnIndex: 0 });
  }

  async function removeRow(row: NotesTableRowBlock, rowIndex: number): Promise<void> {
    if (!canRemoveRow) return;
    await Promise.resolve(onRemoveTableRow(block.id, row.id));
    const nextRowIndex = Math.max(0, Math.min(rowIndex, tableRows.length - 2));
    await focusTableCell({ rowIndex: nextRowIndex, columnIndex: 0 });
  }

  async function addColumnAfter(columnIndex: number): Promise<void> {
    if (!canAddColumn) return;
    await Promise.resolve(onAddTableColumn(block.id, columnIndex));
    await focusTableCell({
      rowIndex: 0,
      columnIndex: Math.min(columnIndex + 1, tableWidth),
    });
  }

  async function removeColumn(columnIndex: number): Promise<void> {
    if (!canRemoveColumn) return;
    await Promise.resolve(onRemoveTableColumn(block.id, columnIndex));
    await focusTableCell({
      rowIndex: 0,
      columnIndex: Math.max(0, Math.min(columnIndex, tableWidth - 2)),
    });
  }
</script>

<section class="notes-table-block my-1 min-w-0" aria-label={t("notes.blockType.table")}>
  <div class="notes-table-toolbar" aria-label={t("notes.tableToolbar")}>
    <button
      type="button"
      class="notes-table-tool-button"
      aria-label={t("notes.addTableRow")}
      title={t("notes.addTableRow")}
      disabled={!canAddRow}
      onclick={() => {
        void addRowAfter(Math.max(0, tableRows.length - 1));
      }}
    >
      <Plus class="size-3.5" aria-hidden="true" />
      <span class="sr-only">{t("notes.addTableRow")}</span>
    </button>
    <button
      type="button"
      class="notes-table-tool-button"
      aria-label={t("notes.addTableColumn")}
      title={t("notes.addTableColumn")}
      disabled={!canAddColumn}
      onclick={() => {
        void addColumnAfter(tableWidth - 1);
      }}
    >
      <Plus class="size-3.5" aria-hidden="true" />
      <span class="sr-only">{t("notes.addTableColumn")}</span>
    </button>
  </div>

  {#if tableRows.length === 0}
    <button
      bind:this={emptyTableButton}
      type="button"
      class="min-h-8 w-full rounded px-1 text-left text-[0.8rem] text-muted-foreground outline-none focus-visible:ring-2 focus-visible:ring-ring"
      onkeydown={handleEmptyTableKeydown}
    >
      {t("notes.emptyTable")}
    </button>
  {:else}
    <div class="notes-table-scroll overflow-x-auto rounded-md border border-border bg-background/70">
      <div
        class="notes-table-column-tools"
        style={`--notes-table-width: ${tableWidth}`}
        aria-hidden="false"
      >
        <div></div>
        {#each tableColumnIndexes as columnIndex}
          <div
            class="notes-table-column-tool-cell"
            role="toolbar"
            aria-label={t("notes.tableColumnActions", columnIndex + 1)}
          >
            <button
              type="button"
              class="notes-table-mini-button"
              aria-label={t("notes.addTableColumnAfter", columnIndex + 1)}
              title={t("notes.addTableColumnAfter", columnIndex + 1)}
              disabled={!canAddColumn}
              onclick={() => {
                void addColumnAfter(columnIndex);
              }}
            >
              <Plus class="size-3" aria-hidden="true" />
            </button>
            <button
              type="button"
              class="notes-table-mini-button notes-table-danger-button"
              aria-label={t("notes.removeTableColumn", columnIndex + 1)}
              title={t("notes.removeTableColumn", columnIndex + 1)}
              disabled={!canRemoveColumn}
              onclick={() => {
                void removeColumn(columnIndex);
              }}
            >
              <Trash2 class="size-3" aria-hidden="true" />
            </button>
          </div>
        {/each}
      </div>
      <table
        class="notes-table"
        style={`--notes-table-width: ${tableWidth}`}
      >
        <tbody>
          {#each tableRows as row, rowIndex (row.id)}
            <tr>
              <th
                class="notes-table-row-tools"
                scope="row"
                aria-label={t("notes.tableRowActions", rowIndex + 1)}
              >
                <button
                  type="button"
                  class="notes-table-mini-button"
                  aria-label={t("notes.addTableRowAfter", rowIndex + 1)}
                  title={t("notes.addTableRowAfter", rowIndex + 1)}
                  disabled={!canAddRow}
                  onclick={() => {
                    void addRowAfter(rowIndex);
                  }}
                >
                  <Plus class="size-3" aria-hidden="true" />
                </button>
                <button
                  type="button"
                  class="notes-table-mini-button notes-table-danger-button"
                  aria-label={t("notes.removeTableRow", rowIndex + 1)}
                  title={t("notes.removeTableRow", rowIndex + 1)}
                  disabled={!canRemoveRow}
                  onclick={() => {
                    void removeRow(row, rowIndex);
                  }}
                >
                  <Trash2 class="size-3" aria-hidden="true" />
                </button>
              </th>
              {#each tableColumnIndexes as columnIndex}
                {@const coordinate = { rowIndex, columnIndex }}
                {@const cellRichText = notesTableCellRichText(row, columnIndex)}
                <td
                  class:notes-table-column-header={block.table.has_column_header && rowIndex === 0}
                  class:notes-table-row-header={block.table.has_row_header && columnIndex === 0}
                >
                  <div
                    use:tableCellEditor={coordinate}
                    class="notes-table-cell-editor"
                    role="textbox"
                    aria-multiline="false"
                    aria-label={t("notes.tableCell", rowIndex + 1, columnIndex + 1)}
                    aria-placeholder={t("notes.tableCellPlaceholder")}
                    contenteditable="true"
                    spellcheck="true"
                    tabindex="0"
                    data-empty={cellRichText.length === 0 ? "true" : undefined}
                    oninput={(event) => {
                      handleCellInput(event, row, coordinate, cellRichText);
                    }}
                    oncopy={(event) => handleCellCopy(event, row, coordinate, cellRichText)}
                    oncut={(event) => handleCellCopy(event, row, coordinate, cellRichText)}
                    onpaste={(event) => {
                      handleCellPaste(event, row, coordinate, cellRichText);
                    }}
                    onkeydown={(event) => {
                      handleCellKeydown(event, coordinate);
                    }}
                  >
                    <NotesRichTextInline richText={cellRichText} />
                  </div>
                </td>
              {/each}
            </tr>
          {/each}
        </tbody>
      </table>
    </div>
  {/if}
</section>

<style>
  .notes-table-toolbar {
    display: flex;
    justify-content: flex-end;
    gap: 0.25rem;
    padding-bottom: 0.25rem;
  }

  .notes-table-tool-button,
  .notes-table-mini-button {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    border-radius: 0.25rem;
    color: hsl(var(--muted-foreground));
    outline: none;
  }

  .notes-table-tool-button {
    min-height: 1.75rem;
    min-width: 1.75rem;
  }

  .notes-table-mini-button {
    min-height: 1.35rem;
    min-width: 1.35rem;
  }

  .notes-table-tool-button:hover,
  .notes-table-mini-button:hover,
  .notes-table-tool-button:focus-visible,
  .notes-table-mini-button:focus-visible {
    background: hsl(var(--accent));
    color: hsl(var(--accent-foreground));
  }

  .notes-table-tool-button:disabled,
  .notes-table-mini-button:disabled {
    cursor: not-allowed;
    opacity: 0.38;
  }

  .notes-table-danger-button {
    color: hsl(var(--destructive));
  }

  .notes-table-column-tools,
  .notes-table {
    min-width: calc(2.25rem + var(--notes-table-width) * 8.5rem);
  }

  .notes-table-column-tools {
    display: grid;
    grid-template-columns: 2.25rem repeat(var(--notes-table-width), minmax(8.5rem, 1fr));
    border-bottom: 1px solid hsl(var(--border));
    background: hsl(var(--muted) / 0.2);
  }

  .notes-table-column-tool-cell {
    display: flex;
    min-width: 0;
    align-items: center;
    justify-content: flex-end;
    gap: 0.1rem;
    border-left: 1px solid hsl(var(--border));
    padding: 0.2rem;
  }

  .notes-table {
    width: 100%;
    table-layout: fixed;
    border-collapse: collapse;
    font-size: calc(0.866667rem * var(--type-scale));
  }

  .notes-table-row-tools {
    width: 2.25rem;
    min-width: 2.25rem;
    border-right: 1px solid hsl(var(--border));
    border-bottom: 1px solid hsl(var(--border));
    background: hsl(var(--muted) / 0.2);
    padding: 0.2rem;
    vertical-align: top;
  }

  .notes-table-row-tools > :global(button + button) {
    margin-top: 0.1rem;
  }

  .notes-table td {
    width: 8.5rem;
    min-width: 8.5rem;
    border-right: 1px solid hsl(var(--border));
    border-bottom: 1px solid hsl(var(--border));
    vertical-align: top;
  }

  .notes-table tr:last-child td,
  .notes-table tr:last-child .notes-table-row-tools {
    border-bottom: 0;
  }

  .notes-table td:last-child {
    border-right: 0;
  }

  .notes-table-cell-editor {
    min-height: 2rem;
    width: 100%;
    min-width: 0;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
    background: transparent;
    padding: 0.35rem 0.5rem;
    outline: none;
  }

  .notes-table-cell-editor[data-empty="true"]::before {
    color: hsl(var(--muted-foreground));
    content: attr(aria-placeholder);
  }

  .notes-table-cell-editor:focus {
    box-shadow: inset 0 0 0 2px hsl(var(--ring));
  }

  .notes-table-column-header,
  .notes-table-row-header {
    background: hsl(var(--muted) / 0.45);
    font-weight: 600;
  }
</style>
