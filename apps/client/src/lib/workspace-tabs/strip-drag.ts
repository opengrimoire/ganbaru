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
