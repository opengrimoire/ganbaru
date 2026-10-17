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
 * Scroll the focused text field's caret into the area left above the soft keyboard. The WebView usually does this
 * itself after it shrinks; this covers a caret inside a tall editable block, which the WebView reveals only by the
 * block's edge.
 */
export function revealFocusedTextEntry(doc: Document = document): void {
  const element = doc.activeElement;
  if (!isTextEntryElement(element)) return;
  const container = scrollContainerOf(element);
  if (!container) return;
  const view = doc.defaultView;
  const containerRect = container.getBoundingClientRect();
  const visible = {
    top: Math.max(containerRect.top, 0),
    bottom: Math.min(containerRect.bottom, view?.visualViewport?.height ?? view?.innerHeight ?? containerRect.bottom),
  };
  const delta = revealScrollDelta(caretSpan(element), visible, KEYBOARD_REVEAL_MARGIN_PX);
  if (delta !== 0) container.scrollTop += delta;
}
