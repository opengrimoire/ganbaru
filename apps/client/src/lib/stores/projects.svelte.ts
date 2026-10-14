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
  loadSavedCollapsedSidebarProjectIds,
  loadSavedProjectActiveViewIds,
  loadSavedProjectSidebarCollapsed,
  loadSavedShowInactiveProjects,
  loadSavedStarredProjectIds,
  loadSavedStarredViewIds,
  pruneProjectActiveViewIds,
  saveActiveProjectId,
  saveCollapsedSidebarProjectIds,
  saveProjectActiveViewIds,
  saveProjectSidebarCollapsed,
  saveShowInactiveProjects,
  saveStarredProjectIds,
  saveStarredViewIds,
} from "$lib/projects/ui-preferences";
import {
  isProjectHostedViewType,
  isProjectTaskViewType,
  projectViewForType,
  projectViewsFor,
  resolveActiveProjectView,
} from "$lib/projects/views";
import { createProjectStoreActions } from "$lib/stores/projects/actions";
import { createProjectStoreSelectors } from "$lib/stores/projects/selectors";
import { getPreferences } from "$lib/stores/preferences.svelte";
import { sortSidebarItems, starredFirst, withStarredId } from "$lib/utils/sidebar-order";
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
  ProjectHostedViewType,
  ProjectView,
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
  views: [],
  customEmojis: [],
});
let loaded = $state(false);
let loading = $state(false);
let loadError = $state<string | null>(null);
let selectedProjectId = $state<string | null>(loadSavedActiveProjectId());
let activeViewIds = $state<Record<string, string>>(loadSavedProjectActiveViewIds());
let hostableViewTypes = $state<ProjectHostedViewType[]>([]);
let sidebarCollapsed = $state(loadSavedProjectSidebarCollapsed());
let collapsedSidebarProjectIds = $state<string[]>(loadSavedCollapsedSidebarProjectIds());
let starredProjectIds = $state<string[]>(loadSavedStarredProjectIds());
let starredViewIds = $state<string[]>(loadSavedStarredViewIds());
let showInactiveProjects = $state(loadSavedShowInactiveProjects());
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
  setSelectedProjectId(firstSidebarProjectId());
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

/** Returns a project's views in sidebar order, keeping hosted views only where this window can host them. */
function viewsForProject(projectId: string | null | undefined): ProjectView[] {
  return starredFirst(projectViewsFor(snapshot.views, projectId, hostableViewTypes), isViewStarred);
}

function isProjectStarred(project: Project): boolean {
  return starredProjectIds.includes(project.id);
}

function isViewStarred(view: ProjectView): boolean {
  return starredViewIds.includes(view.id);
}

/** Orders groups or projects by the global Projects sidebar order. */
function sortedForSidebar<T extends Project | ProjectGroup>(items: readonly T[], starred?: (item: T) => boolean): T[] {
  return sortSidebarItems(
    items,
    getPreferences().projectsSidebarOrder,
    (item) => ({ id: item.id, name: item.name, createdAt: item.createdAt, modifiedAt: item.updatedAt, rank: item.sortOrder }),
    starred,
  );
}

function sidebarGroups(): ProjectGroup[] {
  return sortedForSidebar(selectors.visibleGroups());
}

function sidebarProjectsForGroup(groupId: string): Project[] {
  return sortedForSidebar(selectors.projectsForGroup(groupId), isProjectStarred);
}

function sidebarProjectsForGroupIncludingInactive(groupId: string): Project[] {
  return sortedForSidebar(selectors.projectsForGroupIncludingInactive(groupId), isProjectStarred);
}

/** The first active project in sidebar order, used when no remembered project exists. */
function firstSidebarProjectId(): string | null {
  for (const group of sidebarGroups()) {
    const project = sidebarProjectsForGroup(group.id)[0];
    if (project) return project.id;
  }
  return selectors.firstProjectId();
}

/** Returns the open view of the selected project: the remembered one while it exists, else its first view. */
function activeProjectView(): ProjectView | undefined {
  ensureSelectedProject();
  return resolveActiveProjectView(viewsForProject(selectedProjectId), selectedProjectId ? activeViewIds[selectedProjectId] : null);
}

/** Opens one of a project's views and remembers it for the next visit to that project. */
function selectView(view: ProjectView): void {
  if (activeViewIds[view.projectId] === view.id) return;
  const projectIds = new Set(snapshot.projects.map((project) => project.id));
  activeViewIds = pruneProjectActiveViewIds({ ...activeViewIds, [view.projectId]: view.id }, projectIds);
  saveProjectActiveViewIds(activeViewIds);
}

/** Opens the selected project's view of a task type; does nothing when the project has no view of that type. */
function setActiveView(viewType: ProjectViewId): void {
  const target = projectViewForType(viewsForProject(selectedProjectId), activeProjectView(), viewType);
  if (target) selectView(target);
}

function applyLoadedSnapshot(incoming: ProjectsSnapshot, projectId: string | null | undefined): void {
  if (!projectId) {
    snapshot = {
      ...snapshot,
      groups: incoming.groups,
      projects: incoming.projects,
      viewPreferences: incoming.viewPreferences,
      views: incoming.views,
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
    const response = isInitialLoad
      ? await loadProjectsWorkspace(projectId)
      : await refreshProjectsWorkspace(projectId);
    if (requestId !== loadRequestId) return;
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
    ensureOptionalData("view_preferences", projectId),
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
    /** Every loaded project view, unsorted; use `viewsForProject` for one project's sidebar order. */
    get views(): ProjectView[] {
      return snapshot.views;
    },
    viewsForProject,
    /** Hosted view types this window can open, set by the shell from its visible tabs. */
    get hostableViewTypes(): readonly ProjectHostedViewType[] {
      return hostableViewTypes;
    },
    set hostableViewTypes(types: readonly ProjectHostedViewType[]) {
      if (types.length === hostableViewTypes.length && types.every((type, index) => hostableViewTypes[index] === type)) return;
      hostableViewTypes = [...types];
    },
    /** The open view of the selected project, or undefined when the project has no views. */
    get activeProjectView(): ProjectView | undefined {
      return activeProjectView();
    },
    get activeViewId(): string | null {
      return activeProjectView()?.id ?? null;
    },
    /** Task view type rendered in the Projects workspace, or null while a hosted view is open or no view exists. */
    get activeTaskView(): ProjectViewId | null {
      const view = activeProjectView();
      return view && isProjectTaskViewType(view.viewType) ? view.viewType : null;
    },
    /**
     * Task view type that drives task queries; it reads as the list view while no task view is open. Setting it opens
     * the selected project's view of that type, keeping the active view when it already matches.
     */
    get activeView(): ProjectViewId {
      const view = activeProjectView();
      return view && isProjectTaskViewType(view.viewType) ? view.viewType : "list";
    },
    set activeView(viewType: ProjectViewId) {
      setActiveView(viewType);
    },
    /** Hosted workspace open in the Projects tab, or null while a task view is open. */
    get hostedPlace(): ProjectHostedViewType | null {
      const view = activeProjectView();
      return view && isProjectHostedViewType(view.viewType) ? view.viewType : null;
    },
    selectView,
    /** Leaves a hosted workspace for the project's first task view, for routes that open a project or one of its tasks. */
    showProjectView(): void {
      const view = activeProjectView();
      if (!view || !isProjectHostedViewType(view.viewType)) return;
      const taskView = viewsForProject(view.projectId).find((entry) => isProjectTaskViewType(entry.viewType));
      if (taskView) selectView(taskView);
    },
    /** Whether the docked desktop Projects sidebar is collapsed to its rail. */
    get sidebarCollapsed(): boolean {
      return sidebarCollapsed;
    },
    set sidebarCollapsed(collapsed: boolean) {
      if (sidebarCollapsed === collapsed) return;
      sidebarCollapsed = collapsed;
      saveProjectSidebarCollapsed(collapsed);
    },
    /** Whether a project's section in the Projects sidebar hides its views. Sections start expanded. */
    isSidebarProjectCollapsed(projectId: string): boolean {
      return collapsedSidebarProjectIds.includes(projectId);
    },
    /** Collapses or expands a project's sidebar section, forgetting sections of projects that no longer exist. */
    setSidebarProjectCollapsed(projectId: string, collapsed: boolean): void {
      if (collapsedSidebarProjectIds.includes(projectId) === collapsed) return;
      const projectIds = new Set(snapshot.projects.map((project) => project.id));
      collapsedSidebarProjectIds = (collapsed
        ? [...collapsedSidebarProjectIds, projectId]
        : collapsedSidebarProjectIds.filter((id) => id !== projectId))
        .filter((id) => projectIds.has(id));
      saveCollapsedSidebarProjectIds(collapsedSidebarProjectIds);
    },
    /** Whether a project is starred, which lists it first in its group. */
    isProjectStarred(projectId: string): boolean {
      return starredProjectIds.includes(projectId);
    },
    /** Stars or unstars a project, forgetting stars of projects that no longer exist. */
    setProjectStarred(projectId: string, starred: boolean): void {
      if (starredProjectIds.includes(projectId) === starred) return;
      const projectIds = new Set(snapshot.projects.map((project) => project.id));
      starredProjectIds = withStarredId(starredProjectIds, projectId, starred).filter((id) => projectIds.has(id));
      saveStarredProjectIds(starredProjectIds);
    },
    /** Whether a view is starred, which lists it first in its project section. */
    isViewStarred(viewId: string): boolean {
      return starredViewIds.includes(viewId);
    },
    /** Stars or unstars a view, forgetting stars of views that no longer exist. */
    setViewStarred(viewId: string, starred: boolean): void {
      if (starredViewIds.includes(viewId) === starred) return;
      const viewIds = new Set(snapshot.views.map((view) => view.id));
      starredViewIds = withStarredId(starredViewIds, viewId, starred).filter((id) => viewIds.has(id));
      saveStarredViewIds(starredViewIds);
    },
    /** Whether project pickers list hidden and archived projects, kept until the user turns it off. */
    get showInactiveProjects(): boolean {
      return showInactiveProjects;
    },
    set showInactiveProjects(value: boolean) {
      if (showInactiveProjects === value) return;
      showInactiveProjects = value;
      saveShowInactiveProjects(value);
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
    projectsForGroup: sidebarProjectsForGroup,
    projectsForGroupIncludingInactive: sidebarProjectsForGroupIncludingInactive,
    visibleGroups: sidebarGroups,
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
    ...actions,
  };
}
