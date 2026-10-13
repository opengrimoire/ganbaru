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
  return first.sortOrder - second.sortOrder || first.id.localeCompare(second.id);
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
