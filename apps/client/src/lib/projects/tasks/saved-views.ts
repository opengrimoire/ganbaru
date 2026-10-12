import {
  PROJECT_TASK_GROUP_MODES,
  PROJECT_TASK_SORT_MODES,
  PROJECT_VIEW_IDS,
  type ProjectCustomFieldFilter,
  type ProjectCoreTaskSortMode,
  type ProjectSavedTaskView,
  type ProjectTaskGroupMode,
  type ProjectTaskListColumn,
  type ProjectTaskDependencyFilter,
  type ProjectTaskDueFilter,
  type ProjectTaskScheduleFilter,
  type ProjectTaskSortDirection,
  type ProjectTaskSortMode,
  type ProjectTaskStatusFilter,
  type ProjectViewPreference,
} from "$lib/projects/types";
import {
  customFieldIdFromCustomFieldReference,
  DEFAULT_TASK_LIST_COLUMNS,
  parseTaskListColumns,
} from "./list-columns";
import type { ProjectTaskFilterState } from "$lib/projects/list/view";

export const SAVED_TASK_VIEW_PREFIX = "saved-task-view:";

const TASK_STATUS_FILTERS: ProjectTaskStatusFilter[] = ["all", "open", "blocked", "done"];
const TASK_DUE_FILTERS: ProjectTaskDueFilter[] = ["all", "overdue", "today", "week", "none", "range"];
const TASK_SCHEDULE_FILTERS: ProjectTaskScheduleFilter[] = ["all", "scheduled", "unscheduled"];
const TASK_DEPENDENCY_FILTERS: ProjectTaskDependencyFilter[] = ["all", "linked", "blocked_by", "blocking", "none"];
const TASK_SORT_MODES: ProjectCoreTaskSortMode[] = [...PROJECT_TASK_SORT_MODES];
const TASK_SORT_DIRECTIONS: ProjectTaskSortDirection[] = ["asc", "desc"];
const TASK_GROUP_MODES: ProjectTaskGroupMode[] = [...PROJECT_TASK_GROUP_MODES];

export interface ProjectSavedTaskViewSnapshotInput extends ProjectTaskFilterState {
  id: string;
  projectId: string;
  name: string;
  viewId: ProjectSavedTaskView["viewId"];
  collapsedSectionIds: readonly string[];
  showArchivedTasks: boolean;
  visibleColumns: readonly ProjectTaskListColumn[];
  updatedAt?: string;
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function isOneOf<T extends string>(value: unknown, options: readonly T[]): value is T {
  return typeof value === "string" && options.includes(value as T);
}

function parseProjectViewId(value: unknown, fallback: ProjectSavedTaskView["viewId"]): ProjectSavedTaskView["viewId"] {
  return isOneOf(value, PROJECT_VIEW_IDS) ? value : fallback;
}

export function savedTaskViewPreferenceKey(viewId: string): string {
  return `${SAVED_TASK_VIEW_PREFIX}${viewId}`;
}

export function parseSavedTaskViewPreference(
  preference: ProjectViewPreference,
  customFieldIds?: ReadonlySet<string>,
  customFieldOptionIds?: ReadonlySet<string>,
): ProjectSavedTaskView | undefined {
  if (!preference.preferenceKey.startsWith(SAVED_TASK_VIEW_PREFIX)) return undefined;
  const id = preference.preferenceKey.slice(SAVED_TASK_VIEW_PREFIX.length);
  if (!id.trim()) return undefined;
  let parsed: unknown;
  try {
    parsed = JSON.parse(preference.preferenceValue);
  } catch {
    return undefined;
  }
  if (!isRecord(parsed)) return undefined;
  const name = typeof parsed.name === "string" ? parsed.name.trim() : "";
  if (!name) return undefined;
  const viewId = parseProjectViewId(parsed.viewId, preference.viewId);
  const search = typeof parsed.search === "string" ? parsed.search : "";
  const statusFilter = isOneOf(parsed.statusFilter, TASK_STATUS_FILTERS) ? parsed.statusFilter : "all";
  const sectionFilter = typeof parsed.sectionFilter === "string" && parsed.sectionFilter.trim()
    ? parsed.sectionFilter
    : "all";
  const priorityFilter = typeof parsed.priorityFilter === "string" && parsed.priorityFilter.trim()
    ? parsed.priorityFilter
    : "all";
  const dueFilter = isOneOf(parsed.dueFilter, TASK_DUE_FILTERS) ? parsed.dueFilter : "all";
  const dueRangeStart = parseSavedDate(parsed.dueRangeStart);
  const dueRangeEnd = parseSavedDate(parsed.dueRangeEnd);
  const scheduleFilter = isOneOf(parsed.scheduleFilter, TASK_SCHEDULE_FILTERS)
    ? parsed.scheduleFilter
    : "all";
  const dependencyFilter = isOneOf(parsed.dependencyFilter, TASK_DEPENDENCY_FILTERS)
    ? parsed.dependencyFilter
    : "all";
  const tagFilter = typeof parsed.tagFilter === "string" && parsed.tagFilter.trim()
    ? parsed.tagFilter
    : "all";
  const customFieldFilters = parseCustomFieldFilters(
    parsed.customFieldFilters,
    customFieldIds,
    customFieldOptionIds,
  );
  const sortMode = parseTaskSortMode(parsed.sortMode, customFieldIds);
  const sortDirection = isOneOf(parsed.sortDirection, TASK_SORT_DIRECTIONS) ? parsed.sortDirection : "asc";
  const groupBy = isOneOf(parsed.groupBy, TASK_GROUP_MODES) ? parsed.groupBy : "section";
  const collapsedSectionIds = parseCollapsedSectionIds(parsed.collapsedSectionIds);
  const visibleColumns = parseSavedTaskViewColumns(parsed.visibleColumns, customFieldIds);
  return {
    id,
    projectId: preference.projectId,
    name,
    viewId,
    search,
    statusFilter,
    sectionFilter,
    priorityFilter,
    dueFilter,
    dueRangeStart,
    dueRangeEnd,
    scheduleFilter,
    dependencyFilter,
    tagFilter,
    customFieldFilters,
    sortMode,
    sortDirection,
    groupBy,
    collapsedSectionIds,
    showArchivedTasks: parsed.showArchivedTasks === true,
    visibleColumns,
    updatedAt: preference.updatedAt,
  };
}

function parseCollapsedSectionIds(value: unknown): string[] {
  if (!Array.isArray(value)) return [];
  return Array.from(new Set(value.filter((entry): entry is string =>
    typeof entry === "string" && entry.trim().length > 0
  )));
}

function parseSavedTaskViewColumns(
  value: unknown,
  customFieldIds?: ReadonlySet<string>,
): ProjectTaskListColumn[] {
  if (Array.isArray(value)) {
    return parseTaskListColumns(JSON.stringify({ visibleColumns: value }), customFieldIds);
  }
  return [...DEFAULT_TASK_LIST_COLUMNS];
}

function parseSavedDate(value: unknown): string {
  if (typeof value !== "string") return "";
  const trimmed = value.trim();
  if (!/^\d{4}-\d{2}-\d{2}$/.test(trimmed)) return "";
  const [yearText, monthText, dayText] = trimmed.split("-");
  const year = Number(yearText);
  const month = Number(monthText);
  const day = Number(dayText);
  if (!Number.isInteger(year) || !Number.isInteger(month) || !Number.isInteger(day)) return "";
  if (month < 1 || month > 12) return "";
  const daysInMonth = new Date(Date.UTC(year, month, 0)).getUTCDate();
  if (day < 1 || day > daysInMonth) return "";
  return trimmed;
}

function parseTaskSortMode(
  value: unknown,
  customFieldIds?: ReadonlySet<string>,
): ProjectTaskSortMode {
  if (isOneOf(value, TASK_SORT_MODES)) return value;
  if (typeof value !== "string") return "manual";
  const customFieldId = customFieldIdFromCustomFieldReference(value);
  if (!customFieldId) return "manual";
  if (customFieldIds && !customFieldIds.has(customFieldId)) return "manual";
  return value as ProjectTaskSortMode;
}

function customFieldFilterKey(filter: ProjectCustomFieldFilter): string {
  return filter.fieldId;
}

function parseCustomFieldFilters(
  value: unknown,
  customFieldIds?: ReadonlySet<string>,
  customFieldOptionIds?: ReadonlySet<string>,
): ProjectCustomFieldFilter[] {
  if (!Array.isArray(value)) return [];
  const filters = new Map<string, ProjectCustomFieldFilter>();
  for (const entry of value) {
    const filter = parseCustomFieldFilter(entry, customFieldIds, customFieldOptionIds);
    if (filter) filters.set(customFieldFilterKey(filter), filter);
  }
  return Array.from(filters.values());
}

function parseCustomFieldFilter(
  value: unknown,
  customFieldIds?: ReadonlySet<string>,
  customFieldOptionIds?: ReadonlySet<string>,
): ProjectCustomFieldFilter | undefined {
  if (!isRecord(value)) return undefined;
  const fieldId = typeof value.fieldId === "string" ? value.fieldId.trim() : "";
  if (!fieldId || (customFieldIds && !customFieldIds.has(fieldId))) return undefined;
  if (value.mode === "filled" || value.mode === "empty") {
    return { fieldId, mode: value.mode };
  }
  if (value.mode === "checkbox" && typeof value.checked === "boolean") {
    return { fieldId, mode: "checkbox", checked: value.checked };
  }
  if (value.mode === "option") {
    const optionId = typeof value.optionId === "string" ? value.optionId.trim() : "";
    if (!optionId || (customFieldOptionIds && !customFieldOptionIds.has(optionId))) return undefined;
    return { fieldId, mode: "option", optionId };
  }
  return undefined;
}

export function projectCustomFieldFilterStillExists(
  filter: ProjectCustomFieldFilter,
  fieldIds: ReadonlySet<string>,
  optionIds: ReadonlySet<string>,
): boolean {
  if (!fieldIds.has(filter.fieldId)) return false;
  return filter.mode !== "option" || optionIds.has(filter.optionId);
}

export function projectTaskFilterStateFromSavedTaskView(view: ProjectSavedTaskView): ProjectTaskFilterState {
  return {
    search: view.search,
    statusFilter: view.statusFilter,
    sectionFilter: view.sectionFilter,
    priorityFilter: view.priorityFilter,
    dueFilter: view.dueFilter,
    dueRangeStart: view.dueRangeStart,
    dueRangeEnd: view.dueRangeEnd,
    scheduleFilter: view.scheduleFilter,
    dependencyFilter: view.dependencyFilter,
    tagFilter: view.tagFilter,
    customFieldFilters: view.customFieldFilters,
    groupBy: view.groupBy,
    sortMode: view.sortMode,
    sortDirection: view.sortDirection,
  };
}

export function createProjectSavedTaskViewSnapshot(
  input: ProjectSavedTaskViewSnapshotInput,
): ProjectSavedTaskView {
  return {
    id: input.id,
    projectId: input.projectId,
    name: input.name,
    viewId: input.viewId,
    search: input.search,
    statusFilter: input.statusFilter,
    sectionFilter: input.sectionFilter,
    priorityFilter: input.priorityFilter,
    dueFilter: input.dueFilter,
    dueRangeStart: input.dueRangeStart,
    dueRangeEnd: input.dueRangeEnd,
    scheduleFilter: input.scheduleFilter,
    dependencyFilter: input.dependencyFilter,
    tagFilter: input.tagFilter,
    customFieldFilters: [...input.customFieldFilters],
    sortMode: input.sortMode,
    sortDirection: input.sortDirection,
    groupBy: input.groupBy,
    collapsedSectionIds: [...input.collapsedSectionIds],
    showArchivedTasks: input.showArchivedTasks,
    visibleColumns: [...input.visibleColumns],
    updatedAt: input.updatedAt ?? new Date().toISOString(),
  };
}

export function savedTaskViewPreferenceValue(view: ProjectSavedTaskView): string {
  return JSON.stringify({
    name: view.name,
    viewId: view.viewId,
    search: view.search,
    statusFilter: view.statusFilter,
    sectionFilter: view.sectionFilter,
    priorityFilter: view.priorityFilter,
    dueFilter: view.dueFilter,
    dueRangeStart: view.dueRangeStart,
    dueRangeEnd: view.dueRangeEnd,
    scheduleFilter: view.scheduleFilter,
    dependencyFilter: view.dependencyFilter,
    tagFilter: view.tagFilter,
    customFieldFilters: view.customFieldFilters,
    sortMode: view.sortMode,
    sortDirection: view.sortDirection,
    groupBy: view.groupBy,
    collapsedSectionIds: view.collapsedSectionIds,
    showArchivedTasks: view.showArchivedTasks,
    visibleColumns: view.visibleColumns,
  });
}
