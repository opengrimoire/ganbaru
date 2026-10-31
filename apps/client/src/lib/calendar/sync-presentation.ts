import type { SyncDomainPresentation, SyncTablePresentation } from "$lib/sync/presentation-registry";
import { CALENDAR_SYNC_TABLES, notifyCalendarSyncChanged } from "./sync-events";

/** Titles, descriptions, and timing edited on two devices are chosen in the event panel. A hidden event returns as a new event. */
const EVENTS_PRESENTATION: SyncTablePresentation = {
  conflicts: (t, count) => t("calendar.sync.eventConflicts", count),
  recovery: {
    untitled: (t) => t("calendar.sync.eventUntitled"),
    note: (t) => t("calendar.sync.eventRecoveryNote"),
    restored: (t) => t("calendar.sync.eventRestored"),
    restoreFailed: (t) => t("calendar.sync.eventRestoreFailed"),
  },
};

/** A removed calendar returns with its events moved into the Local calendar. */
const CALENDARS_PRESENTATION: SyncTablePresentation = {
  recovery: {
    untitled: (t) => t("calendar.sync.calendarUntitled"),
    note: (t) => t("calendar.sync.calendarRecoveryNote"),
    restored: (t) => t("calendar.sync.calendarRestored"),
    restoreFailed: (t) => t("calendar.sync.calendarRestoreFailed"),
  },
};

/** Preserved iCalendar objects and components have no Calendar view to return to, so they are only discarded. */
const PRESERVED_PRESENTATION: SyncTablePresentation = {
  recovery: {
    untitled: (t) => t("calendar.sync.preservedUntitled"),
    note: (t) => t("calendar.sync.preservedNote"),
    restorable: false,
  },
};

const TABLE_PRESENTATIONS: Readonly<Record<string, SyncTablePresentation>> = {
  calendars: CALENDARS_PRESENTATION,
  calendar_events: EVENTS_PRESENTATION,
  icalendar_objects: PRESERVED_PRESENTATION,
  icalendar_components: PRESERVED_PRESENTATION,
};

/** How the sync settings present the Calendar. Tables without their own copy use neutral sync copy. */
export const CALENDAR_SYNC_PRESENTATION: SyncDomainPresentation = {
  label: (t) => t("titleBar.tab.calendar"),
  tables: Object.fromEntries(CALENDAR_SYNC_TABLES.map((table) => [table, TABLE_PRESENTATIONS[table] ?? {}])),
  changed: notifyCalendarSyncChanged,
};
