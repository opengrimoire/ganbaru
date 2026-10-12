import { Temporal } from "@js-temporal/polyfill";
import { notesDatabaseSerializeFilters } from "./filters";
import {
  notesDatabaseTableCellText,
  notesDatabaseTableColumns,
  notesDatabaseTableFiltersFromView,
  notesDatabaseTableSortsFromView,
  type NotesDatabaseTableColumn,
} from "./table";
import type {
  NotesDatabaseCalendarConfiguration,
  NotesDatabaseCalendarRowOpenMode,
  NotesDatabaseTableFilter,
  NotesDatabaseTableSort,
  NotesDatabaseView,
  NotesDataSource,
  NotesDataSourceCalendarDay,
  NotesDataSourceCalendarViewUpdate,
  NotesPage,
} from "$lib/notes/types";

type UnknownRecord = Record<string, unknown>;

export interface NotesDatabaseCalendarMonthRange {
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

function calendarConfig(view: NotesDatabaseView): UnknownRecord {
  const configuration = view.configuration;
  if (!isRecord(configuration)) return {};
  const calendar = configuration.calendar;
  return isRecord(calendar) ? calendar : {};
}

/** Read the persisted calendar configuration with local defaults. */
export function notesDatabaseCalendarConfigurationFromView(
  view: NotesDatabaseView,
): NotesDatabaseCalendarConfiguration {
  const calendar = calendarConfig(view);
  const fallbackRange = notesDatabaseCalendarMonthRange(Temporal.Now.plainDateISO().toString());
  const rangeStart = readIsoDate(calendar.range_start, fallbackRange.range_start);
  const rangeEnd = readIsoDate(calendar.range_end, fallbackRange.range_end);
  const rowOpenMode: NotesDatabaseCalendarRowOpenMode =
    calendar.row_open_mode === "full_page" ? "full_page" : "side_panel";
  return {
    date_property_id: typeof calendar.date_property_id === "string" ? calendar.date_property_id : null,
    range_start: rangeEnd < rangeStart ? fallbackRange.range_start : rangeStart,
    range_end: rangeEnd < rangeStart ? fallbackRange.range_end : rangeEnd,
    visible_property_ids: readStringArray(calendar.visible_property_ids),
    row_open_mode: rowOpenMode,
  };
}

/** Build calendar columns from the current data source schema. */
export function notesDatabaseCalendarColumns(
  dataSource: NotesDataSource,
  view: NotesDatabaseView,
): NotesDatabaseTableColumn[] {
  return notesDatabaseTableColumns(dataSource, view);
}

/** Return properties that can be used as the calendar date source. */
export function notesDatabaseCalendarDateColumns(
  columns: readonly NotesDatabaseTableColumn[],
): NotesDatabaseTableColumn[] {
  return columns.filter((column) => column.type === "date");
}

/** Return the properties displayed on calendar row chips. */
export function notesDatabaseCalendarVisibleColumns(
  columns: readonly NotesDatabaseTableColumn[],
  configuration: NotesDatabaseCalendarConfiguration,
): NotesDatabaseTableColumn[] {
  const byId = new Map(columns.map((column) => [column.id, column]));
  const datePropertyId = configuration.date_property_id;
  const configured = configuration.visible_property_ids
    .map((id) => byId.get(id))
    .filter((column): column is NotesDatabaseTableColumn =>
      column !== undefined && column.type !== "title" && column.id !== datePropertyId
    );
  if (configured.length > 0) return configured;
  return columns
    .filter((column) => column.type !== "title" && column.id !== datePropertyId)
    .slice(0, 3);
}

/** Build the first and last day for a month from any date in that month. */
export function notesDatabaseCalendarMonthRange(anchorDate: string): NotesDatabaseCalendarMonthRange {
  const anchor = Temporal.PlainDate.from(anchorDate);
  const rangeStart = Temporal.PlainDate.from({ year: anchor.year, month: anchor.month, day: 1 });
  const rangeEnd = rangeStart.add({ months: 1 }).subtract({ days: 1 });
  return {
    range_start: rangeStart.toString(),
    range_end: rangeEnd.toString(),
  };
}

/** Move a persisted calendar range by whole months. */
export function notesDatabaseCalendarShiftMonth(
  configuration: NotesDatabaseCalendarConfiguration,
  months: number,
): NotesDatabaseCalendarConfiguration {
  const nextAnchor = Temporal.PlainDate.from(configuration.range_start).add({ months });
  return {
    ...configuration,
    ...notesDatabaseCalendarMonthRange(nextAnchor.toString()),
  };
}

/** Build a stable six-week grid for the active calendar range. */
export function notesDatabaseCalendarDays(
  rows: readonly NotesPage[],
  dateColumn: NotesDatabaseTableColumn | null,
  configuration: NotesDatabaseCalendarConfiguration,
): NotesDataSourceCalendarDay[] {
  const monthStart = Temporal.PlainDate.from(configuration.range_start);
  const monthEnd = Temporal.PlainDate.from(configuration.range_end);
  const gridStart = monthStart.subtract({ days: monthStart.dayOfWeek - 1 });
  const days: NotesDataSourceCalendarDay[] = [];
  for (let offset = 0; offset < 42; offset += 1) {
    const date = gridStart.add({ days: offset });
    const inMonth = Temporal.PlainDate.compare(date, monthStart) >= 0
      && Temporal.PlainDate.compare(date, monthEnd) <= 0;
    days.push({
      date: date.toString(),
      in_month: inMonth,
      rows: inMonth && dateColumn ? rowsForDate(rows, dateColumn, date) : [],
    });
  }
  return days;
}

/** Build the update payload expected by the local calendar Tauri command. */
export function notesDatabaseCalendarUpdate(
  configuration: NotesDatabaseCalendarConfiguration,
  visibleColumns: readonly NotesDatabaseTableColumn[],
  filters: readonly NotesDatabaseTableFilter[],
  sorts: readonly NotesDatabaseTableSort[],
): NotesDataSourceCalendarViewUpdate {
  return {
    filter: notesDatabaseSerializeFilters(filters),
    sorts: sorts.map((sort) => ({
      property_id: sort.property_id,
      direction: sort.direction,
    })),
    configuration: {
      date_property_id: configuration.date_property_id,
      range_start: configuration.range_start,
      range_end: configuration.range_end,
      visible_property_ids: visibleColumns.map((column) => column.id),
      row_open_mode: configuration.row_open_mode,
    },
  };
}

/** Return filters stored on a calendar view. */
export function notesDatabaseCalendarFiltersFromView(view: NotesDatabaseView): NotesDatabaseTableFilter[] {
  return notesDatabaseTableFiltersFromView(view);
}

/** Return sorts stored on a calendar view. */
export function notesDatabaseCalendarSortsFromView(view: NotesDatabaseView): NotesDatabaseTableSort[] {
  return notesDatabaseTableSortsFromView(view);
}

/** Return user-facing text for a calendar chip property. */
export function notesDatabaseCalendarRowText(
  page: NotesPage,
  column: NotesDatabaseTableColumn,
): string {
  return notesDatabaseTableCellText(page, column);
}

/** Return the title displayed on a calendar item. */
export function notesDatabaseCalendarRowTitle(
  page: NotesPage,
  columns: readonly NotesDatabaseTableColumn[],
  fallback: string,
): string {
  const titleColumn = columns.find((column) => column.type === "title");
  const title = titleColumn ? notesDatabaseTableCellText(page, titleColumn).trim() : "";
  return title || fallback;
}

/** Build initial row properties for creating a row on a specific calendar date. */
export function notesDatabaseCalendarRowCreateProperties(
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
      date: {
        start: Temporal.PlainDate.from(date).toString(),
        end: null,
        time_zone: null,
      },
    },
  };
}

function rowsForDate(
  rows: readonly NotesPage[],
  dateColumn: NotesDatabaseTableColumn,
  date: Temporal.PlainDate,
): NotesPage[] {
  return rows.filter((row) => {
    const range = rowDateRange(row, dateColumn);
    if (!range) return false;
    return Temporal.PlainDate.compare(range.start, date) <= 0
      && Temporal.PlainDate.compare(range.end, date) >= 0;
  });
}

function rowDateRange(
  page: NotesPage,
  dateColumn: NotesDatabaseTableColumn,
): { start: Temporal.PlainDate; end: Temporal.PlainDate } | null {
  const property = Object.values(page.properties).find((value) =>
    isRecord(value) && value.id === dateColumn.id && value.type === "date"
  );
  if (!isRecord(property) || !isRecord(property.date)) return null;
  const start = plainDateFromBoundary(property.date.start);
  if (!start) return null;
  const end = plainDateFromBoundary(property.date.end) ?? start;
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
