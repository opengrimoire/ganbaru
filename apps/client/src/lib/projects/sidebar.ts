import type { ProjectListKey } from "./lists";
import { projectHasLockedSystemIdentity } from "./system-defaults";
import type { Project, ProjectLifecycleStatus } from "./types";

/**
 * Returns the list whose projects the Projects sidebar shows without a search: the selected project's list (its group,
 * or the projects outside every group), otherwise the first available list.
 */
export function resolveProjectSidebarListKey(
  selectedListKey: ProjectListKey | null,
  availableListKeys: readonly ProjectListKey[],
): ProjectListKey | null {
  if (selectedListKey && availableListKeys.includes(selectedListKey)) return selectedListKey;
  return availableListKeys[0] ?? null;
}

/** A visible group, or the projects outside every group, and the projects the sidebar may list from it. */
export interface ProjectSidebarSearchGroup<TProject extends { name: string }> {
  /** The group name, or `null` for the projects outside every group, which only match by their own name. */
  name: string | null;
  projects: readonly TProject[];
}

/** A project that matches a sidebar search, with the name of its group, or `null` when it has none. */
export interface ProjectSidebarSearchResult<TProject> {
  project: TProject;
  groupName: string | null;
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
    const groupMatches = group.name !== null && normalizeSearchText(group.name).includes(normalizedQuery);
    return group.projects
      .filter((project) => groupMatches || normalizeSearchText(project.name).includes(normalizedQuery))
      .map((project) => ({ project, groupName: group.name }));
  });
}

function normalizeSearchText(value: string): string {
  return value.normalize("NFD").replace(/\p{M}/gu, "").toLocaleLowerCase().replace(/\s+/g, " ").trim();
}

/** What the project row menu offers for one project. */
export interface ProjectSidebarActions {
  /** Built-in projects keep their name and group, so rename and move appear unavailable. */
  identityLocked: boolean;
  canRename: boolean;
  /** Visible groups the project can move to, in sidebar order. */
  moveGroupIds: string[];
  /** Whether the project can leave its group to stand outside every group. */
  canMoveOutOfGroup: boolean;
  /** Whether any move target exists: another group, or outside every group. */
  canMoveToGroup: boolean;
  /** Built-in projects cannot be deleted. */
  canDelete: boolean;
  /** Hide toggles between active and hidden; it reads Show when the project is hidden. */
  visibility: { label: "hide" | "show"; status: ProjectLifecycleStatus };
  /** Archive toggles between active and archived; it reads Restore when the project is archived. */
  archive: { label: "archive" | "restore"; status: ProjectLifecycleStatus };
}

/** Decides the row menu for a project from its lifecycle status, its group, and the visible groups. */
export function projectSidebarActions(
  project: Pick<Project, "id" | "groupId" | "status">,
  visibleGroupIds: readonly string[],
): ProjectSidebarActions {
  const identityLocked = projectHasLockedSystemIdentity(project);
  const moveGroupIds = identityLocked ? [] : visibleGroupIds.filter((groupId) => groupId !== project.groupId);
  const canMoveOutOfGroup = !identityLocked && project.groupId !== null;
  return {
    identityLocked,
    canRename: !identityLocked,
    moveGroupIds,
    canMoveOutOfGroup,
    canMoveToGroup: moveGroupIds.length > 0 || canMoveOutOfGroup,
    canDelete: !identityLocked,
    visibility: project.status === "hidden"
      ? { label: "show", status: "active" }
      : { label: "hide", status: "hidden" },
    archive: project.status === "archived"
      ? { label: "restore", status: "active" }
      : { label: "archive", status: "archived" },
  };
}
