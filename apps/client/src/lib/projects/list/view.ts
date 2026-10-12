import type {
  ProjectCoreTaskListColumn,
  ProjectCustomField,
  ProjectCustomFieldOption,
  ProjectCustomFieldFilter,
  ProjectTag,
  ProjectPriority,
  ProjectSection,
  ProjectStatus,
  ProjectTask,
  ProjectTaskDependencyFilter,
  ProjectTaskDueFilter,
  ProjectTaskGroupMode,
  ProjectTaskTagFilter,
  ProjectTaskListColumn,
  ProjectTaskScheduleFilter,
  ProjectTaskSortDirection,
  ProjectTaskSortMode,
  ProjectTaskStatusFilter,
  ProjectViewPreference,
} from "$lib/projects/types";
import { PROJECT_TASK_LIST_COLUMNS } from "$lib/projects/types";
import type { Translate } from "$lib/i18n/translator.svelte";
import { projectPriorityLabel } from "$lib/projects/display";
import { customFieldIdFromTaskListColumn } from "$lib/projects/tasks/list-columns";
import { deriveProjectFilterChips, type ProjectFilterChip } from "$lib/projects/toolbar";

export interface ProjectTaskFilterState {
  search: string;
  statusFilter: ProjectTaskStatusFilter;
  sectionFilter: string | "all";
  priorityFilter: ProjectPriority | "all";
  dueFilter: ProjectTaskDueFilter;
  dueRangeStart: string;
  dueRangeEnd: string;
  scheduleFilter: ProjectTaskScheduleFilter;
  dependencyFilter: ProjectTaskDependencyFilter;
  tagFilter: ProjectTaskTagFilter;
  customFieldFilters: readonly ProjectCustomFieldFilter[];
  groupBy: ProjectTaskGroupMode;
  sortMode: ProjectTaskSortMode;
  sortDirection: ProjectTaskSortDirection;
}

export const PROJECT_TASK_FILTER_DEFAULTS = Object.freeze({
  search: "",
  statusFilter: "all",
  sectionFilter: "all",
  priorityFilter: "all",
  dueFilter: "all",
  dueRangeStart: "",
  dueRangeEnd: "",
  scheduleFilter: "all",
  dependencyFilter: "all",
  tagFilter: "all",
  customFieldFilters: [],
  groupBy: "section",
  sortMode: "manual",
  sortDirection: "asc",
} satisfies ProjectTaskFilterState);

export const TASK_LIST_COLUMN_WIDTHS_PREFERENCE_KEY = "list-column-widths";
export type ProjectTaskListResizableColumn = "name" | ProjectTaskListColumn;
export type ProjectTaskListColumnWidths = Partial<Record<ProjectTaskListResizableColumn, number>>;
export interface ProjectTaskListColumnResizeGesture {
  column: ProjectTaskListResizableColumn;
  pointerId: number;
  startClientX: number;
  startWidthRem: number;
  rootFontSizePx: number;
  widthsAtStart: ProjectTaskListColumnWidths;
  draftWidths: ProjectTaskListColumnWidths;
  moved: boolean;
}

interface ProjectTaskListColumnWidthBounds {
  min: number;
  autoMin?: number;
  max: number;
  headerMax?: number;
  manualMax?: number;
}

export interface ProjectTaskListGridInput {
  columns: readonly ProjectTaskListColumn[];
  columnWidths?: ProjectTaskListColumnWidths;
  tasks?: readonly ProjectTask[];
  statuses?: readonly ProjectStatus[];
  customFields?: readonly ProjectCustomField[];
  nameLabel?: string;
  sectionLabels?: readonly string[];
  groupLabels?: readonly string[];
  columnLabel?: (column: ProjectTaskListColumn) => string;
  priorityLabel?: (priority: ProjectPriority) => string;
  estimateLabel?: (minutes: number) => string;
  customFieldDisplayValue?: (task: ProjectTask, field: ProjectCustomField) => string | undefined;
  scheduledLabel?: (taskId: string) => string | null;
}

const PROJECT_LIST_SELECTION_TRACK_REM = 1.5;
const PROJECT_LIST_EXPAND_TRACK_REM = 1.75;
const PROJECT_LIST_ADD_COLUMN_TRACK_REM = 2.25;
const PROJECT_LIST_TEXT_PADDING_REM = 1.35;
const PROJECT_LIST_TEXT_CHARACTER_REM = 0.42;
const PROJECT_LIST_HEADER_EXTRA_REM = 0.4;
const PROJECT_LIST_STATUS_BADGE_EXTRA_REM = 1.55;
const PROJECT_LIST_DATE_TIME_TEXT = "0000-00-00 00:00";
const PROJECT_LIST_NAME_WIDTH: ProjectTaskListColumnWidthBounds = { min: 12, autoMin: 24, max: 32 };
const PROJECT_LIST_COLUMN_WIDTHS = {
  status: { min: 7.5, max: 12 },
  start: { min: 7.75, max: 8.25 },
  due: { min: 7.75, max: 8.25 },
  priority: { min: 5.2, max: 6.8 },
  assignee: { min: 5.2, max: 7.2 },
  reviewer: { min: 5.2, max: 7.2 },
  estimate: { min: 5.2, max: 6.8 },
  scheduled: { min: 6.8, max: 11.2 },
  dependencies: { min: 6.8, max: 8.8 },
  custom: { min: 5.6, max: 11.2, headerMax: 16 },
} satisfies Record<string, ProjectTaskListColumnWidthBounds>;

function normalizeProjectTaskListGridInput(
  input: readonly ProjectTaskListColumn[] | ProjectTaskListGridInput,
): ProjectTaskListGridInput {
  return "columns" in input ? input : { columns: input };
}

function isProjectTaskListCoreColumn(value: string): value is ProjectCoreTaskListColumn {
  return PROJECT_TASK_LIST_COLUMNS.includes(value as ProjectCoreTaskListColumn);
}

function isProjectTaskListResizableColumn(
  value: string,
  customFieldIds?: ReadonlySet<string>,
): value is ProjectTaskListResizableColumn {
  if (value === "name" || isProjectTaskListCoreColumn(value)) return true;
  const customFieldId = customFieldIdFromTaskListColumn(value as ProjectTaskListColumn);
  if (!customFieldId) return false;
  return customFieldIds ? customFieldIds.has(customFieldId) : true;
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function projectTaskListColumnLabel(input: ProjectTaskListGridInput, column: ProjectTaskListColumn): string {
  return input.columnLabel?.(column) ?? column;
}

function projectTaskListTextWidthRem(text: string): number {
  const normalized = text.trim();
  if (!normalized) return 0;
  return PROJECT_LIST_TEXT_PADDING_REM + Array.from(normalized).length * PROJECT_LIST_TEXT_CHARACTER_REM;
}

function projectTaskListHeaderWidthRem(text: string, bounds: ProjectTaskListColumnWidthBounds): number {
  const normalized = text.trim();
  if (!normalized) return bounds.autoMin ?? bounds.min;
  const width = projectTaskListTextWidthRem(normalized) + PROJECT_LIST_HEADER_EXTRA_REM;
  return Math.min(bounds.headerMax ?? bounds.max, Math.max(bounds.autoMin ?? bounds.min, width));
}

function projectTaskListStatusWidthRem(text: string): number {
  const normalized = text.trim();
  if (!normalized) return PROJECT_LIST_COLUMN_WIDTHS.status.min;
  return projectTaskListTextWidthRem(normalized) + PROJECT_LIST_STATUS_BADGE_EXTRA_REM;
}

function clampProjectTaskListColumnWidth(width: number, bounds: ProjectTaskListColumnWidthBounds): number {
  return Math.min(bounds.max, Math.max(bounds.autoMin ?? bounds.min, width));
}

export function clampProjectTaskListManualColumnWidth(
  column: ProjectTaskListResizableColumn,
  width: number,
): number {
  const bounds = projectTaskListResizableColumnBounds(column);
  return Math.min(bounds.manualMax ?? 64, Math.max(bounds.min, width));
}

function formatProjectTaskListRem(value: number): string {
  const rounded = Math.round(value * 100) / 100;
  return `${rounded}rem`;
}

export function roundProjectTaskListColumnWidthRem(width: number): number {
  return Math.round(width * 100) / 100;
}

function projectTaskListWidthFromTexts(
  texts: readonly string[],
  bounds: ProjectTaskListColumnWidthBounds,
  textWidth: (text: string) => number = projectTaskListTextWidthRem,
): number {
  const estimatedWidth = texts.reduce((maxWidth, text) => Math.max(maxWidth, textWidth(text)), 0);
  return clampProjectTaskListColumnWidth(estimatedWidth, bounds);
}

function projectTaskListRawWidthFromTexts(
  texts: readonly string[],
  textWidth: (text: string) => number = projectTaskListTextWidthRem,
): number {
  return texts.reduce((maxWidth, text) => Math.max(maxWidth, textWidth(text)), 0);
}

function projectTaskListDateText(date: string | undefined, time: string | undefined): string {
  if (!date) return "";
  return time ? `${date} ${time}` : date;
}

function projectTaskListColumnBounds(column: ProjectTaskListColumn): ProjectTaskListColumnWidthBounds {
  if (column === "status") return PROJECT_LIST_COLUMN_WIDTHS.status;
  if (column === "start") return PROJECT_LIST_COLUMN_WIDTHS.start;
  if (column === "due") return PROJECT_LIST_COLUMN_WIDTHS.due;
  if (column === "priority") return PROJECT_LIST_COLUMN_WIDTHS.priority;
  if (column === "assignee") return PROJECT_LIST_COLUMN_WIDTHS.assignee;
  if (column === "reviewer") return PROJECT_LIST_COLUMN_WIDTHS.reviewer;
  if (column === "estimate") return PROJECT_LIST_COLUMN_WIDTHS.estimate;
  if (column === "scheduled") return PROJECT_LIST_COLUMN_WIDTHS.scheduled;
  if (column === "dependencies") return PROJECT_LIST_COLUMN_WIDTHS.dependencies;
  return PROJECT_LIST_COLUMN_WIDTHS.custom;
}

function projectTaskListResizableColumnBounds(column: ProjectTaskListResizableColumn): ProjectTaskListColumnWidthBounds {
  if (column === "name") return PROJECT_LIST_NAME_WIDTH;
  return projectTaskListColumnBounds(column);
}

function projectTaskListManualWidth(
  input: ProjectTaskListGridInput,
  column: ProjectTaskListResizableColumn,
): number | undefined {
  const width = input.columnWidths?.[column];
  return typeof width === "number" && Number.isFinite(width)
    ? clampProjectTaskListManualColumnWidth(column, width)
    : undefined;
}

function projectTaskListColumnTexts(input: ProjectTaskListGridInput, column: ProjectTaskListColumn): string[] {
  const tasks = input.tasks ?? [];
  const label = projectTaskListColumnLabel(input, column);
  if (column === "status") {
    return [label, ...(input.statuses ?? []).map((status) => status.name)];
  }
  if (column === "start") {
    return [
      label,
      PROJECT_LIST_DATE_TIME_TEXT,
      ...tasks.map((task) => projectTaskListDateText(task.startDate, task.startTime)),
    ];
  }
  if (column === "due") {
    return [
      label,
      PROJECT_LIST_DATE_TIME_TEXT,
      ...tasks.map((task) => projectTaskListDateText(task.dueDate, task.dueTime)),
    ];
  }
  if (column === "priority") {
    return [label, ...tasks.map((task) => input.priorityLabel?.(task.priority) ?? task.priority)];
  }
  if (column === "estimate") {
    return [
      label,
      ...tasks.map((task) =>
        task.estimateMinutes === undefined
          ? ""
          : input.estimateLabel?.(task.estimateMinutes) ?? String(task.estimateMinutes)
      ),
    ];
  }
  if (column === "scheduled") {
    return [label, ...tasks.map((task) => input.scheduledLabel?.(task.id) ?? "")];
  }
  if (column === "dependencies") {
    return [label];
  }
  const customFieldId = customFieldIdFromTaskListColumn(column);
  const customField = customFieldId
    ? input.customFields?.find((field) => field.id === customFieldId)
    : undefined;
  if (!customField || !input.customFieldDisplayValue) return [label];
  return [
    label,
    ...tasks.map((task) => input.customFieldDisplayValue?.(task, customField) ?? ""),
  ];
}

function projectTaskListNameColumnTexts(input: ProjectTaskListGridInput): string[] {
  return [
    input.nameLabel ?? "Name",
    ...(input.sectionLabels ?? []),
    ...(input.groupLabels ?? []),
    ...(input.tasks ?? []).map((task) => task.title),
  ];
}

function projectTaskListNameColumnWidth(input: ProjectTaskListGridInput, allowManualWidth = true): number {
  const manualWidth = projectTaskListManualWidth(input, "name");
  if (allowManualWidth && manualWidth !== undefined) return manualWidth;
  return projectTaskListWidthFromTexts(projectTaskListNameColumnTexts(input), PROJECT_LIST_NAME_WIDTH);
}

export function projectTaskListColumnWidthRem(
  column: ProjectTaskListColumn,
  input: ProjectTaskListGridInput = { columns: [] },
  allowManualWidth = true,
): number {
  const manualWidth = projectTaskListManualWidth(input, column);
  if (allowManualWidth && manualWidth !== undefined) return manualWidth;
  const bounds = projectTaskListColumnBounds(column);
  const headerWidth = projectTaskListHeaderWidthRem(projectTaskListColumnLabel(input, column), bounds);
  const textWidth = column === "status" ? projectTaskListStatusWidthRem : projectTaskListTextWidthRem;
  const contentWidth = projectTaskListWidthFromTexts(
    projectTaskListColumnTexts(input, column),
    bounds,
    textWidth,
  );
  return Math.max(headerWidth, contentWidth);
}

export function projectTaskListResizableColumnWidthRem(
  column: ProjectTaskListResizableColumn,
  input: ProjectTaskListGridInput,
  allowManualWidth = true,
): number {
  if (column === "name") return projectTaskListNameColumnWidth(input, allowManualWidth);
  return projectTaskListColumnWidthRem(column, input, allowManualWidth);
}

export function projectTaskListContentFitColumnWidthRem(
  column: ProjectTaskListResizableColumn,
  input: ProjectTaskListGridInput,
): number {
  const texts = column === "name" ? projectTaskListNameColumnTexts(input) : projectTaskListColumnTexts(input, column);
  const textWidth = column === "status" ? projectTaskListStatusWidthRem : projectTaskListTextWidthRem;
  return clampProjectTaskListManualColumnWidth(column, projectTaskListRawWidthFromTexts(texts, textWidth));
}

export function projectTaskListDoubleClickColumnWidthRem(
  column: ProjectTaskListResizableColumn,
  input: ProjectTaskListGridInput,
): number | undefined {
  const defaultWidth = projectTaskListResizableColumnWidthRem(column, input, false);
  const contentFitWidth = projectTaskListContentFitColumnWidthRem(column, input);
  return contentFitWidth > defaultWidth ? contentFitWidth : undefined;
}

function projectTaskListColumnResizeDraftWidths(
  widthsAtStart: ProjectTaskListColumnWidths,
  column: ProjectTaskListResizableColumn,
  widthRem: number,
): ProjectTaskListColumnWidths {
  return {
    ...widthsAtStart,
    [column]: roundProjectTaskListColumnWidthRem(clampProjectTaskListManualColumnWidth(column, widthRem)),
  };
}

export function startProjectTaskListColumnResize(input: {
  column: ProjectTaskListResizableColumn;
  pointerId: number;
  startClientX: number;
  startWidthRem: number;
  rootFontSizePx: number;
  widthsAtStart: ProjectTaskListColumnWidths;
}): ProjectTaskListColumnResizeGesture {
  const rootFontSizePx = Number.isFinite(input.rootFontSizePx) && input.rootFontSizePx > 0
    ? input.rootFontSizePx
    : 16;
  return {
    column: input.column,
    pointerId: input.pointerId,
    startClientX: input.startClientX,
    startWidthRem: input.startWidthRem,
    rootFontSizePx,
    widthsAtStart: input.widthsAtStart,
    draftWidths: projectTaskListColumnResizeDraftWidths(input.widthsAtStart, input.column, input.startWidthRem),
    moved: false,
  };
}

export function moveProjectTaskListColumnResize(
  gesture: ProjectTaskListColumnResizeGesture,
  clientX: number,
): ProjectTaskListColumnResizeGesture {
  const deltaPx = clientX - gesture.startClientX;
  const deltaRem = deltaPx / gesture.rootFontSizePx;
  return {
    ...gesture,
    draftWidths: projectTaskListColumnResizeDraftWidths(
      gesture.widthsAtStart,
      gesture.column,
      gesture.startWidthRem + deltaRem,
    ),
    moved: gesture.moved || Math.abs(deltaPx) >= 1,
  };
}

export function doubleClickProjectTaskListColumnResizeWidths(input: {
  column: ProjectTaskListResizableColumn;
  widths: ProjectTaskListColumnWidths;
  gridInput: ProjectTaskListGridInput;
}): ProjectTaskListColumnWidths {
  const nextWidths = { ...input.widths };
  const nextWidth = projectTaskListDoubleClickColumnWidthRem(input.column, input.gridInput);
  if (nextWidth === undefined) {
    delete nextWidths[input.column];
  } else {
    nextWidths[input.column] = roundProjectTaskListColumnWidthRem(nextWidth);
  }
  return nextWidths;
}

export function keyboardProjectTaskListColumnResizeWidths(input: {
  column: ProjectTaskListResizableColumn;
  direction: -1 | 1;
  wideStep?: boolean;
  widths: ProjectTaskListColumnWidths;
  gridInput: ProjectTaskListGridInput;
}): ProjectTaskListColumnWidths {
  const step = input.wideStep ? 2 : 0.5;
  const currentWidth = projectTaskListResizableColumnWidthRem(input.column, input.gridInput);
  return {
    ...input.widths,
    [input.column]: roundProjectTaskListColumnWidthRem(
      clampProjectTaskListManualColumnWidth(input.column, currentWidth + input.direction * step),
    ),
  };
}

export function projectTaskListColumnTrack(
  column: ProjectTaskListColumn,
  input: ProjectTaskListGridInput = { columns: [] },
): string {
  return formatProjectTaskListRem(projectTaskListColumnWidthRem(column, input));
}

export function projectTaskListGridColumnWidths(
  input: readonly ProjectTaskListColumn[] | ProjectTaskListGridInput,
): number[] {
  const normalizedInput = normalizeProjectTaskListGridInput(input);
  return [
    PROJECT_LIST_SELECTION_TRACK_REM,
    PROJECT_LIST_EXPAND_TRACK_REM,
    projectTaskListNameColumnWidth(normalizedInput),
    ...normalizedInput.columns.map((column) => projectTaskListColumnWidthRem(column, normalizedInput)),
    PROJECT_LIST_ADD_COLUMN_TRACK_REM,
  ];
}

export function projectTaskListGridTemplate(
  input: readonly ProjectTaskListColumn[] | ProjectTaskListGridInput,
): string {
  return projectTaskListGridColumnWidths(input)
    .map(formatProjectTaskListRem)
    .join(" ");
}

export function projectTaskListLeadingGridTemplate(
  input: readonly ProjectTaskListColumn[] | ProjectTaskListGridInput,
): string {
  const leadingTracks = projectTaskListGridColumnWidths(input)
    .slice(0, 2)
    .map(formatProjectTaskListRem);
  return [...leadingTracks, "minmax(0, 1fr)"].join(" ");
}

export function projectTaskListGridMinWidth(
  input: readonly ProjectTaskListColumn[] | ProjectTaskListGridInput,
): string {
  const remWidth = projectTaskListGridColumnWidths(input).reduce((total, width) => total + width, 0);
  return formatProjectTaskListRem(remWidth);
}

export function parseProjectTaskListColumnWidths(
  value: string | undefined,
  customFieldIds?: ReadonlySet<string>,
): ProjectTaskListColumnWidths {
  if (value === undefined) return {};
  let parsed: unknown;
  try {
    parsed = JSON.parse(value);
  } catch {
    return {};
  }
  const rawWidths = isRecord(parsed) && "widths" in parsed ? parsed.widths : parsed;
  if (!isRecord(rawWidths)) return {};
  const widths: ProjectTaskListColumnWidths = {};
  for (const [column, rawWidth] of Object.entries(rawWidths)) {
    if (!isProjectTaskListResizableColumn(column, customFieldIds)) continue;
    if (typeof rawWidth !== "number" || !Number.isFinite(rawWidth)) continue;
    widths[column] = clampProjectTaskListManualColumnWidth(column, rawWidth);
  }
  return widths;
}

export function taskListColumnWidthsPreferenceValue(widths: ProjectTaskListColumnWidths): string {
  const normalizedEntries = Object.entries(widths)
    .filter((entry): entry is [ProjectTaskListResizableColumn, number] =>
      isProjectTaskListResizableColumn(entry[0])
      && typeof entry[1] === "number"
      && Number.isFinite(entry[1])
    )
    .map(([column, width]) => [column, clampProjectTaskListManualColumnWidth(column, width)] as const)
    .sort(([firstColumn], [secondColumn]) => firstColumn.localeCompare(secondColumn));
  return JSON.stringify({ widths: Object.fromEntries(normalizedEntries) });
}

export function taskListColumnWidthsForProject(
  preferences: readonly ProjectViewPreference[],
  projectId: string | null | undefined,
  customFieldIds?: ReadonlySet<string>,
): ProjectTaskListColumnWidths {
  if (!projectId) return {};
  const preference = preferences.find((entry) =>
    entry.projectId === projectId
    && entry.viewId === "list"
    && entry.preferenceKey === TASK_LIST_COLUMN_WIDTHS_PREFERENCE_KEY
  );
  return parseProjectTaskListColumnWidths(preference?.preferenceValue, customFieldIds);
}

export function selectedProjectTaskIdsInView(
  selectedTaskIds: readonly string[],
  visibleTasks: readonly ProjectTask[],
): string[] {
  const visibleTaskIds = new Set(visibleTasks.map((task) => task.id));
  return selectedTaskIds.filter((taskId) => visibleTaskIds.has(taskId));
}

export function projectTaskDataFiltersActive(input: ProjectTaskFilterState): boolean {
  return input.search.trim().length > 0
    || input.statusFilter !== "all"
    || input.sectionFilter !== "all"
    || input.priorityFilter !== "all"
    || input.dueFilter !== "all"
    || input.scheduleFilter !== "all"
    || input.dependencyFilter !== "all"
    || input.tagFilter !== "all"
    || input.customFieldFilters.length > 0;
}

export function projectTaskFiltersActive(input: ProjectTaskFilterState): boolean {
  return projectTaskDataFiltersActive(input)
    || input.groupBy !== "section"
    || input.sortMode !== "manual"
    || input.sortDirection !== "asc";
}

export interface ProjectTaskActiveFilterChipInput extends ProjectTaskFilterState {
  sections: readonly ProjectSection[];
  tags: readonly ProjectTag[];
  customFields: readonly ProjectCustomField[];
  customFieldOptionsForField: (fieldId: string) => readonly ProjectCustomFieldOption[];
  normalizedDueRangeStart?: string;
  normalizedDueRangeEnd?: string;
  t: Translate;
}

export function projectTaskStatusFilterLabel(filter: ProjectTaskStatusFilter, t: Translate): string {
  if (filter === "open") return t("projects.filters.open");
  if (filter === "blocked") return t("projects.filters.blocked");
  if (filter === "done") return t("projects.filters.done");
  return t("projects.filters.allStatuses");
}

export function projectTaskDueFilterLabel(filter: ProjectTaskDueFilter, t: Translate): string {
  if (filter === "overdue") return t("projects.filters.overdue");
  if (filter === "today") return t("projects.filters.today");
  if (filter === "week") return t("projects.filters.thisWeek");
  if (filter === "none") return t("projects.filters.noDueDate");
  if (filter === "range") return t("projects.filters.dueRange");
  return t("projects.filters.allDueDates");
}

export function projectTaskScheduleFilterLabel(filter: ProjectTaskScheduleFilter, t: Translate): string {
  if (filter === "scheduled") return t("projects.filters.scheduled");
  if (filter === "unscheduled") return t("projects.filters.unscheduled");
  return t("projects.filters.allSchedule");
}

export function projectTaskDependencyFilterLabel(filter: ProjectTaskDependencyFilter, t: Translate): string {
  if (filter === "linked") return t("projects.filters.hasDependencies");
  if (filter === "blocked_by") return t("projects.filters.blockedByDependencies");
  if (filter === "blocking") return t("projects.filters.blockingDependencies");
  if (filter === "none") return t("projects.filters.noDependencies");
  return t("projects.filters.allDependencies");
}

export function projectTaskTagFilterLabel(
  filter: ProjectTaskTagFilter,
  tags: readonly ProjectTag[],
  t: Translate,
): string {
  if (filter === "all") return t("projects.filters.allTags");
  if (filter === "none") return t("projects.filters.noTags");
  return tags.find((tag) => tag.id === filter)?.name ?? t("projects.filters.allTags");
}

export function projectTaskSectionFilterLabel(
  filter: string | "all",
  sections: readonly ProjectSection[],
  t: Translate,
): string {
  if (filter === "all") return t("projects.filters.allSections");
  return sections.find((section) => section.id === filter)?.name ?? t("projects.filters.allSections");
}

export function projectTaskDueFilterChipLabel(input: {
  filter: ProjectTaskDueFilter;
  normalizedStart?: string;
  normalizedEnd?: string;
  t: Translate;
}): string {
  if (input.filter !== "range") return projectTaskDueFilterLabel(input.filter, input.t);
  const start = input.normalizedStart ?? input.t("projects.filters.dueRangeStart");
  const end = input.normalizedEnd ?? input.t("projects.filters.dueRangeEnd");
  return input.t("projects.filters.dueRangeChip", start, end);
}

export function projectTaskCustomFieldFilterLabel(
  filter: ProjectCustomFieldFilter,
  customFields: readonly ProjectCustomField[],
  customFieldOptionsForField: (fieldId: string) => readonly ProjectCustomFieldOption[],
  t: Translate,
): string {
  const field = customFields.find((entry) => entry.id === filter.fieldId);
  const fieldName = field?.name ?? t("projects.columns.customField");
  if (filter.mode === "filled") return t("projects.filters.customFieldChip", fieldName, t("projects.filters.filled"));
  if (filter.mode === "empty") return t("projects.filters.customFieldChip", fieldName, t("projects.filters.empty"));
  if (filter.mode === "checkbox") {
    return t(
      "projects.filters.customFieldChip",
      fieldName,
      filter.checked ? t("projects.customFields.checked") : t("projects.customFields.unchecked"),
    );
  }
  if (filter.mode === "option") {
    const optionName = customFields
      .flatMap((entry) => customFieldOptionsForField(entry.id))
      .find((option) => option.id === filter.optionId)?.name ?? t("projects.filters.allValues");
    return t("projects.filters.customFieldChip", fieldName, optionName);
  }
  return fieldName;
}

export function projectTaskActiveFilterChips(input: ProjectTaskActiveFilterChipInput): ProjectFilterChip[] {
  return deriveProjectFilterChips({
    search: input.search,
    statusLabel: input.statusFilter === "all" ? undefined : projectTaskStatusFilterLabel(input.statusFilter, input.t),
    sectionLabel: input.sectionFilter === "all"
      ? undefined
      : projectTaskSectionFilterLabel(input.sectionFilter, input.sections, input.t),
    priorityLabel: input.priorityFilter === "all" ? undefined : projectPriorityLabel(input.priorityFilter, input.t),
    dueLabel: input.dueFilter === "all"
      ? undefined
      : projectTaskDueFilterChipLabel({
        filter: input.dueFilter,
        normalizedStart: input.normalizedDueRangeStart,
        normalizedEnd: input.normalizedDueRangeEnd,
        t: input.t,
      }),
    scheduleLabel: input.scheduleFilter === "all"
      ? undefined
      : projectTaskScheduleFilterLabel(input.scheduleFilter, input.t),
    dependencyLabel: input.dependencyFilter === "all"
      ? undefined
      : projectTaskDependencyFilterLabel(input.dependencyFilter, input.t),
    tagFilterLabel: input.tagFilter === "all"
      ? undefined
      : projectTaskTagFilterLabel(input.tagFilter, input.tags, input.t),
    customFieldFilters: input.customFieldFilters,
    customFieldFilterLabel: (filter) =>
      projectTaskCustomFieldFilterLabel(filter, input.customFields, input.customFieldOptionsForField, input.t),
  });
}
