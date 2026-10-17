import type { CalendarViewMode } from "$lib/calendar/types";

export type DayHeaderReturnMode = Exclude<CalendarViewMode, "day">;

export const DEFAULT_DAY_HEADER_RETURN_MODE: DayHeaderReturnMode = "week";

export function nextDayHeaderReturnMode(
  current: DayHeaderReturnMode,
  committedMode: CalendarViewMode,
): DayHeaderReturnMode {
  return committedMode === "day" ? current : committedMode;
}
