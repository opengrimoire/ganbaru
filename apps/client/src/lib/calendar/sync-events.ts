const CALENDAR_SYNC_CHANGED_EVENT = "ganbaru-calendar-sync-changed";

/** Tells this window's Calendar store that sync recovery changed rows outside it. */
export function notifyCalendarSyncChanged(): void {
  if (typeof window !== "undefined") window.dispatchEvent(new CustomEvent(CALENDAR_SYNC_CHANGED_EVENT));
}

/** Listens for {@link notifyCalendarSyncChanged}, and returns a function that stops listening. */
export function onCalendarSyncChanged(listener: () => void): () => void {
  if (typeof window === "undefined") return () => undefined;
  window.addEventListener(CALENDAR_SYNC_CHANGED_EVENT, listener);
  return () => window.removeEventListener(CALENDAR_SYNC_CHANGED_EVENT, listener);
}
