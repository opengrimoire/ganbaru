import { describe, expect, it } from "vitest";
import {
  notesDatabaseDateBoundaryParts,
  notesDatabaseDateHasTime,
  notesDatabaseDateValue,
  notesDatabaseDateWithDay,
  notesDatabaseDateWithEnd,
  notesDatabaseDateWithoutTime,
  notesDatabaseDateWithTime,
  notesDatabaseDateWithTimeZone,
  notesDatabaseIsDateBoundaryValid,
  notesDatabaseIsTimeZoneValid,
} from "./date";

describe("Notes database dates", () => {
  it("accepts calendar dates, local wall times, and offset timestamps", () => {
    for (const value of ["2024-02-29", "2026-10-01T09:30", "2026-10-01T09:30:10.123", "2026-10-01T09:30:00-06:00", "2026-10-01T15:30:00Z"]) {
      expect(notesDatabaseIsDateBoundaryValid(value)).toBe(true);
    }
  });
  it("rejects impossible calendar boundaries instead of accepting date-shaped text", () => {
    for (const value of ["2026-02-29", "2026-04-31", "2026-10-01T24:00", "2026-10-01T09:60", "2026-10-01garbage", "2026-10-01T09:00+25:00"]) {
      expect(notesDatabaseIsDateBoundaryValid(value)).toBe(false);
    }
  });
  it("retains all canonical date fields without coercing external values", () => {
    expect(notesDatabaseDateValue({ start: "2026-10-01", end: "2026-10-03", time_zone: "America/Monterrey" }))
      .toEqual({ start: "2026-10-01", end: "2026-10-03", time_zone: "America/Monterrey" });
    expect(notesDatabaseDateValue({ start: 123, end: false })).toBeNull();
    expect(notesDatabaseIsTimeZoneValid("America/Monterrey")).toBe(true);
    expect(notesDatabaseIsTimeZoneValid("Unknown/Nowhere")).toBe(false);
  });
});

describe("Notes database date range editing", () => {
  it("splits boundaries into the date, wall time, and offset the pickers edit", () => {
    expect(notesDatabaseDateBoundaryParts("2026-10-01")).toEqual({ date: "2026-10-01", time: null, offset: "" });
    expect(notesDatabaseDateBoundaryParts("2026-10-01T09:30:10.123-06:00")).toEqual({ date: "2026-10-01", time: "09:30", offset: "-06:00" });
    expect(notesDatabaseDateBoundaryParts("2026-10-01T09:30Z")).toEqual({ date: "2026-10-01", time: "09:30", offset: "Z" });
    expect(notesDatabaseDateBoundaryParts("October 1")).toBeNull();
  });
  it("starts a single date when picking a day without a value", () => {
    expect(notesDatabaseDateWithDay(null, "start", "2026-10-11")).toEqual({ start: "2026-10-11", end: null, time_zone: null });
  });
  it("moves a boundary to another day while keeping its time, offset, and zone", () => {
    const value = { start: "2026-10-01T09:30:00-06:00", end: "2026-10-03T18:00:00-06:00", time_zone: "America/Monterrey" };
    expect(notesDatabaseDateWithDay(value, "end", "2026-10-05"))
      .toEqual({ ...value, end: "2026-10-05T18:00:00-06:00" });
  });
  it("swaps the boundaries when a picked day would end the range before it starts", () => {
    const value = { start: "2026-10-05", end: "2026-10-07", time_zone: null };
    expect(notesDatabaseDateWithDay(value, "end", "2026-10-02")).toEqual({ start: "2026-10-02", end: "2026-10-05", time_zone: null });
    expect(notesDatabaseDateWithDay(value, "start", "2026-10-09")).toEqual({ start: "2026-10-07", end: "2026-10-09", time_zone: null });
  });
  it("gives a whole-day end the start time so a range never mixes days and times", () => {
    const value = { start: "2026-10-01", end: "2026-10-03", time_zone: null };
    expect(notesDatabaseDateWithTime(value, "start", "09:00")).toEqual({ start: "2026-10-01T09:00", end: "2026-10-03T09:00", time_zone: null });
    const timed = { start: "2026-10-01T09:00", end: "2026-10-03T18:00", time_zone: null };
    expect(notesDatabaseDateWithTime(timed, "end", "20:30")).toEqual({ ...timed, end: "2026-10-03T20:30" });
  });
  it("keeps the offset and drops seconds when a time is picked, and orders a same-day range", () => {
    expect(notesDatabaseDateWithTime({ start: "2026-10-01T09:30:10Z", end: null, time_zone: null }, "start", "11:00"))
      .toEqual({ start: "2026-10-01T11:00Z", end: null, time_zone: null });
    expect(notesDatabaseDateWithTime({ start: "2026-10-01T10:00", end: "2026-10-01T12:00", time_zone: null }, "end", "08:00"))
      .toEqual({ start: "2026-10-01T08:00", end: "2026-10-01T10:00", time_zone: null });
  });
  it("removes times and their zone from both boundaries together", () => {
    expect(notesDatabaseDateWithoutTime({ start: "2026-10-01T09:00:00-06:00", end: "2026-10-02T10:00", time_zone: "America/Monterrey" }))
      .toEqual({ start: "2026-10-01", end: "2026-10-02", time_zone: null });
    expect(notesDatabaseDateHasTime({ start: "2026-10-01", end: "2026-10-02T10:00", time_zone: null })).toBe(true);
    expect(notesDatabaseDateHasTime({ start: "2026-10-01", end: null, time_zone: null })).toBe(false);
    expect(notesDatabaseDateHasTime(null)).toBe(false);
  });
  it("turns a single date into a range on the same boundary and back", () => {
    const single = { start: "2026-10-01T09:00", end: null, time_zone: "America/Monterrey" };
    const range = notesDatabaseDateWithEnd(single, true);
    expect(range).toEqual({ ...single, end: "2026-10-01T09:00" });
    expect(notesDatabaseDateWithEnd({ ...range, end: "2026-10-04T09:00" }, true).end).toBe("2026-10-04T09:00");
    expect(notesDatabaseDateWithEnd(range, false)).toEqual(single);
  });
  it("clears the time zone for blank text and trims a named zone", () => {
    const value = { start: "2026-10-01T09:00", end: null, time_zone: "America/Monterrey" };
    expect(notesDatabaseDateWithTimeZone(value, "  ").time_zone).toBeNull();
    expect(notesDatabaseDateWithTimeZone(value, " Europe/Madrid ").time_zone).toBe("Europe/Madrid");
  });
});
