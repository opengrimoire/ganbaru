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
  const fitsSideBySide = width - gap >= MIN_SPLIT_PANE_WIDTH * 2;
  const fitsStacked = height - gap >= MIN_SPLIT_PANE_HEIGHT * 2;
  const preferred = orientation === "side-by-side" ? fitsSideBySide : fitsStacked;
  const other = orientation === "side-by-side" ? fitsStacked : fitsSideBySide;
  if (preferred) return orientation;
  if (other) return orientation === "side-by-side" ? "stacked" : "side-by-side";
  return "single";
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
