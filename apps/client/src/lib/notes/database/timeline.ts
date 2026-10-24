import { Temporal } from "@js-temporal/polyfill";
import { notesDateMentionValueWithDates } from "$lib/notes/rich-text/date-mention-edit";
import { notesDatabaseDateValue } from "./date";
import { notesDatabaseSerializeFilters } from "./filters";
import {
  notesDatabaseMatchingPayload,
  notesDatabaseTableCellText,
  notesDatabaseTableColumns,
  notesDatabaseTableFiltersFromView,
  notesDatabaseTableSortsFromView,
  type NotesDatabaseTableColumn,
} from "./table";
import type {
  NotesDatabaseTableFilter,
  NotesDatabaseTableSort,
  NotesDatabaseTimelineConfiguration,
  NotesDatabaseTimelineRowOpenMode,
  NotesDatabaseView,
  NotesDataSource,
  NotesDataSourceTimelineGroup,
  NotesDataSourceTimelineViewUpdate,
  NotesPage,
} from "$lib/notes/types";

type UnknownRecord = Record<string, unknown>;

const TIMELINE_EMPTY_GROUP_ID = "__empty__";
const TIMELINE_UNGROUPED_ID = "__ungrouped__";
const TIMELINE_GROUP_PROPERTY_TYPES = new Set<string>([
  "select",
  "multi_select",
  "status",
  "checkbox",
  "people",
  "date",
]);

export interface NotesDataSourceTimelineItem {
  row: NotesPage;
  start: string;
  end: string;
  grid_column: string;
}

export interface NotesDatabaseTimelineMonthRange {
  range_start: string;
  range_end: string;
}

function isRecord(value: unknown): value is UnknownRecord {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function readStringArray(value: unknown): string[] {
  if (!Array.isArray(value)) return [];
  return value.filter((item): item is string => typeof item === "string");
}

function timelineConfig(view: NotesDatabaseView): UnknownRecord {
  const configuration = view.configuration;
  if (!isRecord(configuration)) return {};
  const timeline = configuration.timeline;
  return isRecord(timeline) ? timeline : {};
}

/** Read the persisted timeline configuration with local defaults. */
export function notesDatabaseTimelineConfigurationFromView(
  view: NotesDatabaseView,
): NotesDatabaseTimelineConfiguration {
  const timeline = timelineConfig(view);
  const fallbackRange = notesDatabaseTimelineMonthRange(Temporal.Now.plainDateISO().toString());
  const rangeStart = readIsoDate(timeline.range_start, fallbackRange.range_start);
  const rangeEnd = readIsoDate(timeline.range_end, fallbackRange.range_end);
  const rowOpenMode: NotesDatabaseTimelineRowOpenMode =
    timeline.row_open_mode === "full_page" ? "full_page" : "side_panel";
  return {
    date_property_id: typeof timeline.date_property_id === "string" ? timeline.date_property_id : null,
    group_property_id: typeof timeline.group_property_id === "string" ? timeline.group_property_id : null,
    group_order: readStringArray(timeline.group_order),
    hidden_group_ids: readStringArray(timeline.hidden_group_ids),
    range_start: rangeEnd < rangeStart ? fallbackRange.range_start : rangeStart,
    range_end: rangeEnd < rangeStart ? fallbackRange.range_end : rangeEnd,
    visible_property_ids: readStringArray(timeline.visible_property_ids),
    row_open_mode: rowOpenMode,
  };
}

/** Build timeline columns from the current data source schema. */
export function notesDatabaseTimelineColumns(
  dataSource: NotesDataSource,
  view: NotesDatabaseView,
): NotesDatabaseTableColumn[] {
  return notesDatabaseTableColumns(dataSource, view);
}

/** Return date properties that can place rows on the timeline. */
export function notesDatabaseTimelineDateColumns(
  columns: readonly NotesDatabaseTableColumn[],
): NotesDatabaseTableColumn[] {
  return columns.filter((column) => column.type === "date");
}

/** Return properties that can group timeline lanes. */
export function notesDatabaseTimelineGroupableColumns(
  columns: readonly NotesDatabaseTableColumn[],
): NotesDatabaseTableColumn[] {
  return columns.filter((column) => TIMELINE_GROUP_PROPERTY_TYPES.has(column.type));
}

/** Return the properties shown on timeline items. */
export function notesDatabaseTimelineVisibleColumns(
  columns: readonly NotesDatabaseTableColumn[],
  configuration: NotesDatabaseTimelineConfiguration,
): NotesDatabaseTableColumn[] {
  const byId = new Map(columns.map((column) => [column.id, column]));
  const configured = configuration.visible_property_ids
    .map((id) => byId.get(id))
    .filter((column): column is NotesDatabaseTableColumn =>
      column !== undefined
      && column.type !== "title"
      && column.id !== configuration.date_property_id
      && column.id !== configuration.group_property_id
    );
  if (configured.length > 0) return configured;
  return columns
    .filter((column) => column.type !== "title")
    .filter((column) => column.id !== configuration.date_property_id)
    .filter((column) => column.id !== configuration.group_property_id)
    .slice(0, 3);
}

/** Build the first and last day for a month from any date in that month. */
export function notesDatabaseTimelineMonthRange(anchorDate: string): NotesDatabaseTimelineMonthRange {
  const anchor = Temporal.PlainDate.from(anchorDate);
  const rangeStart = Temporal.PlainDate.from({ year: anchor.year, month: anchor.month, day: 1 });
  const rangeEnd = rangeStart.add({ months: 1 }).subtract({ days: 1 });
  return {
    range_start: rangeStart.toString(),
    range_end: rangeEnd.toString(),
  };
}

/** Move a persisted timeline range by whole months. */
export function notesDatabaseTimelineShiftMonth(
  configuration: NotesDatabaseTimelineConfiguration,
  months: number,
): NotesDatabaseTimelineConfiguration {
  const nextAnchor = Temporal.PlainDate.from(configuration.range_start).add({ months });
  return {
    ...configuration,
    ...notesDatabaseTimelineMonthRange(nextAnchor.toString()),
  };
}

/** Build every date column rendered in the horizontal timeline range. */
export function notesDatabaseTimelineDates(configuration: NotesDatabaseTimelineConfiguration): string[] {
  const start = Temporal.PlainDate.from(configuration.range_start);
  const end = Temporal.PlainDate.from(configuration.range_end);
  const dates: string[] = [];
  for (
    let cursor = start;
    Temporal.PlainDate.compare(cursor, end) <= 0;
    cursor = cursor.add({ days: 1 })
  ) {
    dates.push(cursor.toString());
  }
  return dates;
}

/** Build timeline groups from sorted rows and persisted grouping settings. */
export function notesDatabaseTimelineGroups(
  rows: readonly NotesPage[],
  columns: readonly NotesDatabaseTableColumn[],
  configuration: NotesDatabaseTimelineConfiguration,
): NotesDataSourceTimelineGroup[] {
  const groupColumn = configuration.group_property_id
    ? columns.find((column) => column.id === configuration.group_property_id) ?? null
    : null;
  if (!groupColumn) {
    return [{
      id: TIMELINE_UNGROUPED_ID,
      name: "All rows",
      color: "default",
      hidden: false,
      rows: [...rows],
    }];
  }

  const groups = new Map<string, NotesDataSourceTimelineGroup>();
  for (const group of initialGroups(groupColumn, configuration)) {
    groups.set(group.id, group);
  }
  for (const row of rows) {
    for (const groupId of rowGroupIds(row, groupColumn)) {
      const group = groups.get(groupId) ?? dynamicGroup(groupId, groupColumn);
      group.rows.push(row);
      groups.set(groupId, group);
    }
  }

  const ordered: NotesDataSourceTimelineGroup[] = [];
  for (const groupId of configuration.group_order) {
    const group = groups.get(groupId);
    if (!group) continue;
    ordered.push(group);
    groups.delete(groupId);
  }
  ordered.push(...groups.values());
  return ordered;
}

/** Build visual timeline items with one-based CSS grid column spans. */
export function notesDatabaseTimelineItems(
  rows: readonly NotesPage[],
  dateColumn: NotesDatabaseTableColumn | null,
  configuration: NotesDatabaseTimelineConfiguration,
): NotesDataSourceTimelineItem[] {
  if (!dateColumn) return [];
  const rangeStart = Temporal.PlainDate.from(configuration.range_start);
  const rangeEnd = Temporal.PlainDate.from(configuration.range_end);
  return rows.flatMap((row) => {
    const range = rowDateRange(row, dateColumn);
    if (!range) return [];
    if (
      Temporal.PlainDate.compare(range.end, rangeStart) < 0
      || Temporal.PlainDate.compare(range.start, rangeEnd) > 0
    ) {
      return [];
    }
    const visibleStart = maxDate(range.start, rangeStart);
    const visibleEnd = minDate(range.end, rangeEnd);
    const startColumn = daysBetween(rangeStart, visibleStart) + 1;
    const span = daysBetween(visibleStart, visibleEnd) + 1;
    return [{
      row,
      start: range.start.toString(),
      end: range.end.toString(),
      grid_column: `${startColumn} / span ${span}`,
    }];
  });
}

/** Build the update payload expected by the local timeline Tauri command. */
export function notesDatabaseTimelineUpdate(
  configuration: NotesDatabaseTimelineConfiguration,
  visibleColumns: readonly NotesDatabaseTableColumn[],
  filters: readonly NotesDatabaseTableFilter[],
  sorts: readonly NotesDatabaseTableSort[],
): NotesDataSourceTimelineViewUpdate {
  return {
    filter: notesDatabaseSerializeFilters(filters),
    sorts: sorts.map((sort) => ({
      property_id: sort.property_id,
      direction: sort.direction,
    })),
    configuration: {
      date_property_id: configuration.date_property_id,
      group_property_id: configuration.group_property_id,
      group_order: configuration.group_order,
      hidden_group_ids: configuration.hidden_group_ids,
      range_start: configuration.range_start,
      range_end: configuration.range_end,
      visible_property_ids: visibleColumns.map((column) => column.id),
      row_open_mode: configuration.row_open_mode,
    },
  };
}

/** Return filters stored on a timeline view. */
export function notesDatabaseTimelineFiltersFromView(view: NotesDatabaseView): NotesDatabaseTableFilter[] {
  return notesDatabaseTableFiltersFromView(view);
}

/** Return sorts stored on a timeline view. */
export function notesDatabaseTimelineSortsFromView(view: NotesDatabaseView): NotesDatabaseTableSort[] {
  return notesDatabaseTableSortsFromView(view);
}

/** Return user-facing text for a timeline item property. */
export function notesDatabaseTimelineRowText(
  page: NotesPage,
  column: NotesDatabaseTableColumn,
): string {
  return notesDatabaseTableCellText(page, column);
}

/** Return the title displayed on a timeline item. */
export function notesDatabaseTimelineRowTitle(
  page: NotesPage,
  columns: readonly NotesDatabaseTableColumn[],
  fallback: string,
): string {
  const titleColumn = columns.find((column) => column.type === "title");
  const title = titleColumn ? notesDatabaseTableCellText(page, titleColumn).trim() : "";
  return title || fallback;
}

/** Build initial row properties for creating a row on a timeline date. */
export function notesDatabaseTimelineRowCreateProperties(
  dataSource: NotesDataSource,
  datePropertyId: string,
  date: string,
): Record<string, unknown> {
  const propertyKey = dataSourcePropertyKey(dataSource, datePropertyId, "date");
  if (!propertyKey) return {};
  return {
    [propertyKey]: {
      id: datePropertyId,
      type: "date",
      date: notesDatabaseTimelineDateValue(date, date),
    },
  };
}

/** Build the date payload used when resizing a timeline row. */
export function notesDatabaseTimelineDateValue(start: string, end: string): Record<string, unknown> {
  const startDate = Temporal.PlainDate.from(start);
  const endDate = Temporal.PlainDate.from(end);
  const normalizedEnd = Temporal.PlainDate.compare(endDate, startDate) < 0 ? startDate : endDate;
  return {
    start: startDate.toString(),
    end: normalizedEnd.equals(startDate) ? null : normalizedEnd.toString(),
    time_zone: null,
  };
}

/** The date a resized timeline row saves, keeping the reminder its date property already has. */
export function notesDatabaseTimelineResizedDateValue(
  row: NotesPage,
  datePropertyId: string,
  start: string,
  end: string,
): Record<string, unknown> {
  const next = notesDatabaseTimelineDateValue(start, end);
  const property = Object.values(row.properties).find((value) => isRecord(value) && value.id === datePropertyId);
  const previous = isRecord(property) ? notesDatabaseDateValue(property.date) : null;
  const dates = notesDatabaseDateValue(next);
  return previous && dates ? { ...notesDateMentionValueWithDates(previous, dates) } : next;
}

function initialGroups(
  groupColumn: NotesDatabaseTableColumn,
  configuration: NotesDatabaseTimelineConfiguration,
): NotesDataSourceTimelineGroup[] {
  switch (groupColumn.type) {
    case "select":
    case "multi_select":
    case "status":
      return [
        ...groupColumn.options.map((option) => createGroup(option.id, option.name, option.color, configuration)),
        createGroup(TIMELINE_EMPTY_GROUP_ID, "No value", "default", configuration),
      ];
    case "checkbox":
      return [
        createGroup("false", "Unchecked", "gray", configuration),
        createGroup("true", "Checked", "green", configuration),
      ];
    case "date":
    case "people":
      return [createGroup(TIMELINE_EMPTY_GROUP_ID, "No value", "default", configuration)];
    default:
      return [createGroup(TIMELINE_UNGROUPED_ID, "All rows", "default", configuration)];
  }
}

function createGroup(
  id: string,
  name: string,
  color: string,
  configuration: NotesDatabaseTimelineConfiguration,
): NotesDataSourceTimelineGroup {
  return {
    id,
    name,
    color,
    hidden: configuration.hidden_group_ids.includes(id),
    rows: [],
  };
}

function rowGroupIds(row: NotesPage, groupColumn: NotesDatabaseTableColumn): string[] {
  const payload = rowPropertyPayload(row, groupColumn);
  switch (groupColumn.type) {
    case "select":
    case "status":
      return isRecord(payload) && typeof payload.id === "string" && payload.id
        ? [payload.id]
        : [TIMELINE_EMPTY_GROUP_ID];
    case "multi_select":
      if (!Array.isArray(payload)) return [TIMELINE_EMPTY_GROUP_ID];
      return nonEmptyGroupIds(payload
        .filter(isRecord)
        .map((item) => item.id)
        .filter((id): id is string => typeof id === "string" && id.length > 0));
    case "checkbox":
      return typeof payload === "boolean" ? [String(payload)] : ["false"];
    case "date":
      return isRecord(payload) && typeof payload.start === "string" && payload.start
        ? [payload.start]
        : [TIMELINE_EMPTY_GROUP_ID];
    case "people":
      if (!Array.isArray(payload)) return [TIMELINE_EMPTY_GROUP_ID];
      return nonEmptyGroupIds(payload
        .filter(isRecord)
        .map((item) => item.id)
        .filter((id): id is string => typeof id === "string" && id.length > 0));
    default:
      return [TIMELINE_UNGROUPED_ID];
  }
}

function nonEmptyGroupIds(groupIds: string[]): string[] {
  return groupIds.length > 0 ? groupIds : [TIMELINE_EMPTY_GROUP_ID];
}

function dynamicGroup(
  groupId: string,
  groupColumn: NotesDatabaseTableColumn,
): NotesDataSourceTimelineGroup {
  if (groupId === TIMELINE_EMPTY_GROUP_ID) {
    return { id: groupId, name: "No value", color: "default", hidden: false, rows: [] };
  }
  if (groupColumn.type === "date") {
    return { id: groupId, name: groupId, color: "blue", hidden: false, rows: [] };
  }
  if (groupColumn.type === "people") {
    return { id: groupId, name: `Person ${groupId}`, color: "purple", hidden: false, rows: [] };
  }
  return { id: groupId, name: groupId, color: "default", hidden: false, rows: [] };
}

function rowPropertyPayload(
  page: NotesPage,
  column: NotesDatabaseTableColumn,
): unknown {
  return notesDatabaseMatchingPayload(page, column);
}

function rowDateRange(
  page: NotesPage,
  dateColumn: NotesDatabaseTableColumn,
): { start: Temporal.PlainDate; end: Temporal.PlainDate } | null {
  const payload = rowPropertyPayload(page, dateColumn);
  if (!isRecord(payload)) return null;
  const start = plainDateFromBoundary(payload.start);
  if (!start) return null;
  const end = plainDateFromBoundary(payload.end) ?? start;
  return { start, end };
}

function plainDateFromBoundary(value: unknown): Temporal.PlainDate | null {
  if (typeof value !== "string") return null;
  const dateText = value.slice(0, 10);
  try {
    return Temporal.PlainDate.from(dateText);
  } catch {
    return null;
  }
}

function dataSourcePropertyKey(
  dataSource: NotesDataSource,
  propertyId: string,
  propertyType: string,
): string | null {
  for (const [key, property] of Object.entries(dataSource.properties)) {
    if (!isRecord(property)) continue;
    if (property.id === propertyId && property.type === propertyType) return key;
  }
  return null;
}

function readIsoDate(value: unknown, fallback: string): string {
  if (typeof value !== "string") return fallback;
  try {
    return Temporal.PlainDate.from(value).toString();
  } catch {
    return fallback;
  }
}

function maxDate(left: Temporal.PlainDate, right: Temporal.PlainDate): Temporal.PlainDate {
  return Temporal.PlainDate.compare(left, right) >= 0 ? left : right;
}

function minDate(left: Temporal.PlainDate, right: Temporal.PlainDate): Temporal.PlainDate {
  return Temporal.PlainDate.compare(left, right) <= 0 ? left : right;
}

function daysBetween(start: Temporal.PlainDate, end: Temporal.PlainDate): number {
  return start.until(end, { largestUnit: "days" }).days;
}
