import { CHAT_ALL_WEEKDAYS_MASK, CHAT_MAX_QUIET_WINDOWS, type ChatQuietWindow } from "$lib/chat/contracts";

/** Minutes between the times the quiet window editor offers. */
export const CHAT_QUIET_TIME_STEP_MINUTES = 15;
const MINUTES_PER_DAY = 1_440;

/** The window a new quiet period starts with: every night from 22:00 to 07:00. */
export const CHAT_DEFAULT_QUIET_WINDOW: ChatQuietWindow = {
  weekdays: CHAT_ALL_WEEKDAYS_MASK,
  startMinute: 22 * 60,
  endMinute: 7 * 60,
};

/** The local minutes a time choice offers, one per step, plus `current` when it falls between steps. */
export function chatQuietTimeMinutes(current: number, step = CHAT_QUIET_TIME_STEP_MINUTES): number[] {
  const minutes: number[] = [];
  for (let minute = 0; minute < MINUTES_PER_DAY; minute += step) minutes.push(minute);
  if (!minutes.includes(current)) minutes.push(current);
  return minutes.sort((left, right) => left - right);
}

/** Appends the default window, or returns `null` when the device already keeps the most windows. */
export function addChatQuietWindow(windows: readonly ChatQuietWindow[]): ChatQuietWindow[] | null {
  if (windows.length >= CHAT_MAX_QUIET_WINDOWS) return null;
  return [...windows, { ...CHAT_DEFAULT_QUIET_WINDOW }];
}

/**
 * Replaces fields of the window at `index`. Returns `null` when the change would make the window
 * start and end at the same minute, which the backend refuses.
 */
export function updateChatQuietWindow(
  windows: readonly ChatQuietWindow[],
  index: number,
  patch: Partial<ChatQuietWindow>,
): ChatQuietWindow[] | null {
  const current = windows[index];
  if (!current) return null;
  const next = { ...current, ...patch };
  if (next.startMinute === next.endMinute || next.weekdays === 0) return null;
  return windows.map((window, position) => (position === index ? next : window));
}

/** Removes the window at `index`. */
export function removeChatQuietWindow(windows: readonly ChatQuietWindow[], index: number): ChatQuietWindow[] {
  return windows.filter((_, position) => position !== index);
}
