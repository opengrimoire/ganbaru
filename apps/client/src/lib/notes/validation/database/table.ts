import type { NotesDataSourceTableView, NotesDatabaseTableSortDirection } from "$lib/notes/contracts/database";
import { notesDatabaseParseFilters } from "$lib/notes/database/filters";
import { isNotesTableRowOpenMode } from ".././blocks";
import { readBoolean, readInteger, readNullableString, readRecord, readString, readStringArray } from ".././readers";
import { parseNotesPage } from ".././workspace";
import { parseDataSourceGroupCounts, parseNotesDataSource, parseNotesDatabaseView } from "./base";
import { parseNotesTablePresentation } from "./table-presentation";
import { NOTES_DATABASE_MAX_COLLAPSED_ROWS, parseNotesDatabaseRowHierarchy } from "$lib/notes/database/row-hierarchy";
import { isNotesUuid } from "$lib/notes/links/block-link";

function isNotesTableSortDirection(value: unknown): value is NotesDatabaseTableSortDirection {
  return value === "ascending" || value === "descending";
}

function validateNotesTableConfiguration(value: Record<string, unknown> | null): void {
  if (value === null) return;
  if (value.type !== undefined && value.type !== "table") {
    throw new Error("database table configuration.type must be table");
  }
  const table = readRecord(value.table, "database table configuration.table");
  readStringArray(table.property_order, "database table configuration.property_order");
  readStringArray(table.hidden_property_ids, "database table configuration.hidden_property_ids");
  const columnWidths = readRecord(
    table.column_widths ?? {},
    "database table configuration.column_widths",
  );
  for (const [propertyId, width] of Object.entries(columnWidths)) {
    readString(propertyId, "database table configuration.column_widths key");
    const parsedWidth = readInteger(width, `database table configuration.column_widths.${propertyId}`);
    if (parsedWidth < 96 || parsedWidth > 480) {
      throw new Error(`database table configuration.column_widths.${propertyId} is out of range`);
    }
  }
  const rowOpenMode = table.row_open_mode ?? "full_page";
  parseNotesTablePresentation(table.presentation);
  if (table.group_property_id !== undefined) readNullableString(table.group_property_id, "database table group_property_id");
  for (const key of ["group_order", "collapsed_group_ids"]) {
    if (table[key] !== undefined) {
      const ids = readStringArray(table[key], `database table ${key}`);
      if (ids.length > 500 || ids.some((id) => !id || id.length > 2000)) throw new Error("Table group identities exceed supported bounds");
    }
  }
  if (table.hide_empty_groups !== undefined) readBoolean(table.hide_empty_groups, "database table hide_empty_groups");
  if (table.collapsed_row_ids !== undefined) {
    const ids = readStringArray(table.collapsed_row_ids, "database table collapsed_row_ids");
    if (ids.length > NOTES_DATABASE_MAX_COLLAPSED_ROWS || ids.some((id) => !isNotesUuid(id))) throw new Error("Table collapsed row identities exceed supported bounds");
  }
  if (!isNotesTableRowOpenMode(rowOpenMode)) {
    throw new Error("database table configuration.row_open_mode must be supported");
  }
}

export function validateNotesTableFilter(value: Record<string, unknown> | null): void {
  notesDatabaseParseFilters(value);
}

export function validateNotesTableSorts(value: Record<string, unknown>[]): void {
  for (const [index, sort] of value.entries()) {
    readString(sort.property_id, `database table sorts[${index}].property_id`);
    if (!isNotesTableSortDirection(sort.direction)) {
      throw new Error(`database table sorts[${index}].direction must be supported`);
    }
  }
}

export function parseNotesDataSourceTableView(value: unknown): NotesDataSourceTableView {
  const record = readRecord(value, "data source table view");
  const dataSource = parseNotesDataSource(record.data_source);
  const view = parseNotesDatabaseView(record.view);
  if (view.type !== "table") {
    throw new Error("data source table view.view.type must be table");
  }
  if (view.data_source_id !== dataSource.id) {
    throw new Error("data source table view ids must match");
  }
  validateNotesTableConfiguration(view.configuration);
  validateNotesTableFilter(view.filter);
  validateNotesTableSorts(view.sorts);
  if (!Array.isArray(record.rows)) throw new Error("data source table view.rows must be an array");
  return {
    data_source: dataSource,
    view,
    rows: record.rows.map(parseNotesPage),
    total_row_count: readInteger(record.total_row_count, "data source table view.total_row_count"),
    next_cursor: readNullableString(record.next_cursor, "data source table view.next_cursor"),
    has_more: readBoolean(record.has_more, "data source table view.has_more"),
    group_counts: parseDataSourceGroupCounts(record.group_counts ?? {}, "data source table view.group_counts"),
    calculations: parseTableCalculations(record.calculations),
    row_hierarchy: parseNotesDatabaseRowHierarchy(record.row_hierarchy),
  };
}

/** Validate complete-source numeric calculations at the IPC boundary. */
function parseTableCalculationValues(value: unknown): Record<string, number | null> {
  const values = readRecord(value, "table calculation values");
  const parsed: Record<string, number | null> = {};
  for (const [id, number] of Object.entries(values)) {
    if (number !== null && (typeof number !== "number" || !Number.isFinite(number))) throw new Error("Table calculation must be finite or null");
    Object.defineProperty(parsed, id, { value: number, enumerable: true, configurable: true, writable: true });
  }
  return parsed;
}

/** Keep overall results separate from arbitrary canonical group identities. */
function parseTableCalculations(value: unknown): NonNullable<NotesDataSourceTableView["calculations"]> {
  if (value === undefined) return { overall: {}, groups: {} };
  const record = readRecord(value, "table calculations");
  const groups = readRecord(record.groups, "table calculation groups");
  const result: Record<string, Record<string, number | null>> = {};
  for (const [groupId, properties] of Object.entries(groups)) {
    const parsed = parseTableCalculationValues(properties);
    Object.defineProperty(result, groupId, { value: parsed, enumerable: true, configurable: true, writable: true });
  }
  return { overall: parseTableCalculationValues(record.overall), groups: result };
}
