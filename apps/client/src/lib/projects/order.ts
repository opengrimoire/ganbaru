import { compareOrderKeyThenId } from "$lib/utils/order-key";
import type { MoveDirection, ProjectOrderPlacement } from "./types";

/** A row of one ordered Projects group. */
export interface ProjectOrderedRow {
  id: string;
  orderKey: string;
}

/** Orders rows like native Projects lists: by order key, then by id. */
export function compareProjectOrder(left: ProjectOrderedRow, right: ProjectOrderedRow): number {
  return compareOrderKeyThenId(left.orderKey, left.id, right.orderKey, right.id);
}

/** Orders tasks within a section or parent by their section order key, then by id. */
export function compareTaskSectionOrder(
  left: { id: string; sectionOrderKey: string },
  right: { id: string; sectionOrderKey: string },
): number {
  return compareOrderKeyThenId(left.sectionOrderKey, left.id, right.sectionOrderKey, right.id);
}

/** Orders tasks within a status by their status order key, then by id. */
export function compareTaskStatusOrder(
  left: { id: string; statusOrderKey: string },
  right: { id: string; statusOrderKey: string },
): number {
  return compareOrderKeyThenId(left.statusOrderKey, left.id, right.statusOrderKey, right.id);
}

/** Position of each row in its native order, for consumers that rank by number. */
export function projectOrderRanks(rows: readonly ProjectOrderedRow[]): Map<string, number> {
  return new Map(rows.toSorted(compareProjectOrder).map((row, index) => [row.id, index]));
}

/**
 * Local keys that keep `ids` in the given order until the native result replaces them. They only need to sort among
 * themselves, so equal-width base 36 indexes are enough.
 */
export function provisionalOrderKeys(ids: readonly string[]): Map<string, string> {
  const width = Math.max(1, (ids.length - 1).toString(36).length);
  return new Map(ids.map((id, index) => [id, index.toString(36).padStart(width, "0")]));
}

/** The placement that puts a row directly before or after the sibling `targetId`. */
export function placementBeside(targetId: string, position: "before" | "after"): ProjectOrderPlacement {
  return { kind: position, id: targetId };
}

/**
 * The placement that moves `id` one step in `direction` among `ordered`, or null at either end. Moving toward the
 * start goes before the previous sibling, and moving toward the end goes after the next one.
 */
export function adjacentPlacement(
  ordered: readonly { id: string }[],
  id: string,
  direction: MoveDirection,
): ProjectOrderPlacement | null {
  const index = ordered.findIndex((entry) => entry.id === id);
  const target = index < 0 ? undefined : ordered[index + direction];
  if (!target) return null;
  return placementBeside(target.id, direction < 0 ? "before" : "after");
}

/** Input for a drop onto a list shown in native order (`asc`) or reversed (`desc`). */
export interface ProjectDropInput {
  /** Rows in the order they appear on screen; may include the dragged row when it already belongs to this list. */
  visibleIds: readonly string[];
  draggedId: string;
  /** The row the pointer is over; dropping without one appends to the visible end. */
  overId?: string;
  position?: "before" | "after";
  direction: "asc" | "desc";
}

/**
 * The native placement for a drop, expressed against the visible neighbour, or null when the row would stay where it
 * is. A reversed list swaps before and after, because its visible end is the native start.
 */
export function dropPlacement(input: ProjectDropInput): ProjectOrderPlacement | null {
  const originalIndex = input.visibleIds.indexOf(input.draggedId);
  const candidates = input.visibleIds.filter((id) => id !== input.draggedId);
  const overIndex = input.overId === undefined ? -1 : candidates.indexOf(input.overId);
  const insertIndex = overIndex >= 0 ? overIndex + (input.position === "after" ? 1 : 0) : candidates.length;
  if (insertIndex === originalIndex) return null;
  const previous = candidates[insertIndex - 1];
  const next = candidates[insertIndex];
  const reversed = input.direction === "desc";
  if (previous !== undefined) return placementBeside(previous, reversed ? "before" : "after");
  if (next !== undefined) return placementBeside(next, reversed ? "after" : "before");
  return { kind: "end" };
}

/**
 * The placement that leaves `id` where `orderedIds` lists it: after its previous sibling, before its next one when it
 * is first, and null when it is the only id or missing.
 */
export function placementInOrder(orderedIds: readonly string[], id: string): ProjectOrderPlacement | null {
  const index = orderedIds.indexOf(id);
  if (index < 0) return null;
  const previous = orderedIds[index - 1];
  if (previous !== undefined) return placementBeside(previous, "after");
  const next = orderedIds[index + 1];
  return next === undefined ? null : placementBeside(next, "before");
}
