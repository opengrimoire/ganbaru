import { hasOnlyShortcutModifier, type KeyboardModifierState } from "$lib/keyboard-shortcuts";

/** How a pointer gesture asked for a new tab. */
export interface OpenInNewTabIntent {
  /** Keep the current tab in front, as browsers do for a middle click or Ctrl+click. */
  background: boolean;
}

export type OpenInNewTabEvent = KeyboardModifierState & Pick<MouseEvent, "type" | "button">;

/**
 * Read an "open in new tab" request from a click: a middle click or a Ctrl+click (Cmd on Apple platforms)
 * opens in the background, and adding Shift brings the new tab to the front. A plain click returns null.
 * Ctrl+Enter on a focused row arrives as a click with the modifier, so keyboard users get the same gesture.
 */
export function openInNewTabIntent(
  event: OpenInNewTabEvent,
  platform?: string,
): OpenInNewTabIntent | null {
  if (event.type === "auxclick") {
    return event.button === 1 && !event.altKey ? { background: !event.shiftKey } : null;
  }
  if (event.type !== "click" || event.button !== 0) return null;
  if (hasOnlyShortcutModifier(event, {}, platform)) return { background: true };
  if (hasOnlyShortcutModifier(event, { shift: true }, platform)) return { background: false };
  return null;
}

/** Stop a middle press from starting the platform's autoscroll on a row that opens tabs. */
export function preventMiddleClickAutoscroll(event: Pick<MouseEvent, "button" | "preventDefault">): void {
  if (event.button === 1) event.preventDefault();
}
