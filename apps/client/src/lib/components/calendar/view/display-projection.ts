import type { CalendarEvent, EventSurfaceStatus } from "$lib/calendar/types";
import type { CalendarEditPreview } from "$lib/api/calendar-edit";
import type { EditSessionState } from "$lib/components/calendar/edit-session.svelte";
import type { CreatePreview } from "$lib/components/calendar/edit-session.svelte";
import {
  buildCreateDisplay,
  closedDisplay,
  PENDING_CREATE_ID,
} from "$lib/components/calendar/display-events";
import { getEventSurfaceStatusForIdentity } from "$lib/calendar/utils";
import type { computeViewWindow } from "$lib/calendar/utils";

type ViewWindow = ReturnType<typeof computeViewWindow>;

/** Apply route and calendar visibility to already projected occurrences. */
export function visibleCalendarEvents(input: {
  events: CalendarEvent[];
  visibleCalendarIds: ReadonlySet<string>;
  filter?: (event: CalendarEvent) => boolean;
}): CalendarEvent[] {
  const visible = input.events.filter((event) => input.visibleCalendarIds.has(event.calendarId));
  return input.filter ? visible.filter(input.filter) : visible;
}

/** Merge native source-family review with cached occurrences and immediate card feedback. */
export function projectCalendarDisplay(input: {
  storeEvents: CalendarEvent[];
  frozenEvents: CalendarEvent[] | null;
  state: EditSessionState;
  createPreview: CreatePreview | null;
  changes: Partial<CalendarEvent>;
  dirty: boolean;
  nativePreview: CalendarEditPreview | null;
  window: ViewWindow;
  suppressEditPreview: boolean;
}) {
  if (input.frozenEvents) return closedDisplay(input.frozenEvents);
  if (input.state.mode === "closed") return closedDisplay(input.storeEvents);
  if (input.state.mode === "create") {
    if (input.nativePreview) {
      const preview = input.nativePreview;
      // A cached native identity proves creation was accepted. Coincident time
      // ranges are unrelated and cannot resolve an uncertain Save.
      if (input.storeEvents.some((event) => (event.recurringParentId ?? event.id) === preview.editedId)) {
        return closedDisplay(input.storeEvents);
      }
      const pendingId = (id: string) => id === preview.editingId
        ? PENDING_CREATE_ID : `${PENDING_CREATE_ID}::${id}`;
      return { events: [...input.storeEvents, ...preview.window.windowEvents.map((event) =>
        ({ ...event, id: pendingId(event.id) }))],
        previewedIds: new Set([...preview.previewedIds].map(pendingId)),
        editingId: preview.editingId ? pendingId(preview.editingId) : undefined };
    }
    return buildCreateDisplay(
      input.storeEvents,
      input.createPreview,
      input.changes,
      input.window,
    );
  }
  if (input.suppressEditPreview) return closedDisplay(input.storeEvents);
  if (input.nativePreview) {
    const preview = input.nativePreview;
    const unrelated = input.storeEvents.filter((event) =>
      (event.recurringParentId ?? event.id) !== preview.sourceId);
    return { events: [...unrelated, ...preview.window.windowEvents],
      previewedIds: new Set(preview.previewedIds), editingId: preview.editingId };
  }
  // Immediate selected-card interaction is presentation. Native review owns series expansion.
  const selectedId = input.state.instanceEvent.id;
  const events = input.storeEvents.map((event) => {
    if (event.id !== selectedId || !input.dirty) return event;
    const overlay = { ...event };
    for (const field of ["title", "start", "end", "color", "allDay"] as const) {
      if (Object.hasOwn(input.changes, field)) Object.assign(overlay, { [field]: input.changes[field] });
    }
    return overlay;
  });
  return { events, previewedIds: new Set([selectedId]), editingId: selectedId };
}

/** Freeze the last visible projection while its durable commit is in flight. */
export function buildCalendarSaveFreeze(events: CalendarEvent[]): CalendarEvent[] {
  return events.map((event) => ({ ...event }));
}

export function projectCalendarSurfaceStatuses(input: {
  events: CalendarEvent[];
  identityByCalendarId: ReadonlyMap<string, string>;
  pendingStatus: EventSurfaceStatus | undefined;
  pendingEventId: string | undefined;
}): CalendarEvent[] {
  return input.events.map((event) => {
    if (input.pendingStatus && event.id === input.pendingEventId) {
      return { ...event, surfaceStatus: input.pendingStatus };
    }
    const status = getEventSurfaceStatusForIdentity(
      event,
      input.identityByCalendarId.get(event.calendarId),
    );
    return status === undefined ? event : { ...event, surfaceStatus: status };
  });
}
