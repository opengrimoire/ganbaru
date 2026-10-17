/** One header cell measured when a column drag starts, in pixels relative to its header row. */
export interface CollectionColumnSlot {
  id: string;
  left: number;
  width: number;
  /** Whether the column can move; leading fixed columns, such as a required title, keep their place. */
  movable: boolean;
}

/** Distance from a scroll container edge where a column drag scrolls the table, in pixels. */
export const COLLECTION_COLUMN_AUTO_SCROLL_ZONE = 48;
/** Scroll distance per animation frame with the pointer at the container edge, in pixels. */
export const COLLECTION_COLUMN_AUTO_SCROLL_MAX_STEP = 12;

/**
 * Index among the header's columns where the dragged column lands when its left edge is at `left`.
 * The column passes a neighbor once its leading edge crosses the neighbor's center, so wide and narrow columns both
 * move after about half the neighbor's width. It never lands before the leading fixed columns.
 *
 * @returns The target index, or -1 when the dragged column is not among the slots.
 */
export function collectionColumnDropIndex(slots: readonly CollectionColumnSlot[], draggedId: string, left: number): number {
  const draggedIndex = slots.findIndex((slot) => slot.id === draggedId);
  const dragged = slots[draggedIndex];
  if (!dragged) return -1;
  const right = left + dragged.width;
  let before = 0;
  slots.forEach((slot, index) => {
    if (index === draggedIndex) return;
    const center = slot.left + slot.width / 2;
    if (center < (index < draggedIndex ? left : right)) before += 1;
  });
  return Math.max(collectionColumnFixedPrefix(slots, draggedId), before);
}

/**
 * Horizontal offset of each column other than the dragged one while the dragged column previews `targetIndex`:
 * the columns it passed shift by its width toward its original place.
 */
export function collectionColumnOffsets(slots: readonly CollectionColumnSlot[], draggedId: string, targetIndex: number): ReadonlyMap<string, number> {
  const offsets = new Map<string, number>();
  const draggedIndex = slots.findIndex((slot) => slot.id === draggedId);
  const dragged = slots[draggedIndex];
  if (!dragged) return offsets;
  slots.forEach((slot, index) => {
    if (index === draggedIndex) return;
    if (draggedIndex < targetIndex && index > draggedIndex && index <= targetIndex) offsets.set(slot.id, -dragged.width);
    else if (targetIndex < draggedIndex && index >= targetIndex && index < draggedIndex) offsets.set(slot.id, dragged.width);
  });
  return offsets;
}

/**
 * Clamp the dragged column's left edge to the movable part of the header, so it slides along the columns it can
 * swap with instead of leaving the table.
 */
export function collectionColumnDragLeft(slots: readonly CollectionColumnSlot[], draggedId: string, left: number): number {
  const dragged = slots.find((slot) => slot.id === draggedId);
  const last = slots.at(-1);
  if (!dragged || !last) return left;
  const first = slots[collectionColumnFixedPrefix(slots, draggedId)] ?? dragged;
  const min = Math.min(first.left, dragged.left);
  const max = Math.max(min, last.left + last.width - dragged.width);
  return Math.min(max, Math.max(min, left));
}

/**
 * Scroll step for one animation frame while a dragged column's pointer is near a container edge, faster closer to
 * the edge. The left zone starts after `leftInset`, such as the width of frozen columns covering that edge.
 */
export function collectionColumnAutoScrollStep(pointerX: number, containerLeft: number, containerRight: number, leftInset = 0): number {
  const start = containerLeft + leftInset;
  const fromLeft = pointerX - start;
  const fromRight = containerRight - pointerX;
  if (fromLeft < COLLECTION_COLUMN_AUTO_SCROLL_ZONE && fromLeft <= fromRight) {
    return -Math.ceil(COLLECTION_COLUMN_AUTO_SCROLL_MAX_STEP * Math.min(1, 1 - fromLeft / COLLECTION_COLUMN_AUTO_SCROLL_ZONE));
  }
  if (fromRight < COLLECTION_COLUMN_AUTO_SCROLL_ZONE) {
    return Math.ceil(COLLECTION_COLUMN_AUTO_SCROLL_MAX_STEP * Math.min(1, 1 - fromRight / COLLECTION_COLUMN_AUTO_SCROLL_ZONE));
  }
  return 0;
}

/** Move one item to `targetIndex`, returning a new array; unknown items leave the order unchanged. */
export function collectionMoveItem<T>(items: readonly T[], item: T, targetIndex: number): T[] {
  const index = items.indexOf(item);
  if (index < 0) return [...items];
  const next = items.filter((_, candidate) => candidate !== index);
  next.splice(Math.max(0, Math.min(next.length, targetIndex)), 0, item);
  return next;
}

/**
 * Place a column at `targetIndex` among the visible columns while keeping hidden columns in the complete order.
 * The column lands just before the visible column that follows it, or just after the one that precedes it.
 */
export function collectionPlaceVisibleItem<T>(all: readonly T[], visible: readonly T[], item: T, targetIndex: number): T[] {
  if (!all.includes(item) || !visible.includes(item)) return [...all];
  const placed = collectionMoveItem(visible, item, targetIndex);
  const index = placed.indexOf(item);
  const rest = all.filter((candidate) => candidate !== item);
  const next = index + 1 < placed.length ? rest.indexOf(placed[index + 1] as T) : -1;
  const previous = index > 0 ? rest.indexOf(placed[index - 1] as T) : -1;
  const anchor = next >= 0 ? next : previous >= 0 ? previous + 1 : -1;
  if (anchor < 0) return [...all];
  rest.splice(anchor, 0, item);
  return rest;
}

/** Arrange items by a preferred ID order; items missing from it follow in their existing relative order. */
export function collectionOrderById<T extends { id: string }>(items: readonly T[], order: readonly string[]): T[] {
  const byId = new Map(items.map((item) => [item.id, item]));
  const ordered = order.flatMap((id) => {
    const item = byId.get(id);
    byId.delete(id);
    return item ? [item] : [];
  });
  return [...ordered, ...items.filter((item) => byId.has(item.id))];
}

/** Number of fixed columns that lead the header, ignoring the dragged column. */
function collectionColumnFixedPrefix(slots: readonly CollectionColumnSlot[], draggedId: string): number {
  let count = 0;
  for (const slot of slots) {
    if (slot.id === draggedId) continue;
    if (slot.movable) break;
    count += 1;
  }
  return count;
}
