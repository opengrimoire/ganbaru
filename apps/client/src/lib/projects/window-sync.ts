import type { ProjectMutation } from "$lib/projects/types";
import { createVaultChangeChannel } from "$lib/windows/vault-change-channel";

/** Key published when a committed change names no project, such as a group edit. */
export const PROJECTS_WORKSPACE_CHANGE_KEY = "workspace";

const projectChanges = createVaultChangeChannel("projects-changed");

/** Ids of the projects a committed mutation touched, else the projects the window shows, else the workspace key. */
export function projectMutationChangeKeys(mutation: ProjectMutation, shownProjectIds: readonly (string | null)[]): string[] {
  const { changed } = mutation;
  const keys = new Set<string>([
    ...changed.projects.map((project) => project.id),
    ...[
      ...changed.tasks, ...changed.sections, ...changed.statuses, ...changed.priorities,
      ...changed.tags, ...changed.customFields, ...changed.views,
    ].map((row) => row.projectId),
  ]);
  if (keys.size > 0) return [...keys];
  const shown = new Set(shownProjectIds.filter((projectId): projectId is string => Boolean(projectId)));
  return shown.size > 0 ? [...shown] : [PROJECTS_WORKSPACE_CHANGE_KEY];
}

/** Tell other windows of this vault that Projects data changed. */
export function publishProjectChanges(mutation: ProjectMutation, shownProjectIds: readonly (string | null)[]): void {
  projectChanges.publish(projectMutationChangeKeys(mutation, shownProjectIds));
}

/** Receive the project ids (or the workspace key) other windows changed, gathered per pause in their editing. */
export function listenForProjectChanges(onChange: (keys: ReadonlySet<string>) => void): Promise<() => void> {
  return projectChanges.listen(onChange);
}
