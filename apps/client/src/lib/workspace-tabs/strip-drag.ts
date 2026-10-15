import type { WorkspaceTabPlacement } from "./model";

/** Horizontal extent of a shown tab, in layout coordinates that ignore running slide animations. */
export interface TabStripSlot {
  id: string;
  left: number;
  width: number;
}

/** Vertical extent of the bar that holds the tab strip. */
export interface TabStripBand {
  top: number;
  bottom: number;
}

/**
 * Slot a dragged tab lands in: the one whose center is nearest the dragged tab's center. Tabs share one width,
 * so the slots stay put while tabs trade places and the dragged tab swaps once it covers half of a neighbor.
 */
export function tabStripDropIndex(slots: readonly TabStripSlot[], center: number): number {
  let nearest = 0;
  let nearestDistance = Number.POSITIVE_INFINITY;
  slots.forEach((slot, index) => {
    const distance = Math.abs(slot.left + slot.width / 2 - center);
    if (distance < nearestDistance) {
      nearest = index;
      nearestDistance = distance;
    }
  });
  return nearest;
}

/** Move that puts `id` at `index` among `ids`, or null when it is already there or not among them. */
export function tabStripMoveTarget(
  ids: readonly string[],
  id: string,
  index: number,
): { id: string; placement: WorkspaceTabPlacement } | null {
  const current = ids.indexOf(id);
  const others = ids.filter((other) => other !== id);
  const last = others.at(-1);
  if (current < 0 || last === undefined) return null;
  const target = Math.min(Math.max(index, 0), others.length);
  if (target === current) return null;
  const before = others[target];
  return before === undefined ? { id: last, placement: "after" } : { id: before, placement: "before" };
}

/**
 * Whether a tab dragged to `y` has been pulled far enough off its bar to leave the window, as when a browser
 * tab is dragged down from its strip. `margin` is how far past the bar the pointer may stray while reordering.
 */
export function tabDragLeftBar(y: number, band: TabStripBand, margin: number): boolean {
  return y < band.top - margin || y > band.bottom + margin;
}

/**
 * Left edge, in strip coordinates, that a dragged tab takes under the pointer: the pointer minus where the tab
 * was grabbed, kept within the strip's tabs so the tab never slides past either end.
 */
export function tabStripVisualLeft(pointerX: number, grabX: number, width: number, stripRight: number): number {
  return Math.min(Math.max(pointerX - grabX, 0), Math.max(stripRight - width, 0));
}

/** Right edge of the shown tabs, or zero for an empty strip. */
export function tabStripRight(slots: readonly TabStripSlot[]): number {
  return slots.reduce((right, slot) => Math.max(right, slot.left + slot.width), 0);
}

/** Gap a tab entering the strip at `x` takes: the number of shown tabs whose center lies before it. */
export function tabStripInsertIndex(slots: readonly TabStripSlot[], x: number): number {
  return slots.filter((slot) => slot.left + slot.width / 2 < x).length;
}

/**
 * Neighbor that puts a tab arriving from elsewhere at `index` among `ids`: before the tab now there, after the
 * last one past the end, or null for an empty strip.
 */
export function tabStripInsertTarget(
  ids: readonly string[],
  index: number,
): { id: string; placement: WorkspaceTabPlacement } | null {
  const at = Math.min(Math.max(index, 0), ids.length);
  const before = ids[at];
  if (before !== undefined) return { id: before, placement: "before" };
  const last = ids.at(-1);
  return last === undefined ? null : { id: last, placement: "after" };
}

/** Position in `ids` that `target`, from {@link tabStripInsertTarget}, names, or the end when it names none. */
export function tabStripInsertPosition(
  ids: readonly string[],
  target: { id: string; placement: WorkspaceTabPlacement } | null,
): number {
  const index = target ? ids.indexOf(target.id) : -1;
  if (index < 0) return ids.length;
  return target?.placement === "after" ? index + 1 : index;
}
