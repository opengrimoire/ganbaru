import { getConfigKey, setConfigKey } from "$lib/vault/config";
import { parseStarredIds } from "$lib/utils/sidebar-order";

const ACTIVE_PROJECT_ID_CONFIG_KEY = "projects.activeProjectId";
const ACTIVE_VIEW_IDS_CONFIG_KEY = "projects.activeViewIds";
const SIDEBAR_COLLAPSED_CONFIG_KEY = "projects.sidebarCollapsed";
const SIDEBAR_COLLAPSED_PROJECT_IDS_CONFIG_KEY = "projects.sidebarCollapsedProjectIds";
const SHOW_INACTIVE_PROJECTS_CONFIG_KEY = "projects.showInactiveProjects";
const STARRED_PROJECT_IDS_CONFIG_KEY = "projects.starredProjectIds";
const STARRED_VIEW_IDS_CONFIG_KEY = "projects.starredViewIds";

export function parseStoredProjectId(value: unknown): string | null {
  if (typeof value !== "string") return null;
  const trimmed = value.trim();
  return trimmed.length > 0 ? trimmed : null;
}

/** Restores the last open view of each project, dropping malformed entries. */
export function parseStoredProjectActiveViewIds(value: unknown): Record<string, string> {
  if (typeof value !== "object" || value === null || Array.isArray(value)) return {};
  const result: Record<string, string> = {};
  for (const [projectId, viewId] of Object.entries(value)) {
    const parsedProjectId = parseStoredProjectId(projectId);
    const parsedViewId = parseStoredProjectId(viewId);
    if (parsedProjectId && parsedViewId) result[parsedProjectId] = parsedViewId;
  }
  return result;
}

/** Keeps remembered views only for projects that still exist, so deleted projects do not accumulate in config. */
export function pruneProjectActiveViewIds(
  activeViewIds: Readonly<Record<string, string>>,
  projectIds: ReadonlySet<string>,
): Record<string, string> {
  return Object.fromEntries(Object.entries(activeViewIds).filter(([projectId]) => projectIds.has(projectId)));
}

export function loadSavedActiveProjectId(): string | null {
  return parseStoredProjectId(getConfigKey<unknown>(ACTIVE_PROJECT_ID_CONFIG_KEY, undefined));
}

export function saveActiveProjectId(projectId: string | null): void {
  setConfigKey(ACTIVE_PROJECT_ID_CONFIG_KEY, projectId ?? undefined);
}

export function loadSavedProjectActiveViewIds(): Record<string, string> {
  return parseStoredProjectActiveViewIds(getConfigKey<unknown>(ACTIVE_VIEW_IDS_CONFIG_KEY, undefined));
}

export function saveProjectActiveViewIds(activeViewIds: Readonly<Record<string, string>>): void {
  setConfigKey(ACTIVE_VIEW_IDS_CONFIG_KEY, Object.keys(activeViewIds).length > 0 ? activeViewIds : undefined);
}

export function loadSavedProjectSidebarCollapsed(): boolean {
  return getConfigKey<unknown>(SIDEBAR_COLLAPSED_CONFIG_KEY, undefined) === true;
}

export function saveProjectSidebarCollapsed(collapsed: boolean): void {
  setConfigKey(SIDEBAR_COLLAPSED_CONFIG_KEY, collapsed || undefined);
}

/** Restores the ids of projects whose sidebar section is collapsed, trimmed and without duplicates. */
export function parseStoredProjectIds(value: unknown): string[] {
  if (!Array.isArray(value)) return [];
  const ids = value.flatMap((entry) => parseStoredProjectId(entry) ?? []);
  return [...new Set(ids)];
}

export function loadSavedCollapsedSidebarProjectIds(): string[] {
  return parseStoredProjectIds(getConfigKey<unknown>(SIDEBAR_COLLAPSED_PROJECT_IDS_CONFIG_KEY, undefined));
}

export function saveCollapsedSidebarProjectIds(projectIds: readonly string[]): void {
  setConfigKey(SIDEBAR_COLLAPSED_PROJECT_IDS_CONFIG_KEY, projectIds.length > 0 ? [...projectIds] : undefined);
}

export function loadSavedStarredProjectIds(): string[] {
  return parseStarredIds(getConfigKey<unknown>(STARRED_PROJECT_IDS_CONFIG_KEY, undefined));
}

export function saveStarredProjectIds(projectIds: readonly string[]): void {
  setConfigKey(STARRED_PROJECT_IDS_CONFIG_KEY, projectIds.length > 0 ? [...projectIds] : undefined);
}

export function loadSavedStarredViewIds(): string[] {
  return parseStarredIds(getConfigKey<unknown>(STARRED_VIEW_IDS_CONFIG_KEY, undefined));
}

export function saveStarredViewIds(viewIds: readonly string[]): void {
  setConfigKey(STARRED_VIEW_IDS_CONFIG_KEY, viewIds.length > 0 ? [...viewIds] : undefined);
}

/** Whether the project eye button was left on, listing hidden and archived projects in the sidebar and pickers. */
export function loadSavedShowInactiveProjects(): boolean {
  return getConfigKey<unknown>(SHOW_INACTIVE_PROJECTS_CONFIG_KEY, undefined) === true;
}

export function saveShowInactiveProjects(show: boolean): void {
  setConfigKey(SHOW_INACTIVE_PROJECTS_CONFIG_KEY, show || undefined);
}
