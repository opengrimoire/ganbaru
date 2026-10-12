import type { NotesDataSourceListView } from "$lib/notes/contracts/database";
import { isNotesListRowOpenMode } from ".././blocks";
import { readRecord, readString, readStringArray } from ".././readers";
import { parseNotesPage } from ".././workspace";
import { parseDataSourceGroupCounts, parseDataSourceWindowMetadata, parseNotesDataSource, parseNotesDatabaseView } from "./base";
import { validateNotesTableFilter, validateNotesTableSorts } from "./table";

function validateNotesListConfiguration(value: Record<string, unknown> | null): void {
  if (value === null) return;
  if (value.type !== undefined && value.type !== "list") {
    throw new Error("database list configuration.type must be list");
  }
  const list = readRecord(value.list, "database list configuration.list");
  if (list.group_property_id !== null && list.group_property_id !== undefined) {
    readString(list.group_property_id, "database list configuration.group_property_id");
  }
  readStringArray(list.group_order, "database list configuration.group_order");
  readStringArray(list.hidden_group_ids, "database list configuration.hidden_group_ids");
  readStringArray(list.visible_property_ids, "database list configuration.visible_property_ids");
  const rowOpenMode = list.row_open_mode ?? "side_panel";
  if (!isNotesListRowOpenMode(rowOpenMode)) {
    throw new Error("database list configuration.row_open_mode must be supported");
  }
}

export function parseNotesDataSourceListView(value: unknown): NotesDataSourceListView {
  const record = readRecord(value, "data source list view");
  const dataSource = parseNotesDataSource(record.data_source);
  const view = parseNotesDatabaseView(record.view);
  if (view.type !== "list") {
    throw new Error("data source list view.view.type must be list");
  }
  if (view.data_source_id !== dataSource.id) {
    throw new Error("data source list view ids must match");
  }
  validateNotesListConfiguration(view.configuration);
  validateNotesTableFilter(view.filter);
  validateNotesTableSorts(view.sorts);
  if (!Array.isArray(record.rows)) {
    throw new Error("data source list view.rows must be an array");
  }
  return {
    data_source: dataSource,
    view,
    rows: record.rows.map(parseNotesPage),
    ...parseDataSourceWindowMetadata(record, "data source list view"),
    group_counts: parseDataSourceGroupCounts(record.group_counts, "data source list view.group_counts"),
  };
}
