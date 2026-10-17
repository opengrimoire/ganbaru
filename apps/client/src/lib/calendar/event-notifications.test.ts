import { describe, expect, it } from "vitest";
import {
  formatEventNotificationBody,
  formatEventNotificationLead,
  formatNotificationOffset,
} from "./event-notifications";
import { translate } from "$lib/i18n/translator.svelte";

describe("formatEventNotificationLead", () => {
  it("formats immediate starts", () => {
    expect(formatEventNotificationLead(0)).toBe("Starting now");
    expect(formatEventNotificationLead(-3)).toBe("Starting now");
  });

  it("formats minute lead times", () => {
    expect(formatEventNotificationLead(1)).toBe("Starts in 1 minute");
    expect(formatEventNotificationLead(15)).toBe("Starts in 15 minutes");
  });

  it("formats hour lead times", () => {
    expect(formatEventNotificationLead(60)).toBe("Starts in 1 hour");
    expect(formatEventNotificationLead(125)).toBe("Starts in 2 hours 5 minutes");
  });
});

describe("formatEventNotificationBody", () => {
  const now = new Date(2026, 4, 22, 9, 0);

  it("includes lead time, time range, and location", () => {
    expect(
      formatEventNotificationBody(
        {
          start: "2026-05-22 09:30",
          end: "2026-05-22 10:00",
          location: "Office",
        },
        now,
      ),
    ).toBe("Starts in 30 minutes\n09:30 - 10:00\nOffice");
  });

  it("includes the date for events outside today", () => {
    expect(
      formatEventNotificationBody(
        {
          start: "2026-05-23 11:00",
          end: "2026-05-23 12:00",
        },
        now,
      ),
    ).toBe("Starts in 1 day\n2026-05-23 11:00 - 12:00");
  });

  it("formats all-day events", () => {
    expect(
      formatEventNotificationBody(
        {
          start: "2026-05-22",
          end: "2026-05-22",
          allDay: true,
        },
        now,
      ),
    ).toBe("Starting now\nAll day");
  });

  it("normalizes multiline locations", () => {
    expect(
      formatEventNotificationBody(
        {
          start: "2026-05-22 09:10",
          end: "2026-05-22 09:20",
          location: "Room 1\nSecond floor",
        },
        now,
      ),
    ).toBe("Starts in 10 minutes\n09:10 - 09:20\nRoom 1 Second floor");
  });
});

describe("formatNotificationOffset", () => {
  it("names each offset by its largest whole unit", () => {
    expect([0, 5, 60, 90, 1440, 1500, 2880, 10080, 20160].map((minutes) => formatNotificationOffset(minutes, translate)))
      .toEqual([
        "At start",
        "5 minutes before",
        "1 hour before",
        "1h 30m before",
        "1 day before",
        "1d 1h before",
        "2 days before",
        "1 week before",
        "2 weeks before",
      ]);
  });
});
