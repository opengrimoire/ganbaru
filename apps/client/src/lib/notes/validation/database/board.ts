import type { NotesDataSourceBoardGroup, NotesDataSourceBoardView } from "$lib/notes/contracts/database";
import { isNotesBoardRowOpenMode } from ".././blocks";
import { readBoolean, readRecord, readString, readStringArray } from ".././readers";
import { parseNotesPage } from ".././workspace";
import { parseDataSourceGroupCounts, parseDataSourceWindowMetadata, parseNotesDataSource, parseNotesDatabaseView } from "./base";
import { validateNotesTableFilter, validateNotesTableSorts } from "./table";

function validateNotesBoardConfiguration(value: Record<string, unknown> | null): void {
  if (value === null) return;
  if (value.type !== undefined && value.type !== "board") {
    throw new Error("database board configuration.type must be board");
  }
  const board = readRecord(value.board, "database board configuration.board");
  if (board.group_property_id !== null && board.group_property_id !== undefined) {
    readString(board.group_property_id, "database board configuration.group_property_id");
  }
  readStringArray(board.group_order, "database board configuration.group_order");
  readStringArray(board.hidden_group_ids, "database board configuration.hidden_group_ids");
  readStringArray(board.visible_property_ids, "database board configuration.visible_property_ids");
  const rowOpenMode = board.row_open_mode ?? "full_page";
  if (!isNotesBoardRowOpenMode(rowOpenMode)) {
    throw new Error("database board configuration.row_open_mode must be supported");
  }
}

function parseNotesDataSourceBoardGroup(
  value: unknown,
  label: string,
): NotesDataSourceBoardGroup {
  const record = readRecord(value, label);
  const rows = Array.isArray(record.rows)
    ? record.rows.map(parseNotesPage)
    : null;
  if (rows === null) throw new Error(`${label}.rows must be an array`);
  return {
    id: readString(record.id, `${label}.id`),
    name: readString(record.name, `${label}.name`),
    color: readString(record.color, `${label}.color`),
    hidden: readBoolean(record.hidden, `${label}.hidden`),
    rows,
  };
}

export function parseNotesDataSourceBoardView(value: unknown): NotesDataSourceBoardView {
  const record = readRecord(value, "data source board view");
  const dataSource = parseNotesDataSource(record.data_source);
  const view = parseNotesDatabaseView(record.view);
  if (view.type !== "board") {
    throw new Error("data source board view.view.type must be board");
  }
  if (view.data_source_id !== dataSource.id) {
    throw new Error("data source board view ids must match");
  }
  validateNotesBoardConfiguration(view.configuration);
  validateNotesTableFilter(view.filter);
  validateNotesTableSorts(view.sorts);
  if (!Array.isArray(record.groups)) {
    throw new Error("data source board view.groups must be an array");
  }
  return {
    data_source: dataSource,
    view,
    groups: record.groups.map((group, index) =>
      parseNotesDataSourceBoardGroup(group, `data source board view.groups[${index}]`)
    ),
    ...parseDataSourceWindowMetadata(record, "data source board view"),
    group_counts: parseDataSourceGroupCounts(record.group_counts, "data source board view.group_counts"),
  };
}
