import { Temporal } from "@js-temporal/polyfill";
import { describe, expect, it } from "vitest";
import {
  CHAT_NO_REPEAT,
  CHAT_WORKDAYS_MASK,
  chatLocalMinuteTime,
  chatRecurrenceFromRepeat,
  chatRepeatFromRecurrence,
  chatScheduledNextDelivery,
  chatTimeLocalMinute,
  chatWeekdayBit,
  chatWeekdaysOfMask,
  chatScheduleIsFuture,
  chatScheduleSuggestions,
  chatScheduleUtc,
  defaultChatScheduleSelection,
} from "./message-scheduling";
import {
  CHAT_DEFAULT_QUIET_WINDOW,
  addChatQuietWindow,
  chatQuietTimeMinutes,
  removeChatQuietWindow,
  updateChatQuietWindow,
} from "./quiet-periods";

describe("Chat message scheduling", () => {
  it("offers a rounded later-today choice plus distinct morning choices", () => {
    const suggestions = chatScheduleSuggestions(Temporal.PlainDateTime.from("2026-08-04T10:10"));

    expect(suggestions).toEqual([
      { kind: "later_today", date: "2026-08-04", time: "11:30" },
      { kind: "tomorrow_morning", date: "2026-08-05", time: "09:00" },
      { kind: "monday_morning", date: "2026-08-10", time: "09:00" },
    ]);
  });

  it("drops the later-today choice near the end of the day", () => {
    const suggestions = chatScheduleSuggestions(Temporal.PlainDateTime.from("2026-08-04T20:15"));

    expect(suggestions.map((suggestion) => suggestion.kind)).toEqual([
      "tomorrow_morning",
      "monday_morning",
    ]);
    expect(defaultChatScheduleSelection(Temporal.PlainDateTime.from("2026-08-04T23:45"))).toEqual({
      date: "2026-08-05",
      time: "01:00",
    });
  });

  it("does not duplicate tomorrow when tomorrow is Monday", () => {
    const suggestions = chatScheduleSuggestions(Temporal.PlainDateTime.from("2026-08-02T12:00"));

    expect(suggestions[1]).toMatchObject({ kind: "tomorrow_morning", date: "2026-08-03" });
    expect(suggestions[2]).toMatchObject({ kind: "monday_morning", date: "2026-08-10" });
  });

  it("converts local wall time to UTC and rejects invalid or past selections", () => {
    expect(chatScheduleUtc({ date: "2026-08-03", time: "09:00" }, "America/Monterrey"))
      .toBe("2026-08-03T15:00:00Z");
    expect(chatScheduleUtc({ date: "invalid", time: "09:00" }, "America/Monterrey"))
      .toBeNull();
    expect(chatScheduleIsFuture("2026-08-03T15:00:00Z", Date.parse("2026-08-03T14:59:00Z")))
      .toBe(true);
    expect(chatScheduleIsFuture("2026-08-03T15:00:00Z", Date.parse("2026-08-03T14:59:45Z")))
      .toBe(false);
  });
});

describe("Chat repeat options", () => {
  const selection = { date: "2026-11-13", time: "08:30" };
  const zone = "Europe/Madrid";

  it("maps ISO weekdays to mask bits and back in week order", () => {
    expect(chatWeekdayBit(1)).toBe(1);
    expect(chatWeekdayBit(7)).toBe(64);
    expect(chatWeekdaysOfMask(CHAT_WORKDAYS_MASK)).toEqual([1, 2, 3, 4, 5]);
    expect(chatWeekdaysOfMask(0b100_0001)).toEqual([1, 7]);
    expect(chatWeekdaysOfMask(0)).toEqual([]);
  });

  it("converts local minutes and times both ways and refuses invalid times", () => {
    expect(chatTimeLocalMinute("08:30")).toBe(510);
    expect(chatTimeLocalMinute("23:59")).toBe(1_439);
    expect(chatTimeLocalMinute("24:00")).toBeNull();
    expect(chatTimeLocalMinute("8:30")).toBeNull();
    expect(chatLocalMinuteTime(510)).toBe("08:30");
    expect(chatLocalMinuteTime(0)).toBe("00:00");
  });

  it("builds fixed time rules at the selected local minute with deferral during quiet periods", () => {
    expect(chatRecurrenceFromRepeat(CHAT_NO_REPEAT, selection, zone)).toBeNull();
    expect(chatRecurrenceFromRepeat({ ...CHAT_NO_REPEAT, choice: "daily" }, selection, zone)).toEqual({
      rule: { kind: "fixed_time", localMinute: 510, weekdays: 127 },
      timeZone: zone,
      quietPolicy: "defer",
    });
    expect(chatRecurrenceFromRepeat({ ...CHAT_NO_REPEAT, choice: "weekdays" }, selection, zone)?.rule)
      .toEqual({ kind: "fixed_time", localMinute: 510, weekdays: CHAT_WORKDAYS_MASK });
    // 2026-11-13 is a Friday.
    expect(chatRecurrenceFromRepeat({ ...CHAT_NO_REPEAT, choice: "weekly" }, selection, zone)?.rule)
      .toEqual({ kind: "fixed_time", localMinute: 510, weekdays: chatWeekdayBit(5) });
    expect(chatRecurrenceFromRepeat({ ...CHAT_NO_REPEAT, choice: "custom_days", weekdays: 0b110_0000 }, selection, zone)?.rule)
      .toEqual({ kind: "fixed_time", localMinute: 510, weekdays: 0b110_0000 });
  });

  it("refuses repeat selections the backend would refuse", () => {
    expect(chatRecurrenceFromRepeat({ ...CHAT_NO_REPEAT, choice: "custom_days", weekdays: 0 }, selection, zone)).toBeNull();
    expect(chatRecurrenceFromRepeat({ ...CHAT_NO_REPEAT, choice: "interval", intervalHours: 0 }, selection, zone)).toBeNull();
    expect(chatRecurrenceFromRepeat({ ...CHAT_NO_REPEAT, choice: "interval", intervalHours: 721 }, selection, zone)).toBeNull();
    expect(chatRecurrenceFromRepeat({ ...CHAT_NO_REPEAT, choice: "interval", intervalHours: 1.5 }, selection, zone)).toBeNull();
    expect(chatRecurrenceFromRepeat({ ...CHAT_NO_REPEAT, choice: "daily" }, { ...selection, time: "25:00" }, zone)).toBeNull();
  });

  it("builds interval rules in minutes", () => {
    expect(chatRecurrenceFromRepeat({ ...CHAT_NO_REPEAT, choice: "interval", intervalHours: 6 }, selection, zone))
      .toEqual({ rule: { kind: "interval", minutes: 360 }, timeZone: zone, quietPolicy: "defer" });
  });

  it("reads a rule back into the repeat control", () => {
    expect(chatRepeatFromRecurrence(null)).toEqual(CHAT_NO_REPEAT);
    const fixed = (weekdays: number) => ({ rule: { kind: "fixed_time" as const, localMinute: 0, weekdays }, timeZone: zone, quietPolicy: "defer" as const });
    expect(chatRepeatFromRecurrence(fixed(127)).choice).toBe("daily");
    expect(chatRepeatFromRecurrence(fixed(CHAT_WORKDAYS_MASK)).choice).toBe("weekdays");
    expect(chatRepeatFromRecurrence(fixed(0b101))).toEqual({ ...CHAT_NO_REPEAT, choice: "custom_days", weekdays: 0b101 });
    const interval = (minutes: number) => ({ rule: { kind: "interval" as const, minutes }, timeZone: zone, quietPolicy: "defer" as const });
    expect(chatRepeatFromRecurrence(interval(180))).toEqual({ ...CHAT_NO_REPEAT, choice: "interval", intervalHours: 3 });
    expect(chatRepeatFromRecurrence(interval(90)).intervalHours).toBe(2);
  });

  it("names the next delivery of a series, preferring a quiet period deferral", () => {
    const scheduledFor = "2026-11-13T07:30:00.000Z";
    expect(chatScheduledNextDelivery({ scheduledFor, recurrence: null })).toBe(scheduledFor);
    const recurrence = {
      rule: { kind: "interval" as const, minutes: 60 },
      timeZone: zone,
      quietPolicy: "defer" as const,
      nextOccurrenceAt: "2026-11-14T07:30:00.000Z",
      firedCount: 1,
      skippedOccurrences: 0,
      deferredUntil: null,
    };
    expect(chatScheduledNextDelivery({ scheduledFor, recurrence })).toBe(recurrence.nextOccurrenceAt);
    expect(chatScheduledNextDelivery({ scheduledFor, recurrence: { ...recurrence, deferredUntil: "2026-11-14T08:00:00.000Z" } }))
      .toBe("2026-11-14T08:00:00.000Z");
  });
});

describe("Chat quiet window editing", () => {
  it("offers quarter hours and keeps a stored minute between them", () => {
    const minutes = chatQuietTimeMinutes(0);
    expect(minutes).toHaveLength(96);
    expect(minutes.at(-1)).toBe(1_425);
    const withStored = chatQuietTimeMinutes(437);
    expect(withStored).toHaveLength(97);
    expect(withStored.indexOf(437)).toBe(withStored.indexOf(435) + 1);
  });

  it("adds windows up to the device limit", () => {
    let windows = addChatQuietWindow([]);
    expect(windows).toEqual([CHAT_DEFAULT_QUIET_WINDOW]);
    for (let count = 1; count < 8; count += 1) windows = addChatQuietWindow(windows ?? []);
    expect(windows).toHaveLength(8);
    expect(addChatQuietWindow(windows ?? [])).toBeNull();
  });

  it("refuses edits that make a window empty or start where it ends", () => {
    const windows = [CHAT_DEFAULT_QUIET_WINDOW, { weekdays: 1, startMinute: 60, endMinute: 120 }];
    expect(updateChatQuietWindow(windows, 1, { endMinute: 180 })?.[1]).toEqual({ weekdays: 1, startMinute: 60, endMinute: 180 });
    expect(updateChatQuietWindow(windows, 1, { endMinute: 60 })).toBeNull();
    expect(updateChatQuietWindow(windows, 0, { weekdays: 0 })).toBeNull();
    expect(updateChatQuietWindow(windows, 5, { weekdays: 1 })).toBeNull();
    expect(removeChatQuietWindow(windows, 0)).toEqual([windows[1]]);
  });
});
