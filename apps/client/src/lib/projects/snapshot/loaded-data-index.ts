import type {
  Project,
  ProjectChecklistItem,
  ProjectCustomEmoji,
  ProjectCustomField,
  ProjectCustomFieldOption,
  ProjectCustomFieldOptionValue,
  ProjectCustomFieldValue,
  ProjectGroup,
  ProjectPriorityConfig,
  ProjectSavedTaskView,
  ProjectSection,
  ProjectsSnapshot,
  ProjectStatus,
  ProjectTag,
  ProjectTask,
  ProjectTaskChangeEvent,
  ProjectTaskDependency,
  ProjectTaskEventLink,
  ProjectTaskTagLink,
  ProjectViewPreference,
} from "$lib/projects/types";
import { parseSavedTaskViewPreference } from "$lib/projects/tasks/saved-views";

export type ProjectLoadedDataIndexSource = ProjectsSnapshot | ProjectLoadedDataIndex;

export interface ProjectLoadedDataIndex {
  readonly source: ProjectsSnapshot;
  readonly activeProjects: readonly Project[];
  readonly visibleGroups: readonly ProjectGroup[];
  readonly projectById: ReadonlyMap<string, Project>;
  readonly groupById: ReadonlyMap<string, ProjectGroup>;
  readonly projectsByGroup: ReadonlyMap<string, readonly Project[]>;
  readonly activeProjectsByGroup: ReadonlyMap<string, readonly Project[]>;
  readonly sectionById: ReadonlyMap<string, ProjectSection>;
  readonly sectionsByProject: ReadonlyMap<string, readonly ProjectSection[]>;
  readonly activeSectionsByProject: ReadonlyMap<string, readonly ProjectSection[]>;
  readonly statusById: ReadonlyMap<string, ProjectStatus>;
  readonly statusesByProject: ReadonlyMap<string, readonly ProjectStatus[]>;
  readonly defaultStatusByProject: ReadonlyMap<string, ProjectStatus>;
  readonly doneStatusByProject: ReadonlyMap<string, ProjectStatus>;
  readonly reopenStatusByProject: ReadonlyMap<string, ProjectStatus>;
  readonly prioritiesByProject: ReadonlyMap<string, readonly ProjectPriorityConfig[]>;
  readonly priorityById: ReadonlyMap<string, ProjectPriorityConfig>;
  readonly taskById: ReadonlyMap<string, ProjectTask>;
  readonly taskIdsByProject: ReadonlyMap<string, ReadonlySet<string>>;
  readonly tasksByProject: ReadonlyMap<string, readonly ProjectTask[]>;
  readonly activeTasksByProject: ReadonlyMap<string, readonly ProjectTask[]>;
  readonly tasksByParent: ReadonlyMap<string, readonly ProjectTask[]>;
  readonly activeTasksByParent: ReadonlyMap<string, readonly ProjectTask[]>;
  readonly activeTopLevelTasksBySection: ReadonlyMap<string, readonly ProjectTask[]>;
  readonly activeTopLevelTasksByStatus: ReadonlyMap<string, readonly ProjectTask[]>;
  readonly checklistItemsByTask: ReadonlyMap<string, readonly ProjectChecklistItem[]>;
  readonly checklistItemById: ReadonlyMap<string, ProjectChecklistItem>;
  readonly tagById: ReadonlyMap<string, ProjectTag>;
  readonly tagsByProject: ReadonlyMap<string, readonly ProjectTag[]>;
  readonly tagByProjectAndName: ReadonlyMap<string, ProjectTag>;
  readonly taskTagLinksByTask: ReadonlyMap<string, readonly ProjectTaskTagLink[]>;
  readonly taskTagLinkByKey: ReadonlyMap<string, ProjectTaskTagLink>;
  readonly tagsByTask: ReadonlyMap<string, readonly ProjectTag[]>;
  unlinkedTagsForTask(task: ProjectTask): readonly ProjectTag[];
  readonly customFieldById: ReadonlyMap<string, ProjectCustomField>;
  readonly customFieldsByProject: ReadonlyMap<string, readonly ProjectCustomField[]>;
  readonly customFieldByProjectAndName: ReadonlyMap<string, ProjectCustomField>;
  readonly customFieldOptionsByField: ReadonlyMap<string, readonly ProjectCustomFieldOption[]>;
  readonly customFieldOptionById: ReadonlyMap<string, ProjectCustomFieldOption>;
  readonly customFieldOptionByFieldAndName: ReadonlyMap<string, ProjectCustomFieldOption>;
  readonly customFieldValueByTaskAndField: ReadonlyMap<string, ProjectCustomFieldValue>;
  readonly customFieldOptionIdsByTaskAndField: ReadonlyMap<string, ReadonlySet<string>>;
  readonly customFieldOptionValueByKey: ReadonlyMap<string, ProjectCustomFieldOptionValue>;
  readonly customFieldOptionsByTaskAndField: ReadonlyMap<string, readonly ProjectCustomFieldOption[]>;
  readonly dependenciesBlockingTask: ReadonlyMap<string, readonly ProjectTaskDependency[]>;
  readonly dependenciesBlockedByTask: ReadonlyMap<string, readonly ProjectTaskDependency[]>;
  readonly dependencyById: ReadonlyMap<string, ProjectTaskDependency>;
  readonly eventLinksByTask: ReadonlyMap<string, readonly ProjectTaskEventLink[]>;
  readonly eventLinksByEvent: ReadonlyMap<string, readonly ProjectTaskEventLink[]>;
  readonly eventLinkByKey: ReadonlyMap<string, ProjectTaskEventLink>;
  readonly taskChangeEventsByTask: ReadonlyMap<string, readonly ProjectTaskChangeEvent[]>;
  readonly taskChangeEventsByProject: ReadonlyMap<string, readonly ProjectTaskChangeEvent[]>;
  readonly taskChangeEventById: ReadonlyMap<string, ProjectTaskChangeEvent>;
  readonly viewPreferencesByProject: ReadonlyMap<string, readonly ProjectViewPreference[]>;
  readonly viewPreferenceByKey: ReadonlyMap<string, ProjectViewPreference>;
  readonly savedTaskViewsByProject: ReadonlyMap<string, readonly ProjectSavedTaskView[]>;
  readonly customEmojis: readonly ProjectCustomEmoji[];
  readonly customEmojiById: ReadonlyMap<string, ProjectCustomEmoji>;
  readonly maxGroupSortOrder: number;
  readonly maxCustomEmojiSortOrder: number;
}

function sortByOrderAndName<T extends { sortOrder: number; name: string }>(a: T, b: T): number {
  return a.sortOrder - b.sortOrder || a.name.localeCompare(b.name);
}

function sortTasksBySectionOrder(a: ProjectTask, b: ProjectTask): number {
  return a.sectionSortOrder - b.sectionSortOrder || a.createdAt.localeCompare(b.createdAt);
}

function sortTasksByStatusOrder(a: ProjectTask, b: ProjectTask): number {
  return a.statusSortOrder - b.statusSortOrder || a.createdAt.localeCompare(b.createdAt);
}

function append<K, V>(map: Map<K, V[]>, key: K, value: V): void {
  const values = map.get(key);
  if (values) values.push(value);
  else map.set(key, [value]);
}

function sortGroups<K, V>(map: Map<K, V[]>, compare: (a: V, b: V) => number): void {
  for (const values of map.values()) values.sort(compare);
}

export function projectPairKey(first: string, second: string): string {
  return `${first}\u0000${second}`;
}

/** Builds every lookup needed by selectors from only the collections present in one snapshot. */
export function buildProjectLoadedDataIndex(source: ProjectsSnapshot): ProjectLoadedDataIndex {
  const projectById = new Map(source.projects.map((project) => [project.id, project]));
  const groupById = new Map(source.groups.map((group) => [group.id, group]));
  const projectsByGroup = new Map<string, Project[]>();
  const activeProjectsByGroup = new Map<string, Project[]>();
  for (const project of source.projects) {
    append(projectsByGroup, project.groupId, project);
    if (project.status === "active") append(activeProjectsByGroup, project.groupId, project);
  }
  sortGroups(projectsByGroup, sortByOrderAndName);
  sortGroups(activeProjectsByGroup, sortByOrderAndName);
  const activeProjects = source.projects.filter((project) => project.status === "active").sort(sortByOrderAndName);
  const visibleGroups = source.groups
    .filter((group) => !group.hiddenAt && !group.archivedAt)
    .sort(sortByOrderAndName);

  const sectionById = new Map<string, ProjectSection>();
  const sectionsByProject = new Map<string, ProjectSection[]>();
  const activeSectionsByProject = new Map<string, ProjectSection[]>();
  for (const section of source.sections) {
    sectionById.set(section.id, section);
    append(sectionsByProject, section.projectId, section);
    if (!section.archivedAt && !section.hiddenAt) append(activeSectionsByProject, section.projectId, section);
  }
  sortGroups(sectionsByProject, sortByOrderAndName);
  sortGroups(activeSectionsByProject, sortByOrderAndName);

  const statusById = new Map<string, ProjectStatus>();
  const statusesByProject = new Map<string, ProjectStatus[]>();
  for (const status of source.statuses) {
    statusById.set(status.id, status);
    append(statusesByProject, status.projectId, status);
  }
  sortGroups(statusesByProject, sortByOrderAndName);
  const defaultStatusByProject = new Map<string, ProjectStatus>();
  const doneStatusByProject = new Map<string, ProjectStatus>();
  const reopenStatusByProject = new Map<string, ProjectStatus>();
  for (const [projectId, statuses] of statusesByProject) {
    const defaultStatus = statuses.find((status) => status.name.toLowerCase() === "to do")
      ?? statuses.find((status) => status.category === "not_started")
      ?? statuses[0];
    const doneStatus = statuses.find((status) => status.terminal);
    const reopenStatus = statuses.find((status) => status.name.toLowerCase() === "to do")
      ?? statuses.find((status) => !status.terminal)
      ?? statuses[0];
    if (defaultStatus) defaultStatusByProject.set(projectId, defaultStatus);
    if (doneStatus) doneStatusByProject.set(projectId, doneStatus);
    if (reopenStatus) reopenStatusByProject.set(projectId, reopenStatus);
  }
  const prioritiesByProject = new Map<string, ProjectPriorityConfig[]>();
  const priorityById = new Map<string, ProjectPriorityConfig>();
  for (const priority of source.priorities) {
    priorityById.set(priority.id, priority);
    append(prioritiesByProject, priority.projectId, priority);
  }
  sortGroups(prioritiesByProject, sortByOrderAndName);

  const taskById = new Map<string, ProjectTask>();
  const mutableTaskIdsByProject = new Map<string, Set<string>>();
  const tasksByProject = new Map<string, ProjectTask[]>();
  const activeTasksByProject = new Map<string, ProjectTask[]>();
  const tasksByParent = new Map<string, ProjectTask[]>();
  const activeTasksByParent = new Map<string, ProjectTask[]>();
  const activeTopLevelTasksBySection = new Map<string, ProjectTask[]>();
  const activeTopLevelTasksByStatus = new Map<string, ProjectTask[]>();
  for (const task of source.tasks) {
    taskById.set(task.id, task);
    const projectTaskIds = mutableTaskIdsByProject.get(task.projectId);
    if (projectTaskIds) projectTaskIds.add(task.id);
    else mutableTaskIdsByProject.set(task.projectId, new Set([task.id]));
    append(tasksByProject, task.projectId, task);
    if (task.parentTaskId) {
      append(tasksByParent, task.parentTaskId, task);
      if (!task.archivedAt) append(activeTasksByParent, task.parentTaskId, task);
    }
    if (!task.archivedAt) {
      append(activeTasksByProject, task.projectId, task);
      if (!task.parentTaskId) {
        append(activeTopLevelTasksBySection, projectPairKey(task.projectId, task.sectionId), task);
        append(activeTopLevelTasksByStatus, projectPairKey(task.projectId, task.statusId), task);
      }
    }
  }
  sortGroups(tasksByProject, sortTasksBySectionOrder);
  sortGroups(activeTasksByProject, sortTasksBySectionOrder);
  sortGroups(tasksByParent, sortTasksBySectionOrder);
  sortGroups(activeTasksByParent, sortTasksBySectionOrder);
  sortGroups(activeTopLevelTasksBySection, sortTasksBySectionOrder);
  sortGroups(activeTopLevelTasksByStatus, sortTasksByStatusOrder);

  const checklistItemsByTask = new Map<string, ProjectChecklistItem[]>();
  const checklistItemById = new Map<string, ProjectChecklistItem>();
  for (const item of source.checklistItems) {
    checklistItemById.set(item.id, item);
    append(checklistItemsByTask, item.taskId, item);
  }
  sortGroups(
    checklistItemsByTask,
    (a, b) => a.sortOrder - b.sortOrder || a.createdAt.localeCompare(b.createdAt),
  );

  const tagById = new Map<string, ProjectTag>();
  const tagsByProject = new Map<string, ProjectTag[]>();
  const tagByProjectAndName = new Map<string, ProjectTag>();
  for (const tag of source.tags) {
    tagById.set(tag.id, tag);
    append(tagsByProject, tag.projectId, tag);
    const nameKey = projectPairKey(tag.projectId, tag.name.trim().toLowerCase());
    if (!tagByProjectAndName.has(nameKey)) tagByProjectAndName.set(nameKey, tag);
  }
  sortGroups(tagsByProject, sortByOrderAndName);
  const taskTagLinksByTask = new Map<string, ProjectTaskTagLink[]>();
  const taskTagLinkByKey = new Map<string, ProjectTaskTagLink>();
  for (const link of source.taskTagLinks) {
    taskTagLinkByKey.set(projectPairKey(link.taskId, link.tagId), link);
    append(taskTagLinksByTask, link.taskId, link);
  }
  const tagsByTask = new Map<string, ProjectTag[]>();
  for (const [taskId, links] of taskTagLinksByTask) {
    const tags = links.flatMap((link) => {
      const tag = tagById.get(link.tagId);
      return tag ? [tag] : [];
    });
    tags.sort(sortByOrderAndName);
    tagsByTask.set(taskId, tags);
  }
  const unlinkedTagsByTask = new Map<string, readonly ProjectTag[]>();
  const readUnlinkedTagsForTask = (task: ProjectTask): readonly ProjectTag[] => {
    const existing = unlinkedTagsByTask.get(task.id);
    if (existing) return existing;
    const linkedTagIds = new Set((taskTagLinksByTask.get(task.id) ?? []).map((link) => link.tagId));
    const tags = (tagsByProject.get(task.projectId) ?? []).filter((tag) => !linkedTagIds.has(tag.id));
    unlinkedTagsByTask.set(task.id, tags);
    return tags;
  };

  const customFieldById = new Map<string, ProjectCustomField>();
  const customFieldsByProject = new Map<string, ProjectCustomField[]>();
  const customFieldByProjectAndName = new Map<string, ProjectCustomField>();
  for (const field of source.customFields) {
    customFieldById.set(field.id, field);
    append(customFieldsByProject, field.projectId, field);
    const nameKey = projectPairKey(field.projectId, field.name.trim().toLowerCase());
    if (!customFieldByProjectAndName.has(nameKey)) customFieldByProjectAndName.set(nameKey, field);
  }
  sortGroups(customFieldsByProject, sortByOrderAndName);
  const customFieldOptionsByField = new Map<string, ProjectCustomFieldOption[]>();
  const customFieldOptionById = new Map<string, ProjectCustomFieldOption>();
  const customFieldOptionByFieldAndName = new Map<string, ProjectCustomFieldOption>();
  for (const option of source.customFieldOptions) {
    customFieldOptionById.set(option.id, option);
    append(customFieldOptionsByField, option.fieldId, option);
    const nameKey = projectPairKey(option.fieldId, option.name.trim().toLowerCase());
    if (!customFieldOptionByFieldAndName.has(nameKey)) {
      customFieldOptionByFieldAndName.set(nameKey, option);
    }
  }
  sortGroups(customFieldOptionsByField, sortByOrderAndName);
  const customFieldValueByTaskAndField = new Map<string, ProjectCustomFieldValue>();
  for (const value of source.customFieldValues) {
    customFieldValueByTaskAndField.set(projectPairKey(value.taskId, value.fieldId), value);
  }
  const mutableOptionIdsByTaskAndField = new Map<string, Set<string>>();
  const customFieldOptionValueByKey = new Map<string, ProjectCustomFieldOptionValue>();
  for (const value of source.customFieldOptionValues) {
    const key = projectPairKey(value.taskId, value.fieldId);
    const optionIds = mutableOptionIdsByTaskAndField.get(key);
    if (optionIds) optionIds.add(value.optionId);
    else mutableOptionIdsByTaskAndField.set(key, new Set([value.optionId]));
    customFieldOptionValueByKey.set(projectPairKey(key, value.optionId), value);
  }
  const customFieldOptionsByTaskAndField = new Map<string, ProjectCustomFieldOption[]>();
  for (const [key, optionIds] of mutableOptionIdsByTaskAndField) {
    const fieldId = key.slice(key.indexOf("\u0000") + 1);
    customFieldOptionsByTaskAndField.set(
      key,
      (customFieldOptionsByField.get(fieldId) ?? []).filter((option) => optionIds.has(option.id)),
    );
  }

  const dependenciesBlockingTask = new Map<string, ProjectTaskDependency[]>();
  const dependenciesBlockedByTask = new Map<string, ProjectTaskDependency[]>();
  const dependencyById = new Map<string, ProjectTaskDependency>();
  for (const dependency of source.dependencies) {
    dependencyById.set(dependency.id, dependency);
    append(dependenciesBlockingTask, dependency.blockedTaskId, dependency);
    append(dependenciesBlockedByTask, dependency.blockingTaskId, dependency);
  }
  const eventLinksByTask = new Map<string, ProjectTaskEventLink[]>();
  const eventLinksByEvent = new Map<string, ProjectTaskEventLink[]>();
  const eventLinkByKey = new Map<string, ProjectTaskEventLink>();
  for (const link of source.eventLinks) {
    eventLinkByKey.set(projectPairKey(link.taskId, link.eventId), link);
    append(eventLinksByTask, link.taskId, link);
    append(eventLinksByEvent, link.eventId, link);
  }

  const taskChangeEventsByTask = new Map<string, ProjectTaskChangeEvent[]>();
  const taskChangeEventsByProject = new Map<string, ProjectTaskChangeEvent[]>();
  const taskChangeEventById = new Map<string, ProjectTaskChangeEvent>();
  for (const event of source.taskChangeEvents) {
    taskChangeEventById.set(event.id, event);
    append(taskChangeEventsByTask, event.taskId, event);
    const projectId = taskById.get(event.taskId)?.projectId;
    if (projectId) append(taskChangeEventsByProject, projectId, event);
  }
  const newestFirst = (a: ProjectTaskChangeEvent, b: ProjectTaskChangeEvent) =>
    b.occurredAt.localeCompare(a.occurredAt);
  sortGroups(taskChangeEventsByTask, newestFirst);
  sortGroups(taskChangeEventsByProject, newestFirst);
  const viewPreferencesByProject = new Map<string, ProjectViewPreference[]>();
  const viewPreferenceByKey = new Map<string, ProjectViewPreference>();
  for (const preference of source.viewPreferences) {
    viewPreferenceByKey.set(
      projectPairKey(projectPairKey(preference.projectId, preference.viewId), preference.preferenceKey),
      preference,
    );
    append(viewPreferencesByProject, preference.projectId, preference);
  }
  const savedTaskViewsByProject = new Map<string, ProjectSavedTaskView[]>();
  for (const [projectId, preferences] of viewPreferencesByProject) {
    const fields = customFieldsByProject.get(projectId) ?? [];
    const fieldIds = new Set(fields.map((field) => field.id));
    const optionIds = new Set(
      fields.flatMap((field) =>
        (customFieldOptionsByField.get(field.id) ?? []).map((option) => option.id)
      ),
    );
    const views = preferences
      .map((preference) => parseSavedTaskViewPreference(preference, fieldIds, optionIds))
      .filter((view): view is ProjectSavedTaskView => view !== undefined)
      .sort((a, b) => b.updatedAt.localeCompare(a.updatedAt) || a.name.localeCompare(b.name));
    savedTaskViewsByProject.set(projectId, views);
  }

  return {
    source,
    activeProjects,
    visibleGroups,
    projectById,
    groupById,
    projectsByGroup,
    activeProjectsByGroup,
    sectionById,
    sectionsByProject,
    activeSectionsByProject,
    statusById,
    statusesByProject,
    defaultStatusByProject,
    doneStatusByProject,
    reopenStatusByProject,
    prioritiesByProject,
    priorityById,
    taskById,
    taskIdsByProject: mutableTaskIdsByProject,
    tasksByProject,
    activeTasksByProject,
    tasksByParent,
    activeTasksByParent,
    activeTopLevelTasksBySection,
    activeTopLevelTasksByStatus,
    checklistItemsByTask,
    checklistItemById,
    tagById,
    tagsByProject,
    tagByProjectAndName,
    taskTagLinksByTask,
    taskTagLinkByKey,
    tagsByTask,
    unlinkedTagsForTask: readUnlinkedTagsForTask,
    customFieldById,
    customFieldsByProject,
    customFieldByProjectAndName,
    customFieldOptionsByField,
    customFieldOptionById,
    customFieldOptionByFieldAndName,
    customFieldValueByTaskAndField,
    customFieldOptionIdsByTaskAndField: mutableOptionIdsByTaskAndField,
    customFieldOptionValueByKey,
    customFieldOptionsByTaskAndField,
    dependenciesBlockingTask,
    dependenciesBlockedByTask,
    dependencyById,
    eventLinksByTask,
    eventLinksByEvent,
    eventLinkByKey,
    taskChangeEventsByTask,
    taskChangeEventsByProject,
    taskChangeEventById,
    viewPreferencesByProject,
    viewPreferenceByKey,
    savedTaskViewsByProject,
    customEmojis: source.customEmojis,
    customEmojiById: new Map(source.customEmojis.map((emoji) => [emoji.id, emoji])),
    maxGroupSortOrder: source.groups.reduce((maximum, group) => Math.max(maximum, group.sortOrder), 0),
    maxCustomEmojiSortOrder: source.customEmojis.reduce(
      (maximum, emoji) => Math.max(maximum, emoji.sortOrder),
      0,
    ),
  };
}

export interface ProjectLoadedDataIndexReader {
  read(source: ProjectsSnapshot): ProjectLoadedDataIndex;
}

/** Creates a last-snapshot cache suitable for one store instance. */
export function createProjectLoadedDataIndexReader(
  onBuild?: (source: ProjectsSnapshot) => void,
): ProjectLoadedDataIndexReader {
  let previousSource: ProjectsSnapshot | undefined;
  let previousIndex: ProjectLoadedDataIndex | undefined;
  return {
    read(source) {
      if (source !== previousSource || !previousIndex) {
        previousSource = source;
        previousIndex = buildProjectLoadedDataIndex(source);
        onBuild?.(source);
      }
      return previousIndex;
    },
  };
}

const snapshotIndexes = new WeakMap<ProjectsSnapshot, ProjectLoadedDataIndex>();

/** Resolves an existing index or builds one cached by snapshot identity. */
export function projectLoadedDataIndex(
  source: ProjectLoadedDataIndexSource,
): ProjectLoadedDataIndex {
  if ("source" in source) return source;
  const existing = snapshotIndexes.get(source);
  if (existing) return existing;
  const index = buildProjectLoadedDataIndex(source);
  snapshotIndexes.set(source, index);
  return index;
}
