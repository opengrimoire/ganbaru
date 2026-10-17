import { FLOATING_WIDTH } from "$lib/components/ui/floating-width";

export interface NotesRowContextMenuGeometryInput {
  clientX: number;
  clientY: number;
  viewportWidth: number;
  viewportHeight: number;
}

export interface NotesRowContextMenuGeometry {
  left: number;
  top: number;
  width: number;
  maxHeight: number;
}

const MENU_WIDTH_PX = FLOATING_WIDTH.sm;
const MENU_ESTIMATED_HEIGHT_PX = 330;
const MENU_EDGE_GAP_PX = 8;

/** Clamp a Notes row context menu to the visible viewport. */
export function notesRowContextMenuGeometry({
  clientX,
  clientY,
  viewportWidth,
  viewportHeight,
}: NotesRowContextMenuGeometryInput): NotesRowContextMenuGeometry {
  const availableWidth = Math.max(0, viewportWidth - MENU_EDGE_GAP_PX * 2);
  const availableHeight = Math.max(0, viewportHeight - MENU_EDGE_GAP_PX * 2);
  const width = Math.min(MENU_WIDTH_PX, availableWidth);
  const maxHeight = Math.min(MENU_ESTIMATED_HEIGHT_PX, availableHeight);
  const left = Math.min(
    Math.max(MENU_EDGE_GAP_PX, clientX),
    Math.max(MENU_EDGE_GAP_PX, viewportWidth - width - MENU_EDGE_GAP_PX),
  );
  const top = Math.min(
    Math.max(MENU_EDGE_GAP_PX, clientY),
    Math.max(MENU_EDGE_GAP_PX, viewportHeight - maxHeight - MENU_EDGE_GAP_PX),
  );
  return { left, top, width, maxHeight };
}

/** Serialize Notes row context menu geometry for a fixed-position surface. */
export function notesRowContextMenuStyle(geometry: NotesRowContextMenuGeometry): string {
  return [
    `left: ${geometry.left}px`,
    `top: ${geometry.top}px`,
    `width: ${geometry.width}px`,
    `max-height: ${geometry.maxHeight}px`,
  ].join("; ");
}
