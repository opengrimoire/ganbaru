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
import { listenForProjectChanges, publishProjectChanges } from "$lib/projects/window-sync";
import { applyCalendarEventProjectAssignments } from "$lib/projects/calendar-assignments";
import { configKeyChanged, onConfigChangedElsewhere } from "$lib/vault/config";
import {
  loadSavedActiveProjectId,
  loadSavedCollapsedSidebarProjectIds,
  loadSavedProjectActiveViewIds,
  loadSavedProjectSidebarCollapsed,
  loadSavedShowInactiveProjects,
  loadSavedStarredProjectIds,
  loadSavedStarredViewIds,
  pruneProjectActiveViewIds,
  STARRED_PROJECT_IDS_CONFIG_KEY,
  STARRED_VIEW_IDS_CONFIG_KEY,
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
import type { WorkspacePaneScope } from "$lib/workspace-tabs/model";
import { getWorkspacePaneScope } from "$lib/workspace-tabs/pane-scope";
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
let hostableViewTypes = $state<ProjectHostedViewType[]>([]);
let collapsedSidebarProjectIds = $state<string[]>(loadSavedCollapsedSidebarProjectIds());
let starredProjectIds = $state<string[]>(loadSavedStarredProjectIds());
let starredViewIds = $state<string[]>(loadSavedStarredViewIds());
let showInactiveProjects = $state(loadSavedShowInactiveProjects());

onConfigChangedElsewhere((keys) => {
  if (configKeyChanged(keys, STARRED_PROJECT_IDS_CONFIG_KEY)) starredProjectIds = loadSavedStarredProjectIds();
  if (configKeyChanged(keys, STARRED_VIEW_IDS_CONFIG_KEY)) starredViewIds = loadSavedStarredViewIds();
});
let loadedProjectIds = $state<string[]>([]);
let loadRequestId = 0;
let initialLoadPromise: Promise<void> | null = null;
const optionalDataLoadedKeys = new Set<string>();
const optionalDataRequests = new Map<string, Promise<void>>();
let optionalDataVersion = $state(0);
let taskMutationRevision = $state(0);
const taskDetailCache = new ProjectTaskDetailCache(4 * 1024 * 1024);
const taskDetailRequests = new Map<string, Promise<ProjectTaskDetailData>>();
/** Task details requested most recently, newest last; refreshed in place when another window changes their project. */
const recentDetailTaskIds: string[] = [];
const RECENT_DETAIL_TASK_LIMIT = 4;
/** Quiet reloads tried while this window keeps committing before waiting for the next change. */
const OTHER_WINDOW_REFRESH_ATTEMPTS = 3;
let listeningForOtherWindows = false;
let otherWindowRefresh: Promise<void> | null = null;
let otherWindowRefreshQueued = false;
const selectors = createProjectStoreSelectors(() => snapshot);

/**
 * What one split pane's Projects view shows: its project, the view open in each project, its sidebar, and its
 * task page. The project data and the other preferences are shared by the window. Only the primary pane
 * remembers its choices for the next launch; a secondary pane starts from the primary's.
 */
class ProjectsPane {
  selectedProjectId = $state<string | null>(null);
  activeViewIds = $state<Record<string, string>>({});
  sidebarCollapsed = $state(false);
  taskViewPage = $state<ProjectTaskViewPage | null>(null);
  taskViewLoading = $state(false);
  taskViewError = $state<string | null>(null);
  selectProjectRequestId = 0;
  readonly taskViewRequestGate = new ProjectTaskViewRequestGate();
  readonly owner: boolean;

  constructor(primary: ProjectsPane | null) {
    this.owner = primary === null;
    this.selectedProjectId = primary ? primary.selectedProjectId : loadSavedActiveProjectId();
    this.activeViewIds = primary ? { ...primary.activeViewIds } : loadSavedProjectActiveViewIds();
    this.sidebarCollapsed = primary ? primary.sidebarCollapsed : loadSavedProjectSidebarCollapsed();
  }
}

const primaryPane = new ProjectsPane(null);
const panes: Partial<Record<WorkspacePaneScope, ProjectsPane>> = { primary: primaryPane };

function paneFor(scope: WorkspacePaneScope): ProjectsPane {
  return panes[scope] ??= new ProjectsPane(primaryPane);
}

function allPanes(): ProjectsPane[] {
  return Object.values(panes);
}

/** The projects the window's panes show, each once. */
function shownProjectIds(): (string | null)[] {
  return [...new Set(allPanes().map((pane) => pane.selectedProjectId))];
}

function ensureSelectedProject(pane: ProjectsPane): void {
  if (snapshot.projects.length === 0) return;
  if (pane.selectedProjectId && snapshot.projects.some((project) => project.id === pane.selectedProjectId)) {
    return;
  }
  setSelectedProjectId(pane, firstSidebarProjectId());
}

function cancelTaskView(pane: ProjectsPane): void {
  pane.taskViewRequestGate.cancel();
  pane.taskViewLoading = false;
}

function setSelectedProjectId(pane: ProjectsPane, projectId: string | null): void {
  if (pane.selectedProjectId === projectId) return;
  cancelTaskView(pane);
  pane.selectedProjectId = projectId;
  if (pane.owner) saveActiveProjectId(projectId);
}

function deselectProjectsOfGroup(groupId: string): void {
  for (const pane of allPanes()) {
    if (pane.selectedProjectId && selectors.projectById(pane.selectedProjectId)?.groupId === groupId) {
      setSelectedProjectId(pane, null);
    }
  }
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

/** Returns the open view of a pane's project: the remembered one while it exists, else its first view. */
function activeProjectView(pane: ProjectsPane): ProjectView | undefined {
  ensureSelectedProject(pane);
  const projectId = pane.selectedProjectId;
  return resolveActiveProjectView(viewsForProject(projectId), projectId ? pane.activeViewIds[projectId] : null);
}

/** Opens one of a project's views in a pane and remembers it for the pane's next visit to that project. */
function selectView(pane: ProjectsPane, view: ProjectView): void {
  if (pane.activeViewIds[view.projectId] === view.id) return;
  const projectIds = new Set(snapshot.projects.map((project) => project.id));
  pane.activeViewIds = pruneProjectActiveViewIds({ ...pane.activeViewIds, [view.projectId]: view.id }, projectIds);
  if (pane.owner) saveProjectActiveViewIds(pane.activeViewIds);
}

/** Opens a pane project's view of a task type; does nothing when the project has no view of that type. */
function setActiveView(pane: ProjectsPane, viewType: ProjectViewId): void {
  const target = projectViewForType(viewsForProject(pane.selectedProjectId), activeProjectView(pane), viewType);
  if (target) selectView(pane, target);
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

/**
 * Load the workspace and one project's data. A quiet reload refreshes data another window changed: it keeps the
 * current task view request and loading state, and re-reads recently opened task details alongside the workspace so
 * an open detail panel never falls back to a summary. When a pane asks, the project the workspace resolves becomes
 * that pane's selection and its pending task view request is dropped.
 */
async function reload(
  projectId: string | null = primaryPane.selectedProjectId,
  quiet = false,
  pane: ProjectsPane | null = null,
): Promise<void> {
  listenForOtherWindows();
  if (pane && !quiet) cancelTaskView(pane);
  const requestId = ++loadRequestId;
  const isInitialLoad = !loaded;
  const customEmojiKey = projectOptionalDataKey("custom_emojis", null);
  const refreshCustomEmojis = optionalDataLoadedKeys.has(customEmojiKey)
    || optionalDataRequests.has(customEmojiKey);
  if (!quiet) {
    loading = true;
    loadError = null;
  }
  try {
    const [response, details] = await Promise.all([
      isInitialLoad ? loadProjectsWorkspace(projectId) : refreshProjectsWorkspace(projectId),
      quiet ? loadRecentTaskDetails(projectId) : Promise.resolve([]),
    ]);
    if (requestId !== loadRequestId) return;
    const resolvedProjectId = response.resolvedProjectId;
    const optionalKindsToRefresh = resolvedProjectId
      ? PROJECT_SCOPED_OPTIONAL_DATA_KINDS.filter((kind) => {
          const key = projectOptionalDataKey(kind, resolvedProjectId);
          return optionalDataLoadedKeys.has(key) || optionalDataRequests.has(key);
        })
      : [];
    if (quiet && resolvedProjectId) {
      for (const task of snapshot.tasks) {
        if (task.projectId === resolvedProjectId) taskDetailCache.deleteTask(task.id);
      }
    }
    applyLoadedSnapshot(response.snapshot, resolvedProjectId);
    if (quiet) {
      restoreTaskDetails(details, resolvedProjectId);
      taskMutationRevision += 1;
    }
    if (pane) setSelectedProjectId(pane, resolvedProjectId);
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
    if (!quiet) loadError = error instanceof Error ? error.message : String(error);
    throw error;
  } finally {
    if (requestId === loadRequestId) loading = false;
  }
}

async function ensureLoaded(): Promise<void> {
  if (loaded) return;
  if (!initialLoadPromise) {
    initialLoadPromise = reload(primaryPane.selectedProjectId, false, primaryPane).finally(() => {
      initialLoadPromise = null;
    });
  }
  await initialLoadPromise;
}

/** Load a project's data without changing any pane's selection. */
async function ensureProjectData(projectId: string | null | undefined): Promise<void> {
  if (!projectId) return;
  if (!loaded) await ensureLoaded();
  if (isProjectDataLoaded(projectId)) return;
  await reload(projectId);
}

async function selectProject(pane: ProjectsPane, projectId: string | null): Promise<void> {
  const requestId = ++pane.selectProjectRequestId;
  if (!projectId || isProjectDataLoaded(projectId)) {
    setSelectedProjectId(pane, projectId);
    return;
  }
  await ensureProjectData(projectId);
  if (requestId !== pane.selectProjectRequestId) return;
  if (!isProjectDataLoaded(projectId)) return;
  setSelectedProjectId(pane, projectId);
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
  pane: ProjectsPane,
  request: ProjectTaskViewRequest,
  append = false,
  retainedTaskIds: readonly string[] = [],
): Promise<void> {
  const gate = pane.taskViewRequestGate;
  const requestId = gate.begin();
  pane.taskViewLoading = true;
  pane.taskViewError = null;
  try {
    const page = await loadProjectTaskView(request);
    if (!gate.isCurrent(requestId) || page.projectId !== request.projectId || page.view !== request.view) return;
    mergeTaskViewPage(page, append, retainedTaskIds);
    const current = pane.taskViewPage;
    pane.taskViewPage = append && current
      ? {
          ...page,
          tasks: [...new Map([...current.tasks, ...page.tasks].map((task) => [task.id, task])).values()],
        }
      : page;
  } catch (error) {
    if (!gate.isCurrent(requestId)) return;
    pane.taskViewError = error instanceof Error ? error.message : String(error);
    throw error;
  } finally {
    if (gate.isCurrent(requestId)) pane.taskViewLoading = false;
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
  rememberDetailTask(taskId);
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

function rememberDetailTask(taskId: string): void {
  const index = recentDetailTaskIds.indexOf(taskId);
  if (index >= 0) recentDetailTaskIds.splice(index, 1);
  recentDetailTaskIds.push(taskId);
  if (recentDetailTaskIds.length > RECENT_DETAIL_TASK_LIMIT) recentDetailTaskIds.shift();
}

/** Fresh details of the recently opened tasks of one project; a task removed elsewhere is skipped. */
async function loadRecentTaskDetails(projectId: string | null): Promise<ProjectTaskDetailData[]> {
  const taskIds = recentDetailTaskIds.filter((taskId) => {
    const task = selectors.taskById(taskId);
    return task?.detailLoaded && task.projectId === projectId;
  });
  const details = await Promise.all(taskIds.map((taskId) => loadProjectTaskDetail(taskId).catch(() => null)));
  return details.filter((detail): detail is ProjectTaskDetailData => detail !== null);
}

function restoreTaskDetails(details: readonly ProjectTaskDetailData[], projectId: string | null): void {
  for (const detail of details) {
    if (detail.task.projectId !== projectId || !selectors.taskById(detail.task.id)) continue;
    taskDetailCache.set(detail);
    mergeTaskDetail(detail);
  }
}

/** Read one task's detail from the database, replacing any cached copy, and return the canonical task. */
async function refreshTaskDetail(projectId: string, taskId: string): Promise<ProjectTask> {
  const detail = await loadProjectTaskDetail(taskId);
  if (detail.task.projectId !== projectId) throw new Error("Task detail project mismatch");
  taskDetailCache.set(detail);
  mergeTaskDetail(detail);
  taskMutationRevision += 1;
  return detail.task;
}

function listenForOtherWindows(): void {
  if (listeningForOtherWindows) return;
  listeningForOtherWindows = true;
  void listenForProjectChanges((projectIds) => {
    void refreshFromOtherWindow(projectIds);
  }).catch((error: unknown) => {
    listeningForOtherWindows = false;
    console.warn("Listening for Projects changes from other windows failed", error);
  });
}

/**
 * Show what another window committed. Changed projects no pane shows reload when next selected; the shown ones
 * reload quietly once local commands settle, and again if this window commits while they read.
 */
function refreshFromOtherWindow(projectIds: ReadonlySet<string>): Promise<void> {
  const shown = new Set(shownProjectIds());
  loadedProjectIds = loadedProjectIds.filter((projectId) => shown.has(projectId) || !projectIds.has(projectId));
  if (otherWindowRefresh) {
    otherWindowRefreshQueued = true;
    return otherWindowRefresh;
  }
  otherWindowRefresh = (async () => {
    do {
      otherWindowRefreshQueued = false;
      await reloadQuietlyBetweenCommands();
    } while (otherWindowRefreshQueued);
  })()
    .catch((error: unknown) => console.warn("Refreshing Projects after another window's change failed", error))
    .finally(() => { otherWindowRefresh = null; });
  return otherWindowRefresh;
}

async function reloadQuietlyBetweenCommands(): Promise<void> {
  for (let attempt = 0; attempt < OTHER_WINDOW_REFRESH_ATTEMPTS; attempt += 1) {
    if (!loaded) return;
    await actions.whenMutationsIdle();
    const committed = actions.readMutationActivity().committed;
    const shown = shownProjectIds().filter((projectId): projectId is string => Boolean(projectId));
    // One at a time, since a newer workspace load supersedes the one in flight.
    for (const projectId of shown.length > 0 ? shown : [null]) await reload(projectId, true);
    if (actions.readMutationActivity().committed === committed) return;
  }
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
    try {
      await applyCalendarEventProjectAssignments(assignments);
    } catch (error) {
      console.error("Failed to apply committed calendar event project assignments", error);
    }
  },
  deselectProjectsOfGroup,
  reload,
  ensureProjectData,
  ensureTaskDetailData,
  refreshTaskDetail,
  onCommitted: (mutation) => publishProjectChanges(mutation, shownProjectIds()),
});

function createProjectsStore(pane: ProjectsPane) {
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
      ensureSelectedProject(pane);
      return pane.selectedProjectId;
    },
    set selectedProjectId(projectId: string | null) {
      setSelectedProjectId(pane, projectId);
    },
    get selectedProject(): Project | undefined {
      ensureSelectedProject(pane);
      return selectors.projectById(pane.selectedProjectId);
    },
    get selectedGroup(): ProjectGroup | undefined {
      const selectedProject = selectors.projectById(pane.selectedProjectId);
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
      return activeProjectView(pane);
    },
    get activeViewId(): string | null {
      return activeProjectView(pane)?.id ?? null;
    },
    /** Task view type rendered in the Projects workspace, or null while a hosted view is open or no view exists. */
    get activeTaskView(): ProjectViewId | null {
      const view = activeProjectView(pane);
      return view && isProjectTaskViewType(view.viewType) ? view.viewType : null;
    },
    /**
     * Task view type that drives task queries; it reads as the list view while no task view is open. Setting it opens
     * the selected project's view of that type, keeping the active view when it already matches.
     */
    get activeView(): ProjectViewId {
      const view = activeProjectView(pane);
      return view && isProjectTaskViewType(view.viewType) ? view.viewType : "list";
    },
    set activeView(viewType: ProjectViewId) {
      setActiveView(pane, viewType);
    },
    /** Hosted workspace open in the Projects tab, or null while a task view is open. */
    get hostedPlace(): ProjectHostedViewType | null {
      const view = activeProjectView(pane);
      return view && isProjectHostedViewType(view.viewType) ? view.viewType : null;
    },
    selectView(view: ProjectView): void {
      selectView(pane, view);
    },
    /** Leaves a hosted workspace for the project's first task view, for routes that open a project or one of its tasks. */
    showProjectView(): void {
      const view = activeProjectView(pane);
      if (!view || !isProjectHostedViewType(view.viewType)) return;
      const taskView = viewsForProject(view.projectId).find((entry) => isProjectTaskViewType(entry.viewType));
      if (taskView) selectView(pane, taskView);
    },
    /** Whether the docked desktop Projects sidebar is collapsed to its rail. */
    get sidebarCollapsed(): boolean {
      return pane.sidebarCollapsed;
    },
    set sidebarCollapsed(collapsed: boolean) {
      if (pane.sidebarCollapsed === collapsed) return;
      pane.sidebarCollapsed = collapsed;
      if (pane.owner) saveProjectSidebarCollapsed(collapsed);
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
      return pane.taskViewPage;
    },
    get taskViewLoading(): boolean {
      return pane.taskViewLoading;
    },
    get taskViewError(): string | null {
      return pane.taskViewError;
    },
    get taskMutationRevision(): number {
      return taskMutationRevision;
    },
    /** Reload the workspace with a project's data, which becomes this pane's selection; defaults to its current one. */
    load(projectId: string | null = pane.selectedProjectId, quiet = false): Promise<void> {
      return reload(projectId, quiet, pane);
    },
    ensureLoaded,
    ensureProjectData,
    ensureProjectViewData,
    ensureProjectToolbarData,
    loadTaskView(request: ProjectTaskViewRequest, append = false, retainedTaskIds: readonly string[] = []): Promise<void> {
      return loadTaskView(pane, request, append, retainedTaskIds);
    },
    ensureTaskDetailData,
    ensureCustomEmojis,
    isProjectOptionalDataLoaded,
    isProjectViewDataLoaded,
    isProjectDataLoaded,
    selectProject(projectId: string | null): Promise<void> {
      return selectProject(pane, projectId);
    },
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
    /** Create a project and select it in this pane. */
    async addProject(...args: Parameters<typeof actions.addProject>): Promise<string | undefined> {
      const projectId = await actions.addProject(...args);
      if (projectId) setSelectedProjectId(pane, projectId);
      return projectId;
    },
  };
}

/** The Projects store as one split pane sees it. */
export type ProjectsStore = ReturnType<typeof createProjectsStore>;

const stores: Partial<Record<WorkspacePaneScope, ProjectsStore>> = {};

/** The Projects store of one split pane; the panes share project data and differ in what they show. */
export function getProjectsStore(scope: WorkspacePaneScope): ProjectsStore {
  return stores[scope] ??= createProjectsStore(paneFor(scope));
}

/** The primary pane's Projects store, for app-wide code outside a pane. */
export function getProjects(): ProjectsStore {
  return getProjectsStore("primary");
}

/** The Projects store of the calling view's split pane. Call during component initialization. */
export function useProjects(): ProjectsStore {
  return getProjectsStore(getWorkspacePaneScope());
}
