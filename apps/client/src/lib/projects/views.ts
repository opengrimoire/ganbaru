import { compareProjectOrder } from "./order";
import {
  PROJECT_HOSTED_VIEW_TYPES,
  PROJECT_VIEW_IDS,
  PROJECT_VIEW_TYPES,
  type ProjectHostedViewType,
  type ProjectView,
  type ProjectViewId,
  type ProjectViewType,
} from "./types";

export function isProjectViewType(value: unknown): value is ProjectViewType {
  return typeof value === "string" && PROJECT_VIEW_TYPES.includes(value as ProjectViewType);
}

/** Task views render the project's tasks and may appear more than once per project. */
export function isProjectTaskViewType(value: unknown): value is ProjectViewId {
  return typeof value === "string" && PROJECT_VIEW_IDS.includes(value as ProjectViewId);
}

/** Hosted views open another feature's workspace scoped to the project and appear at most once per project. */
export function isProjectHostedViewType(value: unknown): value is ProjectHostedViewType {
  return typeof value === "string" && PROJECT_HOSTED_VIEW_TYPES.includes(value as ProjectHostedViewType);
}

function compareViews(first: ProjectView, second: ProjectView): number {
  return compareProjectOrder(first, second);
}

/**
 * Returns one project's views in sidebar order. Hosted views are kept only when the window can host their feature,
 * so a window without a Notes tab never lists the project's Notes view.
 */
export function projectViewsFor(
  views: readonly ProjectView[],
  projectId: string | null | undefined,
  hostableTypes: readonly ProjectHostedViewType[],
): ProjectView[] {
  if (!projectId) return [];
  return views
    .filter((view) => view.projectId === projectId
      && (!isProjectHostedViewType(view.viewType) || hostableTypes.includes(view.viewType)))
    .sort(compareViews);
}

/** Opens the remembered view while it still exists, else the first view; a project without views has none. */
export function resolveActiveProjectView(
  projectViews: readonly ProjectView[],
  rememberedId: string | null | undefined,
): ProjectView | undefined {
  return projectViews.find((view) => view.id === rememberedId) ?? projectViews[0];
}

/** Returns the view of `viewType` to open: the active one when it already has that type, else the first match. */
export function projectViewForType(
  projectViews: readonly ProjectView[],
  active: ProjectView | undefined,
  viewType: ProjectViewType,
): ProjectView | undefined {
  if (active?.viewType === viewType) return active;
  return projectViews.find((view) => view.viewType === viewType);
}

/** Task views can repeat; hosted views can be added only while the project has none of that type. */
export function canAddProjectViewType(projectViews: readonly ProjectView[], viewType: ProjectViewType): boolean {
  return isProjectTaskViewType(viewType) || !projectViews.some((view) => view.viewType === viewType);
}

/** One entry of the add view menu; an unavailable type is a hosted view the project already has. */
export interface ProjectViewAddOption {
  viewType: ProjectViewType;
  available: boolean;
}

/**
 * Lists the view types the add view menu offers for one project, in menu order. Hosted types this window cannot host
 * are left out, since a view added there would never show in this window's sidebar.
 * @param projectViews Every view of the project, including hosted views this window cannot open.
 */
export function projectViewAddOptions(
  projectViews: readonly ProjectView[],
  hostableTypes: readonly ProjectHostedViewType[],
): ProjectViewAddOption[] {
  return PROJECT_VIEW_TYPES
    .filter((viewType) => !isProjectHostedViewType(viewType) || hostableTypes.includes(viewType))
    .map((viewType) => ({ viewType, available: canAddProjectViewType(projectViews, viewType) }));
}

/**
 * Returns the complete view order of a project after placing one view before or after another, or null when nothing
 * changes. The order lists every view of the project, including hosted views this window hides, because a reorder
 * must name each of them once.
 */
export function reorderedProjectViewIds(
  views: readonly ProjectView[],
  projectId: string,
  viewId: string,
  targetId: string,
  placement: "before" | "after",
): string[] | null {
  if (viewId === targetId) return null;
  const current = projectViewsFor(views, projectId, PROJECT_HOSTED_VIEW_TYPES).map((view) => view.id);
  if (!current.includes(viewId) || !current.includes(targetId)) return null;
  const remaining = current.filter((id) => id !== viewId);
  const targetIndex = remaining.indexOf(targetId) + (placement === "after" ? 1 : 0);
  const next = [...remaining.slice(0, targetIndex), viewId, ...remaining.slice(targetIndex)];
  return next.every((id, index) => id === current[index]) ? null : next;
}

/** Returns the full view order after moving a view one step past its visible neighbor, or null at either end. */
export function projectViewIdsMovedByStep(
  views: readonly ProjectView[],
  visibleViews: readonly ProjectView[],
  viewId: string,
  step: -1 | 1,
): string[] | null {
  const index = visibleViews.findIndex((view) => view.id === viewId);
  const neighbor = index < 0 ? undefined : visibleViews[index + step];
  if (!neighbor) return null;
  return reorderedProjectViewIds(views, neighbor.projectId, viewId, neighbor.id, step < 0 ? "before" : "after");
}
