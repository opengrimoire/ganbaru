import { DEFAULT_SPLIT_RATIO, clampSplitRatio, type WorkspaceSplitOrientation } from "./model";

/** Narrowest a pane of a side-by-side split view gets before the split view falls back to another layout. */
export const MIN_SPLIT_PANE_WIDTH = 360;
/** Shortest a pane of a stacked split view gets before the split view falls back to another layout. */
export const MIN_SPLIT_PANE_HEIGHT = 240;
/** Width of the divider between the panes, which is also its grab area. */
export const SPLIT_DIVIDER_SIZE = 6;
/** Share of the space one keyboard step on the divider moves it by. */
export const SPLIT_KEYBOARD_STEP = 0.05;

/** How a split view shows its tabs in the space it has: as chosen, the other way, or only the focused tab. */
export type WorkspaceSplitLayout = WorkspaceSplitOrientation | "single";

/** The size of the area a window's split view shows in. */
export interface WorkspaceSplitArea {
  width: number;
  height: number;
}

/** How a split arranged one way can change: the way its panes read as shown, and whether they fit the other way. */
export interface WorkspaceSplitArrangement {
  orientation: WorkspaceSplitOrientation;
  canTurn: boolean;
}

/** The orientation other than `orientation`. */
export function oppositeSplitOrientation(orientation: WorkspaceSplitOrientation): WorkspaceSplitOrientation {
  return orientation === "side-by-side" ? "stacked" : "side-by-side";
}

function splitOrientationFits(orientation: WorkspaceSplitOrientation, area: WorkspaceSplitArea, gap: number): boolean {
  return orientation === "side-by-side"
    ? area.width - gap >= MIN_SPLIT_PANE_WIDTH * 2
    : area.height - gap >= MIN_SPLIT_PANE_HEIGHT * 2;
}

/**
 * Lay out a split view in `width` by `height`, with `gap` between the panes. The chosen orientation wins when both
 * panes fit that way, then the other one, and when neither fits only the focused tab shows.
 */
export function workspaceSplitLayout(
  orientation: WorkspaceSplitOrientation,
  width: number,
  height: number,
  gap: number,
): WorkspaceSplitLayout {
  const area = { width, height };
  if (splitOrientationFits(orientation, area, gap)) return orientation;
  const other = oppositeSplitOrientation(orientation);
  return splitOrientationFits(other, area, gap) ? other : "single";
}

/**
 * What arranging a split chosen as `orientation` offers in `area`. The panes read as the layout shows them, or as
 * chosen when only one shows, so closing the left or the top view names what the user sees. Turning them is offered
 * only when they fit the other way, since otherwise nothing on screen would change.
 */
export function workspaceSplitArrangement(
  orientation: WorkspaceSplitOrientation,
  area: WorkspaceSplitArea,
  gap: number,
): WorkspaceSplitArrangement {
  const layout = workspaceSplitLayout(orientation, area.width, area.height, gap);
  const shown = layout === "single" ? orientation : layout;
  return { orientation: shown, canTurn: splitOrientationFits(oppositeSplitOrientation(shown), area, gap) };
}

/**
 * Keep the first pane's share of `extent` (the panes' total length along the split) within the stored bounds and
 * leave each pane at least `minPane` long. When both cannot hold, the panes share the space equally.
 */
export function fitSplitRatio(ratio: number, extent: number, minPane: number): number {
  const clamped = clampSplitRatio(ratio);
  if (extent <= 0) return clamped;
  const minShare = minPane / extent;
  if (minShare >= DEFAULT_SPLIT_RATIO) return DEFAULT_SPLIT_RATIO;
  return Math.min(1 - minShare, Math.max(minShare, clamped));
}

/** The pane length a split layout keeps at least, along its axis. */
export function minSplitPaneLength(layout: WorkspaceSplitOrientation): number {
  return layout === "side-by-side" ? MIN_SPLIT_PANE_WIDTH : MIN_SPLIT_PANE_HEIGHT;
}
