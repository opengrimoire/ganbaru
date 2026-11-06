/** The height of one hour at 100% zoom, in pixels. */
export const CALENDAR_DEFAULT_HOUR_HEIGHT = 50;

/** The Calendar's zoom levels, in percent. */
export const CALENDAR_ZOOM_PERCENT_LEVELS = [50, 75, 100, 125, 150, 200, 300, 400] as const;

/** The height of one hour, in pixels, at a zoom level given in percent. */
export function calendarZoomHourHeightForPercent(percent: number): number {
  return (CALENDAR_DEFAULT_HOUR_HEIGHT * percent) / 100;
}

/** The minutes a drag snaps to when one hour is `hourHeight` pixels tall: finer as the grid grows. */
export function calendarGridMinutesForHourHeight(hourHeight: number): number {
  if (hourHeight >= 120) return 5;
  if (hourHeight >= 60) return 10;
  if (hourHeight >= 40) return 15;
  return 30;
}

/** The minutes a drag snaps to at a zoom level given in percent. */
export function calendarZoomGridMinutesForPercent(percent: number): number {
  return calendarGridMinutesForHourHeight(calendarZoomHourHeightForPercent(percent));
}
