/** A viewport rectangle in CSS pixels. */
export interface NotesNavigationRect {
  left: number;
  top: number;
  width: number;
  height: number;
}

/** A viewport point in CSS pixels. */
export interface NotesNavigationPoint {
  x: number;
  y: number;
}

/** The card that follows the pointer during a sidebar drag: its size and the point inside it that the pointer holds. */
export interface NotesNavigationDragCardGeometry {
  width: number;
  height: number;
  grab: NotesNavigationPoint;
}

function clamp(value: number, min: number, max: number): number {
  return Math.min(Math.max(value, min), max);
}

/**
 * Size the drag card from the row it lifts off, so it starts exactly over that row and the pointer keeps holding the
 * same spot. A width cap keeps a wide row from covering the editor; the grab point stays inside the capped card.
 */
export function notesNavigationDragCard(
  source: NotesNavigationRect,
  pointer: NotesNavigationPoint,
  maxWidth: number,
): NotesNavigationDragCardGeometry {
  const width = Math.max(0, Math.min(source.width, maxWidth));
  const height = Math.max(0, source.height);
  return {
    width,
    height,
    grab: {
      x: clamp(pointer.x - source.left, 0, width),
      y: clamp(pointer.y - source.top, 0, height),
    },
  };
}

/** Return the card's top-left corner for a pointer position. */
export function notesNavigationDragCardOrigin(
  card: NotesNavigationDragCardGeometry,
  pointer: NotesNavigationPoint,
): NotesNavigationPoint {
  return { x: pointer.x - card.grab.x, y: pointer.y - card.grab.y };
}

/**
 * Return the last index of the subtree that starts at `startIndex` in a flattened tree, where each later row with a
 * greater depth is nested under it.
 */
export function notesNavigationSubtreeEnd(depths: readonly number[], startIndex: number): number {
  const depth = depths[startIndex];
  if (depth === undefined) return startIndex;
  let endIndex = startIndex;
  while (endIndex + 1 < depths.length && (depths[endIndex + 1] ?? 0) > depth) endIndex += 1;
  return endIndex;
}

/** `lifted` follows the pointer, `settling` glides into a row, and `vanishing` fades where it was released. */
export type NotesNavigationDragCardPhase = "lifted" | "settling" | "vanishing";
