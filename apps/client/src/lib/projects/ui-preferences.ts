import { getConfigKey, setConfigKey } from "$lib/vault/config";
import { PROJECT_VIEW_IDS, type ProjectViewId } from "./types";

export const DEFAULT_PROJECT_VIEW_ID: ProjectViewId = "list";

const ACTIVE_PROJECT_ID_CONFIG_KEY = "projects.activeProjectId";
const ACTIVE_VIEW_CONFIG_KEY = "projects.activeView";

export function isProjectViewId(value: unknown): value is ProjectViewId {
  return typeof value === "string" && PROJECT_VIEW_IDS.includes(value as ProjectViewId);
}

export function parseStoredProjectViewId(value: unknown): ProjectViewId {
  return isProjectViewId(value) ? value : DEFAULT_PROJECT_VIEW_ID;
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
