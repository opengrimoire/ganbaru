import { touchMovedBeyondSlop } from "$lib/utils/touch-hold";

export type CalendarSwipeAxis = "horizontal" | "vertical" | null;

export interface CalendarSwipeCommitInput {
  deltaX: number;
  elapsedMs: number;
  viewportWidth: number;
}

export function calendarSwipeAxis(deltaX: number, deltaY: number): CalendarSwipeAxis {
  if (!touchMovedBeyondSlop(deltaX, deltaY)) return null;
  return Math.abs(deltaX) > Math.abs(deltaY) * 1.15 ? "horizontal" : "vertical";
}

export function calendarSwipeDirection(deltaX: number): "back" | "forward" | null {
  if (deltaX === 0) return null;
  return deltaX < 0 ? "forward" : "back";
}

export function calendarSwipeNavigation(
  input: CalendarSwipeCommitInput,
): "back" | "forward" | null {
  const elapsedMs = Math.max(1, input.elapsedMs);
  const velocity = input.deltaX / elapsedMs;
  const distanceThreshold = Math.min(72, Math.max(36, input.viewportWidth * 0.12));
  const committed = Math.abs(input.deltaX) >= distanceThreshold || Math.abs(velocity) >= 0.3;
  if (!committed) return null;
  return calendarSwipeDirection(input.deltaX);
}
