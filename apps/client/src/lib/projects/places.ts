import type { ProjectViewId } from "./types";

/** Task layouts reachable from the Tasks place, in toolbar order. */
export const PROJECT_TASK_LAYOUT_IDS = ["list", "kanban", "calendar", "gantt"] as const satisfies readonly ProjectViewId[];
export type ProjectTaskLayoutId = (typeof PROJECT_TASK_LAYOUT_IDS)[number];

export const DEFAULT_PROJECT_TASK_LAYOUT_ID: ProjectTaskLayoutId = "list";

/** Places in the Projects sidebar, in sidebar order. */
export const PROJECT_PLACES = ["overview", "tasks"] as const;
export type ProjectPlace = (typeof PROJECT_PLACES)[number];

export function isProjectTaskLayoutId(value: unknown): value is ProjectTaskLayoutId {
  return typeof value === "string" && PROJECT_TASK_LAYOUT_IDS.includes(value as ProjectTaskLayoutId);
}

/** Returns the sidebar place that shows a project view: the dashboard is the overview, every other view is a task layout. */
export function projectPlaceForView(view: ProjectViewId): ProjectPlace {
  return view === "dashboard" ? "overview" : "tasks";
}

/** Returns the view a place opens, reopening the last task layout for Tasks. */
export function projectViewForPlace(place: ProjectPlace, lastTaskLayout: ProjectTaskLayoutId): ProjectViewId {
  return place === "overview" ? "dashboard" : lastTaskLayout;
}
