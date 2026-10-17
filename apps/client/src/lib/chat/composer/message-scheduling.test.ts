import { Temporal } from "@js-temporal/polyfill";
import { describe, expect, it } from "vitest";
import {
  chatScheduleIsFuture,
  chatScheduleSuggestions,
  chatScheduleUtc,
  defaultChatScheduleSelection,
} from "./message-scheduling";

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
