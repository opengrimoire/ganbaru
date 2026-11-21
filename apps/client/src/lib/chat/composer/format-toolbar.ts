import { SHORTCUT_MODIFIER_TOKEN, isApplePlatform } from "$lib/keyboard-shortcuts";
import type { ViewportBox } from "$lib/utils/keep-in-viewport";
import { VIEWPORT_EDGE_MARGIN_PX } from "$lib/utils/keep-in-viewport";
import type { ChatComposerSelection } from "./rich-text";

/** How the user last changed the composer selection. */
export type ChatFormatToolbarInputKind = "pointer" | "touch" | "keyboard";

/** Everything that decides whether the formatting toolbar may show for a selection. */
export interface ChatFormatToolbarVisibilityInput {
  /** Whether focus is in the editor or inside the toolbar itself. */
  focused: boolean;
  disabled: boolean;
  selection: ChatComposerSelection | null;
  /** Whether an input method composition owns the editor. */
  composing: boolean;
  /** Whether an autocomplete menu is open over the composer. */
  menuOpen: boolean;
  /** The selection the user dismissed with Escape or a resize, until the selection changes. */
  dismissed: ChatComposerSelection | null;
  /** The Android shell, whose native selection menu stays in charge. */
  mobileShell: boolean;
  lastInput: ChatFormatToolbarInputKind;
}

/**
 * Returns whether the formatting toolbar shows: for a non-collapsed selection in a focused,
 * enabled desktop composer, outside composition and autocomplete, and not after a touch
 * selection, where the platform selection menu is the formatting surface.
 */
export function chatFormatToolbarVisible(input: ChatFormatToolbarVisibilityInput): boolean {
  const { selection } = input;
  if (!input.focused || input.disabled || !selection || selection.start === selection.end) return false;
  if (input.composing || input.menuOpen || input.mobileShell || input.lastInput === "touch") return false;
  return !sameSelection(input.dismissed, selection);
}

/** Whether two selections cover the same range. */
export function sameSelection(a: ChatComposerSelection | null, b: ChatComposerSelection | null): boolean {
  return a !== null && b !== null && a.start === b.start && a.end === b.end;
}

/** Inputs for placing the toolbar against the selected text. */
export interface ChatFormatToolbarPlacementInput {
  /** Bounding box of the selected text in viewport coordinates. */
  selection: ViewportBox;
  /** The area where selected text is visible: the editor's scroll box within the viewport. */
  clip: ViewportBox;
  /** The safe viewport the toolbar stays inside. */
  viewport: ViewportBox;
  width: number;
  height: number;
  /** Space between the selection and the toolbar. */
  gap?: number;
  margin?: number;
}

/** Where the toolbar goes, in viewport coordinates. */
export interface ChatFormatToolbarPlacement {
  left: number;
  top: number;
  side: "above" | "below";
}

/**
 * Places the toolbar centered over the visible part of the selection, above it when there is
 * room and below it otherwise, clamped to the viewport horizontally. Returns `null` when no
 * selected text is visible or the toolbar fits on neither side without covering the selection.
 */
export function chatFormatToolbarPlacement(input: ChatFormatToolbarPlacementInput): ChatFormatToolbarPlacement | null {
  const gap = input.gap ?? 6;
  const margin = input.margin ?? VIEWPORT_EDGE_MARGIN_PX;
  const visible = intersect(input.selection, input.clip);
  if (!visible) return null;
  const minTop = input.viewport.top + margin;
  const maxBottom = input.viewport.bottom - margin;
  const aboveTop = visible.top - gap - input.height;
  const belowTop = visible.bottom + gap;
  let side: ChatFormatToolbarPlacement["side"];
  let top: number;
  if (aboveTop >= minTop) {
    side = "above";
    top = aboveTop;
  } else if (belowTop + input.height <= maxBottom) {
    side = "below";
    top = belowTop;
  } else {
    return null;
  }
  const minLeft = input.viewport.left + margin;
  const maxLeft = Math.max(minLeft, input.viewport.right - margin - input.width);
  const centered = (visible.left + visible.right) / 2 - input.width / 2;
  return { left: Math.round(Math.min(Math.max(centered, minLeft), maxLeft)), top: Math.round(top), side };
}

function intersect(a: ViewportBox, b: ViewportBox): ViewportBox | null {
  const box = {
    left: Math.max(a.left, b.left),
    top: Math.max(a.top, b.top),
    right: Math.min(a.right, b.right),
    bottom: Math.min(a.bottom, b.bottom),
  };
  return box.left <= box.right && box.top < box.bottom ? box : null;
}

/**
 * Converts a `Mod + Shift + 7` shortcut to the `aria-keyshortcuts` form, such as
 * `Control+Shift+7`, with Meta for the shortcut modifier on Apple platforms.
 */
export function chatFormatAriaKeyShortcut(shortcut: string, platform?: string): string {
  return shortcut.split(" + ").map((part) => {
    if (part === SHORTCUT_MODIFIER_TOKEN) return isApplePlatform(platform) ? "Meta" : "Control";
    return part;
  }).join("+");
}

/**
 * Returns the toolbar button index an arrow, Home, or End key moves to, wrapping at the ends,
 * or `null` for other keys.
 */
export function chatFormatToolbarRovingIndex(key: string, current: number, count: number): number | null {
  if (count <= 0) return null;
  if (key === "ArrowRight" || key === "ArrowDown") return (current + 1) % count;
  if (key === "ArrowLeft" || key === "ArrowUp") return (current - 1 + count) % count;
  if (key === "Home") return 0;
  if (key === "End") return count - 1;
  return null;
}
