import {
  loadProjectTaskDetail,
  loadProjectTaskView,
  loadProjectsOptionalData,
  loadProjectsWorkspace,
  refreshProjectsWorkspace,
} from "$lib/api/projects";
import { ProjectTaskDetailCache } from "$lib/projects/tasks/detail-cache";
import { ProjectTaskViewRequestGate } from "$lib/projects/tasks/view-request-gate";
import {
  mergeProjectOptionalData,
  PROJECT_SCOPED_OPTIONAL_DATA_KINDS,
  projectOptionalDataKey,
  projectViewOptionalDataKinds,
} from "$lib/projects/snapshot/optional-data";
import { mergeProjectSnapshot } from "$lib/projects/snapshot/queries";
import {
  loadSavedActiveProjectId,
  loadSavedProjectViewId,
  saveActiveProjectId,
  saveProjectViewId,
} from "$lib/projects/ui-preferences";
import { createProjectStoreActions } from "$lib/stores/projects/actions";
import { createProjectStoreSelectors } from "$lib/stores/projects/selectors";
import type {
  Project,
  ProjectChecklistItem,
  ProjectCustomEmoji,
  ProjectCustomField,
  ProjectCustomFieldOption,
  ProjectCustomFieldOptionValue,
  ProjectCustomFieldValue,
  ProjectGroup,
  ProjectTag,
  ProjectPriorityConfig,
  ProjectOptionalDataKind,
  ProjectsSnapshot,
  ProjectSection,
  ProjectStatus,
  ProjectTask,
  ProjectTaskDetailData,
  ProjectTaskViewPage,
  ProjectTaskViewRequest,
  ProjectTaskChangeEvent,
  ProjectTaskDependency,
  ProjectTaskEventLink,
  ProjectTaskTagLink,
  ProjectViewPreference,
  ProjectViewId,
} from "$lib/projects/types";

let snapshot = $state<ProjectsSnapshot>({
  groups: [],
  projects: [],
  sections: [],
  statuses: [],
  priorities: [],
  tasks: [],
  checklistItems: [],
  tags: [],
  taskTagLinks: [],
  customFields: [],
  customFieldOptions: [],
  customFieldValues: [],
  customFieldOptionValues: [],
  dependencies: [],
  eventLinks: [],
  taskChangeEvents: [],
  viewPreferences: [],
  customEmojis: [],
});
let loaded = $state(false);
let loading = $state(false);
let loadError = $state<string | null>(null);
let selectedProjectId = $state<string | null>(loadSavedActiveProjectId());
let activeView = $state<ProjectViewId>(loadSavedProjectViewId());
let loadedProjectIds = $state<string[]>([]);
let loadRequestId = 0;
let selectProjectRequestId = 0;
let initialLoadPromise: Promise<void> | null = null;
const optionalDataLoadedKeys = new Set<string>();
const optionalDataRequests = new Map<string, Promise<void>>();
let optionalDataVersion = $state(0);
let taskViewPage = $state<ProjectTaskViewPage | null>(null);
let taskViewLoading = $state(false);
let taskViewError = $state<string | null>(null);
let taskMutationRevision = $state(0);
const taskViewRequestGate = new ProjectTaskViewRequestGate();
const taskDetailCache = new ProjectTaskDetailCache(4 * 1024 * 1024);
const taskDetailRequests = new Map<string, Promise<ProjectTaskDetailData>>();
const selectors = createProjectStoreSelectors(() => snapshot);

function ensureSelectedProject(): void {
  if (snapshot.projects.length === 0) return;
  if (selectedProjectId && snapshot.projects.some((project) => project.id === selectedProjectId)) {
    return;
  }
  setSelectedProjectId(selectors.firstProjectId());
}

function setSelectedProjectId(projectId: string | null): void {
  if (selectedProjectId === projectId) return;
  taskViewRequestGate.cancel();
  taskViewLoading = false;
  selectedProjectId = projectId;
  saveActiveProjectId(projectId);
}

function isProjectDataLoaded(projectId: string | null | undefined): boolean {
  return Boolean(projectId && loadedProjectIds.includes(projectId));
}

function markProjectDataLoaded(projectId: string): void {
  if (loadedProjectIds.includes(projectId)) return;
  loadedProjectIds = [...loadedProjectIds, projectId];
}

function setActiveView(view: ProjectViewId): void {
  if (activeView === view) return;
  activeView = view;
  saveProjectViewId(view);
}

function applyLoadedSnapshot(incoming: ProjectsSnapshot, projectId: string | null | undefined): void {
  if (!projectId) {
    snapshot = {
      ...snapshot,
      groups: incoming.groups,
      projects: incoming.projects,
      viewPreferences: incoming.viewPreferences,
      customEmojis: incoming.customEmojis,
    };
    return;
  }
  snapshot = mergeProjectSnapshot(
    snapshot,
    {
      ...incoming,
      viewPreferences: snapshot.viewPreferences,
      customEmojis: snapshot.customEmojis,
    },
    projectId,
  );
  if (incoming.projects.some((project) => project.id === projectId)) {
    markProjectDataLoaded(projectId);
  }
}

async function reload(projectId: string | null = selectedProjectId): Promise<void> {
  taskViewRequestGate.cancel();
  taskViewLoading = false;
  const requestId = ++loadRequestId;
  const isInitialLoad = !loaded;
  const customEmojiKey = projectOptionalDataKey("custom_emojis", null);
  const refreshCustomEmojis = optionalDataLoadedKeys.has(customEmojiKey)
    || optionalDataRequests.has(customEmojiKey);
  loading = true;
  loadError = null;
  try {
    let response = isInitialLoad
      ? await loadProjectsWorkspace(projectId, activeView)
      : await refreshProjectsWorkspace(projectId, activeView);
    if (requestId !== loadRequestId) return;
    if (response.activeView !== activeView) {
      response = await refreshProjectsWorkspace(response.resolvedProjectId, activeView);
      if (requestId !== loadRequestId) return;
    }
    const resolvedProjectId = response.resolvedProjectId;
    const optionalKindsToRefresh = resolvedProjectId
      ? PROJECT_SCOPED_OPTIONAL_DATA_KINDS.filter((kind) => {
          const key = projectOptionalDataKey(kind, resolvedProjectId);
          return optionalDataLoadedKeys.has(key) || optionalDataRequests.has(key);
        })
      : [];
    applyLoadedSnapshot(response.snapshot, resolvedProjectId);
    setSelectedProjectId(resolvedProjectId);
    loaded = true;
    if (resolvedProjectId && optionalKindsToRefresh.length > 0) {
      await Promise.allSettled(
        optionalKindsToRefresh
          .map((kind) => optionalDataRequests.get(projectOptionalDataKey(kind, resolvedProjectId)))
          .filter((request): request is Promise<void> => Boolean(request)),
      );
      if (requestId !== loadRequestId) return;
      for (const kind of optionalKindsToRefresh) {
        if (optionalDataLoadedKeys.delete(projectOptionalDataKey(kind, resolvedProjectId))) {
          optionalDataVersion += 1;
        }
      }
      await Promise.all(optionalKindsToRefresh.map((kind) =>
        ensureOptionalData(kind, resolvedProjectId)
      ));
    }
    if (refreshCustomEmojis) {
      const existing = optionalDataRequests.get(customEmojiKey);
      if (existing) await Promise.allSettled([existing]);
      if (requestId !== loadRequestId) return;
      if (optionalDataLoadedKeys.delete(customEmojiKey)) optionalDataVersion += 1;
      await ensureCustomEmojis();
    }
    if (snapshot.groups.some((group) => group.icon.startsWith("custom-emoji:"))
      || snapshot.projects.some((project) => project.icon.startsWith("custom-emoji:"))) {
      void ensureCustomEmojis().catch((error) => {
        console.error("load project custom emoji metadata failed", error);
      });
    }
  } catch (error) {
    if (requestId !== loadRequestId) return;
    loadError = error instanceof Error ? error.message : String(error);
    throw error;
  } finally {
    if (requestId === loadRequestId) loading = false;
  }
}

async function ensureLoaded(): Promise<void> {
  if (loaded) return;
  if (!initialLoadPromise) {
    initialLoadPromise = reload().finally(() => {
      initialLoadPromise = null;
    });
  }
  await initialLoadPromise;
}

async function ensureProjectData(projectId: string | null | undefined): Promise<void> {
  if (!projectId) return;
  if (!loaded) await ensureLoaded();
  if (isProjectDataLoaded(projectId)) return;
  await reload(projectId);
}

async function selectProject(projectId: string | null): Promise<void> {
  const requestId = ++selectProjectRequestId;
  if (!projectId || isProjectDataLoaded(projectId)) {
    setSelectedProjectId(projectId);
    return;
  }
  await ensureProjectData(projectId);
  if (requestId !== selectProjectRequestId) return;
  if (!isProjectDataLoaded(projectId)) return;
  setSelectedProjectId(projectId);
}

async function ensureOptionalData(
  kind: ProjectOptionalDataKind,
  projectId: string | null,
): Promise<void> {
  if (kind !== "custom_emojis" && !projectId) return;
  const key = projectOptionalDataKey(kind, projectId);
  if (optionalDataLoadedKeys.has(key)) return;
  const existing = optionalDataRequests.get(key);
  if (existing) return existing;
  const requestGeneration = loadRequestId;
  const request = loadProjectsOptionalData(kind, projectId)
    .then((incoming) => {
      if (requestGeneration !== loadRequestId) return;
      if (incoming.kind !== kind || incoming.projectId !== projectId) return;
      snapshot = mergeProjectOptionalData(snapshot, incoming);
      optionalDataLoadedKeys.add(key);
      optionalDataVersion += 1;
    })
    .finally(() => {
      if (optionalDataRequests.get(key) === request) optionalDataRequests.delete(key);
    });
  optionalDataRequests.set(key, request);
  return request;
}

async function ensureProjectViewData(
  projectId: string | null | undefined,
  view: ProjectViewId,
): Promise<void> {
  if (!projectId) return;
  await Promise.all(projectViewOptionalDataKinds(view).map((kind) =>
    ensureOptionalData(kind, projectId)
  ));
}

/** Reports whether every optional collection a project view needs has loaded. */
function isProjectViewDataLoaded(projectId: string | null | undefined, view: ProjectViewId): boolean {
  if (!projectId) return false;
  return projectViewOptionalDataKinds(view).every((kind) => isProjectOptionalDataLoaded(kind, projectId));
}

async function ensureProjectToolbarData(projectId: string | null | undefined): Promise<void> {
  if (!projectId) return;
  await Promise.all([
    ensureProjectData(projectId),
    ensureOptionalData("saved_views", projectId),
  ]);
}

function replaceTaskScopedRows<T>(
  current: readonly T[],
  incoming: readonly T[],
  affectedTaskIds: ReadonlySet<string>,
  belongsToTask: (row: T, taskIds: ReadonlySet<string>) => boolean,
): T[] {
  return [...current.filter((row) => !belongsToTask(row, affectedTaskIds)), ...incoming];
}

function mergeTaskViewPage(
  page: ProjectTaskViewPage,
  append: boolean,
  retainedTaskIds: readonly string[],
): void {
  const loadedProjectTaskIds = new Set(
    snapshot.tasks.filter((task) => task.projectId === page.projectId).map((task) => task.id),
  );
  const retainedIdSet = new Set(retainedTaskIds);
  const retainedDetails = snapshot.tasks.filter((task) =>
    task.projectId === page.projectId
    && (task.detailLoaded || retainedIdSet.has(task.id))
    && !page.tasks.some((row) => row.id === task.id)
  );
  const existingById = new Map(snapshot.tasks.map((task) => [task.id, task]));
  const incomingTasks = page.tasks.map((task) => {
    const existing = existingById.get(task.id);
    return existing?.detailLoaded && existing.updatedAt === task.updatedAt ? existing : task;
  });
  const projectTasks = append
    ? [...snapshot.tasks.filter((task) => task.projectId === page.projectId), ...incomingTasks]
    : [...retainedDetails, ...incomingTasks];
  const deduplicated = [...new Map(projectTasks.map((task) => [task.id, task])).values()];
  const affectedTaskIds = append
    ? new Set(page.tasks.map((task) => task.id))
    : loadedProjectTaskIds;
  const loadedProjectFieldIds = new Set(
    snapshot.customFields
      .filter((field) => field.projectId === page.projectId)
      .map((field) => field.id),
  );
  snapshot = {
    ...snapshot,
    tasks: [...snapshot.tasks.filter((task) => task.projectId !== page.projectId), ...deduplicated],
    taskTagLinks: replaceTaskScopedRows(snapshot.taskTagLinks, page.taskTagLinks, affectedTaskIds,
      (row, ids) => ids.has(row.taskId)),
    customFieldValues: replaceTaskScopedRows(snapshot.customFieldValues, page.customFieldValues,
      affectedTaskIds, (row, ids) => ids.has(row.taskId)),
    customFieldOptionValues: replaceTaskScopedRows(
      snapshot.customFieldOptionValues, page.customFieldOptionValues, affectedTaskIds,
      (row, ids) => ids.has(row.taskId),
    ),
    dependencies: replaceTaskScopedRows(snapshot.dependencies, page.dependencies, affectedTaskIds,
      (row, ids) => ids.has(row.blockingTaskId) || ids.has(row.blockedTaskId)),
    eventLinks: replaceTaskScopedRows(snapshot.eventLinks, page.eventLinks, affectedTaskIds,
      (row, ids) => ids.has(row.taskId)),
    tags: [...snapshot.tags.filter((tag) => tag.projectId !== page.projectId), ...page.tags],
    customFields: [
      ...snapshot.customFields.filter((field) => field.projectId !== page.projectId),
      ...page.customFields,
    ],
    customFieldOptions: [
      ...snapshot.customFieldOptions.filter((option) => !loadedProjectFieldIds.has(option.fieldId)),
      ...page.customFieldOptions,
    ],
  };
}

async function loadTaskView(
  request: ProjectTaskViewRequest,
  append = false,
  retainedTaskIds: readonly string[] = [],
): Promise<void> {
  const requestId = taskViewRequestGate.begin();
  taskViewLoading = true;
  taskViewError = null;
  try {
    const page = await loadProjectTaskView(request);
    if (!taskViewRequestGate.isCurrent(requestId) || page.projectId !== request.projectId || page.view !== request.view) return;
    mergeTaskViewPage(page, append, retainedTaskIds);
    taskViewPage = append && taskViewPage
      ? {
          ...page,
          tasks: [...new Map([...taskViewPage.tasks, ...page.tasks].map((task) => [task.id, task])).values()],
        }
      : page;
  } catch (error) {
    if (!taskViewRequestGate.isCurrent(requestId)) return;
    taskViewError = error instanceof Error ? error.message : String(error);
    throw error;
  } finally {
    if (taskViewRequestGate.isCurrent(requestId)) taskViewLoading = false;
  }
}

function mergeTaskDetail(detail: ProjectTaskDetailData): void {
  const taskId = detail.task.id;
  const relatedIds = new Set(detail.relatedTasks.map((task) => task.id));
  relatedIds.add(taskId);
  const loadedProjectFieldIds = new Set(
    snapshot.customFields
      .filter((field) => field.projectId === detail.task.projectId)
      .map((field) => field.id),
  );
  snapshot = {
    ...snapshot,
    tasks: [
      ...snapshot.tasks.filter((task) => !relatedIds.has(task.id)),
      ...detail.relatedTasks.filter((task) => task.id !== taskId),
      detail.task,
    ],
    checklistItems: replaceTaskScopedRows(snapshot.checklistItems, detail.checklistItems,
      new Set([taskId]), (row, ids) => ids.has(row.taskId)),
    taskTagLinks: replaceTaskScopedRows(snapshot.taskTagLinks, detail.taskTagLinks,
      new Set([taskId]), (row, ids) => ids.has(row.taskId)),
    customFieldValues: replaceTaskScopedRows(snapshot.customFieldValues, detail.customFieldValues,
      new Set([taskId]), (row, ids) => ids.has(row.taskId)),
    customFieldOptionValues: replaceTaskScopedRows(
      snapshot.customFieldOptionValues, detail.customFieldOptionValues, new Set([taskId]),
      (row, ids) => ids.has(row.taskId),
    ),
    dependencies: replaceTaskScopedRows(snapshot.dependencies, detail.dependencies,
      new Set([taskId]), (row, ids) => ids.has(row.blockingTaskId) || ids.has(row.blockedTaskId)),
    eventLinks: replaceTaskScopedRows(snapshot.eventLinks, detail.eventLinks,
      new Set([taskId]), (row, ids) => ids.has(row.taskId)),
    taskChangeEvents: replaceTaskScopedRows(snapshot.taskChangeEvents, detail.taskChangeEvents,
      new Set([taskId]), (row, ids) => ids.has(row.taskId)),
    tags: [...snapshot.tags.filter((tag) => tag.projectId !== detail.task.projectId), ...detail.tags],
    customFields: [
      ...snapshot.customFields.filter((field) => field.projectId !== detail.task.projectId),
      ...detail.customFields,
    ],
    customFieldOptions: [
      ...snapshot.customFieldOptions.filter((option) => !loadedProjectFieldIds.has(option.fieldId)),
      ...detail.customFieldOptions,
    ],
  };
}

async function ensureTaskDetailData(
  projectId: string | null | undefined,
  taskId: string,
): Promise<void> {
  if (!projectId) return;
  const summary = selectors.taskById(taskId);
  if (summary?.detailLoaded) return;
  const cached = summary ? taskDetailCache.get(taskId, summary.updatedAt) : undefined;
  if (cached) {
    mergeTaskDetail(cached);
    return;
  }
  const existing = taskDetailRequests.get(taskId);
  if (existing) {
    await existing;
    return;
  }
  const requestGeneration = loadRequestId;
  const request = loadProjectTaskDetail(taskId).then((detail) => {
    if (requestGeneration !== loadRequestId) return detail;
    if (detail.task.projectId !== projectId) throw new Error("Task detail project mismatch");
    taskDetailCache.set(detail);
    mergeTaskDetail(detail);
    return detail;
  }).finally(() => taskDetailRequests.delete(taskId));
  taskDetailRequests.set(taskId, request);
  await request;
}

async function ensureCustomEmojis(): Promise<void> {
  await ensureOptionalData("custom_emojis", null);
}

function isProjectOptionalDataLoaded(
  kind: ProjectOptionalDataKind,
  projectId: string | null,
): boolean {
  void optionalDataVersion;
  return optionalDataLoadedKeys.has(projectOptionalDataKey(kind, projectId));
}

function updateSnapshot(updater: (current: ProjectsSnapshot) => ProjectsSnapshot): void {
  snapshot = updater(snapshot);
  taskMutationRevision += 1;
}

const actions = createProjectStoreActions({
  selectors,
  readSnapshot: () => snapshot,
  readLoadGeneration: () => loadRequestId,
  updateSnapshot,
  async applyCalendarEventProjectAssignments(assignments) {
    if (assignments.length === 0) return;
    try {
      const { getCalendar } = await import("$lib/stores/calendar.svelte");
      await getCalendar().applyProjectAssignments(assignments);
    } catch (error) {
      console.error("Failed to apply committed calendar event project assignments", error);
    }
  },
  readSelectedProjectId: () => selectedProjectId,
  setSelectedProjectId,
  reload,
  ensureProjectData,
  ensureTaskDetailData,
});

export function getProjects() {
  const {
    projectById,
    groupById,
    projectsForGroup,
    projectsForGroupIncludingInactive,
    visibleGroups,
    sectionsForProject,
    sectionsForProjectIncludingInactive,
    statusesForProject,
    prioritiesForProject,
    tasksForProject,
    tasksForProjectIncludingArchived,
    topLevelTasksForSection,
    topLevelTasksForStatus,
    subtasksForTask,
    subtasksForTaskIncludingArchived,
    defaultSection,
    defaultStatus,
    doneStatus,
    reopenStatus,
    statusById,
    taskById,
    eventLinksForTask,
    eventLinksForEvent,
    taskChangeEventsForTask,
    recentTaskChangeEventsForProject,
    checklistItemsForTask,
    tagsForProject,
    tagById,
    taskTagLinksForTask,
    tagsForTask,
    unlinkedTagsForTask,
    customFieldsForProject,
    customFieldById,
    customFieldOptionsForField,
    customFieldValueForTask,
    customFieldOptionValuesForTask,
    dependenciesBlockingTask,
    dependenciesBlockedByTask,
    savedTaskViewsForProject,
  } = selectors;

  return {
    get snapshot(): ProjectsSnapshot {
      return snapshot;
    },
    get groups(): ProjectGroup[] {
      return snapshot.groups;
    },
    get projects(): Project[] {
      return snapshot.projects;
    },
    get sections(): ProjectSection[] {
      return snapshot.sections;
    },
    get statuses(): ProjectStatus[] {
      return snapshot.statuses;
    },
    get priorities(): ProjectPriorityConfig[] {
      return snapshot.priorities;
    },
    get tasks(): ProjectTask[] {
      return snapshot.tasks;
    },
    get checklistItems(): ProjectChecklistItem[] {
      return snapshot.checklistItems;
    },
    get tags(): ProjectTag[] {
      return snapshot.tags;
    },
    get taskTagLinks(): ProjectTaskTagLink[] {
      return snapshot.taskTagLinks;
    },
    get customFields(): ProjectCustomField[] {
      return snapshot.customFields;
    },
    get customFieldOptions(): ProjectCustomFieldOption[] {
      return snapshot.customFieldOptions;
    },
    get customFieldValues(): ProjectCustomFieldValue[] {
      return snapshot.customFieldValues;
    },
    get customFieldOptionValues(): ProjectCustomFieldOptionValue[] {
      return snapshot.customFieldOptionValues;
    },
    get dependencies(): ProjectTaskDependency[] {
      return snapshot.dependencies;
    },
    get eventLinks(): ProjectTaskEventLink[] {
      return snapshot.eventLinks;
    },
    get taskChangeEvents(): ProjectTaskChangeEvent[] {
      return snapshot.taskChangeEvents;
    },
    get viewPreferences(): ProjectViewPreference[] {
      return snapshot.viewPreferences;
    },
    get customEmojis(): ProjectCustomEmoji[] {
      return snapshot.customEmojis;
    },
    get selectedProjectId(): string | null {
      ensureSelectedProject();
      return selectedProjectId;
    },
    set selectedProjectId(projectId: string | null) {
      setSelectedProjectId(projectId);
    },
    get selectedProject(): Project | undefined {
      ensureSelectedProject();
      return selectors.projectById(selectedProjectId);
    },
    get selectedGroup(): ProjectGroup | undefined {
      const selectedProject = selectors.projectById(selectedProjectId);
      return selectors.groupById(selectedProject?.groupId);
    },
    get activeView(): ProjectViewId {
      return activeView;
    },
    set activeView(view: ProjectViewId) {
      setActiveView(view);
    },
    get loaded(): boolean {
      return loaded;
    },
    get loading(): boolean {
      return loading;
    },
    get loadError(): string | null {
      return loadError;
    },
    get taskViewPage(): ProjectTaskViewPage | null {
      return taskViewPage;
    },
    get taskViewLoading(): boolean {
      return taskViewLoading;
    },
    get taskViewError(): string | null {
      return taskViewError;
    },
    get taskMutationRevision(): number {
      return taskMutationRevision;
    },
    load: reload,
    ensureLoaded,
    ensureProjectData,
    ensureProjectViewData,
    ensureProjectToolbarData,
    loadTaskView,
    ensureTaskDetailData,
    ensureCustomEmojis,
    isProjectOptionalDataLoaded,
    isProjectViewDataLoaded,
    isProjectDataLoaded,
    selectProject,
    projectById,
    groupById,
    projectsForGroup,
    projectsForGroupIncludingInactive,
    visibleGroups,
    sectionsForProject,
    sectionsForProjectIncludingInactive,
    statusesForProject,
    prioritiesForProject,
    tasksForProject,
    tasksForProjectIncludingArchived,
    topLevelTasksForSection,
    topLevelTasksForStatus,
    subtasksForTask,
    subtasksForTaskIncludingArchived,
    defaultSection,
    defaultStatus,
    doneStatus,
    reopenStatus,
    statusById,
    taskById,
    eventLinksForTask,
    eventLinksForEvent,
    taskChangeEventsForTask,
    recentTaskChangeEventsForProject,
    checklistItemsForTask,
    tagsForProject,
    tagById,
    taskTagLinksForTask,
    tagsForTask,
    unlinkedTagsForTask,
    customFieldsForProject,
    customFieldById,
    customFieldOptionsForField,
    customFieldValueForTask,
    customFieldOptionValuesForTask,
    dependenciesBlockingTask,
    dependenciesBlockedByTask,
    savedTaskViewsForProject,
    ...actions,
  };
}
