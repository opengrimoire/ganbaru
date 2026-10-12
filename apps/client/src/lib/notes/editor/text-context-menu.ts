import { FLOATING_WIDTH } from "$lib/components/ui/floating-width";

export interface NotesTextMenuPoint {
  x: number;
  y: number;
}

export interface NotesTextMenuRect {
  top: number;
  right: number;
  bottom: number;
  left: number;
}

export interface NotesTextMenuViewport {
  width: number;
  height: number;
}

export interface NotesTextMenuPosition {
  left: number;
  top: number;
  width: number;
  maxHeight: number;
}

/** Keep an existing selection when a right click lands on one of its text fragments. */
export function notesTextContextMenuSelectionAtPoint(
  rects: readonly NotesTextMenuRect[],
  point: NotesTextMenuPoint,
): boolean {
  return rects.some((rect) =>
    point.x >= rect.left
    && point.x <= rect.right
    && point.y >= rect.top
    && point.y <= rect.bottom
  );
}

const MENU_MARGIN = 8;
const MENU_GAP = 4;
const MENU_WIDTH = FLOATING_WIDTH.sm;
export const NOTES_TEXT_CONTEXT_SUBMENU_WIDTH = FLOATING_WIDTH.sm;

function clamp(value: number, minimum: number, maximum: number): number {
  return Math.min(Math.max(value, minimum), Math.max(minimum, maximum));
}

/** Position the text context menu at a right click while keeping it in view. */
export function notesTextContextMenuPosition(
  point: NotesTextMenuPoint,
  viewport: NotesTextMenuViewport,
  menuHeight?: number,
): NotesTextMenuPosition {
  const width = Math.min(MENU_WIDTH, Math.max(0, viewport.width - MENU_MARGIN * 2));
  const maxHeight = Math.min(440, Math.max(0, viewport.height - MENU_MARGIN * 2));
  const height = Math.min(menuHeight ?? maxHeight, maxHeight);
  return {
    left: clamp(point.x, MENU_MARGIN, viewport.width - width - MENU_MARGIN),
    top: clamp(point.y, MENU_MARGIN, viewport.height - height - MENU_MARGIN),
    width,
    maxHeight,
  };
}

/** Place a context submenu beside its trigger, flipping at viewport edges. */
export function notesTextContextSubmenuPosition(
  anchor: NotesTextMenuRect,
  size: { width: number; height: number },
  viewport: NotesTextMenuViewport,
): NotesTextMenuPoint {
  const width = Math.min(size.width, Math.max(0, viewport.width - MENU_MARGIN * 2));
  const height = Math.min(size.height, Math.max(0, viewport.height - MENU_MARGIN * 2));
  const roomRight = viewport.width - anchor.right - MENU_GAP - MENU_MARGIN;
  const roomLeft = anchor.left - MENU_GAP - MENU_MARGIN;
  const preferredLeft = roomRight >= width || roomRight >= roomLeft
    ? anchor.right + MENU_GAP
    : anchor.left - width - MENU_GAP;
  return {
    x: clamp(preferredLeft, MENU_MARGIN, viewport.width - width - MENU_MARGIN),
    y: clamp(anchor.top, MENU_MARGIN, viewport.height - height - MENU_MARGIN),
  };
}
