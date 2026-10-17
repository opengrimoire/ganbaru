export type NotesEditorScrollAlignment = "nearest" | "center";

export interface NotesEditorScrollGeometry {
  scrollTop: number;
  maxScrollTop: number;
  viewportTop: number;
  viewportBottom: number;
  targetTop: number;
  targetBottom: number;
  padding: number;
  alignment: NotesEditorScrollAlignment;
  preventScroll?: boolean;
}

function clampScrollTop(scrollTop: number, maxScrollTop: number): number {
  return Math.min(Math.max(scrollTop, 0), Math.max(maxScrollTop, 0));
}

/**
 * Calculate the Notes editor viewport offset needed to reveal a focused block.
 */
export function notesEditorScrollTopForTarget(
  geometry: NotesEditorScrollGeometry,
): number {
  if (geometry.preventScroll) return geometry.scrollTop;
  const {
    scrollTop,
    maxScrollTop,
    viewportTop,
    viewportBottom,
    targetTop,
    targetBottom,
    padding,
    alignment,
  } = geometry;
  if (alignment === "center") {
    const viewportCenter = (viewportTop + viewportBottom) / 2;
    const targetCenter = (targetTop + targetBottom) / 2;
    return clampScrollTop(scrollTop + targetCenter - viewportCenter, maxScrollTop);
  }

  const visibleTop = viewportTop + padding;
  const visibleBottom = viewportBottom - padding;
  const availableHeight = Math.max(visibleBottom - visibleTop, 0);
  const targetHeight = Math.max(targetBottom - targetTop, 0);
  if (targetHeight > availableHeight || targetTop < visibleTop) {
    return clampScrollTop(scrollTop + targetTop - visibleTop, maxScrollTop);
  }
  if (targetBottom > visibleBottom) {
    return clampScrollTop(scrollTop + targetBottom - visibleBottom, maxScrollTop);
  }
  return clampScrollTop(scrollTop, maxScrollTop);
}
