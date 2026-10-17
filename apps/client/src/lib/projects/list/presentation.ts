import { PALETTE_SIZE, type EventColor } from "$lib/calendar/types";
import { projectTaskListGridColumnWidths, type ProjectTaskListGridInput, type ProjectTaskListResizableColumn } from "./view";
import { PROJECT_TASK_GROUP_MODES, PROJECT_TASK_LIST_COLUMNS, type ProjectCustomField, type ProjectTask, type ProjectTaskGroupMode, type ProjectTaskSortMode, type ProjectViewPreference } from "$lib/projects/types";
import { customFieldIdFromTaskListColumn } from "$lib/projects/tasks/list-columns";
import type { AppLocale } from "$lib/i18n/locales";

export const PROJECT_LIST_PRESENTATION_KEY = "list-presentation";
export const PROJECT_COLUMN_CALCULATIONS = ["none", "count", "filled", "empty", "sum", "average", "minimum", "maximum"] as const;
export type ProjectColumnCalculation = (typeof PROJECT_COLUMN_CALCULATIONS)[number];
export const PROJECT_COLUMN_DATE_FORMATS = ["locale", "iso", "relative"] as const;
export const PROJECT_COLUMN_TIME_FORMATS = ["locale", "12_hour", "24_hour", "hidden"] as const;
export const PROJECT_COLUMN_NUMBER_FORMATS = ["number", "commas", "percent"] as const;
export type ProjectColumnDateFormat = (typeof PROJECT_COLUMN_DATE_FORMATS)[number];
export type ProjectColumnTimeFormat = (typeof PROJECT_COLUMN_TIME_FORMATS)[number];
export type ProjectColumnNumberFormat = (typeof PROJECT_COLUMN_NUMBER_FORMATS)[number];
export interface ProjectRowColorRule {
  id: string;
  property: "status" | "priority";
  value: string;
  color: EventColor;
}
export interface ProjectListPresentation {
  wrappedColumns: ProjectTaskListResizableColumn[];
  frozenThrough: ProjectTaskListResizableColumn | null;
  calculations: Partial<Record<ProjectTaskListResizableColumn, ProjectColumnCalculation>>;
  colorRules: ProjectRowColorRule[];
  dateFormats: Partial<Record<ProjectTaskListResizableColumn, ProjectColumnDateFormat>>;
  timeFormats: Partial<Record<"start" | "due", ProjectColumnTimeFormat>>;
  numberFormats: Partial<Record<ProjectTaskListResizableColumn, ProjectColumnNumberFormat>>;
}

/** Return independent defaults for projects without saved table presentation. */
export function defaultProjectListPresentation(): ProjectListPresentation {
  return { wrappedColumns: [], frozenThrough: null, calculations: {}, colorRules: [], dateFormats: {}, timeFormats: {}, numberFormats: {} };
}

/** Validate persisted ids and bounded color rules before they reach styles or controls. */
export function parseProjectListPresentation(value: unknown, fields: readonly ProjectCustomField[]): ProjectListPresentation {
  const result = defaultProjectListPresentation();
  if (typeof value !== "object" || value === null || Array.isArray(value)) return result;
  const record = value as Record<string, unknown>;
  const available = new Set<string>(["name", ...PROJECT_TASK_LIST_COLUMNS, ...fields.map((field) => `custom:${field.id}`)]);
  const isColumn = (candidate: unknown): candidate is ProjectTaskListResizableColumn => typeof candidate === "string" && available.has(candidate);
  if (Array.isArray(record.wrappedColumns)) result.wrappedColumns = [...new Set(record.wrappedColumns.filter(isColumn))];
  if (isColumn(record.frozenThrough)) result.frozenThrough = record.frozenThrough;
  for (const [key, options] of [
    ["dateFormats", PROJECT_COLUMN_DATE_FORMATS], ["timeFormats", PROJECT_COLUMN_TIME_FORMATS], ["numberFormats", PROJECT_COLUMN_NUMBER_FORMATS],
  ] as const) {
    const entries = record[key];
    if (typeof entries !== "object" || entries === null || Array.isArray(entries)) continue;
    for (const [id, format] of Object.entries(entries)) {
      if (!isColumn(id) || !options.some((option) => option === format)) continue;
      const field = fields.find((candidate) => candidate.id === (id === "name" ? undefined : customFieldIdFromTaskListColumn(id)));
      if (key === "dateFormats" && (id === "start" || id === "due" || field?.fieldType === "date")) result.dateFormats[id] = format as ProjectColumnDateFormat;
      if (key === "timeFormats" && (id === "start" || id === "due")) result.timeFormats[id] = format as ProjectColumnTimeFormat;
      if (key === "numberFormats" && field?.fieldType === "number") result.numberFormats[id] = format as ProjectColumnNumberFormat;
    }
  }
  if (typeof record.calculations === "object" && record.calculations !== null && !Array.isArray(record.calculations)) {
    for (const [id, calculation] of Object.entries(record.calculations)) {
      if (isColumn(id) && projectColumnCalculationOptions(id, fields).some((option) => option === calculation)) {
        result.calculations[id] = calculation as ProjectColumnCalculation;
      }
    }
  }
  if (Array.isArray(record.colorRules)) {
    const seen = new Set<string>();
    for (const entry of record.colorRules.slice(0, 32)) {
      if (typeof entry !== "object" || entry === null || Array.isArray(entry)) continue;
      const rule = entry as Record<string, unknown>;
      if (typeof rule.id !== "string" || !rule.id || seen.has(rule.id)
        || (rule.property !== "status" && rule.property !== "priority")
        || typeof rule.value !== "string" || !rule.value
        || typeof rule.color !== "number" || !Number.isInteger(rule.color) || rule.color < 0 || rule.color >= PALETTE_SIZE) continue;
      seen.add(rule.id);
      result.colorRules.push({ id: rule.id, property: rule.property, value: rule.value, color: rule.color });
    }
  }
  return result;
}

/** Restore one list view's presentation, falling back to defaults when the stored value is malformed. */
export function projectListPresentationForView(preferences: readonly ProjectViewPreference[], viewId: string | null, fields: readonly ProjectCustomField[]): ProjectListPresentation {
  const preference = viewId ? preferences.find((entry) => entry.viewId === viewId && entry.preferenceKey === PROJECT_LIST_PRESENTATION_KEY) : undefined;
  if (!preference) return defaultProjectListPresentation();
  try { return parseProjectListPresentation(JSON.parse(preference.preferenceValue), fields); }
  catch { return defaultProjectListPresentation(); }
}

/** Offer numeric reductions only for columns with canonical numeric values. */
export function projectColumnCalculationOptions(column: ProjectTaskListResizableColumn, fields: readonly ProjectCustomField[]): readonly ProjectColumnCalculation[] {
  const fieldId = column === "name" ? undefined : customFieldIdFromTaskListColumn(column);
  const numeric = column === "estimate" || fields.some((field) => field.id === fieldId && field.fieldType === "number");
  return numeric ? PROJECT_COLUMN_CALCULATIONS : PROJECT_COLUMN_CALCULATIONS.slice(0, 4);
}

/** Resolve supported property sorting without changing canonical manual task order. */
export function projectListColumnSortMode(column: ProjectTaskListResizableColumn): ProjectTaskSortMode | null {
  if (column === "name") return "title";
  if (column === "status" || column === "start" || column === "priority" || column === "due" || column === "estimate" || column === "scheduled") return column;
  return column.startsWith("custom:") ? column as ProjectTaskSortMode : null;
}

/** Resolve the task grouping a column can drive; custom fields and other columns have none. */
export function projectListColumnGroupMode(column: ProjectTaskListResizableColumn): ProjectTaskGroupMode | null {
  const mode = PROJECT_TASK_GROUP_MODES.find((candidate) => candidate !== "section" && candidate === column);
  return mode ?? null;
}

/** Calculate frozen offsets from the same responsive tracks used to render the table. */
export function projectListFrozenOffsets(presentation: ProjectListPresentation, input: ProjectTaskListGridInput, viewportWidthRem?: number): Partial<Record<ProjectTaskListResizableColumn | "selection" | "expand", number>> {
  const columns = ["name", ...input.columns] as ProjectTaskListResizableColumn[];
  const end = presentation.frozenThrough === null ? -1 : columns.indexOf(presentation.frozenThrough);
  if (end < 0) return {};
  const widths = projectTaskListGridColumnWidths(input);
  const scrollingTracks = widths.slice(3, -1);
  const reserve = scrollingTracks.length ? Math.min(...scrollingTracks) : widths.at(-1) ?? 0;
  const budget = viewportWidthRem === undefined ? Number.POSITIVE_INFINITY : Math.max(0, viewportWidthRem - reserve);
  if (widths[0] + widths[1] > budget) return {};
  const result: Partial<Record<ProjectTaskListResizableColumn | "selection" | "expand", number>> = { selection: 0, expand: widths[0] };
  let offset = widths[0] + widths[1];
  for (const [index, column] of columns.entries()) {
    if (index > end) break;
    if (offset + widths[index + 2] > budget) break;
    result[column] = offset;
    offset += widths[index + 2];
  }
  return result;
}

/** Apply the first matching rule so overlapping rules have an explicit priority. */
export function projectListRowColor(task: ProjectTask, rules: readonly ProjectRowColorRule[]): EventColor | undefined {
  return rules.find((rule) => (rule.property === "status" ? task.statusId : task.priority) === rule.value)?.color;
}

/** Format a canonical date without changing the persisted calendar day. */
export function projectListDateLabel(date: string, format: ProjectColumnDateFormat, locale: AppLocale, today: string): string {
  const timestamp = Date.parse(`${date}T12:00:00Z`);
  if (!Number.isFinite(timestamp) || format === "iso") return date;
  if (format === "relative") {
    const difference = Math.round((timestamp - Date.parse(`${today}T12:00:00Z`)) / 86_400_000);
    if (Number.isFinite(difference)) return new Intl.RelativeTimeFormat(locale, { numeric: "auto" }).format(difference, "day");
  }
  return new Intl.DateTimeFormat(locale, { dateStyle: "medium", timeZone: "UTC" }).format(timestamp);
}

/** Apply display formatting while preserving the canonical numeric value for edits and queries. */
export function projectListNumberLabel(value: number, format: ProjectColumnNumberFormat, locale: AppLocale): string {
  return new Intl.NumberFormat(locale, { useGrouping: format === "commas", style: format === "percent" ? "percent" : "decimal", maximumFractionDigits: 6 }).format(value);
}

/** Display wall-clock time without converting it to another time zone. */
export function projectListTimeLabel(time: string, format: ProjectColumnTimeFormat, locale: AppLocale): string {
  if (format === "hidden") return "";
  const timestamp = Date.parse(`2000-01-01T${time}:00Z`);
  if (!Number.isFinite(timestamp)) return time;
  return new Intl.DateTimeFormat(locale, { hour: "numeric", minute: "2-digit", timeZone: "UTC", hour12: format === "12_hour" ? true : format === "24_hour" ? false : undefined }).format(timestamp);
}
