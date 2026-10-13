import { Temporal } from "@js-temporal/polyfill";
import { untrack } from "svelte";
import { formatCalendarDate } from "$lib/calendar/utils";
import type { CalendarEvent } from "$lib/calendar/types";
import type { getCalendar } from "$lib/stores/calendar.svelte";
import type { getProjects } from "$lib/stores/projects.svelte";
import {
  PROJECT_TASK_FILTER_DEFAULTS,
  projectTaskDataFiltersActive,
  taskListColumnWidthsForProject,
  type ProjectTaskFilterState,
  type ProjectTaskListColumnWidths,
} from "$lib/projects/list/view";
import {
  customFieldIdFromCustomFieldReference,
  customFieldIdFromTaskListColumn,
  customTaskListColumn,
  DEFAULT_TASK_LIST_COLUMNS,
  taskListColumnsForProject,
  taskListColumnsMatch,
} from "$lib/projects/tasks/list-columns";
import {
  buildProjectTaskListGroups,
  buildProjectTaskView,
  projectTaskCustomFieldKey,
} from "$lib/projects/tasks/view";
import { deriveProjectListColumnControls, toggleProjectListColumn } from "$lib/projects/toolbar";
import { collectionMoveItem } from "$lib/components/collections/column-reorder";
import {
  PROJECT_TASK_LIST_COLUMNS,
  type ProjectCustomFieldFilter,
  type ProjectPriority,
  type ProjectTask,
  type ProjectTaskDependencyFilter,
  type ProjectTaskDueFilter,
  type ProjectTaskGroupMode,
  type ProjectTaskListColumn,
  type ProjectTaskScheduleFilter,
  type ProjectTaskSortDirection,
  type ProjectTaskSortMode,
  type ProjectTaskStatusFilter,
  type ProjectTaskTagFilter,
  type ProjectTaskViewRequest,
} from "$lib/projects/types";
import { projectCalendarEventRootId } from "$lib/projects/scheduling/schedule";
import type { Translate } from "$lib/i18n/translator.svelte";
import type { ProjectCustomFieldValue } from "$lib/projects/types";
import { defaultProjectListPresentation, parseProjectListPresentation, projectListPresentationForProject, type ProjectListPresentation } from "$lib/projects/list/presentation";
import type { ProjectCustomField, ProjectCustomFieldType } from "$lib/projects/types";

type ProjectsStore = ReturnType<typeof getProjects>;
type CalendarStore = ReturnType<typeof getCalendar>;

export interface ProjectTaskQueryControllerInput {
  projects: ProjectsStore;
  calendar: CalendarStore;
  translate: Translate;
}

/** Owns the canonical Projects task query and both backend and local projections. */
export class ProjectTaskQueryController {
  readonly #projects: ProjectsStore;
  readonly #calendar: CalendarStore;
  readonly #translate: ProjectTaskQueryControllerInput["translate"];
  #calculationRequestKey = $state<string | null>(null);
  #calculationRequestGeneration = 0;
  #lastTaskViewLoadKey: string | null = null;
  #presentationWriteGeneration = 0;
  #presentationWriteProjectId = $state<string | null>(null);
  #presentationWriteError = $state<{ projectId: string; message: string } | null>(null);
  #propertyWriteGeneration = 0;
  #propertyWriteProjectId = $state<string | null>(null);
  #propertyWriteError = $state<{ projectId: string; message: string } | null>(null);
  #listColumnWriteGeneration = 0;
  #listColumnWriteProjectId = $state<string | null>(null);
  #listColumnWriteError = $state<{ projectId: string; message: string } | null>(null);

  showInactiveSections = $state(false);
  showArchivedTasks = $state(false);
  search = $state("");
  statusFilter = $state<ProjectTaskStatusFilter>("all");
  sectionFilter = $state<string | "all">("all");
  priorityFilter = $state<ProjectPriority | "all">("all");
  dueFilter = $state<ProjectTaskDueFilter>("all");
  dueRangeStart = $state("");
  dueRangeEnd = $state("");
  scheduleFilter = $state<ProjectTaskScheduleFilter>("all");
  dependencyFilter = $state<ProjectTaskDependencyFilter>("all");
  tagFilter = $state<ProjectTaskTagFilter>("all");
  customFieldFilters = $state<ProjectCustomFieldFilter[]>([]);
  groupBy = $state<ProjectTaskGroupMode>("section");
  sortMode = $state<ProjectTaskSortMode>("manual");
  sortDirection = $state<ProjectTaskSortDirection>("asc");
  listColumns = $state<ProjectTaskListColumn[]>([...DEFAULT_TASK_LIST_COLUMNS]);
  listColumnWidths = $state<ProjectTaskListColumnWidths>({});
  listPresentation = $state<ProjectListPresentation>(defaultProjectListPresentation());
  readonly today = Temporal.Now.plainDateISO().toString();

  constructor(input: ProjectTaskQueryControllerInput) {
    this.#projects = input.projects;
    this.#calendar = input.calendar;
    this.#translate = input.translate;
  }

  get projectId(): string | null {
    return this.#projects.selectedProject?.id ?? null;
  }

  get listColumnsSaving(): boolean {
    return this.projectId !== null && this.#listColumnWriteProjectId === this.projectId;
  }

  get listColumnsError(): string | null {
    return this.#listColumnWriteError?.projectId === this.projectId
      ? this.#listColumnWriteError.message : null;
  }

  get presentationSaving(): boolean { return this.projectId !== null && this.#presentationWriteProjectId === this.projectId; }
  get presentationError(): string | null { return this.#presentationWriteError?.projectId === this.projectId ? this.#presentationWriteError.message : null; }
  get propertySaving(): boolean { return this.projectId !== null && this.#propertyWriteProjectId === this.projectId; }
  get propertyError(): string | null { return this.#propertyWriteError?.projectId === this.projectId ? this.#propertyWriteError.message : null; }

  get allSections() {
    return this.#projects.sectionsForProjectIncludingInactive(this.projectId);
  }

  get sections() {
    return this.showInactiveSections
      ? this.allSections
      : this.#projects.sectionsForProject(this.projectId);
  }

  get inactiveSectionCount(): number {
    return this.allSections.filter((section) => Boolean(section.hiddenAt || section.archivedAt)).length;
  }

  get statuses() {
    return this.#projects.statusesForProject(this.projectId);
  }

  get priorities() {
    return this.#projects.prioritiesForProject(this.projectId);
  }

  get projectTags() {
    return this.#projects.tagsForProject(this.projectId);
  }

  get customFields() {
    return this.#projects.customFieldsForProject(this.projectId);
  }

  get activeTasks(): ProjectTask[] {
    const sectionIds = new Set(this.sections.map((section) => section.id));
    return this.#projects.tasksForProject(this.projectId)
      .filter((task) => sectionIds.has(task.sectionId));
  }

  get tasksIncludingArchived(): ProjectTask[] {
    const sectionIds = new Set(this.sections.map((section) => section.id));
    return this.#projects.tasksForProjectIncludingArchived(this.projectId)
      .filter((task) => sectionIds.has(task.sectionId));
  }

  get allTasks(): ProjectTask[] {
    return this.showArchivedTasks ? this.tasksIncludingArchived : this.activeTasks;
  }

  get archivedTaskCount(): number {
    return this.#projects.taskViewPage?.archivedCount
      ?? this.tasksIncludingArchived.filter((task) => Boolean(task.archivedAt)).length;
  }

  /** Expose complete-result reductions only after the current query finishes successfully. */
  get columnCalculations() {
    const request = this.request();
    const page = this.#projects.taskViewPage;
    return request?.view === "list" && page?.projectId === request.projectId && page.view === "list"
      && !this.#projects.taskViewLoading && !this.#projects.taskViewError
      && this.#calculationRequestKey === this.#taskQueryRevisionKey(request) ? page.columnCalculations : undefined;
  }

  get loadError(): string | null { return this.#projects.taskViewError ?? null; }
  get loading(): boolean { return this.#projects.taskViewLoading; }

  get taskIds(): Set<string> {
    return new Set(this.allTasks.map((task) => task.id));
  }

  get filterState(): ProjectTaskFilterState {
    return {
      search: this.search,
      statusFilter: this.statusFilter,
      sectionFilter: this.sectionFilter,
      priorityFilter: this.priorityFilter,
      dueFilter: this.dueFilter,
      dueRangeStart: normalizeProjectFilterDate(this.dueRangeStart) ?? "",
      dueRangeEnd: normalizeProjectFilterDate(this.dueRangeEnd) ?? "",
      scheduleFilter: this.scheduleFilter,
      dependencyFilter: this.dependencyFilter,
      tagFilter: this.tagFilter,
      customFieldFilters: this.customFieldFilters,
      groupBy: this.groupBy,
      sortMode: this.sortMode,
      sortDirection: this.sortDirection,
    };
  }

  get weekEnd(): string {
    return Temporal.PlainDate.from(this.today).add({ days: 7 }).toString();
  }

  get allEvents(): CalendarEvent[] {
    const projectId = this.projectId;
    if (!projectId) return [];
    return this.#calendar.sourceEvents
      .filter((event) => event.projectId === projectId)
      .sort((first, second) => first.start.localeCompare(second.start));
  }

  get scheduledTaskIds(): Set<string> {
    return new Set(this.#projects.eventLinks
      .filter((link) => link.linkKind === "scheduled")
      .map((link) => link.taskId));
  }

  get nextScheduledStartByTaskId(): Map<string, string> {
    const eventsById = new Map(this.allEvents.map((event) => [event.id, event]));
    const now = formatCalendarDate(new Date());
    const startsByTaskId = new Map<string, string[]>();
    for (const link of this.#projects.eventLinks) {
      if (link.linkKind !== "scheduled" || !this.taskIds.has(link.taskId)) continue;
      const start = eventsById.get(link.eventId)?.start;
      if (!start) continue;
      const starts = startsByTaskId.get(link.taskId) ?? [];
      starts.push(start);
      startsByTaskId.set(link.taskId, starts);
    }
    const result = new Map<string, string>();
    for (const [taskId, starts] of startsByTaskId) {
      starts.sort((first, second) => first.localeCompare(second));
      result.set(taskId, starts.find((start) => start >= now) ?? starts[0]);
    }
    return result;
  }

  readonly localView = $derived.by(() => {
    const taskIds = this.taskIds;
    const valueByTaskField = new Map<string, ProjectCustomFieldValue>();
    for (const value of this.#projects.customFieldValues) {
      if (taskIds.has(value.taskId)) {
        valueByTaskField.set(projectTaskCustomFieldKey(value.taskId, value.fieldId), value);
      }
    }
    const optionIdsByTaskField = new Map<string, Set<string>>();
    for (const value of this.#projects.customFieldOptionValues) {
      if (!taskIds.has(value.taskId)) continue;
      const key = projectTaskCustomFieldKey(value.taskId, value.fieldId);
      const optionIds = optionIdsByTaskField.get(key) ?? new Set<string>();
      optionIds.add(value.optionId);
      optionIdsByTaskField.set(key, optionIds);
    }
    const tagIdsByTaskId = new Map<string, Set<string>>();
    for (const link of this.#projects.taskTagLinks) {
      if (!taskIds.has(link.taskId)) continue;
      const tagIds = tagIdsByTaskId.get(link.taskId) ?? new Set<string>();
      tagIds.add(link.tagId);
      tagIdsByTaskId.set(link.taskId, tagIds);
    }
    const blocked = new Set(this.#projects.dependencies
      .filter((dependency) => taskIds.has(dependency.blockedTaskId))
      .map((dependency) => dependency.blockedTaskId));
    const blocking = new Set(this.#projects.dependencies
      .filter((dependency) => taskIds.has(dependency.blockingTaskId))
      .map((dependency) => dependency.blockingTaskId));
    return buildProjectTaskView({
      tasks: this.allTasks,
      statuses: this.statuses,
      priorities: this.priorities,
      scheduledTaskIds: this.scheduledTaskIds,
      nextScheduledStartByTaskId: this.nextScheduledStartByTaskId,
      taskTagIdsByTaskId: tagIdsByTaskId,
      dependencyBlockedTaskIds: blocked,
      dependencyBlockingTaskIds: blocking,
      today: this.today,
      weekEnd: this.weekEnd,
      ...this.filterState,
      customFieldFilters: [...this.customFieldFilters],
      filtersApplied: true,
      customFields: this.customFields,
      customFieldOptions: this.#projects.customFieldOptions,
      customFieldValuesByTaskField: valueByTaskField,
      customFieldOptionIdsByTaskField: optionIdsByTaskField,
    });
  });

  get tasks(): ProjectTask[] {
    return this.localView.tasks;
  }

  get listGroups() {
    if (this.groupBy === "section") return [];
    return buildProjectTaskListGroups({
      tasks: this.tasks,
      statuses: this.statuses,
      priorities: this.priorities,
      scheduledTaskIds: this.scheduledTaskIds,
      today: this.today,
      weekEnd: this.weekEnd,
      groupBy: this.groupBy,
    });
  }

  get matchingTaskCount(): number {
    return this.#projects.taskViewPage?.matchedCount ?? this.localView.matchedTaskIds.size;
  }

  get activeFilterCount(): number {
    return this.localView.activeFilterCount;
  }

  get filtersActive(): boolean {
    return this.activeFilterCount > 0;
  }

  get filterControlsActive(): boolean {
    return this.filtersActive || this.showArchivedTasks || this.showInactiveSections;
  }

  get groupingActive(): boolean {
    return this.groupBy !== "section";
  }

  get customizeActive(): boolean {
    return !taskListColumnsMatch(this.listColumns, DEFAULT_TASK_LIST_COLUMNS)
      || JSON.stringify(this.listPresentation) !== JSON.stringify(defaultProjectListPresentation());
  }

  get dataFiltersActive(): boolean {
    return projectTaskDataFiltersActive(this.filterState);
  }

  get matchedEventIds(): Set<string> {
    return new Set(this.#projects.activeView === "calendar"
      ? this.#projects.taskViewPage?.matchedEventIds ?? []
      : this.#projects.eventLinks
        .filter((link) => this.localView.matchedTaskIds.has(link.taskId))
        .map((link) => link.eventId));
  }

  eventMatches(event: CalendarEvent): boolean {
    if (!this.projectId || event.projectId !== this.projectId) return false;
    if (!this.dataFiltersActive) return true;
    return this.matchedEventIds.has(event.id)
      || this.matchedEventIds.has(projectCalendarEventRootId(event));
  }

  get columnControls() {
    const available = [
      ...PROJECT_TASK_LIST_COLUMNS,
      ...this.customFields.map((field) => customTaskListColumn(field.id)),
    ];
    return deriveProjectListColumnControls(available, this.listColumns, (column) => this.columnLabel(column));
  }

  columnLabel(column: ProjectTaskListColumn): string {
    const customFieldId = customFieldIdFromTaskListColumn(column);
    if (customFieldId) {
      return this.customFields.find((field) => field.id === customFieldId)?.name
        ?? this.#translate("projects.columns.customField");
    }
    if (column === "priority") return this.#translate("projects.columns.priority");
    if (column === "estimate") return this.#translate("projects.columns.estimate");
    if (column === "start") return this.#translate("projects.columns.start");
    if (column === "due") return this.#translate("projects.columns.due");
    if (column === "scheduled") return this.#translate("projects.columns.scheduled");
    if (column === "dependencies") return this.#translate("projects.columns.dependencies");
    if (column === "assignee") return this.#translate("projects.columns.assignee");
    if (column === "reviewer") return this.#translate("projects.columns.reviewer");
    return this.#translate("projects.columns.status");
  }

  request(): ProjectTaskViewRequest | null {
    const projectId = this.projectId;
    const view = this.#projects.activeView;
    if (!projectId) return null;
    return {
      projectId,
      view,
      pageSize: view === "kanban" ? 50 : view === "gantt" ? 10_000 : 100,
      columnCursors: {},
      showArchived: this.showArchivedTasks,
      visibleSectionIds: this.sections.map((section) => section.id),
      search: this.search,
      statusFilter: this.statusFilter,
      sectionFilter: this.sectionFilter,
      priorityFilter: this.priorityFilter,
      dueFilter: this.dueFilter,
      dueRangeStart: normalizeProjectFilterDate(this.dueRangeStart) ?? "",
      dueRangeEnd: normalizeProjectFilterDate(this.dueRangeEnd) ?? "",
      scheduleFilter: this.scheduleFilter,
      dependencyFilter: this.dependencyFilter,
      tagFilter: this.tagFilter,
      customFieldFilters: [...this.customFieldFilters],
      sortMode: this.sortMode,
      sortDirection: this.sortDirection,
      today: this.today,
      weekEnd: this.weekEnd,
      candidateEventIds: view === "calendar" ? this.allEvents.map((event) => event.id) : [],
    };
  }

  /** Distinguish committed writes from page merges, which must not trigger another identical query. */
  #taskQueryRevisionKey(request: ProjectTaskViewRequest): string {
    return JSON.stringify([request, this.#projects.taskMutationRevision ?? 0]);
  }

  /** Reload changed queries and committed mutations once while keeping selected details retained. */
  loadCurrent(selectedTaskIds: readonly string[], selectedTaskId: string | null): void {
    const request = this.request();
    if (!request || !this.#projects.isProjectDataLoaded(request.projectId)) return;
    const retained = selectedTaskId ? [...selectedTaskIds, selectedTaskId] : [...selectedTaskIds];
    const queryKey = this.#taskQueryRevisionKey(request);
    const loadKey = JSON.stringify([queryKey, [...new Set(retained)].sort()]);
    if (this.#lastTaskViewLoadKey === loadKey) return;
    this.#lastTaskViewLoadKey = loadKey;
    const generation = ++this.#calculationRequestGeneration;
    void this.#projects.loadTaskView(request, false, retained).then(() => {
      if (generation === this.#calculationRequestGeneration) this.#calculationRequestKey = queryKey;
    }).catch((error) => {
      if (generation === this.#calculationRequestGeneration) this.#lastTaskViewLoadKey = null;
      console.error(`load Project ${request.view} task window failed`, error);
    });
  }

  /** Retry an explicit failed read even when its canonical query is unchanged. */
  retryCurrent(selectedTaskIds: readonly string[], selectedTaskId: string | null): void {
    if (this.loading) return;
    this.#lastTaskViewLoadKey = null;
    this.loadCurrent(selectedTaskIds, selectedTaskId);
  }

  loadNextList(selectedTaskIds: readonly string[]): void {
    const request = this.request();
    const cursor = this.#projects.taskViewPage?.nextCursor;
    if (!request || request.view !== "list" || !cursor || this.#projects.taskViewLoading) return;
    void this.#projects.loadTaskView({ ...request, cursor }, true, [...selectedTaskIds]).catch((error) => {
      console.error("load next Project list task page failed", error);
    });
  }

  loadNextKanban(selectedTaskIds: readonly string[]): void {
    const request = this.request();
    const page = this.#projects.taskViewPage;
    if (!request || request.view !== "kanban" || !page || this.#projects.taskViewLoading) return;
    const columnCursors = Object.fromEntries(page.columnCounts
      .filter((column) => column.nextCursor)
      .map((column) => [column.statusId, column.nextCursor as string]));
    if (Object.keys(columnCursors).length === 0) return;
    void this.#projects.loadTaskView({ ...request, columnCursors }, true, [...selectedTaskIds]).catch((error) => {
      console.error("load next Project Kanban task page failed", error);
    });
  }

  repairDisappearingFields(): void {
    const fieldIds = new Set(this.customFields.map((field) => field.id));
    const optionIds = new Set(this.customFields.flatMap((field) =>
      this.#projects.customFieldOptionsForField(field.id).map((option) => option.id)));
    const repaired = repairProjectTaskQueryReferences({
      sectionFilter: this.sectionFilter,
      tagFilter: this.tagFilter,
      customFieldFilters: this.customFieldFilters,
      sortMode: this.sortMode,
      sortDirection: this.sortDirection,
      sectionIds: new Set(this.sections.map((section) => section.id)),
      tagIds: new Set(this.projectTags.map((tag) => tag.id)),
      fieldIds,
      optionIds,
    });
    if (this.sectionFilter !== repaired.sectionFilter) this.sectionFilter = repaired.sectionFilter;
    if (this.tagFilter !== repaired.tagFilter) this.tagFilter = repaired.tagFilter;
    if (!projectCustomFieldFiltersMatch(this.customFieldFilters, repaired.customFieldFilters)) {
      this.customFieldFilters = [...repaired.customFieldFilters];
    }
    if (this.sortMode !== repaired.sortMode) this.sortMode = repaired.sortMode;
    if (this.sortDirection !== repaired.sortDirection) this.sortDirection = repaired.sortDirection;
  }

  syncListPreferences(): void {
    const fieldIds = new Set(this.customFields.map((field) => field.id));
    const columns = taskListColumnsForProject(this.#projects.viewPreferences, this.projectId, fieldIds);
    const widths = taskListColumnWidthsForProject(
      this.#projects.viewPreferences,
      this.projectId,
      fieldIds,
    );
    const current = untrack(() => ({
      columns: this.listColumns,
      widths: this.listColumnWidths,
    }));
    if (!taskListColumnsMatch(current.columns, columns)) this.listColumns = columns;
    if (!projectColumnWidthsMatch(current.widths, widths)) this.listColumnWidths = widths;
    const presentation = projectListPresentationForProject(this.#projects.viewPreferences, this.projectId, this.customFields);
    if (JSON.stringify(untrack(() => this.listPresentation)) !== JSON.stringify(presentation)) this.listPresentation = presentation;
  }

  applyFilterState(state: ProjectTaskFilterState): void {
    this.search = state.search;
    this.statusFilter = state.statusFilter;
    this.sectionFilter = state.sectionFilter;
    this.priorityFilter = state.priorityFilter;
    this.dueFilter = state.dueFilter;
    this.dueRangeStart = state.dueRangeStart;
    this.dueRangeEnd = state.dueRangeEnd;
    this.scheduleFilter = state.scheduleFilter;
    this.dependencyFilter = state.dependencyFilter;
    this.tagFilter = state.tagFilter;
    this.customFieldFilters = [...state.customFieldFilters];
    this.groupBy = state.groupBy;
    this.sortMode = state.sortMode;
    this.sortDirection = state.sortDirection;
  }

  reset(): void {
    this.applyFilterState(PROJECT_TASK_FILTER_DEFAULTS);
  }

  clearCustomFieldFilter(fieldId: string): void {
    this.customFieldFilters = this.customFieldFilters.filter((filter) => filter.fieldId !== fieldId);
  }

  /** Persist optimistic columns and report failures without overwriting a newer project or edit. */
  async #persistListColumns(projectId: string, nextColumns: ProjectTaskListColumn[]): Promise<string | null> {
    const request = ++this.#listColumnWriteGeneration;
    const previousColumns = [...this.listColumns];
    this.#listColumnWriteProjectId = projectId;
    this.#listColumnWriteError = null;
    this.listColumns = nextColumns;
    try {
      await this.#projects.saveTaskListColumns(projectId, nextColumns);
      return null;
    } catch (error: unknown) {
      const message = error instanceof Error ? error.message : String(error);
      if (request === this.#listColumnWriteGeneration && this.projectId === projectId) {
        if (taskListColumnsMatch(this.listColumns, nextColumns)) this.listColumns = previousColumns;
        this.#listColumnWriteError = {
          projectId,
          message: this.#translate("projects.columns.saveFailed", message),
        };
      }
      return message;
    } finally {
      if (request === this.#listColumnWriteGeneration) this.#listColumnWriteProjectId = null;
    }
  }

  /** Retain the saved presentation if a visibility preference cannot be written. */
  async toggleColumn(column: ProjectTaskListColumn): Promise<void> {
    const projectId = this.projectId;
    if (!projectId || this.listColumnsSaving || this.propertySaving) return;
    await this.#persistListColumns(projectId, toggleProjectListColumn(this.listColumns, column));
  }

  /** Move one presentation column a step left or right, preserving task order. */
  async moveColumn(column: ProjectTaskListColumn, direction: -1 | 1): Promise<void> {
    const index = this.listColumns.indexOf(column);
    const target = index + direction;
    if (index < 0 || target < 0 || target >= this.listColumns.length) return;
    await this.placeColumn(column, target);
  }

  /** Move one presentation column to `targetIndex` among the list columns, preserving task order. */
  async placeColumn(column: ProjectTaskListColumn, targetIndex: number): Promise<void> {
    const projectId = this.projectId;
    if (!projectId || this.listColumnsSaving || this.propertySaving || !this.listColumns.includes(column)) return;
    const next = collectionMoveItem(this.listColumns, column, targetIndex);
    if (taskListColumnsMatch(next, this.listColumns)) return;
    await this.#persistListColumns(projectId, next);
  }

  /** Save bounded presentation with rollback and visible errors. */
  async savePresentation(value: ProjectListPresentation): Promise<void> {
    const projectId = this.projectId;
    if (!projectId || this.presentationSaving) return;
    const previous = this.listPresentation;
    const generation = ++this.#presentationWriteGeneration;
    const next = parseProjectListPresentation(value, this.customFields);
    this.listPresentation = next;
    this.#presentationWriteProjectId = projectId;
    this.#presentationWriteError = null;
    try { await this.#projects.saveTaskListPresentation(projectId, next); }
    catch (error: unknown) {
      if (generation === this.#presentationWriteGeneration && this.projectId === projectId) {
        if (JSON.stringify(this.listPresentation) === JSON.stringify(next)) this.listPresentation = previous;
        this.#presentationWriteError = {projectId, message: this.#translate("projects.columns.presentationFailed", error instanceof Error ? error.message : String(error))};
      }
    } finally { if (generation === this.#presentationWriteGeneration) this.#presentationWriteProjectId = null; }
  }

  /**
   * Create or duplicate schema, then insert its new stable column on the requested side of the invoking property.
   * The name column stays first, so insertions beside it always land right after it. A duplicate copies its `source` icon; otherwise `icon` is the chosen icon or null for the type icon.
   * @returns Whether the property was created; failures are reported through `propertyError` or `listColumnsError`.
   */
  async addColumnProperty(name: string, fieldType: ProjectCustomFieldType, anchor: "name" | ProjectTaskListColumn, side: "left" | "right" = "right", { source, icon = null }: { source?: ProjectCustomField; icon?: string | null } = {}): Promise<boolean> {
    const projectId = this.projectId;
    if (!projectId || this.propertySaving || this.listColumnsSaving) return false;
    const generation = ++this.#propertyWriteGeneration;
    this.#propertyWriteProjectId = projectId;
    this.#propertyWriteError = null;
    try {
      if (!name.trim()) throw new Error(this.#translate("projects.columns.nameRequired"));
      if (this.customFields.some((field) => field.name.toLocaleLowerCase() === name.trim().toLocaleLowerCase())) throw new Error(this.#translate("projects.columns.nameExists"));
      const created = source ? await this.#projects.duplicateCustomField(source, name) : await this.#projects.addCustomField(projectId, name, fieldType, icon);
      if (!created || this.projectId !== projectId || generation !== this.#propertyWriteGeneration) return false;
      const column = customTaskListColumn(created.id);
      const next = this.listColumns.filter((entry) => entry !== column);
      const anchorIndex = anchor === "name" ? -1 : next.indexOf(anchor);
      next.splice(side === "left" ? Math.max(0, anchorIndex) : anchorIndex + 1, 0, column);
      return await this.#persistListColumns(projectId, next) === null;
    } catch (error: unknown) {
      if (this.projectId === projectId && generation === this.#propertyWriteGeneration) this.#propertyWriteError = {projectId, message: this.#translate("projects.columns.propertyFailed", error instanceof Error ? error.message : String(error))};
      return false;
    } finally { if (generation === this.#propertyWriteGeneration) this.#propertyWriteProjectId = null; }
  }

  /**
   * Delete a custom field together with its values on every task in the project.
   * @returns Whether the field was deleted; failures are reported through `propertyError`.
   */
  async deleteColumnProperty(field: ProjectCustomField): Promise<boolean> {
    const projectId = this.projectId;
    if (!projectId || this.propertySaving || this.listColumnsSaving) return false;
    const generation = ++this.#propertyWriteGeneration;
    this.#propertyWriteProjectId = projectId;
    this.#propertyWriteError = null;
    try {
      await this.#projects.removeCustomField(field.id);
      return true;
    } catch (error: unknown) {
      if (this.projectId === projectId && generation === this.#propertyWriteGeneration) this.#propertyWriteError = {projectId, message: this.#translate("projects.customFields.deleteFailed", error instanceof Error ? error.message : String(error))};
      return false;
    } finally { if (generation === this.#propertyWriteGeneration) this.#propertyWriteProjectId = null; }
  }

  async updateColumnWidths(
    widths: ProjectTaskListColumnWidths,
    options: { persist?: boolean } = {},
  ): Promise<void> {
    if (options.persist && this.projectId) {
      await this.#projects.saveTaskListColumnWidths(this.projectId, widths);
    } else {
      this.listColumnWidths = widths;
    }
  }
}

export function normalizeProjectFilterDate(value: string): string | undefined {
  const trimmed = value.trim();
  if (!trimmed) return undefined;
  try {
    return Temporal.PlainDate.from(trimmed).toString();
  } catch {
    return undefined;
  }
}

export interface ProjectTaskQueryReferenceInput {
  sectionFilter: string | "all";
  tagFilter: ProjectTaskTagFilter;
  customFieldFilters: readonly ProjectCustomFieldFilter[];
  sortMode: ProjectTaskSortMode;
  sortDirection: ProjectTaskSortDirection;
  sectionIds: ReadonlySet<string>;
  tagIds: ReadonlySet<string>;
  fieldIds: ReadonlySet<string>;
  optionIds: ReadonlySet<string>;
}

function projectCustomFieldFilterStillExists(
  filter: ProjectCustomFieldFilter,
  fieldIds: ReadonlySet<string>,
  optionIds: ReadonlySet<string>,
): boolean {
  if (!fieldIds.has(filter.fieldId)) return false;
  return filter.mode !== "option" || optionIds.has(filter.optionId);
}

/** Repairs query references after project fields or collections disappear. */
export function repairProjectTaskQueryReferences(
  input: ProjectTaskQueryReferenceInput,
): Pick<ProjectTaskFilterState,
  "sectionFilter" | "tagFilter" | "customFieldFilters" | "sortMode" | "sortDirection"> {
  const sectionFilter = input.sectionFilter !== "all" && !input.sectionIds.has(input.sectionFilter)
    ? "all"
    : input.sectionFilter;
  const tagFilter = input.tagFilter !== "all" && input.tagFilter !== "none"
    && !input.tagIds.has(input.tagFilter)
    ? "all"
    : input.tagFilter;
  const customFieldFilters = input.customFieldFilters.filter((filter) =>
    projectCustomFieldFilterStillExists(filter, input.fieldIds, input.optionIds));
  const sortFieldId = customFieldIdFromCustomFieldReference(input.sortMode);
  return sortFieldId && !input.fieldIds.has(sortFieldId)
    ? { sectionFilter, tagFilter, customFieldFilters, sortMode: "manual", sortDirection: "asc" }
    : {
      sectionFilter,
      tagFilter,
      customFieldFilters,
      sortMode: input.sortMode,
      sortDirection: input.sortDirection,
    };
}

function projectCustomFieldFiltersMatch(
  first: readonly ProjectCustomFieldFilter[],
  second: readonly ProjectCustomFieldFilter[],
): boolean {
  return first.length === second.length
    && first.every((filter, index) => JSON.stringify(filter) === JSON.stringify(second[index]));
}

function projectColumnWidthsMatch(
  first: ProjectTaskListColumnWidths,
  second: ProjectTaskListColumnWidths,
): boolean {
  const firstEntries = Object.entries(first);
  const secondEntries = Object.entries(second);
  return firstEntries.length === secondEntries.length
    && firstEntries.every(([key, value]) => second[key as keyof ProjectTaskListColumnWidths] === value);
}
