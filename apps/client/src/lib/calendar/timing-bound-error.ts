/** Stable prefix the backend puts on a refusal caused by an oversized event timing value. */
export const TIMING_TOO_LARGE_CODE = "calendar-timing-too-large";

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
