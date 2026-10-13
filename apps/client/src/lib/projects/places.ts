import { hasOnlyShortcutModifier, type KeyboardModifierState } from "$lib/keyboard-shortcuts";
import type { View } from "$lib/navigation";
import { PROJECT_HOSTED_VIEW_TYPES, type ProjectHostedViewType } from "./types";

/**
 * Returns the hosted view types a window can open: a feature is hosted only where its own tab is visible, so a
 * detached Notes or Chat window never shares its workspace with a Projects tab elsewhere.
 */
export function projectHostablePlaces(visibleViews: readonly View[]): ProjectHostedViewType[] {
  return PROJECT_HOSTED_VIEW_TYPES.filter((place) => visibleViews.includes(place));
}

/**
 * Returns the entry a Ctrl (Cmd on Apple) and digit shortcut opens, by position in the sidebar list. It reads the
 * physical digit key too, so layouts whose top row types symbols without Shift still work.
 */
export function projectPlaceShortcut<T>(
  event: KeyboardModifierState & Pick<KeyboardEvent, "key" | "code" | "isComposing">,
  places: readonly T[],
  platform?: string,
): T | null {
  if (event.isComposing || !hasOnlyShortcutModifier(event, {}, platform)) return null;
  const digit = /^[1-9]$/.test(event.key) ? event.key : /^(?:Digit|Numpad)([1-9])$/.exec(event.code)?.[1];
  if (!digit) return null;
  return places[Number(digit) - 1] ?? null;
}
