import { Temporal } from "@js-temporal/polyfill";
import { describe, expect, it } from "vitest";
import { normalizeNotesDateQuery, parseNotesTypedCalendarDate, splitNotesTypedTime } from "./date-query";

const TODAY = Temporal.PlainDate.from("2026-10-11");

function parse(query: string, locale = "en-US"): string | null {
  return parseNotesTypedCalendarDate(normalizeNotesDateQuery(query), { today: TODAY, locale })?.toString() ?? null;
}

function split(query: string): { dateQuery: string; time: string | null } {
  const result = splitNotesTypedTime(normalizeNotesDateQuery(query));
  return { dateQuery: result.dateQuery, time: result.time?.toString({ smallestUnit: "minute" }) ?? null };
}

describe("splitNotesTypedTime", () => {
  it("reads 24-hour times, partial minutes, and meridiems after a date", () => {
    expect(split("oct 30 12:1")).toEqual({ dateQuery: "oct 30", time: "12:01" });
    expect(split("tomorrow 9am")).toEqual({ dateQuery: "tomorrow", time: "09:00" });
    expect(split("Oct 30, 2026 at 3:45 PM")).toEqual({ dateQuery: "oct 30, 2026", time: "15:45" });
    expect(split("12am")).toEqual({ dateQuery: "", time: "00:00" });
    expect(split("mañana a las 18:30")).toEqual({ dateQuery: "manana", time: "18:30" });
  });

  it("keeps a bare day number and impossible times in the date text", () => {
    expect(split("oct 30")).toEqual({ dateQuery: "oct 30", time: null });
    expect(split("2025-12-2")).toEqual({ dateQuery: "2025-12-2", time: null });
    expect(split("oct 30 25:00")).toEqual({ dateQuery: "oct 30 25:00", time: null });
    expect(split("oct 30 13pm")).toEqual({ dateQuery: "oct 30 13pm", time: null });
  });
});

describe("parseNotesTypedCalendarDate", () => {
  it("reads numeric dates with unpadded months and days", () => {
    expect(parse("2025-12-2")).toBe("2025-12-02");
    expect(parse("2026/1/31")).toBe("2026-01-31");
  });

  it("reads English month names before or after the day, with an optional year", () => {
    expect(parse("oct 30")).toBe("2026-10-30");
    expect(parse("October 30, 2027")).toBe("2027-10-30");
    expect(parse("2 dec 2025")).toBe("2025-12-02");
    expect(parse("sept 5")).toBe("2026-09-05");
  });

  it("reads month names of the active locale alongside English ones", () => {
    expect(parse("30 de octubre", "es")).toBe("2026-10-30");
    expect(parse("ene 3 2027", "es")).toBe("2027-01-03");
    expect(parse("Dic 24", "es")).toBe("2026-12-24");
    expect(parse("dec 24", "es")).toBe("2026-12-24");
  });

  it("rejects ambiguous prefixes, impossible days, and incomplete text", () => {
    expect(parse("ma 3")).toBeNull();
    expect(parse("feb 30")).toBeNull();
    expect(parse("2026-13-01")).toBeNull();
    expect(parse("oct")).toBeNull();
    expect(parse("oct 30 26")).toBeNull();
    expect(parse("next friday")).toBeNull();
  });
});
