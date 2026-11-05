import { BUILD_PLATFORM_PROFILE } from "$lib/platform";

/**
 * Whether opening a surface, restoring a caret, or loading data may focus a text field. On a phone shell the soft
 * keyboard opens only when the user asks to type, so these focus moves are skipped there.
 */
export const OPENING_MAY_FOCUS_TEXT = BUILD_PLATFORM_PROFILE.shell !== "mobile";

/** Space kept between a revealed caret and the edge of its visible area, in CSS pixels. */
export const KEYBOARD_REVEAL_MARGIN_PX = 16;

/** Input types that open the soft keyboard. */
const TEXT_INPUT_TYPES = new Set(["", "text", "search", "email", "url", "tel", "password", "number"]);

export interface VerticalSpan {
  top: number;
  bottom: number;
}

/** Return whether focusing `element` opens the soft keyboard for typing. */
export function isTextEntryElement(element: Element | null): element is HTMLElement {
  if (!(element instanceof HTMLElement)) return false;
  if (element instanceof HTMLTextAreaElement) return !element.readOnly && !element.disabled;
  if (element instanceof HTMLInputElement) {
    return TEXT_INPUT_TYPES.has(element.type.toLowerCase()) && !element.readOnly && !element.disabled;
  }
  return element.isContentEditable === true;
}

/**
 * Return how far to scroll so `target` sits inside `visible` with `margin` to spare: positive scrolls content up,
 * negative scrolls it down, and zero leaves a target that already fits. A target taller than the visible area keeps
 * its top in view.
 */
export function revealScrollDelta(target: VerticalSpan, visible: VerticalSpan, margin: number): number {
  const top = visible.top + margin;
  const bottom = visible.bottom - margin;
  if (bottom <= top) return 0;
  if (target.top < top) return target.top - top;
  if (target.bottom > bottom) return Math.min(target.bottom - bottom, target.top - top);
  return 0;
}

function caretSpan(element: HTMLElement): VerticalSpan {
  if (!(element instanceof HTMLInputElement) && !(element instanceof HTMLTextAreaElement)) {
    const selection = element.ownerDocument.getSelection();
    if (selection && selection.rangeCount > 0 && element.contains(selection.focusNode)) {
      const rect = selection.getRangeAt(0).getBoundingClientRect();
      if (rect.height > 0) return rect;
    }
  }
  return element.getBoundingClientRect();
}

function scrollContainerOf(element: HTMLElement): HTMLElement | null {
  for (let current = element.parentElement; current; current = current.parentElement) {
    const { overflowY } = getComputedStyle(current);
    if ((overflowY === "auto" || overflowY === "scroll") && current.scrollHeight > current.clientHeight) return current;
  }
  return null;
}

/**
 * Attribute that marks the surface following the soft keyboard. Its value is a {@link KeyboardSurfaceMode}.
 */
export const KEYBOARD_SURFACE_ATTRIBUTE = "data-keyboard-surface";

/** CSS variable on a shifted surface holding how far it moves up, in CSS pixels. */
export const KEYBOARD_SHIFT_PROPERTY = "--keyboard-shift";

/**
 * How a surface follows the soft keyboard. A `lift` surface fills the screen down to its bottom edge, so its bottom
 * edge rises above the keyboard and its content shrinks with it. A `shift` surface, such as a centered dialog or an
 * anchored popover, keeps its size and moves up only as far as needed to stay clear of the keyboard.
 */
export type KeyboardSurfaceMode = "lift" | "shift";

/**
 * Return the surface that follows the soft keyboard while `element` has focus: the fixed layer of the innermost modal
 * dialog holding it, or null when focus is on the page itself. Pages and dialogs behind that surface keep their size.
 */
export function keyboardSurfaceOf(element: Element | null): HTMLElement | null {
  const dialog = element?.closest('[aria-modal="true"]');
  if (!(dialog instanceof HTMLElement)) return null;
  const view = dialog.ownerDocument.defaultView;
  if (!view) return dialog;
  for (let current: HTMLElement | null = dialog; current; current = current.parentElement) {
    if (view.getComputedStyle(current).position === "fixed") return current;
  }
  return dialog;
}

/**
 * Return how the surface holding `dialog` follows the keyboard, measured before it follows: `lift` when the dialog
 * reaches the bottom safe area of a viewport `viewportHeight` tall, and `shift` otherwise.
 */
export function keyboardSurfaceMode(dialog: Element, viewportHeight: number, safeBottom: number): KeyboardSurfaceMode {
  const reach = viewportHeight - Math.max(0, safeBottom) - 1;
  return dialog.getBoundingClientRect().bottom >= reach ? "lift" : "shift";
}

/**
 * Return how far a shifted surface moves up so `dialog`, measured at rest, ends `margin` above `keyboardTop`. It stays
 * still while the keyboard does not reach it, and never moves the dialog's top above `topLimit`.
 */
export function keyboardSurfaceShift(dialog: VerticalSpan, keyboardTop: number, topLimit: number, margin: number): number {
  const needed = dialog.bottom + margin - keyboardTop;
  if (!Number.isFinite(needed) || needed <= 0) return 0;
  return Math.max(0, Math.round(Math.min(needed, dialog.top - topLimit)));
}

/**
 * Scroll the focused text field's caret into the area above the soft keyboard, whose top edge is `visibleBottom` in
 * viewport coordinates. The surface holding the field shrinks above the keyboard, and this keeps the caret inside
 * it, including a caret inside a tall editable block.
 */
export function revealFocusedTextEntry(doc: Document = document, visibleBottom?: number): void {
  const element = doc.activeElement;
  if (!isTextEntryElement(element)) return;
  const container = scrollContainerOf(element);
  if (!container) return;
  const view = doc.defaultView;
  const containerRect = container.getBoundingClientRect();
  const visible = {
    top: Math.max(containerRect.top, 0),
    bottom: Math.min(
      containerRect.bottom,
      visibleBottom ?? view?.visualViewport?.height ?? view?.innerHeight ?? containerRect.bottom,
    ),
  };
  const delta = revealScrollDelta(caretSpan(element), visible, KEYBOARD_REVEAL_MARGIN_PX);
  if (delta !== 0) container.scrollTop += delta;
}
