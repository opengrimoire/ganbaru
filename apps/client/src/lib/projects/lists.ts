import type { ProjectGroup } from "./types";

/**
 * Identifies one list of projects in a navigator: a group, or the projects outside every group. Group ids are never
 * blank, so the prefixed keys cannot collide, and component state can use `null` for "nothing open" without
 * confusing it with the ungrouped list.
 */
export type ProjectListKey = `group:${string}` | "ungrouped";

/** The key of the projects outside every group. */
export const UNGROUPED_PROJECT_LIST_KEY: ProjectListKey = "ungrouped";

/** One list of projects in sidebar order: a visible group with its projects, or the projects outside every group. */
export interface ProjectList<TProject> {
  key: ProjectListKey;
  /** The group, or `null` for the projects outside every group. */
  group: ProjectGroup | null;
  projects: TProject[];
}

/** The list key of a group, or of the projects outside every group when `groupId` is `null`. */
export function projectListKey(groupId: string | null): ProjectListKey {
  return groupId === null ? UNGROUPED_PROJECT_LIST_KEY : `group:${groupId}`;
}

/** The group id a list key names, or `null` for the projects outside every group. */
export function projectListGroupId(key: ProjectListKey): string | null {
  return key === UNGROUPED_PROJECT_LIST_KEY ? null : key.slice("group:".length);
}

/**
 * Lists every given group with its projects, in the given order, followed by the projects outside every group. Groups
 * stay even when empty, since a navigator offers to create projects in them; the ungrouped list appears only when it
 * holds a project.
 */
export function buildProjectLists<TProject>(
  groups: readonly ProjectGroup[],
  projectsFor: (groupId: string | null) => TProject[],
): ProjectList<TProject>[] {
  const lists: ProjectList<TProject>[] = groups.map((group) => ({
    key: projectListKey(group.id),
    group,
    projects: projectsFor(group.id),
  }));
  const ungrouped = projectsFor(null);
  if (ungrouped.length > 0) lists.push({ key: UNGROUPED_PROJECT_LIST_KEY, group: null, projects: ungrouped });
  return lists;
}

/** Normalize a project search query for case-insensitive matching by name. */
export function normalizeProjectSearch(query: string): string {
  return query.trim().toLowerCase();
}

/** Whether a project name contains an already normalized query; an empty query matches every project. */
export function projectMatchesSearch(project: { name: string }, normalizedQuery: string): boolean {
  return !normalizedQuery || project.name.toLowerCase().includes(normalizedQuery);
}

/**
 * Narrow project lists to the projects whose names match a search, keeping list and project order and leaving out
 * lists without a match. An empty query keeps every list as it is.
 */
export function filterProjectLists<TProject extends { name: string }>(
  lists: readonly ProjectList<TProject>[],
  query: string,
): ProjectList<TProject>[] {
  const normalizedQuery = normalizeProjectSearch(query);
  if (!normalizedQuery) return [...lists];
  return lists.flatMap((list) => {
    const projects = list.projects.filter((project) => projectMatchesSearch(project, normalizedQuery));
    return projects.length > 0 ? [{ ...list, projects }] : [];
  });
}
