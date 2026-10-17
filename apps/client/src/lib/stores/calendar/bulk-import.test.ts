// @vitest-environment jsdom

import { describe, expect, it } from "vitest";
import type { CalendarEvent } from "$lib/calendar/types";
import type { IcsPreservationPayload } from "$lib/calendar/ics/types";
import { buildBulkImportPayload } from "./bulk-import";

const NOW = "2026-04-29 10:00:00";
const ZONE = "UTC";
const CALENDAR_ID = "cal-1";

function makeEvent(overrides: Partial<CalendarEvent> = {}): CalendarEvent {
  return {
    id: "ignored",
    title: "Imported event",
    start: "2026-05-01 10:00",
    end: "2026-05-01 11:00",
    timezone: ZONE,
    calendarId: CALENDAR_ID,
    sourceUid: "uid-1@example.com",
    sequence: 0,
    ...overrides,
  };
}

function deterministicIds(): () => string {
  let i = 0;
  return () => `new-${++i}`;
}

describe("buildBulkImportPayload", () => {
  it("builds a DB-ready payload with generated candidate ids", () => {
    const payload = buildBulkImportPayload(
      [
        makeEvent({ sourceUid: "a" }),
        makeEvent({ sourceUid: "b" }),
      ],
      CALENDAR_ID,
      NOW,
      ZONE,
      deterministicIds(),
    );

    expect(payload.targetCalendarId).toBe(CALENDAR_ID);
    expect(payload.now).toBe(NOW);
    expect(payload.events.map((event) => event.candidateId)).toEqual([
      "new-1",
      "new-2",
    ]);
    expect(payload.events.map((event) => event.sourceUid)).toEqual(["a", "b"]);
  });

  it("converts event fields into the Rust import DTO shape", () => {
    const payload = buildBulkImportPayload(
      [
        makeEvent({
          color: 4,
          description: "Details",
          notifications: [15, 30],
          exceptions: ["2026-05-02"],
          allDay: true,
          location: "Office",
          url: "https://example.com",
          transparency: "transparent",
          status: "tentative",
          visibility: "private",
          priority: 3,
          categories: ["work"],
          geo: { lat: 1, lng: 2 },
          rdate: ["2026-05-03"],
          extendedProperties: { "X-TEST": "yes" },
          organizer: { email: "owner@example.com", name: "Owner" },
          guestPermissions: {
            canModify: true,
            canInviteOthers: false,
            canSeeOtherGuests: false,
          },
        }),
      ],
      CALENDAR_ID,
      NOW,
      ZONE,
      deterministicIds(),
    );

    const event = payload.events[0];
    expect(event.color).toBe(4);
    expect(event.description).toBe("Details");
    expect(event.notifications).toBe("[15,30]");
    expect(event.exceptions).toBe("[\"2026-05-02\"]");
    expect(event.allDay).toBe(true);
    expect(event.location).toBe("Office");
    expect(event.url).toBe("https://example.com");
    expect(event.transparency).toBe("transparent");
    expect(event.status).toBe("tentative");
    expect(event.visibility).toBe("private");
    expect(event.priority).toBe(3);
    expect(event.categories).toBe("[\"work\"]");
    expect(event.geo).toBe("{\"lat\":1,\"lng\":2}");
    expect(event.rdate).toBe("[\"2026-05-03\"]");
    expect(event.extendedProperties).toBe("{\"X-TEST\":\"yes\"}");
    expect(event.organizer).toBe("{\"email\":\"owner@example.com\",\"name\":\"Owner\"}");
    expect(event.meetingEnabled).toBe(true);
    expect(event.guestCanModify).toBe(true);
    expect(event.guestCanInviteOthers).toBe(false);
    expect(event.guestCanSeeOtherGuests).toBe(false);
  });

  it("preserves an explicitly enabled empty meeting section", () => {
    const payload = buildBulkImportPayload(
      [makeEvent({ meetingEnabled: true })],
      CALENDAR_ID,
      NOW,
      ZONE,
      deterministicIds(),
    );

    const event = payload.events[0];
    expect(event.meetingEnabled).toBe(true);
    expect(event.location).toBe("");
    expect(event.url).toBe("");
    expect(event.attendees).toEqual([]);
  });

  it("maps attendees, alarms, and overrides without SQL assembly", () => {
    const payload = buildBulkImportPayload(
      [
        makeEvent({
          attendees: [
            {
              id: "att-1",
              name: "User",
              email: "user@example.com",
              role: "req-participant",
              status: "accepted",
              rsvp: true,
            },
          ],
          alarms: [
            {
              id: "alarm-1",
              action: "display",
              triggerType: "relative",
              triggerValue: "-PT15M",
              description: "Reminder",
            },
          ],
          overrides: [
            {
              id: "override-1",
              parentEventId: "ignored",
              recurrenceId: "2026-05-01T10:00:00Z",
              title: "Moved",
              start: "2026-05-01 12:00",
              end: "2026-05-01 13:00",
              color: 2,
              status: "confirmed",
              transparency: "opaque",
              visibility: "public",
              extendedProperties: { "X-OVERRIDE": "yes" },
            },
          ],
        }),
      ],
      CALENDAR_ID,
      NOW,
      ZONE,
      deterministicIds(),
    );

    const event = payload.events[0];
    expect(event.attendees).toEqual([
      {
        id: "att-1",
        icalendarComponentId: null,
        icalendarPropertyIndex: null,
        name: "User",
        email: "user@example.com",
        role: "req-participant",
        status: "accepted",
        rsvp: true,
      },
    ]);
    expect(event.alarms).toEqual([
      {
        id: "alarm-1",
        icalendarComponentId: null,
        action: "display",
        triggerType: "relative",
        triggerValue: "-PT15M",
        description: "Reminder",
      },
    ]);
    expect(event.overrides[0]).toMatchObject({
      id: "override-1",
      icalendarComponentId: null,
      recurrenceId: "2026-05-01T10:00:00Z",
      title: "Moved",
      color: 2,
      status: "confirmed",
      transparency: "opaque",
      visibility: "public",
      extendedProperties: "{\"X-OVERRIDE\":\"yes\"}",
    });
    expect(event.overrides[0].startTime).toBe("2026-05-01T12:00:00Z");
    expect(event.overrides[0].endTime).toBe("2026-05-01T13:00:00Z");
  });

  it("stores imported UTC events from the parser display zone without applying the event timezone twice", () => {
    const payload = buildBulkImportPayload(
      [
        makeEvent({
          title: "ABCD",
          timezone: "UTC",
          start: "2026-05-15 14:45",
          end: "2026-05-15 20:15",
        }),
      ],
      CALENDAR_ID,
      NOW,
      "America/Mexico_City",
      deterministicIds(),
    );

    expect(payload.events[0].startTime).toBe("2026-05-15T20:45:00Z");
    expect(payload.events[0].endTime).toBe("2026-05-16T02:15:00Z");
    expect(payload.events[0].timezone).toBe("UTC");
  });

  it("stores imported override wall clocks from the parser display zone", () => {
    const payload = buildBulkImportPayload(
      [
        makeEvent({
          timezone: "America/New_York",
          start: "2026-05-01 08:00",
          end: "2026-05-01 09:00",
          overrides: [
            {
              id: "override-1",
              parentEventId: "ignored",
              recurrenceId: "2026-05-02T14:00:00Z",
              start: "2026-05-02 09:30",
              end: "2026-05-02 10:30",
            },
          ],
        }),
      ],
      CALENDAR_ID,
      NOW,
      "America/Mexico_City",
      deterministicIds(),
    );

    expect(payload.events[0].startTime).toBe("2026-05-01T14:00:00Z");
    expect(payload.events[0].endTime).toBe("2026-05-01T15:00:00Z");
    expect(payload.events[0].overrides[0].startTime).toBe("2026-05-02T15:30:00Z");
    expect(payload.events[0].overrides[0].endTime).toBe("2026-05-02T16:30:00Z");
    expect(payload.events[0].timezone).toBe("America/New_York");
  });

  it("stores all-day override dates as floating date rows", () => {
    const payload = buildBulkImportPayload(
      [
        makeEvent({
          allDay: true,
          timezone: "Asia/Tokyo",
          start: "2026-05-01 00:00",
          end: "2026-05-01 00:00",
          overrides: [
            {
              id: "override-1",
              parentEventId: "ignored",
              recurrenceId: "2026-05-01T00:00:00Z",
              start: "2026-05-02 00:00",
              end: "2026-05-02 00:00",
            },
          ],
        }),
      ],
      CALENDAR_ID,
      NOW,
      "Asia/Tokyo",
      deterministicIds(),
    );

    const overrideRow = payload.events[0].overrides[0];
    expect(overrideRow.startTime).toBe("2026-05-02T00:00:00Z");
    expect(overrideRow.endTime).toBe("2026-05-02T00:00:00Z");
  });

  it("omits empty optional JSON fields", () => {
    const payload = buildBulkImportPayload(
      [makeEvent({ notifications: [], categories: [], extendedProperties: {} })],
      CALENDAR_ID,
      NOW,
      ZONE,
      deterministicIds(),
    );

    const event = payload.events[0];
    expect(event.notifications).toBeNull();
    expect(event.categories).toBeNull();
    expect(event.extendedProperties).toBeNull();
  });

  it("sanitizes imported event and override descriptions before persistence", () => {
    const payload = buildBulkImportPayload(
      [
        makeEvent({
          description:
            '<p onclick="alert(1)">Safe <strong>text</strong><script>alert(1)</script></p>',
          overrides: [
            {
              id: "override-1",
              parentEventId: "ignored",
              recurrenceId: "2026-05-01T10:00:00Z",
              description: '<img src=x onerror="alert(1)"><u>Override</u>',
            },
          ],
        }),
      ],
      CALENDAR_ID,
      NOW,
      ZONE,
      deterministicIds(),
    );

    const event = payload.events[0];
    expect(event.description).toBe("<p>Safe <strong>text</strong></p>");
    expect(event.overrides[0].description).toBe("<u>Override</u>");
  });

  it("adds structured iCalendar preservation data to the import payload", () => {
    const preservation: IcsPreservationPayload = {
      sourceFingerprint: "abc123",
      objects: [
        {
          prodid: "fixture",
          version: "2.0",
          method: "PUBLISH",
          calendarScale: "GREGORIAN",
          rawJcal: ["vcalendar", [], []],
          diagnostics: ["kept unsupported component"],
          components: [
            {
              componentType: "vtodo",
              uid: "todo@example.com",
              rawJcal: ["vtodo", [["uid", {}, "text", "todo@example.com"]], []],
              preservationStatus: "unsupported",
              projectionWarnings: [],
              components: [],
            },
          ],
        },
      ],
    };

    const payload = buildBulkImportPayload(
      [],
      CALENDAR_ID,
      NOW,
      ZONE,
      deterministicIds(),
      preservation,
      "tasks.ics",
      "import-file",
    );

    expect(payload.preservation?.sourceName).toBe("tasks.ics");
    expect(payload.preservation?.sourceFingerprint).toBe("abc123");
    expect(payload.preservation?.objects[0]).toMatchObject({
      id: "new-1",
      prodid: "fixture",
      version: "2.0",
      method: "PUBLISH",
      calendarScale: "GREGORIAN",
    });
    expect(payload.preservation?.objects[0].components[0]).toMatchObject({
      id: "new-2",
      componentType: "vtodo",
      uid: "todo@example.com",
      preservationStatus: "unsupported",
    });
    expect(payload.preservation?.objects[0].components[0].rawJcal).toContain("vtodo");
  });

  it("links projected rows to preserved iCalendar components", () => {
    const preservation: IcsPreservationPayload = {
      sourceFingerprint: "abc123",
      objects: [
        {
          rawJcal: ["vcalendar", [], []],
          diagnostics: [],
          components: [
            {
              componentType: "vevent",
              uid: "uid-1@example.com",
              rawJcal: [
                "vevent",
                [
                  ["uid", {}, "text", "uid-1@example.com"],
                  ["attendee", {}, "cal-address", "mailto:user@example.com"],
                ],
                [
                  [
                    "valarm",
                    [
                      ["action", {}, "text", "DISPLAY"],
                      ["trigger", {}, "duration", "-PT15M"],
                    ],
                    [],
                  ],
                ],
              ],
              preservationStatus: "partial",
              projectionWarnings: [],
              components: [
                {
                  componentType: "valarm",
                  rawJcal: [
                    "valarm",
                    [
                      ["action", {}, "text", "DISPLAY"],
                      ["trigger", {}, "duration", "-PT15M"],
                    ],
                    [],
                  ],
                  preservationStatus: "partial",
                  projectionWarnings: [],
                  components: [],
                },
              ],
            },
            {
              componentType: "vevent",
              uid: "uid-1@example.com",
              recurrenceId: "20260502T100000Z",
              rawJcal: [
                "vevent",
                [
                  ["uid", {}, "text", "uid-1@example.com"],
                  ["recurrence-id", {}, "date-time", "20260502T100000Z"],
                ],
                [],
              ],
              preservationStatus: "partial",
              projectionWarnings: [],
              components: [],
            },
          ],
        },
      ],
    };

    const payload = buildBulkImportPayload(
      [
        makeEvent({
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
              parentEventId: "ignored",
              recurrenceId: "2026-05-02T10:00:00Z",
            },
          ],
        }),
      ],
      CALENDAR_ID,
      NOW,
      ZONE,
      deterministicIds(),
      preservation,
      "meeting.ics",
      "import-file",
    );

    const event = payload.events[0];
    expect(event.icalendarComponentId).toBe("new-2");
    expect(event.attendees[0].icalendarComponentId).toBe("new-2");
    expect(event.attendees[0].icalendarPropertyIndex).toBe(1);
    expect(event.alarms[0].icalendarComponentId).toBe("new-3");
    expect(event.overrides[0].icalendarComponentId).toBe("new-4");
  });

  it("links duplicate master events to the highest sequence preserved component", () => {
    const preservation: IcsPreservationPayload = {
      sourceFingerprint: "abc123",
      objects: [
        {
          rawJcal: ["vcalendar", [], []],
          diagnostics: [],
          components: [
            {
              componentType: "vevent",
              uid: "duplicate@example.com",
              sequence: 0,
              rawJcal: ["vevent", [["uid", {}, "text", "duplicate@example.com"]], []],
              preservationStatus: "partial",
              projectionWarnings: [],
              components: [],
            },
            {
              componentType: "vevent",
              uid: "duplicate@example.com",
              sequence: 1,
              rawJcal: ["vevent", [["uid", {}, "text", "duplicate@example.com"]], []],
              preservationStatus: "partial",
              projectionWarnings: [],
              components: [],
            },
          ],
        },
      ],
    };

    const payload = buildBulkImportPayload(
      [makeEvent({ sourceUid: "duplicate@example.com" })],
      CALENDAR_ID,
      NOW,
      ZONE,
      deterministicIds(),
      preservation,
      "duplicate.ics",
      "import-file",
    );

    expect(payload.events[0].icalendarComponentId).toBe("new-3");
  });
});
