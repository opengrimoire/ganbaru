import { describe, expect, it } from "vitest";
import type { CalendarEvent, RecurrenceConfig } from "$lib/calendar/types";
import {
  notesCalendarEventPreviewWhen,
  notesDateMentionPreviewWhen,
  notesLabeledRoute,
  notesMentionedCalendarEvent,
  notesPageLocationHeading,
  type NotesCalendarEventPreviewLabels,
} from "./mention-preview";

const UNTITLED = "Untitled";
const WEEKLY: RecurrenceConfig = { frequency: "weekly", interval: 1, end: { type: "never" } };
const LABELS: NotesCalendarEventPreviewLabels = {
  allDay: "All day",
  allDayOn: (date) => `All day on ${date}`,
  datedTimeRange: (date, range) => `${date} ${range}`,
};

function event(overrides: Partial<CalendarEvent> = {}): CalendarEvent {
  return {
    id: "event",
    title: "Review",
    start: "2026-10-12 09:00",
    end: "2026-10-12 10:30",
    timezone: "UTC",
    calendarId: "calendar",
    ...overrides,
  };
}

describe("notesLabeledRoute", () => {
  it("labels blank segments as untitled and keeps named ones", () => {
    expect(notesLabeledRoute(["Group", " ", "", "Page"], UNTITLED)).toEqual(["Group", UNTITLED, UNTITLED, "Page"]);
  });
});

describe("notesPageLocationHeading", () => {
  it("splits the page's own title from the route above it", () => {
    expect(notesPageLocationHeading(["Group", "Project", "Folder", "", "Leaf"], UNTITLED))
      .toEqual({ route: ["Group", "Project", "Folder", UNTITLED], title: "Leaf" });
  });

  it("returns an empty route for a top-level page and labels an untitled one", () => {
    expect(notesPageLocationHeading([""], UNTITLED)).toEqual({ route: [], title: UNTITLED });
  });

  it("has no title when the location is empty", () => {
    expect(notesPageLocationHeading([], UNTITLED)).toEqual({ route: [], title: null });
  });
});

describe("notesMentionedCalendarEvent", () => {
  it("prefers the series template over its virtual occurrences", () => {
    const occurrence = event({ id: "series", recurringParentId: "series", start: "2026-10-19 09:00" });
    const template = event({ id: "series", recurrence: WEEKLY });
    expect(notesMentionedCalendarEvent([occurrence, template], "series")).toBe(template);
  });

  it("falls back to a loaded occurrence when only occurrences of the series are loaded", () => {
    const occurrence = event({ id: "series:2026-10-19", recurringParentId: "series" });
    expect(notesMentionedCalendarEvent([occurrence], "series")).toBe(occurrence);
  });

  it("returns undefined for an event outside the loaded window", () => {
    expect(notesMentionedCalendarEvent([event()], "other")).toBeUndefined();
  });
});

describe("notesCalendarEventPreviewWhen", () => {
  it("shows the date and time range of a single timed event in the chosen time format", () => {
    expect(notesCalendarEventPreviewWhen(event(), { locale: "en-US", timeFormat: "24h", labels: LABELS }))
      .toBe("Mon, Oct 12, 2026 09:00 - 10:30");
    expect(notesCalendarEventPreviewWhen(event({ end: "2026-10-12 13:00" }), { locale: "en-US", timeFormat: "12h", labels: LABELS }))
      .toBe("Mon, Oct 12, 2026 9 am - 1 pm");
  });

  it("shows the date of a single all-day event", () => {
    expect(notesCalendarEventPreviewWhen(event({ allDay: true, start: "2026-10-12 00:00" }), { locale: "en-US", timeFormat: "24h", labels: LABELS }))
      .toBe("All day on Mon, Oct 12, 2026");
  });

  it("shows only the time range or all-day label of a recurring series", () => {
    expect(notesCalendarEventPreviewWhen(event({ recurrence: WEEKLY }), { locale: "en-US", timeFormat: "24h", labels: LABELS }))
      .toBe("09:00 - 10:30");
    expect(notesCalendarEventPreviewWhen(event({ recurrence: WEEKLY, allDay: true }), { locale: "en-US", timeFormat: "24h", labels: LABELS }))
      .toBe("All day");
  });

  it("returns null when the stored start cannot be read", () => {
    expect(notesCalendarEventPreviewWhen(event({ start: "not a date" }), { locale: "en-US", timeFormat: "24h", labels: LABELS }))
      .toBeNull();
  });
});

describe("notesDateMentionPreviewWhen", () => {
  it("spells out a single date with its weekday", () => {
    expect(notesDateMentionPreviewWhen({ start: "2026-10-12" }, "en-US")).toBe("Mon, Oct 12, 2026");
  });

  it("spells out a date range", () => {
    const range = notesDateMentionPreviewWhen({ start: "2026-10-12", end: "2026-10-14" }, "en-US");
    expect(range).toContain("Oct 12");
    expect(range).toContain("14");
  });

  it("returns null when the stored date cannot be read", () => {
    expect(notesDateMentionPreviewWhen({ start: "someday" }, "en-US")).toBeNull();
  });
});
