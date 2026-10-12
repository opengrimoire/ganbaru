import * as projectSnapshot from "$lib/projects/snapshot/queries";
import {
  createProjectLoadedDataIndexReader,
  type ProjectLoadedDataIndexReader,
} from "$lib/projects/snapshot/loaded-data-index";
import type {
  Project,
  ProjectChecklistItem,
  ProjectCustomField,
  ProjectCustomFieldOption,
  ProjectCustomFieldValue,
  ProjectGroup,
  ProjectTag,
  ProjectPriorityConfig,
  ProjectSavedTaskView,
  ProjectSection,
  ProjectsSnapshot,
  ProjectStatus,
  ProjectTask,
  ProjectTaskChangeEvent,
  ProjectTaskDependency,
  ProjectTaskEventLink,
  ProjectTaskTagLink,
} from "$lib/projects/types";

type ProjectSnapshotReader = () => ProjectsSnapshot;

/**
 * Creates the read side of the project store around the current reactive snapshot.
 */
export function createProjectStoreSelectors(
  readRawSnapshot: ProjectSnapshotReader,
  indexReader: ProjectLoadedDataIndexReader = createProjectLoadedDataIndexReader(),
) {
  const readSnapshot = () => indexReader.read(readRawSnapshot());
  return {
    activeProjects(): Project[] {
      return projectSnapshot.activeProjects(readSnapshot());
    },
    firstProjectId(): string | null {
      return projectSnapshot.firstProjectId(readSnapshot());
    },
    projectById(projectId: string | null | undefined): Project | undefined {
      return projectSnapshot.projectById(readSnapshot(), projectId);
    },
    groupById(groupId: string | null | undefined): ProjectGroup | undefined {
      return projectSnapshot.groupById(readSnapshot(), groupId);
    },
    sectionsForProject(projectId: string | null | undefined): ProjectSection[] {
      return projectSnapshot.sectionsForProject(readSnapshot(), projectId);
    },
    sectionsForProjectIncludingInactive(projectId: string | null | undefined): ProjectSection[] {
      return projectSnapshot.sectionsForProjectIncludingInactive(readSnapshot(), projectId);
    },
    statusesForProject(projectId: string | null | undefined): ProjectStatus[] {
      return projectSnapshot.statusesForProject(readSnapshot(), projectId);
    },
    prioritiesForProject(projectId: string | null | undefined): ProjectPriorityConfig[] {
      return projectSnapshot.prioritiesForProject(readSnapshot(), projectId);
    },
    tasksForProject(projectId: string | null | undefined): ProjectTask[] {
      return projectSnapshot.tasksForProject(readSnapshot(), projectId);
    },
    tasksForProjectIncludingArchived(projectId: string | null | undefined): ProjectTask[] {
      return projectSnapshot.tasksForProjectIncludingArchived(readSnapshot(), projectId);
    },
    topLevelTasksForSection(projectId: string, sectionId: string): ProjectTask[] {
      return projectSnapshot.topLevelTasksForSection(readSnapshot(), projectId, sectionId);
    },
    topLevelTasksForStatus(projectId: string, statusId: string): ProjectTask[] {
      return projectSnapshot.topLevelTasksForStatus(readSnapshot(), projectId, statusId);
    },
    subtasksForTask(parentTaskId: string | null | undefined): ProjectTask[] {
      return projectSnapshot.subtasksForTask(readSnapshot(), parentTaskId);
    },
    subtasksForTaskIncludingArchived(parentTaskId: string | null | undefined): ProjectTask[] {
      return projectSnapshot.subtasksForTaskIncludingArchived(readSnapshot(), parentTaskId);
    },
    projectsForGroup(groupId: string): Project[] {
      return projectSnapshot.projectsForGroup(readSnapshot(), groupId);
    },
    projectsForGroupIncludingInactive(groupId: string): Project[] {
      return projectSnapshot.projectsForGroupIncludingInactive(readSnapshot(), groupId);
    },
    visibleGroups(): ProjectGroup[] {
      return projectSnapshot.visibleGroups(readSnapshot());
    },
    defaultSection(projectId: string): ProjectSection | undefined {
      return projectSnapshot.defaultSection(readSnapshot(), projectId);
    },
    defaultStatus(projectId: string): ProjectStatus | undefined {
      return projectSnapshot.defaultStatus(readSnapshot(), projectId);
    },
    doneStatus(projectId: string): ProjectStatus | undefined {
      return projectSnapshot.doneStatus(readSnapshot(), projectId);
    },
    reopenStatus(projectId: string): ProjectStatus | undefined {
      return projectSnapshot.reopenStatus(readSnapshot(), projectId);
    },
    statusById(statusId: string): ProjectStatus | undefined {
      return projectSnapshot.statusById(readSnapshot(), statusId);
    },
    taskById(taskId: string | null | undefined): ProjectTask | undefined {
      return projectSnapshot.taskById(readSnapshot(), taskId);
    },
    eventLinksForTask(taskId: string | null | undefined): ProjectTaskEventLink[] {
      return projectSnapshot.eventLinksForTask(readSnapshot(), taskId);
    },
    eventLinksForEvent(eventId: string | null | undefined): ProjectTaskEventLink[] {
      return projectSnapshot.eventLinksForEvent(readSnapshot(), eventId);
    },
    taskChangeEventsForTask(taskId: string | null | undefined): ProjectTaskChangeEvent[] {
      return projectSnapshot.taskChangeEventsForTask(readSnapshot(), taskId);
    },
    recentTaskChangeEventsForProject(
      projectId: string | null | undefined,
      limit = 8,
    ): ProjectTaskChangeEvent[] {
      return projectSnapshot.recentTaskChangeEventsForProject(readSnapshot(), projectId, limit);
    },
    checklistItemsForTask(taskId: string | null | undefined): ProjectChecklistItem[] {
      return projectSnapshot.checklistItemsForTask(readSnapshot(), taskId);
    },
    tagsForProject(projectId: string | null | undefined): ProjectTag[] {
      return projectSnapshot.tagsForProject(readSnapshot(), projectId);
    },
    tagById(tagId: string | null | undefined): ProjectTag | undefined {
      return projectSnapshot.tagById(readSnapshot(), tagId);
    },
    taskTagLinksForTask(taskId: string | null | undefined): ProjectTaskTagLink[] {
      return projectSnapshot.taskTagLinksForTask(readSnapshot(), taskId);
    },
    tagsForTask(taskId: string | null | undefined): ProjectTag[] {
      return projectSnapshot.tagsForTask(readSnapshot(), taskId);
    },
    unlinkedTagsForTask(task: ProjectTask | null | undefined): ProjectTag[] {
      return projectSnapshot.unlinkedTagsForTask(readSnapshot(), task);
    },
    projectTagByName(projectId: string, name: string): ProjectTag | undefined {
      return projectSnapshot.projectTagByName(readSnapshot(), projectId, name);
    },
    customFieldsForProject(projectId: string | null | undefined): ProjectCustomField[] {
      return projectSnapshot.customFieldsForProject(readSnapshot(), projectId);
    },
    customFieldById(fieldId: string | null | undefined): ProjectCustomField | undefined {
      return projectSnapshot.customFieldById(readSnapshot(), fieldId);
    },
    customFieldByName(projectId: string, name: string): ProjectCustomField | undefined {
      return projectSnapshot.customFieldByName(readSnapshot(), projectId, name);
    },
    customFieldOptionsForField(fieldId: string | null | undefined): ProjectCustomFieldOption[] {
      return projectSnapshot.customFieldOptionsForField(readSnapshot(), fieldId);
    },
    customFieldOptionByName(fieldId: string, name: string): ProjectCustomFieldOption | undefined {
      return projectSnapshot.customFieldOptionByName(readSnapshot(), fieldId, name);
    },
    customFieldValueForTask(
      taskId: string | null | undefined,
      fieldId: string | null | undefined,
    ): ProjectCustomFieldValue | undefined {
      return projectSnapshot.customFieldValueForTask(readSnapshot(), taskId, fieldId);
    },
    customFieldOptionValuesForTask(
      taskId: string | null | undefined,
      fieldId: string | null | undefined,
    ): ProjectCustomFieldOption[] {
      return projectSnapshot.customFieldOptionValuesForTask(readSnapshot(), taskId, fieldId);
    },
    dependenciesBlockingTask(taskId: string | null | undefined): ProjectTaskDependency[] {
      return projectSnapshot.dependenciesBlockingTask(readSnapshot(), taskId);
    },
    dependenciesBlockedByTask(taskId: string | null | undefined): ProjectTaskDependency[] {
      return projectSnapshot.dependenciesBlockedByTask(readSnapshot(), taskId);
    },
    savedTaskViewsForProject(projectId: string | null | undefined): ProjectSavedTaskView[] {
      return projectSnapshot.savedTaskViewsForProject(readSnapshot(), projectId);
    },
    nextGroupSortOrder(): number {
      return projectSnapshot.nextGroupSortOrder(readSnapshot());
    },
    nextProjectSortOrder(groupId: string): number {
      return projectSnapshot.nextProjectSortOrder(readSnapshot(), groupId);
    },
    nextSectionSortOrder(projectId: string): number {
      return projectSnapshot.nextSectionSortOrder(readSnapshot(), projectId);
    },
    nextStatusSortOrder(projectId: string): number {
      return projectSnapshot.nextStatusSortOrder(readSnapshot(), projectId);
    },
    nextPrioritySortOrder(projectId: string): number {
      return projectSnapshot.nextPrioritySortOrder(readSnapshot(), projectId);
    },
    nextTaskSectionSortOrder(projectId: string, sectionId: string): number {
      return projectSnapshot.nextTaskSectionSortOrder(readSnapshot(), projectId, sectionId);
    },
    nextSubtaskSortOrder(parentTaskId: string): number {
      return projectSnapshot.nextSubtaskSortOrder(readSnapshot(), parentTaskId);
    },
    nextChecklistSortOrder(taskId: string): number {
      return projectSnapshot.nextChecklistSortOrder(readSnapshot(), taskId);
    },
    nextTagSortOrder(projectId: string): number {
      return projectSnapshot.nextTagSortOrder(readSnapshot(), projectId);
    },
    nextCustomFieldSortOrder(projectId: string): number {
      return projectSnapshot.nextCustomFieldSortOrder(readSnapshot(), projectId);
    },
    nextCustomFieldOptionSortOrder(fieldId: string): number {
      return projectSnapshot.nextCustomFieldOptionSortOrder(readSnapshot(), fieldId);
    },
    nextCustomEmojiSortOrder(): number {
      return projectSnapshot.nextCustomEmojiSortOrder(readSnapshot());
    },
    nextTaskStatusSortOrder(projectId: string, statusId: string): number {
      return projectSnapshot.nextTaskStatusSortOrder(readSnapshot(), projectId, statusId);
    },
  };
}

export type ProjectStoreSelectors = ReturnType<typeof createProjectStoreSelectors>;
