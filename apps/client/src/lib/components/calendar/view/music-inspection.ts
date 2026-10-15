import type { WorkspacePaneScope } from "$lib/workspace-tabs/model";

/** A shown Calendar tab that can open the event a Music assignment inspection names. */
export interface CalendarMusicInspectionTarget {
  scope: WorkspacePaneScope;
  /** Whether the view's pane has focus; true outside a split view. */
  isFocused: () => boolean;
}

/**
 * Pick the Calendar that opens an inspected event: the focused pane's when it shows a Calendar, otherwise the
 * primary pane's, so a split of two Calendars opens it once.
 */
export function calendarMusicInspectionTarget(
  targets: Iterable<CalendarMusicInspectionTarget>,
): CalendarMusicInspectionTarget | null {
  let fallback: CalendarMusicInspectionTarget | null = null;
  for (const target of targets) {
    if (target.isFocused()) return target;
    if (!fallback || (fallback.scope !== "primary" && target.scope === "primary")) fallback = target;
  }
  return fallback;
}

const targets = new Set<CalendarMusicInspectionTarget>();

/** Offer a Calendar tab view for Music assignment inspection until the returned disposer runs. */
export function registerCalendarMusicInspectionTarget(target: CalendarMusicInspectionTarget): () => void {
  targets.add(target);
  return () => {
    targets.delete(target);
  };
}

/** Whether `target` is the registered Calendar that opens an inspected event. */
export function handlesCalendarMusicInspection(target: CalendarMusicInspectionTarget): boolean {
  return calendarMusicInspectionTarget(targets) === target;
}
