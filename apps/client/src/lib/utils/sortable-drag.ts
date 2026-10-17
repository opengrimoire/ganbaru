/** Pointer travel in CSS pixels before a pressed drag handle starts dragging, so a click or a tap stays one. */
export const SORTABLE_DRAG_THRESHOLD = 4;

/** How long a dropped item takes to settle into its slot, and how long its siblings take to slide aside. */
export const SORTABLE_DRAG_SETTLE_MS = 180;

/**
 * An item's place along its list's axis, in CSS pixels from the start of the list: its top and height in a vertical
 * list, its left edge and width in a horizontal one.
 */
export interface SortableSlot {
  start: number;
  size: number;
}

/** Return the space between two neighboring items, read from the slots so any list gap works. */
function sortableGap(slots: readonly SortableSlot[], index: number): number {
  const slot = slots[index];
  if (!slot) return 0;
  const next = slots[index + 1];
  if (next) return next.start - slot.start - slot.size;
  const previous = slots[index - 1];
  return previous ? slot.start - previous.start - previous.size : 0;
}

/** Return how far the dragged item may travel from its own slot, keeping it between the first and last slots. */
export function sortableDragBounds(slots: readonly SortableSlot[], source: number): { min: number; max: number } {
  const own = slots[source];
  const first = slots[0];
  const last = slots[slots.length - 1];
  if (!own || !first || !last) return { min: 0, max: 0 };
  return { min: first.start - own.start, max: last.start + last.size - own.size - own.start };
}

/**
 * Return the slot the item from `source` lands in after traveling `offset` pixels. Moving forward passes an item once
 * the dragged item's trailing edge crosses that item's center, and moving back passes it once the leading edge does;
 * both compare against the slots as laid out when the drag started.
 */
export function sortableDragTarget(slots: readonly SortableSlot[], source: number, offset: number): number {
  const own = slots[source];
  if (!own) return source;
  const start = own.start + offset;
  let target = source;
  if (offset > 0) {
    for (let index = source + 1; index < slots.length; index += 1) {
      const slot = slots[index];
      if (!slot || start + own.size <= slot.start + slot.size / 2) break;
      target = index;
    }
  } else if (offset < 0) {
    for (let index = source - 1; index >= 0; index -= 1) {
      const slot = slots[index];
      if (!slot || start >= slot.start + slot.size / 2) break;
      target = index;
    }
  }
  return target;
}

/**
 * Return how far the item at `index` slides while the item from `source` hovers over `target`: items between the two
 * move by the dragged item's size plus the list gap to open room at `target`, and every other item stays put.
 */
export function sortableDragShift(slots: readonly SortableSlot[], index: number, source: number, target: number): number {
  const own = slots[source];
  if (!own || index === source) return 0;
  const step = own.size + sortableGap(slots, source);
  if (source < target && index > source && index <= target) return -step;
  if (target < source && index >= target && index < source) return step;
  return 0;
}

/** Return the offset from its own slot at which the item from `source` rests once the gap opens at `target`. */
export function sortableDragLanding(slots: readonly SortableSlot[], source: number, target: number): number {
  const own = slots[source];
  const slot = slots[target];
  if (!own || !slot || target === source) return 0;
  return target > source ? slot.start + slot.size - own.size - own.start : slot.start - own.start;
}
