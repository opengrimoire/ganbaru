import {
  projectLoadedDataIndex,
  projectPairKey,
  type ProjectLoadedDataIndexSource,
} from "./loaded-data-index";
import { normalizeProjectName } from "$lib/projects/text";
import type {
  Project,
  ProjectChecklistItem,
  ProjectCustomField,
  ProjectCustomFieldOption,
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
} from "$lib/projects/types";

function copy<T>(values: readonly T[] | undefined): T[] {
  return values ? values.slice() : [];
}

function lastSortOrder(values: readonly { sortOrder: number }[] | undefined): number {
  return Math.max(0, values?.at(-1)?.sortOrder ?? 0);
}

function lastTaskSectionSortOrder(values: readonly ProjectTask[] | undefined): number {
  return Math.max(0, values?.at(-1)?.sectionSortOrder ?? 0);
}

function lastTaskStatusSortOrder(values: readonly ProjectTask[] | undefined): number {
  return Math.max(0, values?.at(-1)?.statusSortOrder ?? 0);
}

export function activeProjects(source: ProjectLoadedDataIndexSource): Project[] {
  return copy(projectLoadedDataIndex(source).activeProjects);
}

export function firstProjectId(source: ProjectLoadedDataIndexSource): string | null {
  const index = projectLoadedDataIndex(source);
  return index.activeProjects[0]?.id ?? index.source.projects[0]?.id ?? null;
}

export function projectById(
  source: ProjectLoadedDataIndexSource,
  projectId: string | null | undefined,
): Project | undefined {
  return projectId ? projectLoadedDataIndex(source).projectById.get(projectId) : undefined;
}

export function groupById(
  source: ProjectLoadedDataIndexSource,
  groupId: string | null | undefined,
): ProjectGroup | undefined {
  return groupId ? projectLoadedDataIndex(source).groupById.get(groupId) : undefined;
}

export function sectionsForProject(
  source: ProjectLoadedDataIndexSource,
  projectId: string | null | undefined,
): ProjectSection[] {
  return projectId ? copy(projectLoadedDataIndex(source).activeSectionsByProject.get(projectId)) : [];
}

export function sectionsForProjectIncludingInactive(
  source: ProjectLoadedDataIndexSource,
  projectId: string | null | undefined,
): ProjectSection[] {
  return projectId ? copy(projectLoadedDataIndex(source).sectionsByProject.get(projectId)) : [];
}

export function statusesForProject(
  source: ProjectLoadedDataIndexSource,
  projectId: string | null | undefined,
): ProjectStatus[] {
  return projectId ? copy(projectLoadedDataIndex(source).statusesByProject.get(projectId)) : [];
}

export function prioritiesForProject(
  source: ProjectLoadedDataIndexSource,
  projectId: string | null | undefined,
): ProjectPriorityConfig[] {
  return projectId ? copy(projectLoadedDataIndex(source).prioritiesByProject.get(projectId)) : [];
}

export function tasksForProject(
  source: ProjectLoadedDataIndexSource,
  projectId: string | null | undefined,
): ProjectTask[] {
  return projectId ? copy(projectLoadedDataIndex(source).activeTasksByProject.get(projectId)) : [];
}

export function tasksForProjectIncludingArchived(
  source: ProjectLoadedDataIndexSource,
  projectId: string | null | undefined,
): ProjectTask[] {
  return projectId ? copy(projectLoadedDataIndex(source).tasksByProject.get(projectId)) : [];
}

export function topLevelTasksForSection(
  source: ProjectLoadedDataIndexSource,
  projectId: string,
  sectionId: string,
): ProjectTask[] {
  return copy(
    projectLoadedDataIndex(source).activeTopLevelTasksBySection.get(projectPairKey(projectId, sectionId)),
  );
}

export function topLevelTasksForStatus(
  source: ProjectLoadedDataIndexSource,
  projectId: string,
  statusId: string,
): ProjectTask[] {
  return copy(
    projectLoadedDataIndex(source).activeTopLevelTasksByStatus.get(projectPairKey(projectId, statusId)),
  );
}

export function subtasksForTask(
  source: ProjectLoadedDataIndexSource,
  parentTaskId: string | null | undefined,
): ProjectTask[] {
  return parentTaskId ? copy(projectLoadedDataIndex(source).activeTasksByParent.get(parentTaskId)) : [];
}

export function subtasksForTaskIncludingArchived(
  source: ProjectLoadedDataIndexSource,
  parentTaskId: string | null | undefined,
): ProjectTask[] {
  return parentTaskId ? copy(projectLoadedDataIndex(source).tasksByParent.get(parentTaskId)) : [];
}

export function projectsForGroup(source: ProjectLoadedDataIndexSource, groupId: string): Project[] {
  return copy(projectLoadedDataIndex(source).activeProjectsByGroup.get(groupId));
}

export function projectsForGroupIncludingInactive(
  source: ProjectLoadedDataIndexSource,
  groupId: string,
): Project[] {
  return copy(projectLoadedDataIndex(source).projectsByGroup.get(groupId));
}

export function visibleGroups(source: ProjectLoadedDataIndexSource): ProjectGroup[] {
  return copy(projectLoadedDataIndex(source).visibleGroups);
}

export function defaultSection(
  source: ProjectLoadedDataIndexSource,
  projectId: string,
): ProjectSection | undefined {
  return projectLoadedDataIndex(source).activeSectionsByProject.get(projectId)?.[0];
}

export function defaultStatus(
  source: ProjectLoadedDataIndexSource,
  projectId: string,
): ProjectStatus | undefined {
  return projectLoadedDataIndex(source).defaultStatusByProject.get(projectId);
}

export function doneStatus(
  source: ProjectLoadedDataIndexSource,
  projectId: string,
): ProjectStatus | undefined {
  return projectLoadedDataIndex(source).doneStatusByProject.get(projectId);
}

export function reopenStatus(
  source: ProjectLoadedDataIndexSource,
  projectId: string,
): ProjectStatus | undefined {
  return projectLoadedDataIndex(source).reopenStatusByProject.get(projectId);
}

export function statusById(
  source: ProjectLoadedDataIndexSource,
  statusId: string,
): ProjectStatus | undefined {
  return projectLoadedDataIndex(source).statusById.get(statusId);
}

export function taskById(
  source: ProjectLoadedDataIndexSource,
  taskId: string | null | undefined,
): ProjectTask | undefined {
  return taskId ? projectLoadedDataIndex(source).taskById.get(taskId) : undefined;
}

export function eventLinksForTask(
  source: ProjectLoadedDataIndexSource,
  taskId: string | null | undefined,
): ProjectTaskEventLink[] {
  return taskId ? copy(projectLoadedDataIndex(source).eventLinksByTask.get(taskId)) : [];
}

export function eventLinksForEvent(
  source: ProjectLoadedDataIndexSource,
  eventId: string | null | undefined,
): ProjectTaskEventLink[] {
  return eventId ? copy(projectLoadedDataIndex(source).eventLinksByEvent.get(eventId)) : [];
}

export function taskChangeEventsForTask(
  source: ProjectLoadedDataIndexSource,
  taskId: string | null | undefined,
): ProjectTaskChangeEvent[] {
  return taskId ? copy(projectLoadedDataIndex(source).taskChangeEventsByTask.get(taskId)) : [];
}

export function recentTaskChangeEventsForProject(
  source: ProjectLoadedDataIndexSource,
  projectId: string | null | undefined,
  limit = 8,
): ProjectTaskChangeEvent[] {
  if (!projectId) return [];
  return (projectLoadedDataIndex(source).taskChangeEventsByProject.get(projectId) ?? []).slice(0, limit);
}

export function checklistItemsForTask(
  source: ProjectLoadedDataIndexSource,
  taskId: string | null | undefined,
): ProjectChecklistItem[] {
  return taskId ? copy(projectLoadedDataIndex(source).checklistItemsByTask.get(taskId)) : [];
}

export function tagsForProject(
  source: ProjectLoadedDataIndexSource,
  projectId: string | null | undefined,
): ProjectTag[] {
  return projectId ? copy(projectLoadedDataIndex(source).tagsByProject.get(projectId)) : [];
}

export function tagById(
  source: ProjectLoadedDataIndexSource,
  tagId: string | null | undefined,
): ProjectTag | undefined {
  return tagId ? projectLoadedDataIndex(source).tagById.get(tagId) : undefined;
}

export function taskTagLinksForTask(
  source: ProjectLoadedDataIndexSource,
  taskId: string | null | undefined,
): ProjectTaskTagLink[] {
  return taskId ? copy(projectLoadedDataIndex(source).taskTagLinksByTask.get(taskId)) : [];
}

export function tagsForTask(
  source: ProjectLoadedDataIndexSource,
  taskId: string | null | undefined,
): ProjectTag[] {
  return taskId ? copy(projectLoadedDataIndex(source).tagsByTask.get(taskId)) : [];
}

export function unlinkedTagsForTask(
  source: ProjectLoadedDataIndexSource,
  task: ProjectTask | null | undefined,
): ProjectTag[] {
  if (!task) return [];
  return copy(projectLoadedDataIndex(source).unlinkedTagsForTask(task));
}

export function projectTagByName(
  source: ProjectLoadedDataIndexSource,
  projectId: string,
  name: string,
): ProjectTag | undefined {
  const normalized = normalizeProjectName(name).toLowerCase();
  return normalized
    ? projectLoadedDataIndex(source).tagByProjectAndName.get(projectPairKey(projectId, normalized))
    : undefined;
}

export function customFieldsForProject(
  source: ProjectLoadedDataIndexSource,
  projectId: string | null | undefined,
): ProjectCustomField[] {
  return projectId ? copy(projectLoadedDataIndex(source).customFieldsByProject.get(projectId)) : [];
}

export function customFieldById(
  source: ProjectLoadedDataIndexSource,
  fieldId: string | null | undefined,
): ProjectCustomField | undefined {
  return fieldId ? projectLoadedDataIndex(source).customFieldById.get(fieldId) : undefined;
}

export function customFieldByName(
  source: ProjectLoadedDataIndexSource,
  projectId: string,
  name: string,
): ProjectCustomField | undefined {
  const normalized = normalizeProjectName(name).toLowerCase();
  return normalized
    ? projectLoadedDataIndex(source).customFieldByProjectAndName.get(projectPairKey(projectId, normalized))
    : undefined;
}

export function customFieldOptionsForField(
  source: ProjectLoadedDataIndexSource,
  fieldId: string | null | undefined,
): ProjectCustomFieldOption[] {
  return fieldId ? copy(projectLoadedDataIndex(source).customFieldOptionsByField.get(fieldId)) : [];
}

export function customFieldOptionByName(
  source: ProjectLoadedDataIndexSource,
  fieldId: string,
  name: string,
): ProjectCustomFieldOption | undefined {
  const normalized = normalizeProjectName(name).toLowerCase();
  return normalized
    ? projectLoadedDataIndex(source).customFieldOptionByFieldAndName.get(projectPairKey(fieldId, normalized))
    : undefined;
}

export function customFieldValueForTask(
  source: ProjectLoadedDataIndexSource,
  taskId: string | null | undefined,
  fieldId: string | null | undefined,
): ProjectCustomFieldValue | undefined {
  return taskId && fieldId
    ? projectLoadedDataIndex(source).customFieldValueByTaskAndField.get(projectPairKey(taskId, fieldId))
    : undefined;
}

export function customFieldOptionValuesForTask(
  source: ProjectLoadedDataIndexSource,
  taskId: string | null | undefined,
  fieldId: string | null | undefined,
): ProjectCustomFieldOption[] {
  if (!taskId || !fieldId) return [];
  return copy(
    projectLoadedDataIndex(source).customFieldOptionsByTaskAndField.get(projectPairKey(taskId, fieldId)),
  );
}

export function dependenciesBlockingTask(
  source: ProjectLoadedDataIndexSource,
  taskId: string | null | undefined,
): ProjectTaskDependency[] {
  return taskId ? copy(projectLoadedDataIndex(source).dependenciesBlockingTask.get(taskId)) : [];
}

export function dependenciesBlockedByTask(
  source: ProjectLoadedDataIndexSource,
  taskId: string | null | undefined,
): ProjectTaskDependency[] {
  return taskId ? copy(projectLoadedDataIndex(source).dependenciesBlockedByTask.get(taskId)) : [];
}

export function savedTaskViewsForProject(
  source: ProjectLoadedDataIndexSource,
  projectId: string | null | undefined,
): ProjectSavedTaskView[] {
  if (!projectId) return [];
  return copy(projectLoadedDataIndex(source).savedTaskViewsByProject.get(projectId));
}

export function nextGroupSortOrder(source: ProjectLoadedDataIndexSource): number {
  return projectLoadedDataIndex(source).maxGroupSortOrder + 1000;
}

export function nextProjectSortOrder(source: ProjectLoadedDataIndexSource, groupId: string): number {
  return lastSortOrder(projectLoadedDataIndex(source).projectsByGroup.get(groupId)) + 1000;
}

export function nextSectionSortOrder(source: ProjectLoadedDataIndexSource, projectId: string): number {
  return lastSortOrder(projectLoadedDataIndex(source).activeSectionsByProject.get(projectId)) + 1000;
}

export function nextStatusSortOrder(source: ProjectLoadedDataIndexSource, projectId: string): number {
  return lastSortOrder(projectLoadedDataIndex(source).statusesByProject.get(projectId)) + 1000;
}

export function nextPrioritySortOrder(source: ProjectLoadedDataIndexSource, projectId: string): number {
  return lastSortOrder(projectLoadedDataIndex(source).prioritiesByProject.get(projectId)) + 1000;
}

export function nextTaskSectionSortOrder(
  source: ProjectLoadedDataIndexSource,
  projectId: string,
  sectionId: string,
): number {
  return lastTaskSectionSortOrder(
    projectLoadedDataIndex(source).activeTopLevelTasksBySection.get(projectPairKey(projectId, sectionId)),
  ) + 1000;
}

export function nextSubtaskSortOrder(source: ProjectLoadedDataIndexSource, parentTaskId: string): number {
  return lastTaskSectionSortOrder(projectLoadedDataIndex(source).tasksByParent.get(parentTaskId)) + 1000;
}

export function nextChecklistSortOrder(source: ProjectLoadedDataIndexSource, taskId: string): number {
  return lastSortOrder(projectLoadedDataIndex(source).checklistItemsByTask.get(taskId)) + 1000;
}

export function nextTagSortOrder(source: ProjectLoadedDataIndexSource, projectId: string): number {
  return lastSortOrder(projectLoadedDataIndex(source).tagsByProject.get(projectId)) + 1000;
}

export function nextCustomFieldSortOrder(source: ProjectLoadedDataIndexSource, projectId: string): number {
  return lastSortOrder(projectLoadedDataIndex(source).customFieldsByProject.get(projectId)) + 1000;
}

export function nextCustomFieldOptionSortOrder(source: ProjectLoadedDataIndexSource, fieldId: string): number {
  return lastSortOrder(projectLoadedDataIndex(source).customFieldOptionsByField.get(fieldId)) + 1000;
}

export function nextCustomEmojiSortOrder(source: ProjectLoadedDataIndexSource): number {
  return projectLoadedDataIndex(source).maxCustomEmojiSortOrder + 1000;
}

export function nextTaskStatusSortOrder(
  source: ProjectLoadedDataIndexSource,
  projectId: string,
  statusId: string,
): number {
  return lastTaskStatusSortOrder(
    projectLoadedDataIndex(source).activeTopLevelTasksByStatus.get(projectPairKey(projectId, statusId)),
  ) + 1000;
}

function snapshotWithoutProjectData(source: ProjectsSnapshot, projectId: string): ProjectsSnapshot {
  const index = projectLoadedDataIndex(source);
  const taskIds = index.taskIdsByProject.get(projectId) ?? new Set<string>();
  const fieldIds = new Set((index.customFieldsByProject.get(projectId) ?? []).map((field) => field.id));
  const tagIds = new Set((index.tagsByProject.get(projectId) ?? []).map((tag) => tag.id));
  return {
    ...source,
    sections: source.sections.filter((section) => section.projectId !== projectId),
    statuses: source.statuses.filter((status) => status.projectId !== projectId),
    priorities: source.priorities.filter((priority) => priority.projectId !== projectId),
    tasks: source.tasks.filter((task) => task.projectId !== projectId),
    checklistItems: source.checklistItems.filter((item) => !taskIds.has(item.taskId)),
    tags: source.tags.filter((tag) => tag.projectId !== projectId),
    taskTagLinks: source.taskTagLinks.filter((link) => !taskIds.has(link.taskId) && !tagIds.has(link.tagId)),
    customFields: source.customFields.filter((field) => field.projectId !== projectId),
    customFieldOptions: source.customFieldOptions.filter((option) => !fieldIds.has(option.fieldId)),
    customFieldValues: source.customFieldValues.filter((value) =>
      !taskIds.has(value.taskId) && !fieldIds.has(value.fieldId)
    ),
    customFieldOptionValues: source.customFieldOptionValues.filter((value) =>
      !taskIds.has(value.taskId) && !fieldIds.has(value.fieldId)
    ),
    dependencies: source.dependencies.filter((dependency) =>
      !taskIds.has(dependency.blockingTaskId) && !taskIds.has(dependency.blockedTaskId)
    ),
    eventLinks: source.eventLinks.filter((link) => !taskIds.has(link.taskId)),
    taskChangeEvents: source.taskChangeEvents.filter((event) => !taskIds.has(event.taskId)),
  };
}

export function mergeProjectSnapshot(
  current: ProjectsSnapshot,
  incoming: ProjectsSnapshot,
  projectId: string,
): ProjectsSnapshot {
  const base = snapshotWithoutProjectData(current, projectId);
  return {
    ...base,
    groups: incoming.groups,
    projects: incoming.projects,
    viewPreferences: incoming.viewPreferences,
    customEmojis: incoming.customEmojis,
    sections: [...base.sections, ...incoming.sections],
    statuses: [...base.statuses, ...incoming.statuses],
    priorities: [...base.priorities, ...incoming.priorities],
    tasks: [...base.tasks, ...incoming.tasks],
    checklistItems: [...base.checklistItems, ...incoming.checklistItems],
    tags: [...base.tags, ...incoming.tags],
    taskTagLinks: [...base.taskTagLinks, ...incoming.taskTagLinks],
    customFields: [...base.customFields, ...incoming.customFields],
    customFieldOptions: [...base.customFieldOptions, ...incoming.customFieldOptions],
    customFieldValues: [...base.customFieldValues, ...incoming.customFieldValues],
    customFieldOptionValues: [...base.customFieldOptionValues, ...incoming.customFieldOptionValues],
    dependencies: [...base.dependencies, ...incoming.dependencies],
    eventLinks: [...base.eventLinks, ...incoming.eventLinks],
    taskChangeEvents: [...base.taskChangeEvents, ...incoming.taskChangeEvents],
  };
}
