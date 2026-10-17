import { describe, expect, it } from "vitest";
import { Temporal } from "@js-temporal/polyfill";
import { adjacentCalendarWindowRequests, calendarWindowCovers } from "./window-prefetch";

function date(value: string): Temporal.PlainDate {
  return Temporal.PlainDate.from(value);
}

function ranges(start: string, end: string): string[] {
  return adjacentCalendarWindowRequests(date(start), date(end))
    .map((range) => `${range.start.toString()}..${range.end.toString()}`);
}

describe("adjacentCalendarWindowRequests", () => {
  it("detects whether one loaded window fully covers another", () => {
    expect(calendarWindowCovers(
      date("2026-04-26"),
      date("2026-05-04"),
      date("2026-04-29"),
      date("2026-05-01"),
    )).toBe(true);
    expect(calendarWindowCovers(
      date("2026-04-29"),
      date("2026-05-01"),
      date("2026-04-26"),
      date("2026-05-04"),
    )).toBe(false);
  });

  it("uses visible strides for margin-expanded day and week windows", () => {
    expect(ranges("2026-04-29", "2026-05-01")).toEqual([
      "2026-04-30..2026-05-02",
      "2026-04-28..2026-04-30",
    ]);
    expect(ranges("2026-04-26", "2026-05-04")).toEqual([
      "2026-05-03..2026-05-11",
      "2026-04-19..2026-04-27",
    ]);
  });

  it("uses work-cycle strides for margin-expanded weekday and weekend windows", () => {
    expect(ranges("2026-04-26", "2026-05-02")).toEqual([
      "2026-05-01..2026-05-04",
      "2026-04-24..2026-04-27",
    ]);
    expect(ranges("2026-05-01", "2026-05-04")).toEqual([
      "2026-05-03..2026-05-09",
      "2026-04-26..2026-05-02",
    ]);
  });

  it("uses neighboring month windows for margin-expanded month windows", () => {
    expect(ranges("2026-03-29", "2026-05-11")).toEqual([
      "2026-04-26..2026-06-08",
      "2026-02-22..2026-04-06",
    ]);
  });

  it("rejects windows without a known calendar view shape", () => {
    expect(ranges("2026-04-01", "2026-04-08")).toEqual([]);
  });
});
