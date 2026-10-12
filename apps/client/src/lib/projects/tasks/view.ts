import type {
  ProjectCustomField,
  ProjectCustomFieldFilter,
  ProjectCustomFieldOption,
  ProjectCustomFieldValue,
  ProjectPriorityConfig,
  ProjectPriority,
  ProjectStatus,
  ProjectTask,
  ProjectTaskDependencyFilter,
  ProjectTaskDueFilter,
  ProjectTaskGroupMode,
  ProjectTaskTagFilter,
  ProjectTaskScheduleFilter,
  ProjectTaskSortDirection,
  ProjectTaskSortMode,
  ProjectTaskStatusFilter,
} from "$lib/projects/types";
import { customFieldIdFromCustomFieldReference } from "./list-columns";

export interface ProjectTaskViewInput {
  tasks: readonly ProjectTask[];
  statuses: readonly ProjectStatus[];
  priorities: readonly ProjectPriorityConfig[];
  customFields: readonly ProjectCustomField[];
  customFieldOptions: readonly ProjectCustomFieldOption[];
  customFieldValuesByTaskField: ReadonlyMap<string, ProjectCustomFieldValue>;
  customFieldOptionIdsByTaskField: ReadonlyMap<string, ReadonlySet<string>>;
  scheduledTaskIds: ReadonlySet<string>;
  nextScheduledStartByTaskId: ReadonlyMap<string, string>;
  taskTagIdsByTaskId: ReadonlyMap<string, ReadonlySet<string>>;
  dependencyBlockedTaskIds: ReadonlySet<string>;
  dependencyBlockingTaskIds: ReadonlySet<string>;
  today: string;
  weekEnd: string;
  search: string;
  statusFilter: ProjectTaskStatusFilter;
  sectionFilter: string | "all";
  priorityFilter: ProjectPriority | "all";
  dueFilter: ProjectTaskDueFilter;
  dueRangeStart?: string;
  dueRangeEnd?: string;
  scheduleFilter: ProjectTaskScheduleFilter;
  dependencyFilter: ProjectTaskDependencyFilter;
  tagFilter: ProjectTaskTagFilter;
  customFieldFilters: readonly ProjectCustomFieldFilter[];
  groupBy: ProjectTaskGroupMode;
  sortMode: ProjectTaskSortMode;
  sortDirection: ProjectTaskSortDirection;
  filtersApplied?: boolean;
}

export interface ProjectTaskViewResult {
  tasks: ProjectTask[];
  matchedTaskIds: ReadonlySet<string>;
  visibleTaskIds: ReadonlySet<string>;
  activeFilterCount: number;
}

export interface ProjectTaskListGroup {
  id: string;
  value: string;
  tasks: ProjectTask[];
}

export interface ProjectTaskListGroupInput {
  tasks: readonly ProjectTask[];
  statuses: readonly ProjectStatus[];
  priorities: readonly ProjectPriorityConfig[];
  scheduledTaskIds: ReadonlySet<string>;
  today: string;
  weekEnd: string;
  groupBy: Exclude<ProjectTaskGroupMode, "section">;
}

export function projectTaskCustomFieldKey(taskId: string, fieldId: string): string {
  return `${taskId}\u0000${fieldId}`;
}

function withDirection(value: number, direction: ProjectTaskSortDirection): number {
  if (value === 0) return 0;
  return direction === "asc" ? value : -value;
}

function compareText(a: string, b: string, direction: ProjectTaskSortDirection): number {
  return withDirection(a.localeCompare(b), direction);
}

function compareOptionalText(
  a: string | undefined,
  b: string | undefined,
  direction: ProjectTaskSortDirection,
): number {
  if (!a && !b) return 0;
  if (!a) return 1;
  if (!b) return -1;
  return compareText(a, b, direction);
}

function compareOptionalNumber(
  a: number | undefined,
  b: number | undefined,
  direction: ProjectTaskSortDirection,
): number {
  if (a === undefined && b === undefined) return 0;
  if (a === undefined) return 1;
  if (b === undefined) return -1;
  return withDirection(a - b, direction);
}

function manualSectionCompare(a: ProjectTask, b: ProjectTask): number {
  return a.sectionSortOrder - b.sectionSortOrder
    || a.statusSortOrder - b.statusSortOrder
    || a.createdAt.localeCompare(b.createdAt)
    || a.title.localeCompare(b.title);
}

export function manualStatusCompare(a: ProjectTask, b: ProjectTask): number {
  return a.statusSortOrder - b.statusSortOrder
    || a.sectionSortOrder - b.sectionSortOrder
    || a.createdAt.localeCompare(b.createdAt)
    || a.title.localeCompare(b.title);
}

function statusIndexById(statuses: readonly ProjectStatus[]): ReadonlyMap<string, number> {
  return new Map(statuses.map((status, index) => [status.id, index]));
}

function priorityIndexById(priorities: readonly ProjectPriorityConfig[]): ReadonlyMap<string, number> {
  return new Map(priorities.map((priority, index) => [priority.id, index]));
}

function comparePriorityOrder(
  firstPriorityId: string,
  secondPriorityId: string,
  indexByPriorityId: ReadonlyMap<string, number>,
  direction: ProjectTaskSortDirection,
): number {
  const firstIndex = indexByPriorityId.get(firstPriorityId);
  const secondIndex = indexByPriorityId.get(secondPriorityId);
  if (firstIndex === undefined && secondIndex === undefined) return 0;
  if (firstIndex === undefined) return 1;
  if (secondIndex === undefined) return -1;
  return direction === "desc"
    ? firstIndex - secondIndex
    : secondIndex - firstIndex;
}

function taskStatusMatches(
  task: ProjectTask,
  status: ProjectStatus | undefined,
  dependencyBlockedTaskIds: ReadonlySet<string>,
  filter: ProjectTaskStatusFilter,
): boolean {
  if (filter === "all") return true;
  if (filter === "open") return !status?.terminal;
  if (filter === "done") return status?.terminal === true;
  return status?.category === "blocked"
    || Boolean(task.blockerReason?.trim())
    || dependencyBlockedTaskIds.has(task.id);
}

function taskDueMatches(
  task: ProjectTask,
  status: ProjectStatus | undefined,
  filter: ProjectTaskDueFilter,
  today: string,
  weekEnd: string,
  rangeStart: string | undefined,
  rangeEnd: string | undefined,
): boolean {
  if (filter === "all") return true;
  if (filter === "none") return !task.dueDate;
  if (!task.dueDate) return false;
  if (filter === "overdue") return !status?.terminal && task.dueDate < today;
  if (filter === "today") return task.dueDate === today;
  if (filter === "range") {
    return (!rangeStart || task.dueDate >= rangeStart)
      && (!rangeEnd || task.dueDate <= rangeEnd);
  }
  return task.dueDate >= today && task.dueDate <= weekEnd;
}

function taskScheduleMatches(
  taskId: string,
  scheduledTaskIds: ReadonlySet<string>,
  filter: ProjectTaskScheduleFilter,
): boolean {
  if (filter === "all") return true;
  const scheduled = scheduledTaskIds.has(taskId);
  return filter === "scheduled" ? scheduled : !scheduled;
}

function taskDependencyMatches(
  taskId: string,
  blockedTaskIds: ReadonlySet<string>,
  blockingTaskIds: ReadonlySet<string>,
  filter: ProjectTaskDependencyFilter,
): boolean {
  if (filter === "all") return true;
  const blockedByDependency = blockedTaskIds.has(taskId);
  const blockingDependency = blockingTaskIds.has(taskId);
  if (filter === "blocked_by") return blockedByDependency;
  if (filter === "blocking") return blockingDependency;
  if (filter === "none") return !blockedByDependency && !blockingDependency;
  return blockedByDependency || blockingDependency;
}

function taskTagMatches(
  taskId: string,
  taskTagIdsByTaskId: ReadonlyMap<string, ReadonlySet<string>>,
  filter: ProjectTaskTagFilter,
): boolean {
  if (filter === "all") return true;
  const tagIds = taskTagIdsByTaskId.get(taskId) ?? new Set<string>();
  if (filter === "none") return tagIds.size === 0;
  return tagIds.has(filter);
}

function taskCustomFieldValue(
  input: ProjectTaskViewInput,
  taskId: string,
  fieldId: string,
): ProjectCustomFieldValue | undefined {
  return input.customFieldValuesByTaskField.get(projectTaskCustomFieldKey(taskId, fieldId));
}

function taskCustomFieldOptionIds(
  input: ProjectTaskViewInput,
  taskId: string,
  fieldId: string,
): ReadonlySet<string> {
  return input.customFieldOptionIdsByTaskField.get(projectTaskCustomFieldKey(taskId, fieldId))
    ?? new Set<string>();
}

function taskCustomFieldHasValue(
  input: ProjectTaskViewInput,
  task: ProjectTask,
  field: ProjectCustomField,
): boolean {
  if (field.fieldType === "select" || field.fieldType === "multi_select" || field.fieldType === "status") {
    return taskCustomFieldOptionIds(input, task.id, field.id).size > 0;
  }
  const value = taskCustomFieldValue(input, task.id, field.id);
  if (!value) return false;
  if (field.fieldType === "number") return value.numberValue !== undefined;
  if (field.fieldType === "date") return Boolean(value.dateValue);
  if (field.fieldType === "checkbox") return value.checkboxValue !== undefined;
  return Boolean(value.textValue?.trim());
}

function taskCustomFieldMatches(
  input: ProjectTaskViewInput,
  task: ProjectTask,
  filter: ProjectCustomFieldFilter,
): boolean {
  const field = input.customFields.find((entry) => entry.id === filter.fieldId);
  if (!field) return true;
  if (filter.mode === "filled") return taskCustomFieldHasValue(input, task, field);
  if (filter.mode === "empty") return !taskCustomFieldHasValue(input, task, field);
  if (filter.mode === "checkbox") {
    if (field.fieldType !== "checkbox") return true;
    return taskCustomFieldValue(input, task.id, field.id)?.checkboxValue === filter.checked;
  }
  if (filter.mode !== "option") return true;
  if (field.fieldType !== "select" && field.fieldType !== "multi_select" && field.fieldType !== "status") return true;
  return taskCustomFieldOptionIds(input, task.id, field.id).has(filter.optionId);
}

function taskCustomFieldsMatch(input: ProjectTaskViewInput, task: ProjectTask): boolean {
  return input.customFieldFilters.every((filter) => taskCustomFieldMatches(input, task, filter));
}

function taskSearchMatches(task: ProjectTask, search: string): boolean {
  if (!search) return true;
  return task.title.toLowerCase().includes(search)
    || task.description.toLowerCase().includes(search);
}

function taskMatches(input: ProjectTaskViewInput, task: ProjectTask): boolean {
  const normalizedSearch = input.search.trim().toLowerCase();
  const status = input.statuses.find((entry) => entry.id === task.statusId);
  return taskSearchMatches(task, normalizedSearch)
    && taskStatusMatches(task, status, input.dependencyBlockedTaskIds, input.statusFilter)
    && (input.sectionFilter === "all" || task.sectionId === input.sectionFilter)
    && (input.priorityFilter === "all" || task.priority === input.priorityFilter)
    && taskDueMatches(
      task,
      status,
      input.dueFilter,
      input.today,
      input.weekEnd,
      input.dueRangeStart,
      input.dueRangeEnd,
    )
    && taskScheduleMatches(task.id, input.scheduledTaskIds, input.scheduleFilter)
    && taskDependencyMatches(
      task.id,
      input.dependencyBlockedTaskIds,
      input.dependencyBlockingTaskIds,
      input.dependencyFilter,
    )
    && taskTagMatches(task.id, input.taskTagIdsByTaskId, input.tagFilter)
    && taskCustomFieldsMatch(input, task);
}

function activeFilterCount(input: ProjectTaskViewInput): number {
  return [
    input.search.trim().length > 0,
    input.statusFilter !== "all",
    input.sectionFilter !== "all",
    input.priorityFilter !== "all",
    input.dueFilter !== "all",
    input.scheduleFilter !== "all",
    input.dependencyFilter !== "all",
    input.tagFilter !== "all",
    input.customFieldFilters.length > 0,
    input.sortMode !== "manual" || input.sortDirection !== "asc",
  ].filter(Boolean).length;
}

function customFieldOptionNamesForTask(
  task: ProjectTask,
  field: ProjectCustomField,
  input: ProjectTaskViewInput,
): string | undefined {
  const selectedOptionIds = taskCustomFieldOptionIds(input, task.id, field.id);
  if (selectedOptionIds.size === 0) return undefined;
  const names = input.customFieldOptions
    .filter((option) => option.fieldId === field.id && selectedOptionIds.has(option.id))
    .sort((a, b) => a.sortOrder - b.sortOrder || a.name.localeCompare(b.name))
    .map((option) => option.name);
  return names.length > 0 ? names.join(", ") : undefined;
}

function customFieldSortText(
  task: ProjectTask,
  field: ProjectCustomField,
  input: ProjectTaskViewInput,
): string | undefined {
  if (field.fieldType === "select" || field.fieldType === "multi_select" || field.fieldType === "status") {
    return customFieldOptionNamesForTask(task, field, input);
  }
  const value = taskCustomFieldValue(input, task.id, field.id);
  if (!value) return undefined;
  if (field.fieldType === "date") return value.dateValue || undefined;
  return value.textValue?.trim() || undefined;
}

function compareCustomFieldSortValues(
  a: ProjectTask,
  b: ProjectTask,
  field: ProjectCustomField,
  input: ProjectTaskViewInput,
): number {
  if (field.fieldType === "number") {
    return compareOptionalNumber(
      taskCustomFieldValue(input, a.id, field.id)?.numberValue,
      taskCustomFieldValue(input, b.id, field.id)?.numberValue,
      input.sortDirection,
    );
  }
  if (field.fieldType === "checkbox") {
    const aValue = taskCustomFieldValue(input, a.id, field.id)?.checkboxValue;
    const bValue = taskCustomFieldValue(input, b.id, field.id)?.checkboxValue;
    return compareOptionalNumber(
      aValue === undefined ? undefined : Number(aValue),
      bValue === undefined ? undefined : Number(bValue),
      input.sortDirection,
    );
  }
  return compareOptionalText(
    customFieldSortText(a, field, input),
    customFieldSortText(b, field, input),
    input.sortDirection,
  );
}

function compareTasks(
  a: ProjectTask,
  b: ProjectTask,
  input: ProjectTaskViewInput,
  indexByStatusId: ReadonlyMap<string, number>,
  indexByPriorityId: ReadonlyMap<string, number>,
): number {
  if (input.sortMode === "manual") {
    return withDirection(manualSectionCompare(a, b), input.sortDirection);
  }
  if (input.sortMode === "title") return compareText(a.title, b.title, input.sortDirection) || manualSectionCompare(a, b);
  if (input.sortMode === "start") return compareOptionalText(a.startDate, b.startDate, input.sortDirection)
    || compareOptionalText(a.startTime, b.startTime, input.sortDirection) || manualSectionCompare(a, b);
  if (input.sortMode === "status") {
    return withDirection(
      (indexByStatusId.get(a.statusId) ?? Number.MAX_SAFE_INTEGER)
        - (indexByStatusId.get(b.statusId) ?? Number.MAX_SAFE_INTEGER),
      input.sortDirection,
    ) || manualStatusCompare(a, b);
  }
  if (input.sortMode === "section") {
    return withDirection(a.sectionSortOrder - b.sectionSortOrder, input.sortDirection)
      || manualSectionCompare(a, b);
  }
  if (input.sortMode === "priority") {
    return comparePriorityOrder(a.priority, b.priority, indexByPriorityId, input.sortDirection)
      || manualSectionCompare(a, b);
  }
  if (input.sortMode === "due") {
    return compareOptionalText(a.dueDate, b.dueDate, input.sortDirection)
      || compareOptionalText(a.dueTime, b.dueTime, input.sortDirection)
      || manualSectionCompare(a, b);
  }
  if (input.sortMode === "scheduled") {
    return compareOptionalText(
      input.nextScheduledStartByTaskId.get(a.id),
      input.nextScheduledStartByTaskId.get(b.id),
      input.sortDirection,
    ) || manualSectionCompare(a, b);
  }
  if (input.sortMode === "created") {
    return compareText(a.createdAt, b.createdAt, input.sortDirection)
      || manualSectionCompare(a, b);
  }
  if (input.sortMode === "updated") {
    return compareText(a.updatedAt, b.updatedAt, input.sortDirection)
      || manualSectionCompare(a, b);
  }
  const customFieldId = customFieldIdFromCustomFieldReference(input.sortMode);
  if (customFieldId) {
    const field = input.customFields.find((entry) => entry.id === customFieldId);
    if (field) {
      return compareCustomFieldSortValues(a, b, field, input)
        || manualSectionCompare(a, b);
    }
    return manualSectionCompare(a, b);
  }
  if (input.sortMode === "estimate") {
    return compareOptionalNumber(a.estimateMinutes, b.estimateMinutes, input.sortDirection)
      || manualSectionCompare(a, b);
  }
  return manualSectionCompare(a, b);
}

export function buildProjectTaskView(input: ProjectTaskViewInput): ProjectTaskViewResult {
  const matchedTaskIds = new Set(
    input.tasks.filter((task) => input.filtersApplied || taskMatches(input, task)).map((task) => task.id),
  );
  const visibleTaskIds = new Set(matchedTaskIds);
  for (const task of input.tasks) {
    if (task.parentTaskId && matchedTaskIds.has(task.id)) {
      visibleTaskIds.add(task.parentTaskId);
    }
  }
  const indexByStatusId = statusIndexById(input.statuses);
  const indexByPriorityId = priorityIndexById(input.priorities);
  const tasks = input.tasks
    .filter((task) => visibleTaskIds.has(task.id))
    .sort((a, b) => compareTasks(a, b, input, indexByStatusId, indexByPriorityId));
  return {
    tasks,
    matchedTaskIds,
    visibleTaskIds,
    activeFilterCount: activeFilterCount(input),
  };
}

function groupedTopLevelTasksByValue(
  tasks: readonly ProjectTask[],
  orderedValues: readonly string[],
  valueForTask: (task: ProjectTask) => string,
): ProjectTaskListGroup[] {
  const groupsByValue = new Map<string, ProjectTask[]>();
  for (const task of tasks) {
    if (task.parentTaskId) continue;
    const value = valueForTask(task);
    const valueTasks = groupsByValue.get(value) ?? [];
    valueTasks.push(task);
    groupsByValue.set(value, valueTasks);
  }

  const groups = orderedValues
    .map((value) => ({ id: value, value, tasks: groupsByValue.get(value) ?? [] }));

  const orderedValueSet = new Set(orderedValues);
  const extraGroups = Array.from(groupsByValue.entries())
    .filter(([value]) => !orderedValueSet.has(value))
    .sort(([a], [b]) => a.localeCompare(b))
    .map(([value, groupTasks]) => ({ id: value, value, tasks: groupTasks }));

  return [...groups, ...extraGroups];
}

function statusForTaskId(
  statusesById: ReadonlyMap<string, ProjectStatus>,
  task: ProjectTask,
): ProjectStatus | undefined {
  return statusesById.get(task.statusId);
}

function dueGroupValue(
  task: ProjectTask,
  status: ProjectStatus | undefined,
  today: string,
  weekEnd: string,
): string {
  if (!task.dueDate) return "none";
  if (task.dueDate < today) return status?.terminal ? "earlier" : "overdue";
  if (task.dueDate === today) return "today";
  if (task.dueDate <= weekEnd) return "week";
  return "later";
}

export function buildProjectTaskListGroups(input: ProjectTaskListGroupInput): ProjectTaskListGroup[] {
  if (input.groupBy === "status") {
    const orderedStatusIds = input.statuses.map((status) => status.id);
    return groupedTopLevelTasksByValue(input.tasks, orderedStatusIds, (task) => task.statusId);
  }

  if (input.groupBy === "priority") {
    return groupedTopLevelTasksByValue(
      input.tasks,
      input.priorities.map((priority) => priority.id),
      (task) => task.priority,
    );
  }

  if (input.groupBy === "due") {
    const statusesById = new Map(input.statuses.map((status) => [status.id, status]));
    return groupedTopLevelTasksByValue(
      input.tasks,
      ["overdue", "today", "week", "later", "earlier", "none"],
      (task) => dueGroupValue(
        task,
        statusForTaskId(statusesById, task),
        input.today,
        input.weekEnd,
      ),
    );
  }

  return groupedTopLevelTasksByValue(
    input.tasks,
    ["scheduled", "unscheduled"],
    (task) => input.scheduledTaskIds.has(task.id) ? "scheduled" : "unscheduled",
  );
}
