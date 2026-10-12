import type { CalendarEvent } from "$lib/calendar/types";

/** Identity published by the native Focus owner, independent of device-day evidence. */
export interface ActivePomodoroIdentity {
  occurrenceId: string | null | undefined;
}

/** Preserve opaque template IDs while removing only a qualified recurrence suffix. */
export function rootIdForEvent(event: CalendarEvent): string {
  return event.recurringParentId ?? event.id.replace(/::\d{4}-\d{2}-\d{2}$/, "");
}

/** Match original home-zone identities even when rendering or overrides move the date. */
export function exactOccurrenceId(event: CalendarEvent, template?: CalendarEvent): string {
  const root = event.recurringParentId
    ?? template?.id
    ?? (event.recurrence || event.rdate?.length ? rootIdForEvent(event) : undefined);
  if (!root) return event.id;
  const qualifiedDate = event.id.match(/::(\d{4}-\d{2}-\d{2})$/)?.[1];
  const date = event.recurrenceDate ?? qualifiedDate;
  return date ? `${root}::${date}` : event.id;
}

/** Highlight only the accepted native occurrence, never a segment's device day. */
export function eventMatchesActiveOccurrence(
  event: CalendarEvent,
  active: ActivePomodoroIdentity | undefined,
  template?: CalendarEvent,
): boolean {
  if (!active?.occurrenceId) return false;
  return event.id === active.occurrenceId || exactOccurrenceId(event, template) === active.occurrenceId;
}

/** Compare aliases of one concrete occurrence without interpreting rendered geometry. */
export function isSameConcreteOccurrence(a: CalendarEvent, b: CalendarEvent): boolean {
  if (a.id === b.id) return true;
  const aRecurring = !!a.recurringParentId || /::\d{4}-\d{2}-\d{2}$/.test(a.id) || !!a.recurrence || !!a.rdate?.length;
  const bRecurring = !!b.recurringParentId || /::\d{4}-\d{2}-\d{2}$/.test(b.id) || !!b.recurrence || !!b.rdate?.length;
  return (aRecurring || bRecurring)
    && rootIdForEvent(a) === rootIdForEvent(b)
    && exactOccurrenceId(a) === exactOccurrenceId(b);
}
