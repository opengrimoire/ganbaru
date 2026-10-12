import { describe, expect, it } from "vitest";
import type { CalendarEvent } from "$lib/calendar/types";
import { namespaceImportChildIds } from "./calendar-import-ops-data";

function event(): CalendarEvent {
  return {
    id: "event-1",
    title: "Imported",
    start: "2026-05-01 10:00",
    end: "2026-05-01 11:00",
    timezone: "UTC",
    calendarId: "calendar-1",
    sourceUid: "source-uid-1",
    attendees: [
      {
        id: "att-1",
        email: "user@example.com",
        role: "req-participant",
        status: "needs-action",
        rsvp: false,
      },
    ],
    alarms: [
      {
        id: "alarm-1",
        action: "display",
        triggerType: "relative",
        triggerValue: "-PT15M",
      },
    ],
    overrides: [
      {
        id: "override-1",
        parentEventId: "event-1",
        recurrenceId: "2026-05-01T10:00:00Z",
      },
    ],
  };
}

describe("namespaceImportChildIds", () => {
  it("namespaces attendee, alarm, and override ids", () => {
    const [namespaced] = namespaceImportChildIds([event()], "bench-calendar");

    expect(namespaced.attendees?.[0]?.id).toBe("bench-calendar:attendee:att-1");
    expect(namespaced.alarms?.[0]?.id).toBe("bench-calendar:alarm:alarm-1");
    expect(namespaced.overrides?.[0]?.id).toBe("bench-calendar:override:override-1");
  });

  it("keeps namespace output deterministic and scoped", () => {
    const input = [event()];
    const first = namespaceImportChildIds(input, "a");
    const second = namespaceImportChildIds(input, "a");
    const third = namespaceImportChildIds(input, "b");

    expect(first).toEqual(second);
    expect(first[0].attendees?.[0]?.id).not.toBe(third[0].attendees?.[0]?.id);
  });

  it("does not change parent import identity fields", () => {
    const original = event();
    const [namespaced] = namespaceImportChildIds([original], "bench-calendar");

    expect(namespaced.id).toBe(original.id);
    expect(namespaced.sourceUid).toBe(original.sourceUid);
    expect(namespaced.title).toBe(original.title);
    expect(namespaced.start).toBe(original.start);
    expect(namespaced.end).toBe(original.end);
  });
});

