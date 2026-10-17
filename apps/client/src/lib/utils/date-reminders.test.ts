import { describe, expect, it } from "vitest";
import {
  DATE_ONLY_REMINDER_OFFSETS,
  TIMED_DATE_REMINDER_OFFSETS,
  dateReminderDueAtMs,
  dateReminderOffsets,
  offeredDateReminderMinutes,
} from "./date-reminders";

describe("date reminders", () => {
  it("offers whole days without a time and finer offsets with one", () => {
    expect(dateReminderOffsets(false)).toBe(DATE_ONLY_REMINDER_OFFSETS);
    expect(dateReminderOffsets(true)).toBe(TIMED_DATE_REMINDER_OFFSETS);
    expect(DATE_ONLY_REMINDER_OFFSETS.every((offset) => offset % 1440 === 0)).toBe(true);
  });

  it("keeps an offered offset and falls back to the date itself otherwise", () => {
    expect(offeredDateReminderMinutes(15, true)).toBe(15);
    expect(offeredDateReminderMinutes(15, false)).toBe(0);
    expect(offeredDateReminderMinutes(1440, false)).toBe(1440);
  });

  it("places a timed date in the time zone and subtracts the offset", () => {
    expect(dateReminderDueAtMs("2026-10-20", "17:30", 15, "Europe/Madrid"))
      .toBe(Date.parse("2026-10-20T15:15:00.000Z"));
  });

  it("anchors a date without a time to the reminder hour", () => {
    expect(dateReminderDueAtMs("2026-10-20", null, 1440, "UTC")).toBe(Date.parse("2026-10-19T09:00:00.000Z"));
    expect(dateReminderDueAtMs("2026-10-20", null, 0, "UTC", 7)).toBe(Date.parse("2026-10-20T07:00:00.000Z"));
  });

  it("resolves a skipped wall time across a daylight saving gap", () => {
    expect(dateReminderDueAtMs("2027-03-28", "02:30", 0, "Europe/Madrid"))
      .toBe(Date.parse("2027-03-28T01:30:00.000Z"));
  });

  it("rejects malformed dates and times", () => {
    expect(dateReminderDueAtMs("2026-02-30", null, 0, "UTC")).toBeNull();
    expect(dateReminderDueAtMs("2026-10-20", "25:00", 0, "UTC")).toBeNull();
    expect(dateReminderDueAtMs("2026-10-20", "09:00", 0, "Not/AZone")).toBeNull();
  });
});
