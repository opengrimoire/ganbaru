/**
 * Pure functions that overlay edit session changes onto clean expanded events.
 * Pending IDs belong only to immediate draft interaction, never to persisted commands.
 */

import type { CalendarEvent } from "$lib/calendar/types";
import type { Temporal } from "@js-temporal/polyfill";
import type { CreatePreview } from "./edit-session.svelte";
import { minuteOffsetToDateStr } from "$lib/calendar/utils";

const PENDING_CREATE_ID = "__pending_create__";

/** Already expanded native occurrences and their visible edit contours. */
export interface DisplayResult {
  events: CalendarEvent[];
  previewedIds: Set<string>;
  editingId: string | undefined;
}

/** Inclusive calendar viewport dates. */
export interface ExpansionWindow {
  start: Temporal.PlainDate;
  end: Temporal.PlainDate;
}

function hasChange<K extends keyof CalendarEvent>(changes: Partial<CalendarEvent>, key: K): boolean {
  return Object.hasOwn(changes, key);
}

export function isPendingCreateEventId(id: string): boolean {
  return id === PENDING_CREATE_ID || id.startsWith(`${PENDING_CREATE_ID}::`);
}

function changeOr<K extends keyof CalendarEvent>(
  changes: Partial<CalendarEvent>,
  key: K,
  fallback: CalendarEvent[K] | undefined,
): CalendarEvent[K] | undefined {
  return hasChange(changes, key) ? changes[key] : fallback;
}

/** Compute display events when no edit session is active. */
export function closedDisplay(storeEvents: CalendarEvent[]): DisplayResult {
  return {
    events: storeEvents,
    previewedIds: new Set(),
    editingId: undefined,
  };
}

/** Compute display events for create mode. */
export function buildCreateDisplay(
  storeEvents: CalendarEvent[],
  preview: CreatePreview | null,
  changes: Partial<CalendarEvent>,
  window: ExpansionWindow,
): DisplayResult {
  if (!preview) {
    return {
      events: storeEvents,
      previewedIds: new Set([PENDING_CREATE_ID]),
      editingId: PENDING_CREATE_ID,
    };
  }

  // Prefer changes.end, which carries the panel's cross-midnight end date, over
  // the drag preview's same-day end.
  const isAllDay = hasChange(changes, "allDay")
    ? changes.allDay === true
    : preview.allDay === true;
  const startStr = changes.start
    ? String(changes.start)
    : minuteOffsetToDateStr(preview.dateStr, preview.startMinute);
  const endStr = changes.end
    ? String(changes.end)
    : isAllDay && preview.endDateStr
      ? `${preview.endDateStr} 00:00`
      : minuteOffsetToDateStr(preview.dateStr, preview.endMinute);

  const template: CalendarEvent = {
    id: PENDING_CREATE_ID,
    title: changeOr(changes, "title", preview.title) ?? "",
    start: startStr,
    end: endStr,
    timezone: changes.timezone ?? "",
    calendarId: changes.calendarId ?? "local",
    color: changeOr(changes, "color", preview.color),
    recurrence: changeOr(changes, "recurrence", preview.recurrence),
    pomodoroConfig: changes.pomodoroConfig,
    notifications: changes.notifications,
    meetingEnabled: changes.meetingEnabled,
    location: changes.location,
    description: changes.description,
    url: changes.url,
    transparency: changes.transparency,
    status: changes.status,
    localParticipationStatus: changes.localParticipationStatus,
    allDay: isAllDay || undefined,
  };

  // Immediate feedback covers the authored card. Native review supplies the family.
  const visible = endStr.slice(0, 10) >= window.start.toString()
    && startStr.slice(0, 10) <= window.end.toString();
  const expanded = visible ? [template] : [];
  const ids = new Set(expanded.map((e) => e.id));

  return {
    events: [...storeEvents, ...expanded],
    previewedIds: ids,
    editingId: PENDING_CREATE_ID,
  };
}

export { PENDING_CREATE_ID };
