import type { View } from "$lib/navigation";
import type { ProjectViewId } from "./types";

/** Task layouts reachable from the Tasks place, in toolbar order. */
export const PROJECT_TASK_LAYOUT_IDS = ["list", "kanban", "calendar", "gantt"] as const satisfies readonly ProjectViewId[];
export type ProjectTaskLayoutId = (typeof PROJECT_TASK_LAYOUT_IDS)[number];

export const DEFAULT_PROJECT_TASK_LAYOUT_ID: ProjectTaskLayoutId = "list";

/** Places that open a project view inside the Projects workspace. */
export const PROJECT_VIEW_PLACES = ["overview", "tasks"] as const;
export type ProjectViewPlace = (typeof PROJECT_VIEW_PLACES)[number];

/** Places that host another feature's workspace scoped to the selected project. */
export const PROJECT_HOSTED_PLACES = ["notes"] as const;
export type ProjectHostedPlace = (typeof PROJECT_HOSTED_PLACES)[number];

/** Places in the Projects sidebar, in sidebar order. */
export const PROJECT_PLACES = [...PROJECT_VIEW_PLACES, ...PROJECT_HOSTED_PLACES] as const;
export type ProjectPlace = (typeof PROJECT_PLACES)[number];

export function isProjectTaskLayoutId(value: unknown): value is ProjectTaskLayoutId {
  return typeof value === "string" && PROJECT_TASK_LAYOUT_IDS.includes(value as ProjectTaskLayoutId);
}

export function isProjectHostedPlace(value: unknown): value is ProjectHostedPlace {
  return typeof value === "string" && PROJECT_HOSTED_PLACES.includes(value as ProjectHostedPlace);
}

/** Returns the sidebar place that shows a project view: the dashboard is the overview, every other view is a task layout. */
export function projectPlaceForView(view: ProjectViewId): ProjectViewPlace {
  return view === "dashboard" ? "overview" : "tasks";
}

/** Returns the open place: a hosted workspace wins over the project view underneath it. */
export function resolveProjectPlace(hostedPlace: ProjectHostedPlace | null, view: ProjectViewId): ProjectPlace {
  return hostedPlace ?? projectPlaceForView(view);
}

/** Returns the view a place opens, reopening the last task layout for Tasks. */
export function projectViewForPlace(place: ProjectViewPlace, lastTaskLayout: ProjectTaskLayoutId): ProjectViewId {
  return place === "overview" ? "dashboard" : lastTaskLayout;
}

/**
 * Returns the hosted places a window can open: a feature is hosted only where its own tab is visible, so a detached
 * Notes or Chat window never shares its workspace with a Projects tab elsewhere.
 */
export function projectHostablePlaces(visibleViews: readonly View[]): ProjectHostedPlace[] {
  return PROJECT_HOSTED_PLACES.filter((place) => visibleViews.includes(place));
}

/** Returns the sidebar places in sidebar order, keeping only the hosted places the window can open. */
export function projectSidebarPlaces(hostablePlaces: readonly ProjectHostedPlace[]): ProjectPlace[] {
  return PROJECT_PLACES.filter((place) => !isProjectHostedPlace(place) || hostablePlaces.includes(place));
}
