import { listenSyncApplied } from "$lib/api/sync";
import { touchesCalendar } from "$lib/calendar/sync-events";
import {
  createWindowSyncEnvelope,
  isForeignWindowSyncEnvelope,
  isWindowSyncEnvelope,
} from "$lib/windows/sync";
import {
  emitWindowSync,
  listenWindowSync,
} from "$lib/windows/sync-transport";

const CALENDAR_WINDOW_SYNC_EVENT = "calendar-window-sync";

interface CalendarWindowSyncPayload {
  kind: "data-changed";
}

let calendarSyncInitialized = false;

function isCalendarWindowSyncPayload(value: unknown): value is CalendarWindowSyncPayload {
  if (typeof value !== "object" || value === null) return false;
  const record = value as Record<string, unknown>;
  return record.kind === "data-changed";
}

export function publishCalendarWindowSync(): void {
  emitWindowSync(
    CALENDAR_WINDOW_SYNC_EVENT,
    createWindowSyncEnvelope<CalendarWindowSyncPayload>({ kind: "data-changed" }),
  ).catch((err) => console.warn("calendar window sync failed", err));
}

/** Reloads this window after another window or a linked device changes Calendar rows. */
export function initCalendarWindowSync(reloadCurrentWindow: () => Promise<void>): void {
  if (calendarSyncInitialized) return;
  calendarSyncInitialized = true;
  const reload = (): void => {
    reloadCurrentWindow().catch((err) => {
      console.warn("calendar window sync reload failed", err);
    });
  };
  listenWindowSync<unknown>(CALENDAR_WINDOW_SYNC_EVENT, (event) => {
    const envelope = event.payload;
    if (!isWindowSyncEnvelope(envelope, isCalendarWindowSyncPayload)) return;
    if (!isForeignWindowSyncEnvelope(envelope)) return;
    reload();
  }).catch((err) => console.warn("calendar window sync listener failed", err));
  listenSyncApplied((tables) => {
    if (touchesCalendar(tables)) reload();
  }).catch((err) => console.warn("calendar sync notice listener failed", err));
}
