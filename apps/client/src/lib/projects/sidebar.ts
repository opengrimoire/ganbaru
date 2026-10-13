/**
 * Returns the group whose projects the Projects sidebar lists without a search: the selected project's group, otherwise
 * the first visible group.
 */
export function resolveProjectSidebarGroupId(
  selectedGroupId: string | null,
  visibleGroupIds: readonly string[],
): string | null {
  if (selectedGroupId && visibleGroupIds.includes(selectedGroupId)) return selectedGroupId;
  return visibleGroupIds[0] ?? null;
}

/** A visible group and the projects the sidebar may list from it. */
export interface ProjectSidebarSearchGroup<TProject extends { name: string }> {
  name: string;
  projects: readonly TProject[];
}

/** A project that matches a sidebar search, with the name of its group. */
export interface ProjectSidebarSearchResult<TProject> {
  project: TProject;
  groupName: string;
}

/**
 * Lists the projects of every given group whose name, or whose group's name, contains the query. Matching ignores case
 * and diacritics, and results keep group and project order. An empty query matches nothing.
 */
export function searchProjectSidebar<TProject extends { name: string }>(
  query: string,
  groups: readonly ProjectSidebarSearchGroup<TProject>[],
): ProjectSidebarSearchResult<TProject>[] {
  const normalizedQuery = normalizeSearchText(query);
  if (!normalizedQuery) return [];
  return groups.flatMap((group) => {
    const groupMatches = normalizeSearchText(group.name).includes(normalizedQuery);
    return group.projects
      .filter((project) => groupMatches || normalizeSearchText(project.name).includes(normalizedQuery))
      .map((project) => ({ project, groupName: group.name }));
  });
}

function normalizeSearchText(value: string): string {
  return value.normalize("NFD").replace(/\p{M}/gu, "").toLocaleLowerCase().replace(/\s+/g, " ").trim();
}
