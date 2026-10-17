import { describe, expect, it } from "vitest";
import type { NotesDateMentionRichText, NotesDateMentionValue } from "$lib/notes/types";
import { createDateMentionRichText } from "./core";
import {
  notesDateMentionEditorValue,
  notesDateMentionReminderMinutes,
  notesDateMentionReminderOffsets,
  notesDateMentionText,
  notesDateMentionValueWithDates,
  notesDateMentionWithReminderMinutes,
  notesDateMentionWithValue,
  type NotesDateMentionTextOptions,
} from "./date-mention-edit";

const OPTIONS: NotesDateMentionTextOptions = {
  locale: "en-US",
  timeFormat: "12h",
  labels: {
    remindTitle: (date) => `Remind ${date}`,
    range: (start, end) => `${start} to ${end}`,
  },
};

describe("notesDateMentionText", () => {
  it("spells a date in each format, using the full date by default", () => {
    const value: NotesDateMentionValue = { start: "2026-10-30" };
    expect(notesDateMentionText(value, OPTIONS)).toBe("October 30, 2026");
    expect(notesDateMentionText({ ...value, ganbaru_format: "short" }, OPTIONS)).toBe("Oct 30, 2026");
    expect(notesDateMentionText({ ...value, ganbaru_format: "numeric" }, OPTIONS)).toBe("10/30/2026");
    expect(notesDateMentionText({ ...value, ganbaru_format: "iso" }, OPTIONS)).toBe("2026-10-30");
  });

  it("adds times in the user's clock format, and as 24-hour time in ISO", () => {
    const value: NotesDateMentionValue = { start: "2026-10-30T15:05" };
    expect(notesDateMentionText(value, OPTIONS)).toBe("October 30, 2026 at 3:05 PM");
    expect(notesDateMentionText(value, { ...OPTIONS, timeFormat: "24h" })).toBe("October 30, 2026 at 15:05");
    expect(notesDateMentionText({ ...value, ganbaru_format: "iso" }, OPTIONS)).toBe("2026-10-30 15:05");
  });

  it("joins a range and wraps a reminder in its wording", () => {
    const value: NotesDateMentionValue = {
      start: "2026-10-30",
      end: "2026-11-02",
      ganbaru_format: "short",
      ganbaru_reminder: { enabled: true },
    };
    expect(notesDateMentionText(value, OPTIONS)).toBe("Remind Oct 30, 2026 to Nov 2, 2026");
    expect(notesDateMentionText({ ...value, ganbaru_reminder: { enabled: false } }, OPTIONS))
      .toBe("Oct 30, 2026 to Nov 2, 2026");
  });

  it("follows the active locale", () => {
    expect(notesDateMentionText({ start: "2026-10-30" }, { ...OPTIONS, locale: "es" })).toBe("30 de octubre de 2026");
  });
});

describe("notesDateMentionWithValue", () => {
  const today = createDateMentionRichText({ start: "2026-10-11", end: null, time_zone: null }, "Today");

  it("stores the default format on first edit and spells the chip text from the new value", () => {
    const edited = notesDateMentionWithValue(today, { ...today.mention.date, start: "2026-10-30T12:01" }, OPTIONS);
    expect(edited.mention.date).toEqual({ start: "2026-10-30T12:01", end: null, time_zone: null, ganbaru_format: "full" });
    expect(edited.plain_text).toBe("October 30, 2026 at 12:01 PM");
    expect(edited.annotations).toEqual(today.annotations);
  });

  it("keeps a chosen format and reminder", () => {
    const value: NotesDateMentionValue = { start: "2026-10-30", ganbaru_format: "iso", ganbaru_reminder: { enabled: true } };
    const edited: NotesDateMentionRichText = notesDateMentionWithValue(today, value, OPTIONS);
    expect(edited.plain_text).toBe("Remind 2026-10-30");
    expect(edited.mention.date.ganbaru_reminder).toEqual({ enabled: true });
  });
});

describe("date editor values", () => {
  it("round-trips the dates and keeps the mention's reminder and format", () => {
    const value: NotesDateMentionValue = { start: "2026-10-30", ganbaru_format: "short", ganbaru_reminder: { enabled: true } };
    const dates = notesDateMentionEditorValue(value);
    expect(dates).toEqual({ start: "2026-10-30", end: null, time_zone: null });
    expect(notesDateMentionValueWithDates(value, { ...dates, end: "2026-11-02" })).toEqual({
      start: "2026-10-30",
      end: "2026-11-02",
      time_zone: null,
      ganbaru_format: "short",
      ganbaru_reminder: { enabled: true },
    });
  });
});

describe("date mention reminder offsets", () => {
  const timed: NotesDateMentionValue = { start: "2026-10-30T15:00", ganbaru_reminder: { enabled: true } };

  it("offers minute offsets for times and whole days for dates", () => {
    expect(notesDateMentionReminderOffsets(timed)).toContain(30);
    expect(notesDateMentionReminderOffsets({ start: "2026-10-30" }).every((minutes) => minutes % 1440 === 0)).toBe(true);
  });

  it("stores an offset only when it is ahead of the date and leaves mentions without reminders alone", () => {
    const early = notesDateMentionWithReminderMinutes(timed, 30);
    expect(early.ganbaru_reminder).toEqual({ enabled: true, minutes_before: 30 });
    expect(notesDateMentionReminderMinutes(early)).toBe(30);
    expect(notesDateMentionWithReminderMinutes(early, 0).ganbaru_reminder).toEqual({ enabled: true });
    expect(notesDateMentionWithReminderMinutes({ start: "2026-10-30" }, 30)).toEqual({ start: "2026-10-30" });
  });

  it("keeps whole-day offsets when a time is removed but drops minute offsets", () => {
    const dateOnly = { start: "2026-10-30", end: null, time_zone: null };
    const halfHour = notesDateMentionWithReminderMinutes(timed, 30);
    expect(notesDateMentionValueWithDates(halfHour, dateOnly).ganbaru_reminder).toEqual({ enabled: true });
    const dayBefore = notesDateMentionWithReminderMinutes(timed, 1440);
    expect(notesDateMentionValueWithDates(dayBefore, dateOnly).ganbaru_reminder).toEqual({ enabled: true, minutes_before: 1440 });
  });

  it("keeps every date-only offset when a time is added", () => {
    const dateOnly: NotesDateMentionValue = { start: "2026-10-30", ganbaru_reminder: { enabled: true } };
    const withTime = { start: "2026-10-30T15:00", end: null, time_zone: null };
    for (const minutes of notesDateMentionReminderOffsets(dateOnly)) {
      const value = notesDateMentionWithReminderMinutes(dateOnly, minutes);
      expect(notesDateMentionReminderMinutes(notesDateMentionValueWithDates(value, withTime))).toBe(minutes);
    }
  });

  it("keeps a database date's reminder when its dates change, without adding a mention format", () => {
    const saved = { start: "2026-10-30T15:00", end: null, time_zone: null, ganbaru_reminder: { enabled: true, minutes_before: 1440 } };
    expect(notesDateMentionValueWithDates(saved, { start: "2026-11-02T08:00", end: "2026-11-02T09:00", time_zone: "America/Monterrey" }))
      .toEqual({ start: "2026-11-02T08:00", end: "2026-11-02T09:00", time_zone: "America/Monterrey", ganbaru_reminder: { enabled: true, minutes_before: 1440 } });
  });
});
