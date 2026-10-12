import type {
  ProjectOptionalDataKind,
  ProjectsOptionalData,
  ProjectsSnapshot,
  ProjectViewId,
} from "$lib/projects/types";

export const PROJECT_SCOPED_OPTIONAL_DATA_KINDS = [
  "relationships",
  "custom_fields",
  "history",
  "checklist",
  "saved_views",
] as const satisfies readonly ProjectOptionalDataKind[];

/** Returns the stable cache key for one optional Projects data request. */
export function projectOptionalDataKey(
  kind: ProjectOptionalDataKind,
  projectId: string | null,
): string {
  return kind === "custom_emojis" ? kind : `${projectId ?? ""}:${kind}`;
}

/**
 * Lists optional collections a project view needs before it renders.
 *
 * List reads its visible columns, column widths, and presentation from view preferences, which load with saved views. Rendering it without them shows defaults, and a resize or column change made then would overwrite the saved preference.
 */
export function projectViewOptionalDataKinds(view: ProjectViewId): readonly ProjectOptionalDataKind[] {
  return view === "list" ? ["saved_views"] : [];
}

function projectTaskIds(source: ProjectsSnapshot, projectId: string): Set<string> {
  return new Set(
    source.tasks.filter((task) => task.projectId === projectId).map((task) => task.id),
  );
}

/** Replaces exactly one optional collection family without disturbing other cached projects. */
export function mergeProjectOptionalData(
  source: ProjectsSnapshot,
  incoming: ProjectsOptionalData,
): ProjectsSnapshot {
  if (incoming.kind === "custom_emojis") {
    return { ...source, customEmojis: incoming.customEmojis };
  }
  const projectId = incoming.projectId;
  if (!projectId) return source;
  const taskIds = projectTaskIds(source, projectId);

  if (incoming.kind === "relationships") {
    const oldTagIds = new Set(
      source.tags.filter((tag) => tag.projectId === projectId).map((tag) => tag.id),
    );
    return {
      ...source,
      tags: [...source.tags.filter((tag) => tag.projectId !== projectId), ...incoming.tags],
      taskTagLinks: [
        ...source.taskTagLinks.filter((link) =>
          !taskIds.has(link.taskId) && !oldTagIds.has(link.tagId)
        ),
        ...incoming.taskTagLinks,
      ],
      dependencies: [
        ...source.dependencies.filter((dependency) =>
          !taskIds.has(dependency.blockingTaskId) && !taskIds.has(dependency.blockedTaskId)
        ),
        ...incoming.dependencies,
      ],
      eventLinks: [
        ...source.eventLinks.filter((link) => !taskIds.has(link.taskId)),
        ...incoming.eventLinks,
      ],
    };
  }

  if (incoming.kind === "custom_fields") {
    const oldFieldIds = new Set(
      source.customFields.filter((field) => field.projectId === projectId).map((field) => field.id),
    );
    return {
      ...source,
      customFields: [
        ...source.customFields.filter((field) => field.projectId !== projectId),
        ...incoming.customFields,
      ],
      customFieldOptions: [
        ...source.customFieldOptions.filter((option) => !oldFieldIds.has(option.fieldId)),
        ...incoming.customFieldOptions,
      ],
      customFieldValues: [
        ...source.customFieldValues.filter((value) =>
          !taskIds.has(value.taskId) && !oldFieldIds.has(value.fieldId)
        ),
        ...incoming.customFieldValues,
      ],
      customFieldOptionValues: [
        ...source.customFieldOptionValues.filter((value) =>
          !taskIds.has(value.taskId) && !oldFieldIds.has(value.fieldId)
        ),
        ...incoming.customFieldOptionValues,
      ],
    };
  }

  if (incoming.kind === "history") {
    return {
      ...source,
      taskChangeEvents: [
        ...source.taskChangeEvents.filter((event) => !taskIds.has(event.taskId)),
        ...incoming.taskChangeEvents,
      ],
    };
  }

  if (incoming.kind === "checklist") {
    return {
      ...source,
      checklistItems: [
        ...source.checklistItems.filter((item) => !taskIds.has(item.taskId)),
        ...incoming.checklistItems,
      ],
    };
  }

  return {
    ...source,
    viewPreferences: [
      ...source.viewPreferences.filter((preference) => preference.projectId !== projectId),
      ...incoming.viewPreferences,
    ],
  };
}
