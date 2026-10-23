import { describe, expect, it } from "vitest";
import { CalendarConflictError, mapCalendarConflicts, toCalendarConflictError } from "./calendar-conflicts";

const device = { deviceId: "device-a", deviceLabel: "Phone", ownDevice: false };

const timing = {
  kind: "timing",
  startTime: "2026-10-23T09:00:00Z",
  endTime: "2026-10-23T10:00:00Z",
  timezone: "Europe/Madrid",
  allDay: false,
  rrule: null,
  repeatUntil: null,
  excludedDates: 0,
  addedDates: 1,
  overrides: 2,
};

describe("calendar conflict mapping", () => {
  it("maps text and timing conflicts", () => {
    const read = mapCalendarConflicts({
      conflicts: [
        {
          eventId: "e1",
          field: "title",
          versions: [{ version: "ab", device, editedAtMs: 5, displayed: true, value: { kind: "text", text: "Standup" } }],
        },
        {
          eventId: "e1",
          field: "timing",
          versions: [
            { version: "cd", device, editedAtMs: 6, displayed: false, value: { ...timing, rrule: "FREQ=DAILY", repeatUntil: "2026-12-01T00:00:00Z" } },
          ],
        },
      ],
    });
    expect(read).toHaveLength(2);
    expect(read[0]?.versions[0]?.value).toEqual({ kind: "text", text: "Standup" });
    const value = read[1]?.versions[0]?.value;
    expect(value).toMatchObject({ kind: "timing", rrule: "FREQ=DAILY", addedDates: 1, overrides: 2 });
  });

  it("maps an empty read", () => {
    expect(mapCalendarConflicts({ conflicts: [] })).toEqual([]);
  });

  it("rejects a field the native side does not define", () => {
    expect(() => mapCalendarConflicts({ conflicts: [{ eventId: "e", field: "color", versions: [] }] }))
      .toThrow("calendar conflicts.conflicts[0].field has an unsupported value");
  });

  it("rejects a value kind that does not fit its field", () => {
    expect(() => mapCalendarConflicts({
      conflicts: [{
        eventId: "e",
        field: "timing",
        versions: [{ version: "v", device, editedAtMs: 1, displayed: true, value: { kind: "text", text: "x" } }],
      }],
    })).toThrow("does not fit the timing field");
  });

  it("rejects malformed timing values", () => {
    const withValue = (value: unknown) => ({
      conflicts: [{
        eventId: "e",
        field: "timing",
        versions: [{ version: "v", device, editedAtMs: 1, displayed: true, value }],
      }],
    });
    expect(() => mapCalendarConflicts(withValue({ ...timing, overrides: -1 }))).toThrow();
    expect(() => mapCalendarConflicts(withValue({ ...timing, allDay: "no" }))).toThrow();
    expect(() => mapCalendarConflicts(withValue({ ...timing, rrule: 3 }))).toThrow();
    expect(() => mapCalendarConflicts(withValue({ kind: "other" }))).toThrow("unsupported value");
  });

  it("rejects a malformed read", () => {
    expect(() => mapCalendarConflicts({ conflicts: {} })).toThrow();
    expect(() => mapCalendarConflicts(null)).toThrow();
  });
});

describe("calendar conflict errors", () => {
  it("turns known native codes into typed errors", () => {
    for (const code of ["resolved", "failed"] as const) {
      const error = toCalendarConflictError({ code, message: "text" });
      expect(error).toBeInstanceOf(CalendarConflictError);
      expect(error).toMatchObject({ code, message: "text" });
    }
  });

  it("passes other rejections through", () => {
    const unknownCode = { code: "would_cycle", message: "text" };
    expect(toCalendarConflictError(unknownCode)).toBe(unknownCode);
    expect(toCalendarConflictError("plain")).toBe("plain");
    const noMessage = { code: "failed", message: 4 };
    expect(toCalendarConflictError(noMessage)).toBe(noMessage);
  });
});
