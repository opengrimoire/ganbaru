/** Where a dragged view lands relative to the view under the pointer. */
export type NotesDatabaseViewDropPlacement = "before" | "after";

/**
 * Moves one view next to another in a database's saved view order.
 *
 * @param viewIds Every saved view id in its current order.
 * @param movedId View being moved.
 * @param targetId View it is dropped on.
 * @param placement Whether the moved view lands before or after the target.
 * @returns The new order, or null when either view is missing or the order would not change.
 */
export function moveNotesDatabaseView(
  viewIds: readonly string[],
  movedId: string,
  targetId: string,
  placement: NotesDatabaseViewDropPlacement,
): string[] | null {
  if (movedId === targetId || !viewIds.includes(movedId) || !viewIds.includes(targetId)) return null;
  const remaining = viewIds.filter((id) => id !== movedId);
  const targetIndex = remaining.indexOf(targetId);
  const next = [...remaining];
  next.splice(placement === "before" ? targetIndex : targetIndex + 1, 0, movedId);
  return next.every((id, index) => id === viewIds[index]) ? null : next;
}

/**
 * Moves one view a single step among the views a person can see, such as from a keyboard reorder, keeping views
 * hidden from the list in their saved places.
 *
 * @param viewIds Every saved view id in its current order.
 * @param visibleIds Views shown in the list, in saved order.
 * @param movedId View being moved.
 * @param direction -1 to move toward the start, 1 toward the end.
 * @returns The new order, or null when the view is already at that end or is not listed.
 */
export function stepNotesDatabaseView(
  viewIds: readonly string[],
  visibleIds: readonly string[],
  movedId: string,
  direction: -1 | 1,
): string[] | null {
  const index = visibleIds.indexOf(movedId);
  const neighbor = index < 0 ? undefined : visibleIds[index + direction];
  if (neighbor === undefined) return null;
  return moveNotesDatabaseView(viewIds, movedId, neighbor, direction < 0 ? "before" : "after");
}

/**
 * Chooses the drop placement from the pointer position over a view: the leading half places before it.
 *
 * @param rect Bounds of the view under the pointer.
 * @param point Pointer position in the same coordinate space.
 * @param axis "x" for a horizontal toolbar, "y" for a vertical list.
 */
export function notesDatabaseViewDropPlacement(
  rect: { left: number; top: number; width: number; height: number },
  point: { x: number; y: number },
  axis: "x" | "y",
): NotesDatabaseViewDropPlacement {
  const leading = axis === "x" ? point.x < rect.left + rect.width / 2 : point.y < rect.top + rect.height / 2;
  return leading ? "before" : "after";
}

/**
 * Arranges views by a requested order, such as one still being saved, keeping views the order does not name after
 * the named ones in their current sequence.
 *
 * @param views Views in their current order.
 * @param viewIds Requested order of view ids.
 * @returns A new array in the requested order.
 */
export function orderNotesDatabaseViews<View extends { id: string }>(views: readonly View[], viewIds: readonly string[]): View[] {
  const rank = new Map(viewIds.map((id, index) => [id, index]));
  return views
    .map((view, index) => ({ view, rank: rank.get(view.id) ?? viewIds.length + index }))
    .sort((left, right) => left.rank - right.rank)
    .map(({ view }) => view);
}
