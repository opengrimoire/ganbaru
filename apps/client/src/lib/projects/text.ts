import type { Project } from "./types";

export function normalizeProjectName(value: string): string {
  return value.trim();
}

/** Key under which two project names count as the same: trimmed and compared without case, as the backend does. */
export function projectNameKey(value: string): string {
  return normalizeProjectName(value).toLowerCase();
}

/**
 * Whether another project in the group already uses the name, in any lifecycle status. The backend refuses such a
 * create, rename, or move; this check lets a form say so before submitting.
 */
export function isProjectNameTaken(
  projects: readonly Pick<Project, "id" | "groupId" | "name">[],
  groupId: string,
  name: string,
  exceptProjectId: string | null = null,
): boolean {
  const key = projectNameKey(name);
  if (!key) return false;
  return projects.some((project) =>
    project.groupId === groupId && project.id !== exceptProjectId && projectNameKey(project.name) === key);
}
