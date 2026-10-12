import type {
  ProjectMutation,
  ProjectMutationRemoval,
  ProjectsSnapshot,
} from "$lib/projects/types";

type Identified = { id: string };

function upsertByKey<T>(
  current: readonly T[],
  changed: readonly T[],
  key: (value: T) => string,
): T[] {
  if (changed.length === 0) return current.slice();
  const changedByKey = new Map(changed.map((value) => [key(value), value]));
  const result = current.map((value) => changedByKey.get(key(value)) ?? value);
  const currentKeys = new Set(current.map(key));
  for (const value of changed) {
    if (!currentKeys.has(key(value))) result.push(value);
  }
  return result;
}

function upsertById<T extends Identified>(current: readonly T[], changed: readonly T[]): T[] {
  return upsertByKey(current, changed, (value) => value.id);
}

/** A delayed command or receipt must not replace a newer native revision. */
function upsertRevisioned<T extends Identified & { revision?: number }>(current: readonly T[], changed: readonly T[]): T[] {
  const currentById = new Map(current.map((task) => [task.id, task]));
  return upsertById(current, changed.filter((task) => {
    const existing = currentById.get(task.id);
    return existing?.revision === undefined
      || (task.revision !== undefined && task.revision >= existing.revision);
  }));
}

function removeProject(snapshot: ProjectsSnapshot, projectId: string): ProjectsSnapshot {
  const taskIds = new Set(snapshot.tasks.filter((task) => task.projectId === projectId).map((task) => task.id));
  const tagIds = new Set(snapshot.tags.filter((tag) => tag.projectId === projectId).map((tag) => tag.id));
  const fieldIds = new Set(
    snapshot.customFields.filter((field) => field.projectId === projectId).map((field) => field.id),
  );
  return {
    ...snapshot,
    projects: snapshot.projects.filter((project) => project.id !== projectId),
    sections: snapshot.sections.filter((section) => section.projectId !== projectId),
    statuses: snapshot.statuses.filter((status) => status.projectId !== projectId),
    priorities: snapshot.priorities.filter((priority) => priority.projectId !== projectId),
    tasks: snapshot.tasks.filter((task) => !taskIds.has(task.id)),
    checklistItems: snapshot.checklistItems.filter((item) => !taskIds.has(item.taskId)),
    tags: snapshot.tags.filter((tag) => !tagIds.has(tag.id)),
    taskTagLinks: snapshot.taskTagLinks.filter(
      (link) => !taskIds.has(link.taskId) && !tagIds.has(link.tagId),
    ),
    customFields: snapshot.customFields.filter((field) => !fieldIds.has(field.id)),
    customFieldOptions: snapshot.customFieldOptions.filter((option) => !fieldIds.has(option.fieldId)),
    customFieldValues: snapshot.customFieldValues.filter(
      (value) => !taskIds.has(value.taskId) && !fieldIds.has(value.fieldId),
    ),
    customFieldOptionValues: snapshot.customFieldOptionValues.filter(
      (value) => !taskIds.has(value.taskId) && !fieldIds.has(value.fieldId),
    ),
    dependencies: snapshot.dependencies.filter(
      (dependency) =>
        !taskIds.has(dependency.blockingTaskId) && !taskIds.has(dependency.blockedTaskId),
    ),
    eventLinks: snapshot.eventLinks.filter((link) => !taskIds.has(link.taskId)),
    taskChangeEvents: snapshot.taskChangeEvents.filter((event) => !taskIds.has(event.taskId)),
    viewPreferences: snapshot.viewPreferences.filter((preference) => preference.projectId !== projectId),
  };
}

function applyRemoval(snapshot: ProjectsSnapshot, removal: ProjectMutationRemoval): ProjectsSnapshot {
  switch (removal.kind) {
    case "group": {
      let result = { ...snapshot, groups: snapshot.groups.filter((group) => group.id !== removal.id) };
      for (const project of snapshot.projects) {
        if (project.groupId === removal.id) result = removeProject(result, project.id);
      }
      return result;
    }
    case "project":
      return removeProject(snapshot, removal.id);
    case "status":
      return { ...snapshot, statuses: snapshot.statuses.filter((status) => status.id !== removal.id) };
    case "priority":
      return { ...snapshot, priorities: snapshot.priorities.filter((priority) => priority.id !== removal.id) };
    case "checklist_item":
      return {
        ...snapshot,
        checklistItems: snapshot.checklistItems.filter((item) => item.id !== removal.id),
      };
    case "tag":
      return {
        ...snapshot,
        tags: snapshot.tags.filter((tag) => tag.id !== removal.id),
        taskTagLinks: snapshot.taskTagLinks.filter((link) => link.tagId !== removal.id),
      };
    case "task_tag_link":
      return {
        ...snapshot,
        taskTagLinks: snapshot.taskTagLinks.filter(
          (link) => link.taskId !== removal.taskId || link.tagId !== removal.tagId,
        ),
      };
    case "custom_field": {
      const optionIds = new Set(
        snapshot.customFieldOptions
          .filter((option) => option.fieldId === removal.id)
          .map((option) => option.id),
      );
      return {
        ...snapshot,
        customFields: snapshot.customFields.filter((field) => field.id !== removal.id),
        customFieldOptions: snapshot.customFieldOptions.filter((option) => option.fieldId !== removal.id),
        customFieldValues: snapshot.customFieldValues.filter((value) => value.fieldId !== removal.id),
        customFieldOptionValues: snapshot.customFieldOptionValues.filter(
          (value) => value.fieldId !== removal.id && !optionIds.has(value.optionId),
        ),
      };
    }
    case "custom_field_option":
      return {
        ...snapshot,
        customFieldOptions: snapshot.customFieldOptions.filter((option) => option.id !== removal.id),
        customFieldOptionValues: snapshot.customFieldOptionValues.filter(
          (value) => value.optionId !== removal.id,
        ),
      };
    case "custom_field_value":
      return {
        ...snapshot,
        customFieldValues: snapshot.customFieldValues.filter(
          (value) => value.taskId !== removal.taskId || value.fieldId !== removal.fieldId,
        ),
        customFieldOptionValues: snapshot.customFieldOptionValues.filter(
          (value) => value.taskId !== removal.taskId || value.fieldId !== removal.fieldId,
        ),
      };
    case "dependency":
      return {
        ...snapshot,
        dependencies: snapshot.dependencies.filter((dependency) => dependency.id !== removal.id),
      };
    case "event_link":
      return {
        ...snapshot,
        eventLinks: snapshot.eventLinks.filter(
          (link) => link.taskId !== removal.taskId || link.eventId !== removal.eventId,
        ),
      };
    case "view_preference":
      return {
        ...snapshot,
        viewPreferences: snapshot.viewPreferences.filter(
          (preference) =>
            preference.projectId !== removal.projectId ||
            preference.viewId !== removal.viewId ||
            preference.preferenceKey !== removal.preferenceKey,
        ),
      };
    case "custom_emoji":
      return {
        ...snapshot,
        customEmojis: snapshot.customEmojis.filter((emoji) => emoji.id !== removal.id),
      };
  }
}

/** Applies a committed backend mutation without mutating the previous snapshot. */
export function applyProjectMutation(
  snapshot: ProjectsSnapshot,
  mutation: ProjectMutation,
): ProjectsSnapshot {
  const removed = mutation.removals.reduce(applyRemoval, snapshot);
  const changed = mutation.changed;
  return {
    groups: upsertById(removed.groups, changed.groups),
    projects: upsertById(removed.projects, changed.projects),
    sections: upsertById(removed.sections, changed.sections),
    statuses: upsertById(removed.statuses, changed.statuses),
    priorities: upsertById(removed.priorities, changed.priorities),
    tasks: upsertRevisioned(removed.tasks, changed.tasks),
    checklistItems: upsertById(removed.checklistItems, changed.checklistItems),
    tags: upsertById(removed.tags, changed.tags),
    taskTagLinks: upsertByKey(
      removed.taskTagLinks,
      changed.taskTagLinks,
      (link) => `${link.taskId}\u0000${link.tagId}`,
    ),
    customFields: upsertRevisioned(removed.customFields, changed.customFields),
    customFieldOptions: upsertRevisioned(removed.customFieldOptions, changed.customFieldOptions),
    customFieldValues: upsertByKey(
      removed.customFieldValues,
      changed.customFieldValues,
      (value) => `${value.taskId}\u0000${value.fieldId}`,
    ),
    customFieldOptionValues: upsertByKey(
      removed.customFieldOptionValues,
      changed.customFieldOptionValues,
      (value) => `${value.taskId}\u0000${value.fieldId}\u0000${value.optionId}`,
    ),
    dependencies: upsertById(removed.dependencies, changed.dependencies),
    eventLinks: upsertByKey(
      removed.eventLinks,
      changed.eventLinks,
      (link) => `${link.taskId}\u0000${link.eventId}`,
    ),
    taskChangeEvents: upsertById(removed.taskChangeEvents, changed.taskChangeEvents),
    viewPreferences: upsertByKey(
      removed.viewPreferences,
      changed.viewPreferences,
      (preference) =>
        `${preference.projectId}\u0000${preference.viewId}\u0000${preference.preferenceKey}`,
    ),
    customEmojis: upsertById(removed.customEmojis, changed.customEmojis),
  };
}
