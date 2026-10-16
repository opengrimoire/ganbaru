/** Pointer travel in CSS pixels before a pressed view row starts dragging, so a click stays a click. */
export const PROJECT_VIEW_DRAG_THRESHOLD = 4;

/** How long the lifted card takes to settle into its slot, and how long neighboring rows take to slide aside. */
export const PROJECT_VIEW_DRAG_SETTLE_MS = 180;

/** A viewport rectangle in CSS pixels. */
export interface ProjectViewDragFrame {
  left: number;
  top: number;
  width: number;
  height: number;
}

/** `lifted` follows the pointer and `settling` glides into the slot the view lands in. */
export type ProjectViewDragCardPhase = "lifted" | "settling";

/** Inclusive index range of rows a dragged view may land among. */
export interface ProjectViewDragRange {
  start: number;
  end: number;
}

/**
 * Return the run of rows sharing the dragged row's starred state. Starred views list first, so the run is contiguous and
 * a drag never carries a view across the starred boundary.
 */
export function projectViewDragRange(starred: readonly boolean[], index: number): ProjectViewDragRange {
  const state = starred[index];
  if (state === undefined) return { start: index, end: index };
  let start = index;
  let end = index;
  while (start > 0 && starred[start - 1] === state) start -= 1;
  while (end + 1 < starred.length && starred[end + 1] === state) end += 1;
  return { start, end };
}

/**
 * Return the slot the dragged card's center falls in, measured against the slots as laid out when the drag started,
 * clamped to `range`. Each slot reaches down to the next slot's top, so a center past the last slot keeps the last one.
 */
export function projectViewDragTarget(slotTops: readonly number[], range: ProjectViewDragRange, centerY: number): number {
  for (let index = range.start; index < range.end; index += 1) {
    const next = slotTops[index + 1];
    if (next !== undefined && centerY < next) return index;
  }
  return range.end;
}

/**
 * Return how far the row at `index` slides while the view from `source` hovers over `target`: rows between the two
 * step into their neighbor's slot to open a gap at `target`, and every other row stays put.
 */
export function projectViewDragShift(slotTops: readonly number[], index: number, source: number, target: number): number {
  const top = slotTops[index];
  if (top === undefined || index === source) return 0;
  const neighbor = source < target && index > source && index <= target
    ? slotTops[index - 1]
    : target < source && index >= target && index < source
      ? slotTops[index + 1]
      : undefined;
  return neighbor === undefined ? 0 : neighbor - top;
}

/** Return where a drop at `target` places the dragged view relative to the row now in that slot, or null for no move. */
export function projectViewDragPlacement(
  source: number,
  target: number,
): "before" | "after" | null {
  if (target === source) return null;
  return target > source ? "after" : "before";
}
