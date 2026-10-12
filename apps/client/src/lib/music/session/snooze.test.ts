import { describe, expect, it } from "vitest";
import { Temporal } from "@js-temporal/polyfill";
import { musicSnoozeEndsAt, musicSnoozePreset } from "./snooze";

describe("music Snooze expiry", () => {
  it("adds one local day across DST", () => {
    const now = Temporal.ZonedDateTime.from("2026-03-07T23:30:00-05:00[America/New_York]");
    const end = musicSnoozeEndsAt("day", now.epochMilliseconds, "America/New_York");
    expect(Temporal.Instant.fromEpochMilliseconds(end).toZonedDateTimeISO("America/New_York").toString())
      .toBe("2026-03-08T23:30:00-04:00[America/New_York]");
  });

  it("adds calendar months without turning them into a fixed number of days", () => {
    const now = Temporal.ZonedDateTime.from("2026-01-31T10:00:00-06:00[America/Monterrey]");
    const end = musicSnoozeEndsAt("month", now.epochMilliseconds, "America/Monterrey");
    expect(Temporal.Instant.fromEpochMilliseconds(end).toZonedDateTimeISO("America/Monterrey").toPlainDateTime().toString())
      .toBe("2026-02-28T10:00:00");
  });

  it("recognizes saved day, week, and month choices, but not other snoozes", () => {
    const now = Temporal.ZonedDateTime.from("2026-03-07T23:30:00-05:00[America/New_York]").epochMilliseconds;
    for (const duration of ["day", "week", "month"] as const) {
      expect(musicSnoozePreset(now, musicSnoozeEndsAt(duration, now, "America/New_York"), "America/New_York"))
        .toBe(duration);
    }
    expect(musicSnoozePreset(now, now + 3_600_000, "America/New_York"))
      .toBeNull();
    expect(musicSnoozePreset(now, null, "America/New_York")).toBeNull();
  });
});
