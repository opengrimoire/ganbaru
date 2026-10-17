import type { NotesDataSourceCalendarView } from "$lib/notes/contracts/database";
import { isNotesCalendarRowOpenMode } from ".././blocks";
import { readRecord, readString, readStringArray } from ".././readers";
import { dateMentionBoundaryLooksIso } from ".././rich-text";
import { parseNotesPage } from ".././workspace";
import { parseDataSourceWindowMetadata, parseNotesDataSource, parseNotesDatabaseView } from "./base";
import { validateNotesTableFilter, validateNotesTableSorts } from "./table";

function validateNotesCalendarConfiguration(value: Record<string, unknown> | null): void {
  if (value === null) return;
  if (value.type !== undefined && value.type !== "calendar") {
    throw new Error("database calendar configuration.type must be calendar");
  }
  const calendar = readRecord(value.calendar, "database calendar configuration.calendar");
  if (calendar.date_property_id !== null && calendar.date_property_id !== undefined) {
    readString(calendar.date_property_id, "database calendar configuration.date_property_id");
  }
  const rangeStart = readString(calendar.range_start, "database calendar configuration.range_start");
  const rangeEnd = readString(calendar.range_end, "database calendar configuration.range_end");
  if (!dateMentionBoundaryLooksIso(rangeStart) || rangeStart.length !== 10) {
    throw new Error("database calendar configuration.range_start must be an ISO date");
  }
  if (!dateMentionBoundaryLooksIso(rangeEnd) || rangeEnd.length !== 10) {
    throw new Error("database calendar configuration.range_end must be an ISO date");
  }
  if (rangeEnd < rangeStart) {
    throw new Error("database calendar configuration.range_end must be on or after range_start");
  }
  readStringArray(
    calendar.visible_property_ids,
    "database calendar configuration.visible_property_ids",
  );
  const rowOpenMode = calendar.row_open_mode ?? "side_panel";
  if (!isNotesCalendarRowOpenMode(rowOpenMode)) {
    throw new Error("database calendar configuration.row_open_mode must be supported");
  }
}

export function parseNotesDataSourceCalendarView(value: unknown): NotesDataSourceCalendarView {
  const record = readRecord(value, "data source calendar view");
  const dataSource = parseNotesDataSource(record.data_source);
  const view = parseNotesDatabaseView(record.view);
  if (view.type !== "calendar") {
    throw new Error("data source calendar view.view.type must be calendar");
  }
  if (view.data_source_id !== dataSource.id) {
    throw new Error("data source calendar view ids must match");
  }
  validateNotesCalendarConfiguration(view.configuration);
  validateNotesTableFilter(view.filter);
  validateNotesTableSorts(view.sorts);
  if (!Array.isArray(record.rows)) {
    throw new Error("data source calendar view.rows must be an array");
  }
  return {
    data_source: dataSource,
    view,
    rows: record.rows.map(parseNotesPage),
    ...parseDataSourceWindowMetadata(record, "data source calendar view"),
  };
}
