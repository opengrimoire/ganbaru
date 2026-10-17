import type { NotesDataSourceTimelineView } from "$lib/notes/contracts/database";
import { isNotesTimelineRowOpenMode } from ".././blocks";
import { readRecord, readString, readStringArray } from ".././readers";
import { dateMentionBoundaryLooksIso } from ".././rich-text";
import { parseNotesPage } from ".././workspace";
import { parseDataSourceGroupCounts, parseDataSourceWindowMetadata, parseNotesDataSource, parseNotesDatabaseView } from "./base";
import { validateNotesTableFilter, validateNotesTableSorts } from "./table";

function validateNotesTimelineConfiguration(value: Record<string, unknown> | null): void {
  if (value === null) return;
  if (value.type !== undefined && value.type !== "timeline") {
    throw new Error("database timeline configuration.type must be timeline");
  }
  const timeline = readRecord(value.timeline, "database timeline configuration.timeline");
  if (timeline.date_property_id !== null && timeline.date_property_id !== undefined) {
    readString(timeline.date_property_id, "database timeline configuration.date_property_id");
  }
  if (timeline.group_property_id !== null && timeline.group_property_id !== undefined) {
    readString(timeline.group_property_id, "database timeline configuration.group_property_id");
  }
  readStringArray(timeline.group_order, "database timeline configuration.group_order");
  readStringArray(timeline.hidden_group_ids, "database timeline configuration.hidden_group_ids");
  const rangeStart = readString(timeline.range_start, "database timeline configuration.range_start");
  const rangeEnd = readString(timeline.range_end, "database timeline configuration.range_end");
  if (!dateMentionBoundaryLooksIso(rangeStart) || rangeStart.length !== 10) {
    throw new Error("database timeline configuration.range_start must be an ISO date");
  }
  if (!dateMentionBoundaryLooksIso(rangeEnd) || rangeEnd.length !== 10) {
    throw new Error("database timeline configuration.range_end must be an ISO date");
  }
  if (rangeEnd < rangeStart) {
    throw new Error("database timeline configuration.range_end must be on or after range_start");
  }
  readStringArray(
    timeline.visible_property_ids,
    "database timeline configuration.visible_property_ids",
  );
  const rowOpenMode = timeline.row_open_mode ?? "side_panel";
  if (!isNotesTimelineRowOpenMode(rowOpenMode)) {
    throw new Error("database timeline configuration.row_open_mode must be supported");
  }
}

export function parseNotesDataSourceTimelineView(value: unknown): NotesDataSourceTimelineView {
  const record = readRecord(value, "data source timeline view");
  const dataSource = parseNotesDataSource(record.data_source);
  const view = parseNotesDatabaseView(record.view);
  if (view.type !== "timeline") {
    throw new Error("data source timeline view.view.type must be timeline");
  }
  if (view.data_source_id !== dataSource.id) {
    throw new Error("data source timeline view ids must match");
  }
  validateNotesTimelineConfiguration(view.configuration);
  validateNotesTableFilter(view.filter);
  validateNotesTableSorts(view.sorts);
  if (!Array.isArray(record.rows)) {
    throw new Error("data source timeline view.rows must be an array");
  }
  return {
    data_source: dataSource,
    view,
    rows: record.rows.map(parseNotesPage),
    ...parseDataSourceWindowMetadata(record, "data source timeline view"),
    group_counts: parseDataSourceGroupCounts(record.group_counts, "data source timeline view.group_counts"),
  };
}
