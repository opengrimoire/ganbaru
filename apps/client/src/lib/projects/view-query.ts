import { isCalendarViewMode, type CalendarViewMode } from "$lib/calendar/types";
import { PROJECT_TASK_FILTER_DEFAULTS, type ProjectTaskFilterState } from "./list/view";
import {
  PROJECT_TASK_GROUP_MODES,
  PROJECT_TASK_SORT_MODES,
  type ProjectCoreTaskSortMode,
  type ProjectCustomFieldFilter,
  type ProjectTaskDependencyFilter,
  type ProjectTaskDueFilter,
  type ProjectTaskGroupMode,
  type ProjectTaskScheduleFilter,
  type ProjectTaskSortMode,
  type ProjectTaskStatusFilter,
  type ProjectViewPreference,
} from "./types";

/** View preference key holding a task view's saved filters, sort, grouping, and calendar span. */
export const PROJECT_VIEW_QUERY_PREFERENCE_KEY = "query";

/** Saved query filters; the task search stays out because it belongs to the task finder, not to one view. */
export type ProjectViewQueryFilters = Omit<ProjectTaskFilterState, "search">;

/** The query a task view autosaves and restores whenever it opens. */
export interface ProjectViewQuery {
  filters: ProjectViewQueryFilters;
  showArchivedTasks: boolean;
  showInactiveSections: boolean;
  /** Calendar span of a Calendar view; null keeps the platform default. */
  calendarMode: CalendarViewMode | null;
}

const STATUS_FILTERS = ["all", "open", "blocked", "done"] as const satisfies readonly ProjectTaskStatusFilter[];
const DUE_FILTERS = ["all", "overdue", "today", "week", "none", "range"] as const satisfies readonly ProjectTaskDueFilter[];
const SCHEDULE_FILTERS = ["all", "scheduled", "unscheduled"] as const satisfies readonly ProjectTaskScheduleFilter[];
const DEPENDENCY_FILTERS = ["all", "linked", "blocked_by", "blocking", "none"] as const satisfies readonly ProjectTaskDependencyFilter[];
const FILTER_DATE = /^\d{4}-\d{2}-\d{2}$/;

/** Returns the query of a view that has never been customized. */
export function defaultProjectViewQuery(): ProjectViewQuery {
  const { search: _search, ...filters } = PROJECT_TASK_FILTER_DEFAULTS;
  return {
    filters: { ...filters, customFieldFilters: [] },
    showArchivedTasks: false,
    showInactiveSections: false,
    calendarMode: null,
  };
}

function oneOf<T extends string>(values: readonly T[], value: unknown, fallback: T): T {
  return typeof value === "string" && (values as readonly string[]).includes(value) ? value as T : fallback;
}

function nonEmptyString(value: unknown, fallback: string): string {
  return typeof value === "string" && value.length > 0 ? value : fallback;
}

function filterDate(value: unknown): string {
  return typeof value === "string" && FILTER_DATE.test(value) ? value : "";
}

function sortMode(value: unknown): ProjectTaskSortMode {
  if (typeof value !== "string") return PROJECT_TASK_FILTER_DEFAULTS.sortMode;
  if ((PROJECT_TASK_SORT_MODES as readonly string[]).includes(value)) return value as ProjectCoreTaskSortMode;
  return value.startsWith("custom:") && value.length > "custom:".length
    ? value as ProjectTaskSortMode
    : PROJECT_TASK_FILTER_DEFAULTS.sortMode;
}

function customFieldFilter(value: unknown): ProjectCustomFieldFilter | null {
  if (typeof value !== "object" || value === null) return null;
  const entry = value as Record<string, unknown>;
  if (typeof entry.fieldId !== "string" || entry.fieldId.length === 0) return null;
  if (entry.mode === "filled" || entry.mode === "empty") return { fieldId: entry.fieldId, mode: entry.mode };
  if (entry.mode === "checkbox" && typeof entry.checked === "boolean") {
    return { fieldId: entry.fieldId, mode: "checkbox", checked: entry.checked };
  }
  if (entry.mode === "option" && typeof entry.optionId === "string" && entry.optionId.length > 0) {
    return { fieldId: entry.fieldId, mode: "option", optionId: entry.optionId };
  }
  return null;
}

/**
 * Restores a saved view query from its stored JSON. Each malformed field falls back to its default on its own, so
 * one bad value never discards the rest of the view's setup; references to deleted sections, tags, or fields are
 * repaired later against live project data.
 */
export function parseProjectViewQuery(value: string | null | undefined): ProjectViewQuery {
  const defaults = defaultProjectViewQuery();
  if (!value) return defaults;
  let parsed: unknown;
  try {
    parsed = JSON.parse(value);
  } catch {
    return defaults;
  }
  if (typeof parsed !== "object" || parsed === null || Array.isArray(parsed)) return defaults;
  const stored = parsed as Record<string, unknown>;
  const filters = typeof stored.filters === "object" && stored.filters !== null
    ? stored.filters as Record<string, unknown>
    : {};
  const base = defaults.filters;
  return {
    filters: {
      statusFilter: oneOf(STATUS_FILTERS, filters.statusFilter, base.statusFilter),
      sectionFilter: nonEmptyString(filters.sectionFilter, base.sectionFilter),
      priorityFilter: nonEmptyString(filters.priorityFilter, base.priorityFilter),
      dueFilter: oneOf(DUE_FILTERS, filters.dueFilter, base.dueFilter),
      dueRangeStart: filterDate(filters.dueRangeStart),
      dueRangeEnd: filterDate(filters.dueRangeEnd),
      scheduleFilter: oneOf(SCHEDULE_FILTERS, filters.scheduleFilter, base.scheduleFilter),
      dependencyFilter: oneOf(DEPENDENCY_FILTERS, filters.dependencyFilter, base.dependencyFilter),
      tagFilter: nonEmptyString(filters.tagFilter, base.tagFilter),
      customFieldFilters: Array.isArray(filters.customFieldFilters)
        ? filters.customFieldFilters.flatMap((entry) => customFieldFilter(entry) ?? [])
        : [],
      groupBy: oneOf<ProjectTaskGroupMode>(PROJECT_TASK_GROUP_MODES, filters.groupBy, base.groupBy),
      sortMode: sortMode(filters.sortMode),
      sortDirection: oneOf(["asc", "desc"], filters.sortDirection, base.sortDirection),
    },
    showArchivedTasks: stored.showArchivedTasks === true,
    showInactiveSections: stored.showInactiveSections === true,
    calendarMode: isCalendarViewMode(stored.calendarMode) ? stored.calendarMode : null,
  };
}

/** Serializes a view query with a stable key order, so equal queries compare equal as strings. */
export function serializeProjectViewQuery(query: ProjectViewQuery): string {
  const { filters } = query;
  return JSON.stringify({
    filters: {
      statusFilter: filters.statusFilter,
      sectionFilter: filters.sectionFilter,
      priorityFilter: filters.priorityFilter,
      dueFilter: filters.dueFilter,
      dueRangeStart: filters.dueRangeStart,
      dueRangeEnd: filters.dueRangeEnd,
      scheduleFilter: filters.scheduleFilter,
      dependencyFilter: filters.dependencyFilter,
      tagFilter: filters.tagFilter,
      customFieldFilters: filters.customFieldFilters,
      groupBy: filters.groupBy,
      sortMode: filters.sortMode,
      sortDirection: filters.sortDirection,
    },
    showArchivedTasks: query.showArchivedTasks,
    showInactiveSections: query.showInactiveSections,
    calendarMode: query.calendarMode,
  });
}

/** Returns the stored query JSON of one view, or null while the view has never saved one. */
export function storedProjectViewQuery(
  preferences: readonly ProjectViewPreference[],
  viewId: string,
): string | null {
  return preferences.find((entry) =>
    entry.viewId === viewId && entry.preferenceKey === PROJECT_VIEW_QUERY_PREFERENCE_KEY)?.preferenceValue ?? null;
}
