import { describe, expect, it } from "vitest";
import type { Calendar } from "$lib/calendar/types";
import {
  calendarDisplayName,
  calendarIdentityEmail,
  calendarImportDate,
} from "./display";

function makeCalendar(overrides: Partial<Calendar> = {}): Calendar {
  return {
    id: "cal-1",
    name: "Ganbaru AI",
    color: "",
    source: "local",
    visible: true,
    readOnly: false,
    ...overrides,
  };
}

const expectedDateTimeFormatter = new Intl.DateTimeFormat(undefined, {
  day: "numeric",
  month: "short",
  year: "numeric",
  hour: "numeric",
  minute: "2-digit",
});

function expectedLocalTimestamp(value: string): string {
  return expectedDateTimeFormatter.format(new Date(value));
}

describe("calendar display helpers", () => {
  it("uses the .ics source filename as the imported calendar label", () => {
    const calendar = makeCalendar({
      name: "Stored calendar name",
      source: "ics",
      sourceUrl: "person@example.com.ics",
    });

    expect(calendarDisplayName(calendar)).toBe("person@example.com");
  });

  it("uses the stored name when an imported source URL is unavailable", () => {
    const calendar = makeCalendar({
      name: "person@example.com",
      source: "ics",
    });

    expect(calendarDisplayName(calendar)).toBe("person@example.com");
    expect(calendarImportDate(calendar)).toBeUndefined();
  });

  it("formats imported timestamps in the local date and time", () => {
    const timestamp = "2026-05-26T00:33:21.944Z";
    const calendar = makeCalendar({
      createdAt: timestamp,
      source: "ics",
      sourceUrl: "person@example.com.ics",
    });

    expect(calendarImportDate(calendar)).toBe(expectedLocalTimestamp(timestamp));
    expect(calendarImportDate(calendar)).not.toBe(timestamp);
  });

  it("derives an identity email from the source filename", () => {
    const calendar = makeCalendar({
      source: "ics",
      sourceUrl: "person@example.com.ics",
    });

    expect(calendarIdentityEmail(calendar)).toBe("person@example.com");
  });

  it("does not derive identities from non-email imported calendar names", () => {
    const calendar = makeCalendar({
      name: "work",
      source: "ics",
      sourceUrl: "work.ics",
    });

    expect(calendarIdentityEmail(calendar)).toBeUndefined();
  });
});
