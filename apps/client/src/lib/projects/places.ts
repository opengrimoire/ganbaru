import type { View } from "$lib/navigation";
import { PROJECT_VIEW_IDS, type ProjectViewId } from "./types";

/** Places that host another feature's workspace scoped to the selected project. */
export const PROJECT_HOSTED_PLACES = ["notes", "chat"] as const;
export type ProjectHostedPlace = (typeof PROJECT_HOSTED_PLACES)[number];

/** Places in the Projects sidebar, in sidebar order: every project view, then the hosted workspaces. */
export const PROJECT_PLACES = [...PROJECT_VIEW_IDS, ...PROJECT_HOSTED_PLACES] as const;
export type ProjectPlace = (typeof PROJECT_PLACES)[number];

export function isProjectHostedPlace(value: unknown): value is ProjectHostedPlace {
  return typeof value === "string" && PROJECT_HOSTED_PLACES.includes(value as ProjectHostedPlace);
}

/** Returns the open place: a hosted workspace wins over the project view underneath it. */
export function resolveProjectPlace(hostedPlace: ProjectHostedPlace | null, view: ProjectViewId): ProjectPlace {
  return hostedPlace ?? view;
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
