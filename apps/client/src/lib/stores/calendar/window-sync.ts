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

export function initCalendarWindowSync(reloadCurrentWindow: () => Promise<void>): void {
  if (calendarSyncInitialized) return;
  calendarSyncInitialized = true;
  listenWindowSync<unknown>(CALENDAR_WINDOW_SYNC_EVENT, (event) => {
    const envelope = event.payload;
    if (!isWindowSyncEnvelope(envelope, isCalendarWindowSyncPayload)) return;
    if (!isForeignWindowSyncEnvelope(envelope)) return;
    reloadCurrentWindow().catch((err) => {
      console.warn("calendar window sync reload failed", err);
    });
  }).catch((err) => console.warn("calendar window sync listener failed", err));
}
