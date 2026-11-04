import { describe, expect, it } from "vitest";
import type { CalendarFieldConflict, CalendarTimingValue } from "$lib/api/calendar-conflicts";
import { en } from "$lib/i18n/messages/en";
import { es } from "$lib/i18n/messages/es";
import { translateFromPartialCatalog, type Translate } from "$lib/i18n/translator.svelte";
import {
  conflictedEventIds,
  eventConflicts,
  findEventFieldConflict,
  hasTimingConflict,
  previewConflictText,
  summarizeTiming,
} from "./conflicts";

const tEn: Translate = ((key, ...args) => translateFromPartialCatalog(en, key, ...args)) as Translate;
const tEs: Translate = ((key, ...args) => translateFromPartialCatalog(es, key, ...args)) as Translate;

const device = { deviceId: "d", deviceLabel: "Phone", ownDevice: false };

function conflict(eventId: string, field: CalendarFieldConflict["field"], versions = 2): CalendarFieldConflict {
  return {
    eventId,
    field,
    versions: Array.from({ length: versions }, (_, index) => ({
      version: `v${index}`,
      device,
      editedAtMs: index,
      displayed: index === 0,
      value: { kind: "text" as const, text: `t${index}` },
    })),
  };
}

const timing: CalendarTimingValue = {
  kind: "timing",
  startTime: "2026-10-23T09:00:00Z",
  endTime: "2026-10-23T10:30:00Z",
  timezone: "Europe/Madrid",
  allDay: false,
  rrule: null,
  repeatUntil: null,
  excludedDates: 1,
  addedDates: 2,
  overrides: 3,
};

describe("calendar conflict lookups", () => {
  const all = [conflict("e1", "description"), conflict("e1", "title"), conflict("e2", "timing"), conflict("e3", "title", 0)];

  it("finds the conflict of one field", () => {
    expect(findEventFieldConflict(all, "e1", "title")?.field).toBe("title");
    expect(findEventFieldConflict(all, "e1", "timing")).toBeUndefined();
    expect(findEventFieldConflict(all, "missing", "title")).toBeUndefined();
  });

  it("ignores a conflict without versions", () => {
    expect(findEventFieldConflict(all, "e3", "title")).toBeUndefined();
    expect(conflictedEventIds(all)).toEqual(new Set(["e1", "e2"]));
  });

  it("lists an event's conflicts in dialog order", () => {
    const withTiming = [...all, conflict("e1", "timing")];
    expect(eventConflicts(withTiming, "e1").map((entry) => entry.field)).toEqual(["title", "timing", "description"]);
    expect(eventConflicts(withTiming, "none")).toEqual([]);
  });

  it("reports an open timing conflict only", () => {
    expect(hasTimingConflict(all, "e2")).toBe(true);
    expect(hasTimingConflict(all, "e1")).toBe(false);
  });
});

describe("text previews", () => {
  it("keeps short text and cuts long text on character boundaries", () => {
    expect(previewConflictText("short")).toBe("short");
    expect(previewConflictText("a".repeat(250))).toBe(`${"a".repeat(240)}…`);
    expect(previewConflictText("😀😀😀😀", 2)).toBe("😀😀…");
  });
});

describe("timing summary", () => {
  it("formats a timed version in the event's time zone", () => {
    const summary = summarizeTiming(timing, "en", tEn);
    expect(summary.start).toContain("11:00");
    expect(summary.end).toContain("12:30");
    expect(summary.timezone).toBe("Europe/Madrid");
    expect(summary.allDay).toBe(false);
    expect(summary.recurrence).toBeNull();
    expect([summary.excludedDates, summary.addedDates, summary.overrides]).toEqual([1, 2, 3]);
  });

  it("uses the stored date for an all-day version regardless of the zone", () => {
    const summary = summarizeTiming(
      { ...timing, allDay: true, startTime: "2026-10-23T00:00:00Z", endTime: "2026-10-24T00:00:00Z", timezone: "America/Los_Angeles" },
      "en",
      tEn,
    );
    expect(summary.start).toContain("23");
    expect(summary.end).toContain("24");
    expect(summary.start).not.toContain(":");
    expect(summary.timezone).toBeNull();
  });

  it("summarizes the recurrence with its end", () => {
    const weekly = summarizeTiming({ ...timing, rrule: "FREQ=WEEKLY;BYDAY=MO", repeatUntil: "2026-12-01" }, "en", tEn);
    expect(weekly.recurrence).toMatch(/week/i);
    expect(weekly.recurrence).toMatch(/ends/i);
  });

  it("localizes the dates and the recurrence", () => {
    const summary = summarizeTiming({ ...timing, rrule: "FREQ=DAILY" }, "es", tEs);
    expect(summary.start).toMatch(/oct/i);
    expect(summary.recurrence).toBe(es.calendar.recurrence.daily);
  });

  it("falls back to the stored text for a value it cannot parse", () => {
    const summary = summarizeTiming({ ...timing, startTime: "not-a-time", timezone: "Nowhere/Land" }, "en", tEn);
    expect(summary.start).toBe("not-a-time");
    expect(summary.end).toBeTruthy();
  });
});
