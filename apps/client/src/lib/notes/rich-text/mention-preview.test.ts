import { describe, expect, it } from "vitest";
import {
  notesCalendarEventPreviewWhen,
  notesDateMentionPreviewWhen,
  notesLabeledRoute,
  notesMentionScopeEventTimes,
  notesPageLocationHeading,
  type NotesCalendarEventPreviewLabels,
  type NotesCalendarEventPreviewTimes,
} from "./mention-preview";

const UNTITLED = "Untitled";
const LABELS: NotesCalendarEventPreviewLabels = {
  allDay: "All day",
  allDayOn: (date) => `All day on ${date}`,
  datedTimeRange: (date, range) => `${date} ${range}`,
};

function event(overrides: Partial<NotesCalendarEventPreviewTimes> = {}): NotesCalendarEventPreviewTimes {
  return {
    start: "2026-10-12 09:00",
    end: "2026-10-12 10:30",
    recurring: false,
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
    expect(notesCalendarEventPreviewWhen(event({ recurring: true }), { locale: "en-US", timeFormat: "24h", labels: LABELS }))
      .toBe("09:00 - 10:30");
    expect(notesCalendarEventPreviewWhen(event({ recurring: true, allDay: true }), { locale: "en-US", timeFormat: "24h", labels: LABELS }))
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

  it("adds the time of a timed date in the user's clock format", () => {
    expect(notesDateMentionPreviewWhen({ start: "2026-10-30T12:01" }, "en-US", "12h")).toBe("Fri, Oct 30, 2026, 12:01 PM");
    expect(notesDateMentionPreviewWhen({ start: "2026-10-30T12:01" }, "en-US", "24h")).toBe("Fri, Oct 30, 2026, 12:01");
  });

  it("returns null when the stored date cannot be read", () => {
    expect(notesDateMentionPreviewWhen({ start: "someday" }, "en-US")).toBeNull();
  });
});

describe("notesMentionScopeEventTimes", () => {
  const stored = {
    start_time: "2026-10-12T22:30:00Z",
    end_time: "2026-10-12T23:30:00Z",
    all_day: false,
    recurring: true,
  };

  it("renders stored UTC instants as wall-clock times in the given zone", () => {
    expect(notesMentionScopeEventTimes(stored, "America/Mexico_City")).toEqual({
      start: "2026-10-12 16:30",
      end: "2026-10-12 17:30",
      allDay: false,
      recurring: true,
    });
  });

  it("keeps all-day dates in every zone", () => {
    const allDay = { ...stored, start_time: "2026-10-12T00:00:00Z", end_time: "2026-10-13T00:00:00Z", all_day: true };
    expect(notesMentionScopeEventTimes(allDay, "Pacific/Auckland").start).toBe("2026-10-12 00:00");
    expect(notesMentionScopeEventTimes(allDay, "America/Los_Angeles").start).toBe("2026-10-12 00:00");
  });
});
