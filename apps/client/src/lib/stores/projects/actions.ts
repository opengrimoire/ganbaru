import type { ProjectDependencyCascadePreview } from "$lib/api/project-cascade";
import {
  applyProjectTaskBulk,
  reorderProjectItem,
  type ProjectTaskBulkChange,
  type ProjectReorderItem,
  createProjectChecklistItem,
  createProjectCustomEmoji,
  createProjectCustomField,
  createProjectCustomFieldOption,
  createProjectTag,
  createProject as createProjectBackend,
  createProjectGroup,
  createProjectPriority,
  createProjectSection,
  createProjectStatus,
  createProjectTask,
  createProjectTaskDependency,
  createProjectView,
  deleteProjectChecklistItem,
  deleteProjectCustomEmoji,
  deleteProjectCustomField,
  deleteProjectCustomFieldOption,
  deleteProjectGroup,
  deleteProjectPriority,
  deleteProjectStatus,
  deleteProjectTag,
  deleteProjectTaskDependency,
  deleteProjectView,
  linkProjectTaskEvent,
  linkProjectTaskTag,
  renameProjectView,
  reorderProjectViews,
  searchProjectLinkableEvents,
  setProjectGroupCollapsed,
  unlinkProjectTaskEvent,
  unlinkProjectTaskTag,
  updateProject as updateProjectBackend,
  updateProjectNotesSettings,
  updateProjectChecklistItem,
  updateProjectCustomField,
  updateProjectCustomFieldOption,
  updateProjectCustomFieldValue,
  updateProjectGroup,
  updateProjectPriority,
  updateProjectSection,
  updateProjectStatus,
  updateProjectTag,
  updateProjectTask,
  upsertProjectViewPreference,
} from "$lib/api/projects";
import {
  invalidateAssetUrl,
  invalidateAssetUrlKind,
} from "$lib/api/asset-url-cache";
import type { EventColor } from "$lib/calendar/types";
import type { NotesPageOpenMode } from "$lib/notes/pages/open-mode";
import type { NotesHistoryRetentionDays } from "$lib/notes/history/retention";
import {
  TASK_LIST_COLUMN_WIDTHS_PREFERENCE_KEY,
  taskListColumnWidthsPreferenceValue,
  type ProjectTaskListColumnWidths,
} from "$lib/projects/list/view";
import { normalizeProjectName } from "$lib/projects/text";
import { projectHasLockedSystemIdentity } from "$lib/projects/system-defaults";
import { PROJECT_LIST_PRESENTATION_KEY, type ProjectListPresentation } from "$lib/projects/list/presentation";
import { PROJECT_VIEW_QUERY_PREFERENCE_KEY, serializeProjectViewQuery, type ProjectViewQuery } from "$lib/projects/view-query";
import { applyProjectMutation } from "$lib/projects/snapshot/mutations";
import { parseProjectIcon } from "$lib/projects/icons/values";
import {
  checklistItemUpdatePayload,
  customFieldOptionUpdatePayload,
  customFieldUpdatePayload,
  groupUpdatePayload,
  projectUpdatePayload,
  sectionUpdatePayload,
  tagUpdatePayload,
  taskUpdatePayload,
  type ProjectTaskUpdatePatch,
} from "$lib/projects/snapshot/update-payloads";
import {
  TASK_LIST_COLUMNS_PREFERENCE_KEY,
  taskListColumnsPreferenceValue,
} from "$lib/projects/tasks/list-columns";
import {
  PROJECT_TAG_DEFAULT_COLOR,
  PROJECT_TEMPLATE_DEFAULTS,
  type Project,
  type ProjectChecklistItem,
  type ProjectCreate,
  type ProjectCustomEmoji,
  type ProjectCustomField,
  type ProjectCustomFieldOption,
  type ProjectCustomFieldType,
  type ProjectCustomFieldValueUpdate,
  type ProjectGroup,
  type ProjectLifecycleStatus,
  type ProjectLinkableEvent,
  type ProjectMutation,
  type ProjectMutationRemoval,
  type ProjectPriority,
  type ProjectPriorityConfig,
  type ProjectsSnapshot,
  type ProjectSection,
  type ProjectStatus,
  type ProjectStatusCategory,
  type ProjectTag,
  type ProjectTask,
  type ProjectTaskEventLink,
  type ProjectTaskListColumn,
  type ProjectTaskType,
  type ProjectTemplateId,
  type ProjectUpdate,
  type ProjectViewType,
} from "$lib/projects/types";
import { invalidateProjectTaskReminderSchedule } from "$lib/projects/task-reminder-schedule.svelte";
import type { ProjectStoreSelectors } from "./selectors";

export interface ProjectStoreActionContext {
  selectors: ProjectStoreSelectors;
  readSnapshot: () => ProjectsSnapshot;
  updateSnapshot: (updater: (snapshot: ProjectsSnapshot) => ProjectsSnapshot) => void;
  applyCalendarEventProjectAssignments: (
    assignments: readonly { eventId: string; projectId: string }[],
  ) => Promise<void>;
  readSelectedProjectId: () => string | null;
  readLoadGeneration: () => number;
  setSelectedProjectId: (projectId: string | null) => void;
  reload: (projectId?: string | null) => Promise<void>;
  ensureProjectData: (projectId: string | null | undefined) => Promise<void>;
  ensureTaskDetailData: (projectId: string, taskId: string) => Promise<void>;
}

/**
 * Creates the write side of the project store around state hooks owned by the Svelte store.
 */
export function createProjectStoreActions(context: ProjectStoreActionContext) {
  const {
    ensureProjectData,
    ensureTaskDetailData,
    applyCalendarEventProjectAssignments,
    readLoadGeneration,
    readSelectedProjectId,
    readSnapshot,
    selectors,
    setSelectedProjectId,
    updateSnapshot,
  } = context;
  const mutationGenerations = new Map<string, number>();
  interface PendingProjectOperation {
    operationId: string;
    projectId: string;
    removedProjects: Set<string>;
    removedFields: Set<string>;
    removedOptions: Set<string>;
  }
  const pendingBulkOperations = new Map<string, PendingProjectOperation>();
  let bulkLoadGeneration = readLoadGeneration();

  async function taskForMutation(task: ProjectTask): Promise<ProjectTask> {
    if (task.detailLoaded) return task;
    await ensureTaskDetailData(task.projectId, task.id);
    const hydrated = selectors.taskById(task.id);
    if (!hydrated?.detailLoaded) throw new Error("Task detail was not loaded before mutation");
    return hydrated;
  }

  async function commitMutation(
    key: string,
    request: () => Promise<ProjectMutation>,
    originatingLoadGeneration = readLoadGeneration(),
    pending?: PendingProjectOperation,
  ): Promise<ProjectMutation> {
    const generation = (mutationGenerations.get(key) ?? 0) + 1;
    mutationGenerations.set(key, generation);
    const mutation = await request();
    invalidateProjectTaskReminderSchedule();
    if (
      mutationGenerations.get(key) === generation
      && readLoadGeneration() === originatingLoadGeneration
    ) {
      updateSnapshot((current) => {
        rememberPendingRemovals(current, mutation.removals);
        return applyProjectMutation(current, pending ? filterRemovedReceiptRows(mutation, pending) : mutation);
      });
      await applyCalendarEventProjectAssignments(mutation.calendarEventProjectAssignments);
    }
    return mutation;
  }

  /** Retain deletion evidence only while an intent may still return an old receipt. */
  function rememberPendingRemovals(current: ProjectsSnapshot, removals: ProjectMutationRemoval[]): void {
    for (const pending of pendingBulkOperations.values()) {
      for (const removal of removals) {
        if (removal.kind === "project") pending.removedProjects.add(removal.id);
        if (removal.kind === "group") {
          for (const project of current.projects) {
            if (project.groupId === removal.id) pending.removedProjects.add(project.id);
          }
        }
        if (removal.kind === "custom_field") pending.removedFields.add(removal.id);
        if (removal.kind === "custom_field_option") pending.removedOptions.add(removal.id);
      }
    }
  }

  /**
   * Drop receipt rows whose project, field, or option was explicitly removed while the intent
   * was pending. Absence from a paginated view is not deletion evidence. Options carry no project
   * id, but every pending intent is scoped to one project, so its receipt options belong to fields
   * of `pending.projectId`.
   */
  function filterRemovedReceiptRows(mutation: ProjectMutation, pending: PendingProjectOperation): ProjectMutation {
    const tasks = mutation.changed.tasks.filter((task) => !pending.removedProjects.has(task.projectId));
    const taskIds = new Set(tasks.map((task) => task.id));
    return { ...mutation, changed: { ...mutation.changed, tasks,
      taskChangeEvents: mutation.changed.taskChangeEvents.filter((event) => taskIds.has(event.taskId)),
      customFields: mutation.changed.customFields.filter((field) =>
        !pending.removedFields.has(field.id) && !pending.removedProjects.has(field.projectId)),
      customFieldOptions: mutation.changed.customFieldOptions.filter((option) =>
        !pending.removedProjects.has(pending.projectId)
        && !pending.removedOptions.has(option.id) && !pending.removedFields.has(option.fieldId)),
    } };
  }

  function pendingOperation(key: string, projectId: string): PendingProjectOperation {
    const existing = pendingBulkOperations.get(key);
    if (existing) return existing;
    const pending: PendingProjectOperation = { operationId: crypto.randomUUID(), projectId,
      removedProjects: new Set(), removedFields: new Set(), removedOptions: new Set() };
    pendingBulkOperations.set(key, pending);
    return pending;
  }

  function changedById<T extends { id: string }>(
    values: readonly T[],
    id: string,
    entity: string,
  ): T {
    const value = values.find((candidate) => candidate.id === id);
    if (!value) throw new Error(`${entity} mutation did not return its authoritative row`);
    return value;
  }

  function projectAssetPath(icon: string): string | null {
    const parsed = parseProjectIcon(icon);
    return parsed.kind === "asset" ? parsed.relativePath : null;
  }

  function invalidateReplacedProjectIcon(previousIcon: string, nextIcon: string): void {
    const previousPath = projectAssetPath(previousIcon);
    if (!previousPath || previousPath === projectAssetPath(nextIcon)) return;
    invalidateAssetUrl("project-icon", previousPath);
  }

  async function addGroup(name: string): Promise<void> {
    const displayName = normalizeProjectName(name);
    if (!displayName) return;
    const id = crypto.randomUUID();
    await commitMutation(`group:${id}`, () => createProjectGroup({
      id,
      name: displayName,
      icon: "lucide:folder",
      color: null,
      sortOrder: selectors.nextGroupSortOrder(),
    }));
  }

  async function setGroupCollapsed(groupId: string, collapsed: boolean): Promise<void> {
    await commitMutation(`group:${groupId}`, () => setProjectGroupCollapsed(groupId, collapsed));
  }

  async function updateGroup(
    group: ProjectGroup,
    patch: Partial<Pick<ProjectGroup, "name" | "icon" | "color" | "sortOrder" | "collapsed">>,
  ): Promise<void> {
    const nextName = normalizeProjectName(patch.name ?? group.name);
    if (!nextName) return;
    const update = groupUpdatePayload(group, { ...patch, name: nextName });
    await commitMutation(`group:${group.id}`, () => updateProjectGroup(update));
    invalidateReplacedProjectIcon(group.icon, update.icon);
  }

  async function removeGroup(group: ProjectGroup): Promise<void> {
    await commitMutation(`group:${group.id}`, () => deleteProjectGroup(group.id));
    invalidateAssetUrlKind("project-icon");
    const selectedProjectId = readSelectedProjectId();
    if (selectedProjectId && group.id === selectors.projectById(selectedProjectId)?.groupId) {
      setSelectedProjectId(null);
    }
  }

  async function addProject(groupId: string, name: string, templateId: ProjectTemplateId = "blank"): Promise<void> {
    const displayName = normalizeProjectName(name);
    if (!displayName) return;
    const templateDefaults = PROJECT_TEMPLATE_DEFAULTS[templateId];
    const project: ProjectCreate = {
      id: crypto.randomUUID(),
      groupId,
      templateId,
      name: displayName,
      icon: templateDefaults.icon,
      color: templateDefaults.color,
      sortOrder: selectors.nextProjectSortOrder(groupId),
      defaultEventName: templateDefaults.defaultEventName,
      defaultEventTimeMode: templateDefaults.defaultEventTimeMode,
      defaultEventDurationMinutes: templateDefaults.defaultEventDurationMinutes,
      defaultPomodoroMode: templateDefaults.defaultPomodoroMode,
      defaultPomodoroPresetKey: templateDefaults.defaultPomodoroPresetKey,
      defaultPomodoroFocusMinutes: templateDefaults.defaultPomodoroFocusMinutes,
      defaultPomodoroShortBreakMinutes: templateDefaults.defaultPomodoroShortBreakMinutes,
      defaultPomodoroLongBreakMinutes: templateDefaults.defaultPomodoroLongBreakMinutes,
      defaultPomodoroLongBreakAfterFocusCount: templateDefaults.defaultPomodoroLongBreakAfterFocusCount,
      defaultIdleSettingsSource: templateDefaults.defaultIdleSettingsSource,
      defaultIdlePauseEnabled: templateDefaults.defaultIdlePauseEnabled,
      defaultIdleThresholdMinutes: templateDefaults.defaultIdleThresholdMinutes,
    };
    await commitMutation(`project:${project.id}`, () => createProjectBackend(project));
    setSelectedProjectId(project.id);
  }

  async function updateProject(project: ProjectUpdate): Promise<void> {
    const previousIcon = selectors.projectById(project.id)?.icon ?? project.icon;
    await commitMutation(`project:${project.id}`, () => updateProjectBackend(project));
    invalidateReplacedProjectIcon(previousIcon, project.icon);
  }

  async function setNotesSettings(
    projectId: string,
    openMode: NotesPageOpenMode | null,
    historyRetentionDays: NotesHistoryRetentionDays | null,
  ): Promise<void> {
    await commitMutation(`project:${projectId}`, () =>
      updateProjectNotesSettings(projectId, openMode, historyRetentionDays));
  }

  /** Renames a project. Built-in projects keep their name, and an empty name is rejected. */
  async function renameProject(project: Project, name: string): Promise<void> {
    assertProjectIdentityEditable(project);
    const normalized = normalizeProjectName(name);
    if (!normalized) throw new Error("A project name is required");
    if (normalized === project.name) return;
    await updateProject(projectUpdatePayload(project, { name: normalized }));
  }

  /** Moves a project to the end of another visible group. Built-in projects keep their group. */
  async function moveProjectToGroup(project: Project, groupId: string): Promise<void> {
    assertProjectIdentityEditable(project);
    if (groupId === project.groupId) return;
    if (!selectors.visibleGroups().some((group) => group.id === groupId)) {
      throw new Error(`Project group ${groupId} is not available`);
    }
    const sortOrder = Math.max(
      0,
      ...selectors.projectsForGroupIncludingInactive(groupId).map((entry) => entry.sortOrder),
    ) + 1000;
    await updateProject(projectUpdatePayload(project, { groupId, sortOrder }));
  }

  /** Sets whether a project is active, hidden, or archived. */
  async function setProjectStatus(project: Project, status: ProjectLifecycleStatus): Promise<void> {
    if (status === project.status) return;
    await updateProject(projectUpdatePayload(project, { status }));
  }

  function assertProjectIdentityEditable(project: Project): void {
    if (projectHasLockedSystemIdentity(project)) {
      throw new Error(`Built-in project ${project.id} keeps its name and group`);
    }
  }

  async function addSection(projectId: string, name: string): Promise<void> {
    const displayName = normalizeProjectName(name);
    if (!displayName) return;
    const id = crypto.randomUUID();
    await commitMutation(`section:${id}`, () => createProjectSection({
      id,
      projectId,
      name: displayName,
      sortOrder: selectors.nextSectionSortOrder(projectId),
    }));
  }

  async function updateSection(
    section: ProjectSection,
    patch: Partial<Pick<ProjectSection, "name" | "sortOrder" | "collapsed" | "hiddenAt" | "archivedAt">>,
  ): Promise<void> {
    const nextName = normalizeProjectName(patch.name ?? section.name);
    if (!nextName) return;
    await commitMutation(`section:${section.id}`, () =>
      updateProjectSection(sectionUpdatePayload(section, { ...patch, name: nextName })));
  }

  async function hideSection(section: ProjectSection): Promise<void> {
    await updateSection(section, {
      hiddenAt: new Date().toISOString(),
      archivedAt: undefined,
    });
  }

  async function archiveSection(section: ProjectSection): Promise<void> {
    await updateSection(section, {
      hiddenAt: undefined,
      archivedAt: new Date().toISOString(),
    });
  }

  async function restoreSection(section: ProjectSection): Promise<void> {
    await updateSection(section, {
      hiddenAt: undefined,
      archivedAt: undefined,
    });
  }

  /** Create a status, optionally retaining an identity allocated by a settings draft. */
  async function addStatus(
    projectId: string,
    name: string,
    category: ProjectStatusCategory,
    color: EventColor,
    identity?: { id: string; sortOrder: number },
  ): Promise<void> {
    const displayName = normalizeProjectName(name);
    if (!displayName) return;
    const id = identity?.id ?? crypto.randomUUID();
    await commitMutation(`status:${id}`, () => createProjectStatus({
      id,
      projectId,
      name: displayName,
      category,
      color,
      sortOrder: identity?.sortOrder ?? selectors.nextStatusSortOrder(projectId),
      terminal: category === "done",
    }));
  }

  async function updateStatus(
    status: ProjectStatus,
    patch: Partial<Pick<ProjectStatus, "name" | "category" | "color" | "sortOrder">>,
  ): Promise<void> {
    const displayName = normalizeProjectName(patch.name ?? status.name);
    if (!displayName) return;
    const category = patch.category ?? status.category;
    await commitMutation(`status:${status.id}`, () => updateProjectStatus({
      id: status.id,
      name: displayName,
      category,
      color: patch.color ?? status.color,
      sortOrder: patch.sortOrder ?? status.sortOrder,
      terminal: category === "done",
    }));
  }

  async function moveStatus(status: ProjectStatus, direction: -1 | 1): Promise<void> {
    const ordered = selectors.statusesForProject(status.projectId);
    const index = ordered.findIndex((entry) => entry.id === status.id);
    const target = ordered[index + direction];
    if (index < 0 || !target) return;
    await commitMutation(`status:${status.id}`, () => updateProjectStatus({
      id: status.id,
      name: status.name,
      category: status.category,
      color: status.color,
      sortOrder: target.sortOrder,
      terminal: status.category === "done",
    }));
    await commitMutation(`status:${target.id}`, () => updateProjectStatus({
      id: target.id,
      name: target.name,
      category: target.category,
      color: target.color,
      sortOrder: status.sortOrder,
      terminal: target.category === "done",
    }));
  }

  async function removeStatus(statusId: string): Promise<void> {
    await commitMutation(`status:${statusId}`, () => deleteProjectStatus(statusId));
  }

  /** Create a priority, optionally retaining an identity allocated by a settings draft. */
  async function addPriority(
    projectId: string,
    name: string,
    color: EventColor,
    identity?: { id: string; sortOrder: number },
  ): Promise<void> {
    const displayName = normalizeProjectName(name);
    if (!displayName) return;
    const id = identity?.id ?? crypto.randomUUID();
    await commitMutation(`priority:${id}`, () => createProjectPriority({
      id,
      projectId,
      name: displayName,
      color,
      sortOrder: identity?.sortOrder ?? selectors.nextPrioritySortOrder(projectId),
    }));
  }

  async function updatePriority(
    priority: ProjectPriorityConfig,
    patch: Partial<Pick<ProjectPriorityConfig, "name" | "color" | "sortOrder">>,
  ): Promise<void> {
    const displayName = normalizeProjectName(patch.name ?? priority.name);
    if (!displayName) return;
    await commitMutation(`priority:${priority.id}`, () => updateProjectPriority({
      id: priority.id,
      projectId: priority.projectId,
      name: displayName,
      color: patch.color ?? priority.color,
      sortOrder: patch.sortOrder ?? priority.sortOrder,
    }));
  }

  async function movePriority(priority: ProjectPriorityConfig, direction: -1 | 1): Promise<void> {
    const ordered = selectors.prioritiesForProject(priority.projectId);
    const index = ordered.findIndex((entry) => entry.id === priority.id);
    const target = ordered[index + direction];
    if (index < 0 || !target) return;
    await commitMutation(`priority:${priority.id}`, () => updateProjectPriority({
      id: priority.id,
      projectId: priority.projectId,
      name: priority.name,
      color: priority.color,
      sortOrder: target.sortOrder,
    }));
    await commitMutation(`priority:${target.id}`, () => updateProjectPriority({
      id: target.id,
      projectId: target.projectId,
      name: target.name,
      color: target.color,
      sortOrder: priority.sortOrder,
    }));
  }

  async function removePriority(priority: ProjectPriorityConfig): Promise<void> {
    await commitMutation(`priority:${priority.id}`, () =>
      deleteProjectPriority(priority.projectId, priority.id));
  }

  async function addTask(
    projectId: string,
    title: string,
    sectionId?: string,
    statusId?: string,
    parentTaskId: string | null = null,
  ): Promise<ProjectTask | undefined> {
    const displayTitle = normalizeProjectName(title);
    if (!displayTitle) return;
    await ensureProjectData(projectId);
    const resolvedSectionId = sectionId ?? selectors.defaultSection(projectId)?.id;
    const resolvedStatusId = statusId ?? selectors.defaultStatus(projectId)?.id;
    if (!resolvedSectionId || !resolvedStatusId) {
      throw new Error("project needs at least one section and one status");
    }
    const taskId = crypto.randomUUID();
    const mutation = await commitMutation(`task:${taskId}`, () => createProjectTask({
      id: taskId,
      projectId,
      sectionId: resolvedSectionId,
      statusId: resolvedStatusId,
      parentTaskId,
      title: displayTitle,
    }));
    return changedById(mutation.changed.tasks, taskId, "created task");
  }

  async function addChecklistItem(taskId: string, title: string): Promise<ProjectChecklistItem | undefined> {
    const displayTitle = normalizeProjectName(title);
    if (!displayTitle) return undefined;
    const id = crypto.randomUUID();
    const mutation = await commitMutation(`checklist:${id}`, () => createProjectChecklistItem({
      id,
      taskId,
      title: displayTitle,
      sortOrder: selectors.nextChecklistSortOrder(taskId),
    }));
    return changedById(mutation.changed.checklistItems, id, "created checklist item");
  }

  async function setChecklistItemCompleted(
    item: ProjectChecklistItem,
    completed: boolean,
  ): Promise<void> {
    await commitMutation(`checklist:${item.id}`, () => updateProjectChecklistItem(checklistItemUpdatePayload(item, {
      completedAt: completed ? new Date().toISOString() : undefined,
    })));
  }

  async function updateChecklistItem(
    item: ProjectChecklistItem,
    patch: Partial<Pick<ProjectChecklistItem, "title" | "sortOrder">>,
  ): Promise<void> {
    const nextTitle = normalizeProjectName(patch.title ?? item.title);
    if (!nextTitle) return;
    await commitMutation(`checklist:${item.id}`, () =>
      updateProjectChecklistItem(checklistItemUpdatePayload(item, { ...patch, title: nextTitle })));
  }

  async function moveChecklistItem(item: ProjectChecklistItem, direction: -1 | 1): Promise<void> {
    const ordered = selectors.checklistItemsForTask(item.taskId);
    const index = ordered.findIndex((entry) => entry.id === item.id);
    const target = ordered[index + direction];
    if (index < 0 || !target) return;
    await commitMutation(`checklist:${item.id}`, () =>
      updateProjectChecklistItem(checklistItemUpdatePayload(item, { sortOrder: target.sortOrder })));
    await commitMutation(`checklist:${target.id}`, () =>
      updateProjectChecklistItem(checklistItemUpdatePayload(target, { sortOrder: item.sortOrder })));
  }

  async function removeChecklistItem(itemId: string): Promise<void> {
    await commitMutation(`checklist:${itemId}`, () => deleteProjectChecklistItem(itemId));
  }

  /** Create or reuse a tag, preserving explicit draft identities across the save boundary. */
  async function addTag(
    projectId: string,
    name: string,
    color: ProjectTag["color"] | null = PROJECT_TAG_DEFAULT_COLOR,
    identity?: { id: string; sortOrder: number },
  ): Promise<ProjectTag | undefined> {
    const displayName = normalizeProjectName(name);
    if (!displayName) return undefined;
    const existingTag = selectors.projectTagByName(projectId, displayName);
    if (existingTag) {
      if (identity && existingTag.id !== identity.id) throw new Error("A tag with this name already exists");
      return existingTag;
    }
    const tagId = identity?.id ?? crypto.randomUUID();
    const mutation = await commitMutation(`tag:${tagId}`, () => createProjectTag({
      id: tagId,
      projectId,
      name: displayName,
      color: color ?? PROJECT_TAG_DEFAULT_COLOR,
      sortOrder: identity?.sortOrder ?? selectors.nextTagSortOrder(projectId),
    }));
    return changedById(mutation.changed.tags, tagId, "created tag");
  }

  async function updateTag(
    tag: ProjectTag,
    patch: Partial<Pick<ProjectTag, "name" | "color" | "sortOrder">>,
  ): Promise<void> {
    const nextName = normalizeProjectName(patch.name ?? tag.name);
    if (!nextName) return;
    await commitMutation(`tag:${tag.id}`, () =>
      updateProjectTag(tagUpdatePayload(tag, { ...patch, name: nextName })));
  }

  async function moveTag(tag: ProjectTag, direction: -1 | 1): Promise<void> {
    const ordered = selectors.tagsForProject(tag.projectId);
    const index = ordered.findIndex((entry) => entry.id === tag.id);
    const target = ordered[index + direction];
    if (index < 0 || !target) return;
    await commitMutation(`tag:${tag.id}`, () =>
      updateProjectTag(tagUpdatePayload(tag, { sortOrder: target.sortOrder })));
    await commitMutation(`tag:${target.id}`, () =>
      updateProjectTag(tagUpdatePayload(target, { sortOrder: tag.sortOrder })));
  }

  async function removeTag(tagId: string): Promise<void> {
    await commitMutation(`tag:${tagId}`, () => deleteProjectTag(tagId));
  }

  async function linkTaskTag(taskId: string, tagId: string): Promise<void> {
    await commitMutation(`task-tag:${taskId}:${tagId}`, () => linkProjectTaskTag({ taskId, tagId }));
  }

  async function unlinkTaskTag(taskId: string, tagId: string): Promise<void> {
    await commitMutation(`task-tag:${taskId}:${tagId}`, () => unlinkProjectTaskTag(taskId, tagId));
  }

  async function addAndLinkTaskTag(task: ProjectTask, name: string): Promise<void> {
    const tag = await addTag(task.projectId, name);
    if (!tag) return;
    await linkTaskTag(task.id, tag.id);
  }

  async function addCustomEmoji(name: string, assetPath: string): Promise<ProjectCustomEmoji | undefined> {
    const displayName = normalizeProjectName(name);
    if (!displayName) return undefined;
    const emojiId = crypto.randomUUID();
    const mutation = await commitMutation(`emoji:${emojiId}`, () => createProjectCustomEmoji({
      id: emojiId,
      name: displayName,
      assetPath,
      sortOrder: selectors.nextCustomEmojiSortOrder(),
    }));
    return changedById(mutation.changed.customEmojis, emojiId, "created custom emoji");
  }

  async function removeCustomEmoji(emojiId: string): Promise<void> {
    const assetPath = readSnapshot().customEmojis.find((emoji) => emoji.id === emojiId)?.assetPath;
    await commitMutation(`emoji:${emojiId}`, () => deleteProjectCustomEmoji(emojiId));
    if (assetPath) invalidateAssetUrl("project-icon", assetPath);
    else invalidateAssetUrlKind("project-icon");
  }

  async function addCustomField(
    projectId: string,
    name: string,
    fieldType: ProjectCustomFieldType,
    icon: string | null = null,
  ): Promise<ProjectCustomField | undefined> {
    const displayName = normalizeProjectName(name);
    if (!displayName) return undefined;
    const existingField = selectors.customFieldByName(projectId, displayName);
    if (existingField) return existingField;
    const fieldId = crypto.randomUUID();
    const mutation = await commitMutation(`field:${fieldId}`, () => createProjectCustomField({
      id: fieldId,
      projectId,
      name: displayName,
      fieldType,
      sortOrder: selectors.nextCustomFieldSortOrder(projectId),
      icon,
    }));
    return changedById(mutation.changed.customFields, fieldId, "created custom field");
  }

  async function updateCustomField(
    field: ProjectCustomField,
    patch: Partial<Pick<ProjectCustomField, "name" | "sortOrder" | "icon">>,
  ): Promise<void> {
    const nextName = normalizeProjectName(patch.name ?? field.name);
    if (!nextName) return;
    await commitMutation(`field:${field.id}`, () =>
      updateProjectCustomField(customFieldUpdatePayload(field, { ...patch, name: nextName })));
  }

  /** Duplicate a property's schema and options with fresh identities and empty task values. */
  async function duplicateCustomField(
    field: ProjectCustomField,
    name: string,
  ): Promise<ProjectCustomField | undefined> {
    const displayName = normalizeProjectName(name);
    if (!displayName) return undefined;
    const fieldId = crypto.randomUUID();
    const mutation = await commitMutation(`field:${fieldId}`, () => createProjectCustomField({
      id: fieldId, projectId: field.projectId, name: displayName, fieldType: field.fieldType,
      sortOrder: selectors.nextCustomFieldSortOrder(field.projectId), icon: field.icon, duplicateSourceId: field.id,
    }));
    return changedById(mutation.changed.customFields, fieldId, "duplicated custom field");
  }

  async function moveCustomField(field: ProjectCustomField, direction: -1 | 1): Promise<void> {
    await reorderItem(field.projectId, { kind: "custom_field", id: field.id, expectedOrder: field.sortOrder }, direction);
  }

  async function removeCustomField(fieldId: string): Promise<void> {
    await commitMutation(`field:${fieldId}`, () => deleteProjectCustomField(fieldId));
  }

  async function addCustomFieldOption(
    fieldId: string,
    name: string,
  ): Promise<ProjectCustomFieldOption | undefined> {
    const displayName = normalizeProjectName(name);
    if (!displayName) return undefined;
    const existingOption = selectors.customFieldOptionByName(fieldId, displayName);
    if (existingOption) return existingOption;
    const optionId = crypto.randomUUID();
    const mutation = await commitMutation(`field-option:${optionId}`, () => createProjectCustomFieldOption({
      id: optionId,
      fieldId,
      name: displayName,
      sortOrder: selectors.nextCustomFieldOptionSortOrder(fieldId),
    }));
    return changedById(mutation.changed.customFieldOptions, optionId, "created custom field option");
  }

  async function updateCustomFieldOption(
    option: ProjectCustomFieldOption,
    patch: Partial<Pick<ProjectCustomFieldOption, "name" | "sortOrder">>,
  ): Promise<void> {
    const nextName = normalizeProjectName(patch.name ?? option.name);
    if (!nextName) return;
    await commitMutation(`field-option:${option.id}`, () =>
      updateProjectCustomFieldOption(customFieldOptionUpdatePayload(option, { ...patch, name: nextName })));
  }

  async function moveCustomFieldOption(option: ProjectCustomFieldOption, direction: -1 | 1): Promise<void> {
    const field = selectors.customFieldById(option.fieldId);
    if (!field) throw new Error("The custom field is no longer loaded");
    await reorderItem(field.projectId, {
      kind: "custom_field_option", id: option.id, fieldId: option.fieldId, expectedOrder: option.sortOrder,
    }, direction);
  }

  async function removeCustomFieldOption(optionId: string): Promise<void> {
    await commitMutation(`field-option:${optionId}`, () => deleteProjectCustomFieldOption(optionId));
  }

  async function saveCustomFieldValue(value: ProjectCustomFieldValueUpdate): Promise<void> {
    await commitMutation(`field-value:${value.taskId}:${value.fieldId}`, () =>
      updateProjectCustomFieldValue(value));
  }

  async function updateTask(task: ProjectTask, patch: ProjectTaskUpdatePatch): Promise<void> {
    const loadGeneration = readLoadGeneration();
    const hydrated = await taskForMutation(task);
    await commitMutation(
      `task:${task.id}`,
      () => updateProjectTask(taskUpdatePayload(hydrated, patch)),
      loadGeneration,
    );
  }

  /** Apply exactly the reviewed native graph and retain its identity after uncertain responses. */
  async function applyDependencyCascade(preview: ProjectDependencyCascadePreview): Promise<void> {
    const loadGeneration = readLoadGeneration();
    if (bulkLoadGeneration !== loadGeneration) {
      pendingBulkOperations.clear();
      bulkLoadGeneration = loadGeneration;
    }
    const intent = { projectId: preview.projectId, reviewedDigest: preview.digest };
    const key = `dependency-cascade:${JSON.stringify(intent)}`;
    const pending = pendingOperation(key, preview.projectId);
    await commitMutation(key, async () => {
      const { applyProjectDependencyCascade } = await import("$lib/api/project-cascade");
      if (readLoadGeneration() !== loadGeneration) {
        throw new Error("Project context changed before applying the dependency review");
      }
      return applyProjectDependencyCascade({ ...intent, operationId: pending.operationId });
    }, loadGeneration, pending);
    pendingBulkOperations.delete(key);
  }

  async function setTaskStatus(task: ProjectTask, statusId: string): Promise<void> {
    await setTasksStatus([task], statusId);
  }

  async function setTasksStatus(tasks: ProjectTask[], statusId: string): Promise<void> {
    await applyTaskBulk(tasks, { kind: "status", statusId });
  }

  async function toggleTaskDone(task: ProjectTask): Promise<void> {
    const currentStatus = selectors.statusById(task.statusId);
    const target = currentStatus?.terminal
      ? selectors.reopenStatus(task.projectId)
      : selectors.doneStatus(task.projectId);
    if (!target) return;
    await setTaskStatus(task, target.id);
  }

  async function setTaskPriority(task: ProjectTask, priority: ProjectPriority): Promise<void> {
    await setTasksPriority([task], priority);
  }

  async function setTasksPriority(tasks: ProjectTask[], priority: ProjectPriority): Promise<void> {
    await applyTaskBulk(tasks, { kind: "priority", priority });
  }

  async function archiveTasks(tasks: ProjectTask[]): Promise<void> {
    await applyTaskBulk(tasks, { kind: "archive", archived: true });
  }

  async function restoreTasks(tasks: ProjectTask[]): Promise<void> {
    await applyTaskBulk(tasks, { kind: "archive", archived: false });
  }

  /** Persist one semantic edit and retain its retry identity until its response is accepted. */
  async function applyTaskBulk(tasks: ProjectTask[], change: ProjectTaskBulkChange): Promise<void> {
    const first = tasks[0];
    if (!first) return;
    if (tasks.some((task) => task.projectId !== first.projectId)) {
      throw new Error("Bulk tasks must belong to the same project");
    }
    const loadGeneration = readLoadGeneration();
    if (bulkLoadGeneration !== loadGeneration) {
      pendingBulkOperations.clear();
      bulkLoadGeneration = loadGeneration;
    }
    const intent = {
      projectId: first.projectId,
      change,
      tasks: tasks.map((task) => ({
        id: task.id,
        value: change.kind === "status" ? task.statusId
          : change.kind === "priority" ? task.priority : task.archivedAt ?? null,
      })),
    };
    const key = JSON.stringify(intent);
    const pending = pendingOperation(key, intent.projectId);
    await commitMutation(
      `task-bulk:${key}`,
      () => applyProjectTaskBulk({ ...intent, operationId: pending.operationId }),
      loadGeneration,
      pending,
    );
    pendingBulkOperations.delete(key);
  }

  async function setTaskType(task: ProjectTask, taskType: ProjectTaskType): Promise<void> {
    await updateTask(task, { taskType });
  }

  async function moveTaskInSection(task: ProjectTask, direction: -1 | 1): Promise<void> {
    if (task.parentTaskId) return;
    await reorderTask(task, "section", direction);
  }

  async function moveTaskInStatus(task: ProjectTask, direction: -1 | 1): Promise<void> {
    if (task.parentTaskId) return;
    await reorderTask(task, "status", direction);
  }

  async function moveSubtask(task: ProjectTask, direction: -1 | 1): Promise<void> {
    if (!task.parentTaskId) return;
    await reorderTask(task, "section", direction);
  }

  /** Reorder natively, since sibling selection must include rows outside the loaded view. */
  async function reorderTask(task: ProjectTask, axis: "section" | "status", direction: -1 | 1): Promise<void> {
    await reorderItem(task.projectId, {
      kind: "task", id: task.id, axis, parentTaskId: task.parentTaskId ?? null,
      groupId: axis === "section" ? task.sectionId : task.statusId,
      expectedOrder: axis === "section" ? task.sectionSortOrder : task.statusSortOrder,
    }, direction);
  }

  /** Reorder one item, reusing the intent's operation id until a response is accepted. */
  async function reorderItem(projectId: string, item: ProjectReorderItem, direction: -1 | 1): Promise<void> {
    const loadGeneration = readLoadGeneration();
    if (bulkLoadGeneration !== loadGeneration) {
      pendingBulkOperations.clear();
      bulkLoadGeneration = loadGeneration;
    }
    const intent = { projectId, item, direction };
    const key = `reorder:${JSON.stringify(intent)}`;
    const pending = pendingOperation(key, projectId);
    await commitMutation(key, () => reorderProjectItem({ ...intent, operationId: pending.operationId }), loadGeneration, pending);
    pendingBulkOperations.delete(key);
  }

  async function promoteSubtask(task: ProjectTask): Promise<void> {
    if (!task.parentTaskId) return;
    await updateTask(task, {
      parentTaskId: undefined,
      sectionSortOrder: selectors.nextTaskSectionSortOrder(task.projectId, task.sectionId),
      statusSortOrder: selectors.nextTaskStatusSortOrder(task.projectId, task.statusId),
    });
  }

  async function demoteTaskToSubtask(task: ProjectTask, parentTask: ProjectTask): Promise<void> {
    if (task.parentTaskId || parentTask.parentTaskId || task.id === parentTask.id) return;
    if (task.projectId !== parentTask.projectId) return;
    if (selectors.subtasksForTaskIncludingArchived(task.id).length > 0) return;
    await updateTask(task, {
      parentTaskId: parentTask.id,
      sectionId: parentTask.sectionId,
      sectionSortOrder: selectors.nextSubtaskSortOrder(parentTask.id),
    });
  }

  async function linkTaskEvent(
    taskId: string,
    eventId: string,
    linkKind: ProjectTaskEventLink["linkKind"] = "scheduled",
  ): Promise<void> {
    await commitMutation(`event-link:${taskId}:${eventId}`, () =>
      linkProjectTaskEvent({ taskId, eventId, linkKind }));
  }

  async function unlinkTaskEvent(taskId: string, eventId: string): Promise<void> {
    await commitMutation(`event-link:${taskId}:${eventId}`, () =>
      unlinkProjectTaskEvent(taskId, eventId));
  }

  async function searchLinkableEvents(
    projectId: string,
    taskId: string,
    query: string,
    startDate?: string,
    endDate?: string,
    limit = 12,
  ): Promise<ProjectLinkableEvent[]> {
    return searchProjectLinkableEvents(projectId, taskId, query, startDate, endDate, limit);
  }

  async function addTaskDependency(blockingTaskId: string, blockedTaskId: string): Promise<void> {
    if (!blockingTaskId || !blockedTaskId || blockingTaskId === blockedTaskId) return;
    const id = crypto.randomUUID();
    await commitMutation(`dependency:${id}`, () => createProjectTaskDependency({
      id,
      blockingTaskId,
      blockedTaskId,
      dependencyType: "blocks",
    }));
  }

  async function removeTaskDependency(dependencyId: string): Promise<void> {
    await commitMutation(`dependency:${dependencyId}`, () =>
      deleteProjectTaskDependency(dependencyId));
  }



  /** Persist one preference of one view, serialized per view and key. */
  async function saveViewPreference(viewId: string, preferenceKey: string, preferenceValue: string): Promise<void> {
    await commitMutation(`preference:${viewId}:${preferenceKey}`, () =>
      upsertProjectViewPreference({ viewId, preferenceKey, preferenceValue }));
  }

  async function saveTaskListColumns(
    viewId: string,
    columns: readonly ProjectTaskListColumn[],
  ): Promise<void> {
    await saveViewPreference(viewId, TASK_LIST_COLUMNS_PREFERENCE_KEY, taskListColumnsPreferenceValue(columns));
  }

  async function saveTaskListColumnWidths(
    viewId: string,
    widths: ProjectTaskListColumnWidths,
  ): Promise<void> {
    await saveViewPreference(viewId, TASK_LIST_COLUMN_WIDTHS_PREFERENCE_KEY, taskListColumnWidthsPreferenceValue(widths));
  }

  /** Persist table presentation independently of task data and manual order. */
  async function saveTaskListPresentation(viewId: string, presentation: ProjectListPresentation): Promise<void> {
    await saveViewPreference(viewId, PROJECT_LIST_PRESENTATION_KEY, JSON.stringify(presentation));
  }

  /** Persist a task view's filters, sort, grouping, and calendar span. */
  async function saveViewQuery(viewId: string, query: ProjectViewQuery): Promise<void> {
    await saveViewPreference(viewId, PROJECT_VIEW_QUERY_PREFERENCE_KEY, serializeProjectViewQuery(query));
  }

  /**
   * Append a view to a project.
   * @returns The new view id once the backend has stored it.
   */
  async function addView(projectId: string, viewType: ProjectViewType, name: string | null = null): Promise<string> {
    const id = crypto.randomUUID();
    await commitMutation(`view:${id}`, () =>
      createProjectView({ id, projectId, viewType, name: name?.trim() || null }));
    return id;
  }

  /** Rename a view; a blank name restores the label of its type. */
  async function renameView(viewId: string, name: string): Promise<void> {
    await commitMutation(`view:${viewId}:name`, () => renameProjectView(viewId, name.trim() || null));
  }

  /** Apply a complete view order optimistically and restore the previous order when the backend refuses it. */
  async function reorderViews(projectId: string, viewIds: readonly string[]): Promise<void> {
    const previous = readSnapshot().views;
    const orders = new Map(viewIds.map((id, index) => [id, (index + 1) * 1000]));
    updateSnapshot((current) => ({
      ...current,
      views: current.views.map((view) => {
        const sortOrder = orders.get(view.id);
        return view.projectId === projectId && sortOrder !== undefined ? { ...view, sortOrder } : view;
      }),
    }));
    try {
      await commitMutation(`view-order:${projectId}`, () => reorderProjectViews(projectId, viewIds));
    } catch (error: unknown) {
      updateSnapshot((current) => ({
        ...current,
        views: [...current.views.filter((view) => view.projectId !== projectId), ...previous.filter((view) => view.projectId === projectId)],
      }));
      throw error;
    }
  }

  /** Remove a view entry and its preferences; Notes pages and Chat channels stay. */
  async function removeView(viewId: string): Promise<void> {
    await commitMutation(`view:${viewId}`, () => deleteProjectView(viewId));
  }

  return {
    addView,
    renameView,
    reorderViews,
    removeView,
    addGroup,
    setGroupCollapsed,
    updateGroup,
    removeGroup,
    addProject,
    updateProject,
    setNotesSettings,
    renameProject,
    moveProjectToGroup,
    setProjectStatus,
    addSection,
    updateSection,
    hideSection,
    archiveSection,
    restoreSection,
    addStatus,
    updateStatus,
    moveStatus,
    removeStatus,
    addPriority,
    updatePriority,
    movePriority,
    removePriority,
    addTask,
    addChecklistItem,
    setChecklistItemCompleted,
    updateChecklistItem,
    moveChecklistItem,
    removeChecklistItem,
    addTag,
    updateTag,
    moveTag,
    removeTag,
    linkTaskTag,
    unlinkTaskTag,
    addAndLinkTaskTag,
    addCustomEmoji,
    removeCustomEmoji,
    addCustomField,
    updateCustomField,
    duplicateCustomField,
    moveCustomField,
    removeCustomField,
    addCustomFieldOption,
    updateCustomFieldOption,
    moveCustomFieldOption,
    removeCustomFieldOption,
    saveCustomFieldValue,
    updateTask,
    applyDependencyCascade,
    setTaskStatus,
    setTasksStatus,
    toggleTaskDone,
    setTaskPriority,
    setTasksPriority,
    setTaskType,
    archiveTasks,
    restoreTasks,
    moveTaskInSection,
    moveTaskInStatus,
    moveSubtask,
    promoteSubtask,
    demoteTaskToSubtask,
    linkTaskEvent,
    unlinkTaskEvent,
    searchLinkableEvents,
    addTaskDependency,
    removeTaskDependency,
    saveViewPreference,
    saveTaskListColumns,
    saveTaskListColumnWidths,
    saveTaskListPresentation,
    saveViewQuery,
  };
}
