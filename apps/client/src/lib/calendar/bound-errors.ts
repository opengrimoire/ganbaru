/** Stable prefix the backend puts on a refusal caused by an oversized event timing value. */
export const TIMING_TOO_LARGE_CODE = "calendar-timing-too-large";

/** Stable prefix the backend puts on a refusal or warning caused by an oversized value other than the timing. */
export const VALUE_TOO_LARGE_CODE = "calendar-value-too-large";

/** Tail of the import warning that counts preserved data left out to stay within the size limit. */
const OMISSION_TAIL = "left out to keep this value within the sync size limit";

/** Matches an import refusal that names an event and the group of rows that is too large: `Label (group)`. */
const VALUE_REFUSAL_DETAIL = /^(.*) \(([^()]+)\)$/s;

/**
 * Whether a backend message is the oversized event timing refusal.
 *
 * @param message - Error or import warning text from the backend.
 */
export function isTimingTooLargeMessage(message: string): boolean {
  return message.startsWith(`${TIMING_TOO_LARGE_CODE}:`);
}

/**
 * Event label an oversized-timing import warning names, or null for other messages.
 *
 * @param message - Import warning text from the backend.
 */
export function timingTooLargeImportLabel(message: string): string | null {
  if (!isTimingTooLargeMessage(message)) return null;
  return message.slice(TIMING_TOO_LARGE_CODE.length + 1).trim();
}

/**
 * Whether a backend message is the oversized value refusal or warning (alarms, attendees, and the like).
 *
 * @param message - Error or import warning text from the backend.
 */
export function isValueTooLargeMessage(message: string): boolean {
  return message.startsWith(`${VALUE_TOO_LARGE_CODE}:`);
}

/**
 * Event label an oversized-value import warning names, or null when the message is another warning or only counts
 * preserved data that was left out.
 *
 * @param message - Import warning text from the backend.
 */
export function valueTooLargeImportLabel(message: string): string | null {
  if (!isValueTooLargeMessage(message)) return null;
  const detail = message.slice(VALUE_TOO_LARGE_CODE.length + 1).trim();
  if (detail.endsWith(OMISSION_TAIL)) return null;
  return VALUE_REFUSAL_DETAIL.exec(detail)?.[1] ?? null;
}

/**
 * Whether an import warning reports preserved iCalendar data left out to stay within the size limit.
 *
 * @param message - Import warning text from the backend.
 */
export function isPreservedDataOmissionMessage(message: string): boolean {
  return isValueTooLargeMessage(message) && message.endsWith(OMISSION_TAIL);
}
