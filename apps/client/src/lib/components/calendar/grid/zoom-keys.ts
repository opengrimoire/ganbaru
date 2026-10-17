/** Internal calendar zoom action requested by a keyboard shortcut. */
export type CalendarZoomKeyAction = "in" | "out" | "reset";

/** Keyboard fields read by the calendar zoom shortcuts. */
export type CalendarZoomKeyInput = Pick<KeyboardEvent, "key" | "code" | "shiftKey" | "ctrlKey" | "metaKey">;

/**
 * Physical keys that print "+" with Shift held on common layouts: "Equal" on US and French,
 * "BracketRight" on Spanish and German, plus the numpad. Nordic layouts print "+" unshifted on
 * the "Minus" key, which is matched through `key === "+"` instead: listing "Minus" here would turn
 * US Shift + Minus ("_", zoom out) into zoom in. The Keyboard API may be unavailable in some
 * WebViews (such as Tauri), so physical codes stand in for layout lookups.
 */
const PLUS_KEY_CODES: readonly string[] = ["Equal", "BracketRight", "NumpadAdd"];
const RESET_KEY_CODES: readonly string[] = ["Digit0", "Numpad0"];

/**
 * Map a keydown to a calendar zoom action: Shift + 0 resets, "+" zooms in, "-" zooms out.
 * Ctrl and Meta combinations are reserved for app-level zoom and return null.
 */
export function calendarZoomKeyAction(event: CalendarZoomKeyInput): CalendarZoomKeyAction | null {
  if (event.ctrlKey || event.metaKey) return null;
  if (event.shiftKey && (event.key === "0" || RESET_KEY_CODES.includes(event.code))) return "reset";
  if (event.key === "+" || (event.shiftKey && PLUS_KEY_CODES.includes(event.code))) return "in";
  if (event.key === "-" || event.key === "_") return "out";
  return null;
}
