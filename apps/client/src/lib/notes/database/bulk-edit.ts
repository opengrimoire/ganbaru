import type { NotesDataSourcePropertyType, NotesPage } from "$lib/notes/types";
import {
  notesDatabaseTableCellEditValue,
  notesDatabaseTableColumnCanEdit,
  notesDatabaseTableEditValuesEqual,
  notesDatabaseTableOptionNames,
  type NotesDatabaseTableColumn,
  type NotesDatabaseTableEditValue,
} from "./table";

/**
 * Property types the selection toolbar sets on several rows at once. The title stays per row, and computed, file, and
 * people values have no single value every row could take.
 */
const BULK_EDITABLE_TYPES: ReadonlySet<NotesDataSourcePropertyType> = new Set<NotesDataSourcePropertyType>([
  "checkbox",
  "select",
  "status",
  "multi_select",
  "rich_text",
  "number",
  "url",
  "email",
  "phone_number",
  "date",
  "relation",
]);

/** The value a property shows when it edits several rows: the value they share, or an empty value when they differ. */
export interface NotesDatabaseBulkValue {
  value: NotesDatabaseTableEditValue;
  /** Whether the rows hold different values, so the editor starts empty and applying it replaces each of them. */
  mixed: boolean;
}

/**
 * List the visible properties the selection toolbar can set on every selected row, in view order.
 *
 * @param columns The view's columns in display order.
 */
export function notesDatabaseBulkEditableColumns(columns: readonly NotesDatabaseTableColumn[]): NotesDatabaseTableColumn[] {
  return columns.filter((column) => !column.hidden && BULK_EDITABLE_TYPES.has(column.type) && notesDatabaseTableColumnCanEdit(column));
}

/**
 * Read the value a property shares across rows. Multi-select options are compared by name and relations by target,
 * both regardless of order.
 *
 * @param rows The selected rows.
 * @param column The property to read.
 */
export function notesDatabaseBulkValue(rows: readonly NotesPage[], column: NotesDatabaseTableColumn): NotesDatabaseBulkValue {
  const values = rows.map((row) => rowValue(row, column));
  const first = values[0];
  if (first === undefined) return { value: emptyValue(column), mixed: false };
  return values.every((value) => sameValue(first, value)) ? { value: first, mixed: false } : { value: emptyValue(column), mixed: true };
}

/**
 * Add an item to a list value, or remove it when the list already holds it.
 *
 * @param values The current option names or relation targets.
 * @param item The option name or target id to toggle.
 */
export function notesDatabaseBulkToggledItem(values: readonly string[], item: string): string[] {
  return values.includes(item) ? values.filter((value) => value !== item) : [...values, item];
}

function rowValue(row: NotesPage, column: NotesDatabaseTableColumn): NotesDatabaseTableEditValue {
  return column.type === "multi_select" ? notesDatabaseTableOptionNames(row, column) : notesDatabaseTableCellEditValue(row, column);
}

function emptyValue(column: NotesDatabaseTableColumn): NotesDatabaseTableEditValue {
  switch (column.type) {
    case "checkbox":
      return false;
    case "multi_select":
    case "relation":
      return [];
    case "date":
      return null;
    default:
      return "";
  }
}

function sameValue(left: NotesDatabaseTableEditValue, right: NotesDatabaseTableEditValue): boolean {
  if (Array.isArray(left) && Array.isArray(right)) {
    return notesDatabaseTableEditValuesEqual([...left].sort(), [...right].sort());
  }
  return notesDatabaseTableEditValuesEqual(left, right);
}
