import type { NotesDataSourceSchema } from "$lib/notes/contracts/database";
import type { NotesPage } from "$lib/notes/contracts/core";
import { parseNotesDataSourceSchema } from "$lib/notes/validation/database";
import { readBoolean, readRecord, readRecordArray } from "$lib/notes/validation/readers";
import { parseNotesPage } from "$lib/notes/validation/workspace";
import { parseNotesDatabaseRowHierarchy } from "./row-hierarchy";
import type { NotesDatabaseRowHierarchyMetadata } from "./row-hierarchy";

/** One database row with its computed values, source schema, parent row, and direct sub-items. */
export interface NotesDataSourceRowPageContext {
  /** Source schema with the default table view, whose property order is the row page fallback order. */
  schema: NotesDataSourceSchema;
  row: NotesPage;
  hierarchy: NotesDatabaseRowHierarchyMetadata;
  parent: NotesPage | null;
  children: NotesPage[];
  /** Whether the source's database shell protects its structure from edits. */
  editing_locked: boolean;
}

/** Validate a row page context and check that its pages agree with its hierarchy. */
export function parseNotesDataSourceRowPageContext(value: unknown): NotesDataSourceRowPageContext {
  const record = readRecord(value, "row page context");
  const schema = parseNotesDataSourceSchema(record.schema);
  const row = parseNotesPage(record.row);
  const hierarchy = parseNotesDatabaseRowHierarchy({ [row.id]: record.hierarchy })[row.id];
  if (!hierarchy) throw new Error("row page context.hierarchy is missing");
  const parent = record.parent === null ? null : parseNotesPage(record.parent);
  const children = readRecordArray(record.children, "row page context.children").map(parseNotesPage);
  if ((parent?.id ?? null) !== hierarchy.parent_row_page_id) {
    throw new Error("row page context.parent must match its hierarchy");
  }
  if (children.some((child) => child.id === row.id || child.id === parent?.id)) {
    throw new Error("row page context.children must not contain the row or its parent");
  }
  return {
    schema,
    row,
    hierarchy,
    parent,
    children,
    editing_locked: readBoolean(record.editing_locked, "row page context.editing_locked"),
  };
}
