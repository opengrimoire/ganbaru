import { listenSyncApplied } from "$lib/api/sync";
import type { ProjectMutation } from "$lib/projects/types";
import { createVaultChangeChannel } from "$lib/windows/vault-change-channel";
import { PROJECTS_SYNC_TABLES } from "./sync-events";

/** Key published when a committed change names no project, such as a group edit. */
export const PROJECTS_WORKSPACE_CHANGE_KEY = "workspace";

/**
 * Key for changes sync made to Projects rows. Applied rows can be deleted ones that no longer name their project, so it
 * marks every project as changed.
 */
export const PROJECTS_SYNC_CHANGE_KEY = "sync";

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

/** Loaded projects whose data is still current after a change elsewhere: the shown ones, which reload, and the unchanged ones. */
export function projectsStillLoaded(
  loadedProjectIds: readonly string[],
  shownProjectIds: ReadonlySet<string>,
  changedKeys: ReadonlySet<string>,
): string[] {
  const everyProjectChanged = changedKeys.has(PROJECTS_SYNC_CHANGE_KEY);
  return loadedProjectIds.filter((projectId) =>
    shownProjectIds.has(projectId) || (!everyProjectChanged && !changedKeys.has(projectId)));
}

/** Tell other windows of this vault that Projects data changed. */
export function publishProjectChanges(mutation: ProjectMutation, shownProjectIds: readonly (string | null)[]): void {
  projectChanges.publish(projectMutationChangeKeys(mutation, shownProjectIds));
}

/** Tell other windows of this vault that sync recovery changed Projects rows, so every project reloads. */
export function publishProjectsSyncChange(): void {
  projectChanges.publish([PROJECTS_SYNC_CHANGE_KEY]);
}

/** Whether replicated changes to these tables affect Projects views. */
export function touchesProjects(tables: readonly string[]): boolean {
  return tables.some((table) => PROJECTS_SYNC_TABLES.has(table));
}

/**
 * Receive the project ids (or the workspace key) other windows changed, gathered per pause in their editing, and the
 * sync key when a linked device changed Projects rows.
 */
export async function listenForProjectChanges(onChange: (keys: ReadonlySet<string>) => void): Promise<() => void> {
  const stopWindows = await projectChanges.listen(onChange);
  try {
    const stopReplicated = await listenSyncApplied((tables) => {
      if (touchesProjects(tables)) onChange(new Set([PROJECTS_SYNC_CHANGE_KEY]));
    });
    return () => {
      stopWindows();
      stopReplicated();
    };
  } catch (error: unknown) {
    stopWindows();
    throw error;
  }
}
