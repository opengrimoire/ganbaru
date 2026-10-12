<script lang="ts">
  import Check from "@lucide/svelte/icons/check";
  import Checkbox from "$lib/components/ui/Checkbox.svelte";
  import Select from "$lib/components/ui/Select.svelte";
  import { getLocalization } from "$lib/i18n/translator.svelte";
  import { NOTES_TEXT_COLORS, notesBlockColorStyle } from "$lib/notes/blocks/color";
  import {
    notesDatabaseTableCellText,
    notesDatabaseTableColumnCanEdit,
    type NotesDatabaseTableColumn,
    type NotesDatabaseTableEditValue,
  } from "$lib/notes/database/table";
  import type { NotesPage } from "$lib/notes/types";
  import { cn } from "$lib/utils";
  import { tick } from "svelte";
  import NotesDatabaseDateCell from "./NotesDatabaseDateCell.svelte";
  import NotesDatabaseNumberCell from "./NotesDatabaseNumberCell.svelte";
  import NotesDatabasePropertyValue from "./NotesDatabasePropertyValue.svelte";
  import NotesDatabaseRelationCell from "./NotesDatabaseRelationCell.svelte";

  /**
   * Edits one row property value with the editor its type uses in table cells.
   *
   * The table and row pages share it so both surfaces save, format, and refuse the same values. Grid position
   * attributes let the owner move focus between values with `onNavigate`.
   */
  let {
    row,
    column,
    rowIndex,
    columnIndex,
    value,
    disabled,
    onSave,
    onNavigate,
    onEditingChange = () => {},
    onDraft = () => {},
    onRunButton,
    variant = "cell",
  }: {
    row: NotesPage;
    column: NotesDatabaseTableColumn;
    rowIndex: number;
    columnIndex: number;
    /** The value to show, including an unsaved local edit. */
    value: NotesDatabaseTableEditValue;
    disabled: boolean;
    onSave: (value: NotesDatabaseTableEditValue) => Promise<boolean>;
    onNavigate: (event: KeyboardEvent, rowIndex: number, columnIndex: number) => void;
    /** Reports text and draft editors that must not be replaced by a refresh while open. */
    onEditingChange?: (editing: boolean) => void;
    /** Receives each keystroke of a title before it is saved. */
    onDraft?: (value: string) => void;
    onRunButton: () => void;
    /** Cells fill and center in a table column; page values start at the left and size buttons to their label. */
    variant?: "cell" | "page";
  } = $props();

  const { t } = getLocalization();
  const editable = $derived(notesDatabaseTableColumnCanEdit(column));
  const page = $derived(variant === "page");
  /** Page text values align with the page text, point like links, and switch to a text caret while typing. */
  const pageTextClass = "cursor-pointer px-1.5 placeholder:text-muted-foreground focus:cursor-text";
  const placeholder = $derived(page ? t("notes.databaseTableEmptyCell") : undefined);

  /** Keep wrapped text editable at its measured content height across column resizing. */
  function fitWrappedText(node: HTMLTextAreaElement, _value: string) {
    let disposed = false;
    let width = node.clientWidth;
    const resize = () => {
      if (disposed) return;
      node.style.height = "auto";
      // The border box must also hold the borders, which scrollHeight leaves out, or the text scrolls.
      node.style.height = `${node.scrollHeight + node.offsetHeight - node.clientHeight}px`;
    };
    const observer = typeof ResizeObserver === "undefined" ? null : new ResizeObserver(() => {
      if (node.clientWidth === width) return;
      width = node.clientWidth;
      resize();
    });
    observer?.observe(node);
    node.addEventListener("input", resize);
    void tick().then(resize);
    return {
      update(_nextValue: string) { void tick().then(resize); },
      destroy() { disposed = true; observer?.disconnect(); node.removeEventListener("input", resize); },
    };
  }
</script>

{#if column.type === "checkbox"}
  <label class={cn("flex h-8 items-center", page ? "cursor-pointer justify-start px-1.5" : "justify-center")} data-collection-cell-primary>
    <Checkbox
      data-table-cell="true"
      data-row-index={rowIndex}
      data-column-index={columnIndex}
      checked={value === true}
      disabled={disabled || !editable}
      label={column.name}
      onChange={(checked) => {
        void onSave(checked);
      }}
      onkeydown={(event) => onNavigate(event, rowIndex, columnIndex)}
    />
  </label>
{:else if column.type === "select" || column.type === "status"}
  <Select textSize="collection"
    inline
    appearance="quiet"
    contentAlign="start"
    class="w-full min-w-0"
    ariaLabel={column.name}
    value={String(value)}
    disabled={disabled || !editable}
    options={[{ value: "", label: t("notes.databaseTableEmptyCell") },
      ...(column.options).map((option) => ({ value: String(option.name), label: String(option.name) }))]}
    onChange={(nextValue) => {
      void onSave(nextValue || null);
    }}
    triggerProps={{ "data-table-cell": "true", "data-collection-cell-primary": "", "data-row-index": rowIndex, "data-column-index": columnIndex, "onkeydown": (event) => onNavigate(event, rowIndex, columnIndex) }}
  >
    {#snippet leading(selected)}
      {@const color = column.options.find((option) => option.name === selected)?.color}
      {#if selected}<span class="size-2 shrink-0 rounded-full" style={`${notesBlockColorStyle(NOTES_TEXT_COLORS.find((entry) => entry === color) ?? "default")}; background-color: var(--notes-block-color);`}></span>{/if}
    {/snippet}
  </Select>
{:else if column.type === "relation"}
  <NotesDatabaseRelationCell
    {row}
    {column}
    {rowIndex}
    {columnIndex}
    mutating={disabled}
    onSave={async (next) => { await onSave(next); }}
    {onNavigate}
  />
{:else if column.type === "date"}
  <NotesDatabaseDateCell {row} {column} {rowIndex} {columnIndex} {variant}
    mutating={disabled} onSave={(next) => onSave(next)} {onNavigate} {onEditingChange} />
{:else if column.type === "number"}
  <NotesDatabaseNumberCell {row} {column} {rowIndex} {columnIndex} {variant}
    mutating={disabled} onSave={(next) => onSave(next)} {onNavigate} {onEditingChange} />
{:else if column.type === "button"}
  <button
    data-table-cell="true"
    data-row-index={rowIndex}
    data-column-index={columnIndex}
    type="button"
    class={`inline-flex h-8 ${variant === "page" ? "w-auto max-w-full" : "w-full"} min-w-0 items-center justify-center gap-1 rounded-sm border border-input bg-background px-2 text-[0.8rem] text-foreground outline-none hover:bg-accent focus-visible:ring-2 focus-visible:ring-ring disabled:pointer-events-none`}
    {disabled}
    title={notesDatabaseTableCellText(row, column)}
    onclick={onRunButton}
    onkeydown={(event) => {
      if (event.key === "Enter" || event.key === " ") return;
      onNavigate(event, rowIndex, columnIndex);
    }}
  >
    <Check class="size-3.5 shrink-0" aria-hidden="true" />
    <span class="min-w-0 truncate">
      {notesDatabaseTableCellText(row, column) || column.buttonLabel || column.name}
    </span>
  </button>
{:else if editable && column.displayFormat?.wrap}
  <textarea use:fitWrappedText={String(value)} data-table-cell="true" data-collection-cell-primary data-row-index={rowIndex} data-column-index={columnIndex} rows={1}
    class={cn("block min-h-8 w-full min-w-0 resize-none overflow-hidden rounded-sm border border-transparent bg-transparent px-1 py-1.5 text-foreground outline-none", page && pageTextClass)}
    style="field-sizing: content;" value={String(value)} aria-label={column.name} {placeholder}
    {disabled}
    onfocus={() => onEditingChange(true)}
    oninput={(event) => { if (column.type === "title") onDraft(event.currentTarget.value); }}
    onblur={(event) => { onEditingChange(false); void onSave(event.currentTarget.value); }}
    onkeydown={(event) => onNavigate(event, rowIndex, columnIndex)}></textarea>
{:else if editable}
  <input
    data-table-cell="true" data-collection-cell-primary
    data-row-index={rowIndex}
    data-column-index={columnIndex}
    class={cn("h-8 w-full min-w-0 rounded-sm border border-transparent bg-transparent px-1 text-foreground outline-none", page && pageTextClass)}
    value={String(value)}
    inputmode="text"
    aria-label={column.name}
    {placeholder}
    {disabled}
    onfocus={() => onEditingChange(true)}
    oninput={(event) => { if (column.type === "title") onDraft(event.currentTarget.value); }}
    onblur={(event) => {
      onEditingChange(false);
      void onSave(event.currentTarget.value);
    }}
    onkeydown={(event) => onNavigate(event, rowIndex, columnIndex)}
  />
{:else}
  <button
    data-table-cell="true"
    data-row-index={rowIndex}
    data-column-index={columnIndex}
    type="button"
    class={cn("flex w-full min-w-0 items-center rounded-sm px-1 text-left text-muted-foreground outline-none focus-visible:ring-2 focus-visible:ring-ring",
      column.displayFormat?.wrap ? "min-h-8 py-1.5" : "h-8", page && "cursor-pointer px-1.5")}
    title={t("notes.databaseTableReadonly")}
    onkeydown={(event) => onNavigate(event, rowIndex, columnIndex)}
  >
    <span class={`min-w-0 ${column.displayFormat?.wrap ? "whitespace-pre-wrap wrap-break-word" : "truncate"}`}>
      <NotesDatabasePropertyValue {row} {column} />
    </span>
  </button>
{/if}
