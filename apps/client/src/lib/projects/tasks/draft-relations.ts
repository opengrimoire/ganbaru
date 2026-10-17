import {
  PROJECT_TAG_DEFAULT_COLOR,
  type MoveDirection,
  type ProjectChecklistItem,
  type ProjectCustomFieldValueUpdate,
  type ProjectTag,
  type ProjectTask,
  type ProjectTaskDependency,
} from "$lib/projects/types";
import type { ProjectTaskDetailDraft } from "./detail";

/** Checklist item composed on a task draft. */
export interface ProjectTaskDraftChecklistItem {
  id: string;
  title: string;
  completed: boolean;
}

/** Subtask composed on a task draft; `statusId` is the status it is created in. */
export interface ProjectTaskDraftSubtask {
  id: string;
  title: string;
  statusId: string;
}

/**
 * Relations collected on a task draft. They live only in the dialog until the
 * task is created, then each one is written against the new task.
 */
export interface ProjectTaskDraftRelations {
  parentTaskId: string | null;
  checklist: ProjectTaskDraftChecklistItem[];
  subtasks: ProjectTaskDraftSubtask[];
  tagIds: string[];
  newTagNames: string[];
  blockedByTaskIds: string[];
  eventIds: string[];
}

/** Writes draft relations once the task exists. */
export interface ProjectTaskDraftRelationWriter {
  addChecklistItem: (taskId: string, title: string, completed: boolean) => Promise<void>;
  addSubtask: (parent: ProjectTask, title: string, statusId: string) => Promise<void>;
  linkTag: (taskId: string, tagId: string) => Promise<void>;
  addAndLinkTag: (task: ProjectTask, name: string) => Promise<void>;
  addDependency: (blockingTaskId: string, blockedTaskId: string) => Promise<void>;
  linkEvent: (taskId: string, eventId: string) => Promise<void>;
  saveCustomFieldValue: (value: ProjectCustomFieldValueUpdate) => Promise<void>;
}

/** Custom field value composed on a draft, without the task it belongs to. */
export type ProjectTaskDraftCustomFieldValue = Omit<ProjectCustomFieldValueUpdate, "taskId">;

const DRAFT_NEW_TAG_PREFIX = "draft-tag:";

/** Builds the relation state of a task draft that has nothing attached yet. */
export function emptyProjectTaskDraftRelations(): ProjectTaskDraftRelations {
  return {
    parentTaskId: null,
    checklist: [],
    subtasks: [],
    tagIds: [],
    newTagNames: [],
    blockedByTaskIds: [],
    eventIds: [],
  };
}

/** Reports whether a draft has any relation that creation would write. */
export function projectTaskDraftRelationsChanged(relations: ProjectTaskDraftRelations): boolean {
  return relations.parentTaskId !== null
    || relations.checklist.length > 0
    || relations.subtasks.length > 0
    || relations.tagIds.length > 0
    || relations.newTagNames.length > 0
    || relations.blockedByTaskIds.length > 0
    || relations.eventIds.length > 0;
}

/** Moves one entry of a draft list one step, leaving the list unchanged at its edges. */
export function moveProjectTaskDraftEntry<T extends { id: string }>(
  entries: readonly T[],
  id: string,
  direction: MoveDirection,
): T[] {
  const index = entries.findIndex((entry) => entry.id === id);
  const target = index + direction;
  if (index < 0 || target < 0 || target >= entries.length) return [...entries];
  const next = [...entries];
  [next[index], next[target]] = [next[target], next[index]];
  return next;
}

/** Builds the in-memory task that stands in for a draft in the detail sections. */
export function projectTaskDraftPreview(input: {
  id: string;
  projectId: string;
  draft: ProjectTaskDetailDraft;
  parentTaskId: string | null;
  timestamp: string;
}): ProjectTask {
  return {
    id: input.id,
    projectId: input.projectId,
    sectionId: input.draft.sectionId,
    statusId: input.draft.statusId,
    parentTaskId: input.parentTaskId ?? undefined,
    title: input.draft.title,
    description: input.draft.description,
    priority: input.draft.priority,
    taskType: input.draft.taskType,
    sectionSortOrder: 0,
    statusSortOrder: 0,
    milestone: input.draft.milestone,
    createdAt: input.timestamp,
    updatedAt: input.timestamp,
    detailLoaded: true,
  };
}

/** Projects draft checklist entries as checklist items for display. */
export function projectTaskDraftChecklistItems(
  relations: ProjectTaskDraftRelations,
  taskId: string,
  timestamp: string,
): ProjectChecklistItem[] {
  return relations.checklist.map((item, index) => ({
    id: item.id,
    taskId,
    title: item.title,
    completedAt: item.completed ? timestamp : undefined,
    sortOrder: index,
    createdAt: timestamp,
    updatedAt: timestamp,
  }));
}

/** Projects draft subtasks as tasks under the draft for display. */
export function projectTaskDraftSubtaskTasks(
  relations: ProjectTaskDraftRelations,
  parent: ProjectTask,
): ProjectTask[] {
  return relations.subtasks.map((subtask, index) => ({
    ...parent,
    id: subtask.id,
    parentTaskId: parent.id,
    statusId: subtask.statusId,
    title: subtask.title,
    description: "",
    priority: "normal",
    taskType: "task",
    sectionSortOrder: index,
    milestone: false,
  }));
}

/** Projects draft blockers as dependencies on the draft task for display. */
export function projectTaskDraftDependencies(
  relations: ProjectTaskDraftRelations,
  taskId: string,
  timestamp: string,
): ProjectTaskDependency[] {
  return relations.blockedByTaskIds.map((blockingTaskId) => ({
    id: blockingTaskId,
    blockingTaskId,
    blockedTaskId: taskId,
    dependencyType: "blocks",
    createdAt: timestamp,
  }));
}

/** Projects linked and pending tags of a draft for display. */
export function projectTaskDraftTags(input: {
  relations: ProjectTaskDraftRelations;
  projectTags: readonly ProjectTag[];
  projectId: string;
  timestamp: string;
}): ProjectTag[] {
  const linked = input.relations.tagIds.flatMap((tagId) => {
    const tag = input.projectTags.find((candidate) => candidate.id === tagId);
    return tag ? [tag] : [];
  });
  const pending = input.relations.newTagNames.map((name, index): ProjectTag => ({
    id: `${DRAFT_NEW_TAG_PREFIX}${name}`,
    projectId: input.projectId,
    name,
    color: PROJECT_TAG_DEFAULT_COLOR,
    sortOrder: index,
    createdAt: input.timestamp,
    updatedAt: input.timestamp,
  }));
  return [...linked, ...pending];
}

/** Removes a linked or pending tag from a draft by the ID it is displayed with. */
export function removeProjectTaskDraftTag(
  relations: ProjectTaskDraftRelations,
  tagId: string,
): ProjectTaskDraftRelations {
  if (tagId.startsWith(DRAFT_NEW_TAG_PREFIX)) {
    const name = tagId.slice(DRAFT_NEW_TAG_PREFIX.length);
    return { ...relations, newTagNames: relations.newTagNames.filter((entry) => entry !== name) };
  }
  return { ...relations, tagIds: relations.tagIds.filter((entry) => entry !== tagId) };
}

/** Reports whether a tag name is already linked or pending on a draft. */
export function projectTaskDraftHasTagName(
  relations: ProjectTaskDraftRelations,
  projectTags: readonly ProjectTag[],
  name: string,
): boolean {
  const normalized = name.trim().toLowerCase();
  return relations.newTagNames.some((entry) => entry.toLowerCase() === normalized)
    || projectTags.some((tag) => relations.tagIds.includes(tag.id) && tag.name.trim().toLowerCase() === normalized);
}

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

/**
 * Writes every draft relation against the created task. Each write is
 * independent, so one failure does not stop the rest; the error messages of
 * failed writes are returned for the caller to report.
 */
export async function applyProjectTaskDraftRelations(input: {
  task: ProjectTask;
  relations: ProjectTaskDraftRelations;
  customFieldValues: readonly ProjectTaskDraftCustomFieldValue[];
  writer: ProjectTaskDraftRelationWriter;
}): Promise<string[]> {
  const { task, relations, writer } = input;
  const failures: string[] = [];
  const attempt = async (write: () => Promise<void>): Promise<void> => {
    try {
      await write();
    } catch (error) {
      failures.push(errorMessage(error));
    }
  };
  for (const item of relations.checklist) {
    await attempt(() => writer.addChecklistItem(task.id, item.title, item.completed));
  }
  for (const subtask of relations.subtasks) {
    await attempt(() => writer.addSubtask(task, subtask.title, subtask.statusId));
  }
  for (const tagId of relations.tagIds) {
    await attempt(() => writer.linkTag(task.id, tagId));
  }
  for (const name of relations.newTagNames) {
    await attempt(() => writer.addAndLinkTag(task, name));
  }
  for (const blockingTaskId of relations.blockedByTaskIds) {
    await attempt(() => writer.addDependency(blockingTaskId, task.id));
  }
  for (const eventId of relations.eventIds) {
    await attempt(() => writer.linkEvent(task.id, eventId));
  }
  for (const value of input.customFieldValues) {
    await attempt(() => writer.saveCustomFieldValue({ ...value, taskId: task.id }));
  }
  return failures;
}
