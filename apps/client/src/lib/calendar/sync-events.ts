const CALENDAR_SYNC_CHANGED_EVENT = "ganbaru-calendar-sync-changed";

/** Replicated Calendar tables, as sync names them in applied notices, conflicts, and recovery offers. */
export const CALENDAR_SYNC_TABLES: readonly string[] = [
  "calendars",
  "icalendar_objects",
  "icalendar_components",
  "calendar_events",
  "calendar_event_archives",
  "calendar_event_archive_import_objects",
  "project_task_event_links",
];

/** Applied tables that change what Calendar views render: Calendar rows and the Focus topic behind run history. */
const CALENDAR_VIEW_TABLES: ReadonlySet<string> = new Set([...CALENDAR_SYNC_TABLES, "focus"]);

/** Whether replicated changes to these tables affect Calendar views. */
export function touchesCalendar(tables: readonly string[]): boolean {
  return tables.some((table) => CALENDAR_VIEW_TABLES.has(table));
}

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
