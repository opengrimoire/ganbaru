import type {
  ProjectConflictField,
  ProjectConflictTarget,
  ProjectConflicts,
  ProjectDependencyConflict,
  ProjectFieldConflict,
} from "$lib/api/project-conflicts";

/** Characters of a version shown before it is cut, so a long description does not fill the card. */
export const PROJECT_CONFLICT_PREVIEW_CHARS = 240;

/** The conflict on one text field of a row, or undefined when the field has none. */
export function findFieldConflict(
  conflicts: ProjectConflicts | undefined,
  target: ProjectConflictTarget,
  id: string,
  field: ProjectConflictField,
): ProjectFieldConflict | undefined {
  return conflicts?.fields.find((conflict) =>
    conflict.target === target && conflict.id === id && conflict.field === field && conflict.versions.length > 0);
}

/** Hidden dependencies that involve a task, as the blocking or the blocked side. */
export function dependencyConflictsForTask(
  conflicts: ProjectConflicts | undefined,
  taskId: string,
): ProjectDependencyConflict[] {
  return conflicts?.dependencies.filter((conflict) =>
    conflict.blockingTaskId === taskId || conflict.blockedTaskId === taskId) ?? [];
}

/** Whether a hidden dependency can be added again. It cannot while that would recreate the cycle. */
export function canRestoreDependency(conflict: Pick<ProjectDependencyConflict, "wouldCycle">): boolean {
  return !conflict.wouldCycle;
}

/** Text of a version cut to {@link PROJECT_CONFLICT_PREVIEW_CHARS} characters with an ellipsis. */
export function previewVersionText(text: string, maxChars = PROJECT_CONFLICT_PREVIEW_CHARS): string {
  const characters = Array.from(text);
  return characters.length <= maxChars ? text : `${characters.slice(0, maxChars).join("").trimEnd()}…`;
}
