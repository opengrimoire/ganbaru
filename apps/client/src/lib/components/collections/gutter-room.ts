/** Horizontal space a collection can hang its leading gutter into, left of where its content starts. */
export interface CollectionGutterRoom {
  /** Pixels between the content start and the clipping edge, never negative. */
  room: number;
  /** The nearest ancestor that clips horizontal overflow, or null when only the viewport clips. */
  clip: HTMLElement | null;
}

/**
 * Measure how far a collection's selection and expand gutter can sit left of its content before an ancestor clips it,
 * so the table content can stay aligned with the surrounding body text.
 *
 * @param element The element whose left edge is where the table content should start.
 * @returns The available room and the ancestor that bounds it.
 */
export function collectionGutterRoom(element: HTMLElement): CollectionGutterRoom {
  const left = element.getBoundingClientRect().left;
  for (let ancestor = element.parentElement; ancestor; ancestor = ancestor.parentElement) {
    if (getComputedStyle(ancestor).overflowX === "visible") continue;
    const clipLeft = ancestor.getBoundingClientRect().left + ancestor.clientLeft;
    return { room: Math.max(0, left - clipLeft), clip: ancestor };
  }
  return { room: Math.max(0, left), clip: null };
}
