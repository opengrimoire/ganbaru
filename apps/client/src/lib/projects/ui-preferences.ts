import { getConfigKey, setConfigKey } from "$lib/vault/config";
import {
  DEFAULT_PROJECT_TASK_LAYOUT_ID,
  isProjectHostedPlace,
  isProjectTaskLayoutId,
  type ProjectHostedPlace,
  type ProjectTaskLayoutId,
} from "./places";
import { PROJECT_VIEW_IDS, type ProjectViewId } from "./types";

export const DEFAULT_PROJECT_VIEW_ID: ProjectViewId = "list";

const ACTIVE_PROJECT_ID_CONFIG_KEY = "projects.activeProjectId";
const ACTIVE_VIEW_CONFIG_KEY = "projects.activeView";
const TASK_LAYOUT_CONFIG_KEY = "projects.taskLayout";
const SIDEBAR_COLLAPSED_CONFIG_KEY = "projects.sidebarCollapsed";
const HOSTED_PLACE_CONFIG_KEY = "projects.hostedPlace";

export function isProjectViewId(value: unknown): value is ProjectViewId {
  return typeof value === "string" && PROJECT_VIEW_IDS.includes(value as ProjectViewId);
}

export function parseStoredProjectViewId(value: unknown): ProjectViewId {
  return isProjectViewId(value) ? value : DEFAULT_PROJECT_VIEW_ID;
}

export function parseStoredProjectTaskLayoutId(value: unknown): ProjectTaskLayoutId {
  return isProjectTaskLayoutId(value) ? value : DEFAULT_PROJECT_TASK_LAYOUT_ID;
}

export function parseStoredProjectId(value: unknown): string | null {
  if (typeof value !== "string") return null;
  const trimmed = value.trim();
  return trimmed.length > 0 ? trimmed : null;
}

export function loadSavedActiveProjectId(): string | null {
  return parseStoredProjectId(getConfigKey<unknown>(ACTIVE_PROJECT_ID_CONFIG_KEY, undefined));
}

export function saveActiveProjectId(projectId: string | null): void {
  setConfigKey(ACTIVE_PROJECT_ID_CONFIG_KEY, projectId ?? undefined);
}

export function loadSavedProjectViewId(): ProjectViewId {
  return parseStoredProjectViewId(getConfigKey<unknown>(ACTIVE_VIEW_CONFIG_KEY, undefined));
}

export function saveProjectViewId(viewId: ProjectViewId): void {
  setConfigKey(ACTIVE_VIEW_CONFIG_KEY, viewId);
}

/** Loads the task layout the Tasks place reopens after the overview. */
export function loadSavedProjectTaskLayoutId(): ProjectTaskLayoutId {
  return parseStoredProjectTaskLayoutId(getConfigKey<unknown>(TASK_LAYOUT_CONFIG_KEY, undefined));
}

export function saveProjectTaskLayoutId(layoutId: ProjectTaskLayoutId): void {
  setConfigKey(TASK_LAYOUT_CONFIG_KEY, layoutId);
}

export function loadSavedProjectSidebarCollapsed(): boolean {
  return getConfigKey<unknown>(SIDEBAR_COLLAPSED_CONFIG_KEY, undefined) === true;
}

export function saveProjectSidebarCollapsed(collapsed: boolean): void {
  setConfigKey(SIDEBAR_COLLAPSED_CONFIG_KEY, collapsed || undefined);
}

export function parseStoredProjectHostedPlace(value: unknown): ProjectHostedPlace | null {
  return isProjectHostedPlace(value) ? value : null;
}

/** Loads the hosted workspace the Projects tab reopens, or null for the project view. */
export function loadSavedProjectHostedPlace(): ProjectHostedPlace | null {
  return parseStoredProjectHostedPlace(getConfigKey<unknown>(HOSTED_PLACE_CONFIG_KEY, undefined));
}

export function saveProjectHostedPlace(place: ProjectHostedPlace | null): void {
  setConfigKey(HOSTED_PLACE_CONFIG_KEY, place ?? undefined);
}
