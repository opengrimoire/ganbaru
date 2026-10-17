import type { Translate } from "$lib/i18n/translator.svelte";
import type { Project } from "./types";

export const ROUTINE_GROUP_ID = "group-routine";

const BUILT_IN_ROUTINE_PROJECT_KEYS = {
  "project-routine-learning": "learning",
  "project-routine-reading": "reading",
  "project-routine-eat": "eating",
  "project-routine-exercise": "exercise",
  "project-routine-hygiene": "hygiene",
  "project-routine-commute": "commute",
  "project-routine-social": "social",
  "project-routine-chores": "chores",
  "project-routine-leisure": "leisure",
  "project-routine-meditate": "meditate",
  "project-routine-health": "health",
  "project-routine-sleep": "sleep",
} as const;

export type BuiltInRoutineProjectId = keyof typeof BUILT_IN_ROUTINE_PROJECT_KEYS;

export function isBuiltInRoutineProjectId(projectId: string): projectId is BuiltInRoutineProjectId {
  return Object.hasOwn(BUILT_IN_ROUTINE_PROJECT_KEYS, projectId);
}

export function systemProjectGroupName(
  groupId: string,
  storedName: string,
  t: Translate,
): string {
  return groupId === ROUTINE_GROUP_ID ? t("projects.defaults.routine") : storedName;
}

export function systemProjectName(
  projectId: string,
  storedName: string,
  t: Translate,
): string {
  if (!isBuiltInRoutineProjectId(projectId)) return storedName;
  const key = BUILT_IN_ROUTINE_PROJECT_KEYS[projectId];
  if (key === "learning") return t("projects.defaults.learning");
  if (key === "reading") return t("projects.defaults.reading");
  if (key === "eating") return t("projects.defaults.eating");
  if (key === "exercise") return t("projects.defaults.exercise");
  if (key === "hygiene") return t("projects.defaults.hygiene");
  if (key === "commute") return t("projects.defaults.commute");
  if (key === "social") return t("projects.defaults.social");
  if (key === "chores") return t("projects.defaults.chores");
  if (key === "leisure") return t("projects.defaults.leisure");
  if (key === "meditate") return t("projects.defaults.meditate");
  if (key === "health") return t("projects.defaults.health");
  return t("projects.defaults.sleep");
}

export function projectHasLockedSystemIdentity(project: Pick<Project, "id">): boolean {
  return isBuiltInRoutineProjectId(project.id);
}

export function effectiveProjectDefaultEventName(
  project: Pick<Project, "id" | "name" | "defaultEventName">,
): string | null {
  if (project.defaultEventName !== null) return project.defaultEventName;
  return isBuiltInRoutineProjectId(project.id) ? project.name : null;
}
