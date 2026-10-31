import { describe, expect, it, vi } from "vitest";
import { en } from "$lib/i18n/messages/en";
import { es } from "$lib/i18n/messages/es";
import { translateFromPartialCatalog, type Translate } from "$lib/i18n/translator.svelte";
import { syncPresentations } from "$lib/sync/presentations";
import { CALENDAR_SYNC_TABLES, onCalendarSyncChanged, touchesCalendar } from "./sync-events";
import { CALENDAR_SYNC_PRESENTATION } from "./sync-presentation";

vi.mock("$lib/projects/window-sync", () => ({ publishProjectsSyncChange: vi.fn() }));

const tEn: Translate = ((key, ...args) => translateFromPartialCatalog(en, key, ...args)) as Translate;
const tEs: Translate = ((key, ...args) => translateFromPartialCatalog(es, key, ...args)) as Translate;

const RESTORABLE_TABLES = ["calendars", "calendar_events"];
const DISCARD_ONLY_TABLES = ["icalendar_objects", "icalendar_components"];

describe("calendar sync presentation", () => {
  it("registers every replicated Calendar table once", () => {
    expect(Object.keys(CALENDAR_SYNC_PRESENTATION.tables).sort()).toEqual([...CALENDAR_SYNC_TABLES].sort());
    expect(syncPresentations.labels(tEn)).toContain("Calendar");
  });

  it("titles, explains, and confirms restorable tables in both languages", () => {
    for (const t of [tEn, tEs]) {
      for (const table of RESTORABLE_TABLES) {
        expect(syncPresentations.recoveryUntitled(t, table)).not.toBe(t("sync.recovery.untitled"));
        expect(syncPresentations.recoveryNote(t, table)).toBeTruthy();
        expect(syncPresentations.recoveryRestorable(table)).toBe(true);
        expect(syncPresentations.recoveryRestored(t, table)).not.toBe(t("sync.recovery.restored"));
        expect(syncPresentations.recoveryRestoreFailed(t, table)).not.toBe(t("sync.recovery.failed"));
      }
    }
  });

  it("offers preserved iCalendar objects and components only for discarding", () => {
    for (const table of DISCARD_ONLY_TABLES) {
      expect(syncPresentations.recoveryRestorable(table)).toBe(false);
      expect(syncPresentations.recoveryNote(tEn, table)).toBeTruthy();
      expect(syncPresentations.recoveryNote(tEs, table)).toBeTruthy();
    }
  });

  it("treats every Calendar table and the Focus topic as touching Calendar views", () => {
    for (const table of [...CALENDAR_SYNC_TABLES, "focus"]) expect(touchesCalendar([table])).toBe(true);
    expect(touchesCalendar(["quick_notes", "projects"])).toBe(false);
    expect(touchesCalendar([])).toBe(false);
  });

  it("points event conflicts to the Calendar, with plural forms", () => {
    for (const t of [tEn, tEs]) {
      const one = syncPresentations.conflictLine(t, "calendar_events", 1);
      const many = syncPresentations.conflictLine(t, "calendar_events", 3);
      expect(one).not.toBe(t("sync.conflicts", 1));
      expect(many).not.toBe(one);
      expect(many).toContain("3");
    }
  });

  it("uses neutral copy for tables without their own", () => {
    expect(syncPresentations.conflictLine(tEn, "calendar_event_archives", 2)).toBe(tEn("sync.conflicts", 2));
    expect(syncPresentations.recoveryNote(tEn, "project_task_event_links")).toBeNull();
  });

  it("tells this window's Calendar store that rows changed", () => {
    vi.stubGlobal("window", new EventTarget());
    const listener = vi.fn();
    const stop = onCalendarSyncChanged(listener);
    try {
      CALENDAR_SYNC_PRESENTATION.changed();
    } finally {
      stop();
      vi.unstubAllGlobals();
    }
    expect(listener).toHaveBeenCalledTimes(1);
  });
});
