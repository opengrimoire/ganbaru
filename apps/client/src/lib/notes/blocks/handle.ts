import { FLOATING_WIDTH } from "$lib/components/ui/floating-width";
import { notesBlockInsertMenuStyle } from "./insertion";
import type { NotesBlockInsertMenuPlacementInput } from "./insertion";

export type NotesBlockHandleAction =
  | "turn_into"
  | "color"
  | "copy_link"
  | "duplicate"
  | "comment"
  | "move_up"
  | "move_down"
  | "move_to_page"
  | "delete";

export interface NotesBlockHandleMenuState {
  menuOpen: boolean;
  moveMenuOpen: boolean;
}

export const CLOSED_NOTES_BLOCK_HANDLE_MENUS: NotesBlockHandleMenuState = {
  menuOpen: false,
  moveMenuOpen: false,
};

/** Open block actions only for the current row's non-editable surface. */
export function notesBlockContextMenuPoint(event: MouseEvent): { x: number; y: number } | null {
  if (event.defaultPrevented || !(event.currentTarget instanceof HTMLElement)) return null;
  const target = event.target;
  if (target instanceof Element && (
    target.closest("[data-notes-selectable-block-id]") !== event.currentTarget
    || target.closest('input, textarea, [contenteditable="true"], [data-app-floating-surface]')
  )) return null;
  event.preventDefault();
  event.stopPropagation();
  const rect = event.currentTarget.getBoundingClientRect();
  const keyboardRequest = event.clientX === 0 && event.clientY === 0;
  return {
    x: keyboardRequest ? rect.left : event.clientX,
    y: keyboardRequest ? rect.bottom : event.clientY,
  };
}

/**
 * Plans the nested move menu from a trigger toggle.
 */
export function notesBlockHandleMenuStateAfterMoveToggle(
  current: NotesBlockHandleMenuState,
): NotesBlockHandleMenuState {
  return { menuOpen: true, moveMenuOpen: !current.moveMenuOpen };
}

/**
 * Plans visible block-handle menus after a menu action runs.
 */
export function notesBlockHandleMenuStateAfterAction(
  current: NotesBlockHandleMenuState,
  action: NotesBlockHandleAction,
): NotesBlockHandleMenuState {
  switch (action) {
    case "color":
    case "copy_link":
      return {
        menuOpen: current.menuOpen,
        moveMenuOpen: false,
      };
    case "move_to_page":
    case "turn_into":
    case "duplicate":
    case "comment":
    case "move_up":
    case "move_down":
    case "delete":
      return CLOSED_NOTES_BLOCK_HANDLE_MENUS;
  }
}

/**
 * Places the block action menu inside the viewport.
 */
export function notesBlockHandleActionMenuStyle(
  input: Omit<NotesBlockInsertMenuPlacementInput, "preferredWidth">,
): string {
  return notesBlockInsertMenuStyle({
    ...input,
    preferredWidth: FLOATING_WIDTH.sm,
  });
}
