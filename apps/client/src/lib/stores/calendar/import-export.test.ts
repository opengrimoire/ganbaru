import { beforeEach, describe, expect, it, vi } from "vitest";
import ICAL from "ical.js";
import type { Calendar } from "$lib/calendar/types";
import type { DbFullEvent } from "./event-hydration";
import type { CalendarExportSnapshot } from "./export-snapshot";
import { parseCalendarExportSnapshot } from "./export-snapshot";
import { exportCalendarAsIcs } from "./import-export";

const mocks = vi.hoisted(() => ({ invoke: vi.fn(), localTimezone: vi.fn(() => "America/Monterrey") }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: mocks.invoke }));
vi.mock("$lib/api/db", () => ({ dbUrl: () => "sqlite:active-vault" }));
vi.mock("./event-payloads", async (importOriginal) => ({
  ...await importOriginal<typeof import("./event-payloads")>(),
  localTimezone: mocks.localTimezone,
}));

const calendar: Calendar = {
  id: "calendar", name: "Stale name", color: "blue", source: "local", visible: true, readOnly: false,
};

function event(id: string): DbFullEvent {
  return {
    id, title: `Current ${id}`, start_time: "2026-10-02T15:00:00Z", end_time: "2026-10-02T16:00:00Z",
    timezone: "UTC", calendar_id: "calendar", project_id: null, environment_id: null, playlist_id: null,
    color: null, rrule: null, notifications: null, exceptions: null, repeat_until: null, all_day: 0,
    location: "Room", meeting_enabled: 0, transparency: "opaque", status: "confirmed", local_rsvp_status: null,
    created_at: "2026-10-01T00:00:00Z", rdate: null, rhythm_kind: null, rhythm_source: null,
    preset_key: null, count_focus_duration_minutes: null, count_short_break_minutes: null,
    count_long_break_minutes: null, count_long_break_after_focus_count: null, sequence_steps: null,
    idle_timeout_minutes: null, description: "<p>Full description</p>", url: "https://example.test/meeting",
    source_uid: `uid-${id}`, visibility: "private", priority: 3, categories: '["Team"]', geo: null,
    sequence: 2, extended_properties: '{"X-CURRENT":"value"}', organizer: null,
    guest_can_modify: 0, guest_can_invite_others: 1, guest_can_see_other_guests: 1,
    icalendar_component_id: `component-${id}`, icalendar_preservation_status: "lossless",
    icalendar_projection_warnings: null,
    icalendar_raw_jcal: JSON.stringify(["vevent", [["x-preserved", {"x-param": "kept"}, "text", "original"]], []]),
  };
}

function snapshot(count = 1): CalendarExportSnapshot {
  return {
    calendar: { id: "calendar", name: "Current name", color: "blue", source: "local", source_url: null },
    events: Array.from({ length: count }, (_, index) => ({ event: event(`event-${index}`), attendees: [], alarms: [], overrides: [] })),
    timezones: [],
    passthrough_components: [["vtodo", [["uid", {}, "text", "todo"], ["summary", {}, "text", "Retained task"]], []]],
    metadata: { method: "REQUEST", mixed_methods: false },
  };
}

describe("Calendar export snapshot", () => {
  beforeEach(() => {
    vi.restoreAllMocks();
    mocks.invoke.mockReset();
    mocks.localTimezone.mockReset().mockReturnValue("America/Monterrey");
  });

  it.each([1, 40, 2_500])("exports %i full events with one IPC call and retains the existing preservation codec", async (count) => {
    const rows = snapshot(count);
    mocks.invoke.mockResolvedValue(rows);
    const text = await exportCalendarAsIcs(calendar);
    expect(mocks.invoke).toHaveBeenCalledExactlyOnceWith("calendar_load_export_snapshot", {
      dbUrl: "sqlite:active-vault", calendarId: "calendar",
    });
    const output = new ICAL.Component(ICAL.parse(text));
    expect(output.getFirstPropertyValue("x-wr-calname")).toBe("Current name");
    expect(output.getFirstPropertyValue("method")).toBe("REQUEST");
    expect(output.getAllSubcomponents("vevent")).toHaveLength(count);
    const first = output.getFirstSubcomponent("vevent");
    expect(first?.getFirstPropertyValue("summary")).toBe("Current event-0");
    expect(first?.getFirstPropertyValue("x-preserved")).toBe("original");
    expect(first?.getFirstProperty("x-preserved")?.getParameter("x-param")).toBe("kept");
    expect(first?.getFirstPropertyValue("x-current")).toBe("value");
    expect(first?.getFirstPropertyValue("class")).toBe("PRIVATE");
    expect(output.getFirstSubcomponent("vtodo")?.getFirstPropertyValue("summary")).toBe("Retained task");
    expect(text).toContain("DTSTART:20261002T150000Z");
    expect(mocks.localTimezone).toHaveBeenCalledTimes(1);
  });

  it("captures one render zone before the read even if the device zone changes while waiting", async () => {
    mocks.invoke.mockImplementation(async () => {
      mocks.localTimezone.mockReturnValue("Asia/Tokyo");
      return snapshot();
    });
    const text = await exportCalendarAsIcs(calendar);
    expect(text).toContain("DTSTART:20261002T150000Z");
    expect(mocks.localTimezone).toHaveBeenCalledTimes(1);
  });

  it("uses the current imported calendar source identity and reports mixed methods", async () => {
    const rows = snapshot(0);
    rows.calendar.source = "ics";
    rows.calendar.source_url = "/imports/Current source.ics";
    rows.metadata = { method: null, mixed_methods: true };
    mocks.invoke.mockResolvedValue(rows);
    const warning = vi.spyOn(console, "warn").mockImplementation(() => {});
    const output = new ICAL.Component(ICAL.parse(await exportCalendarAsIcs(calendar)));
    expect(output.getFirstPropertyValue("x-wr-calname")).toBe("Current source");
    expect(output.getFirstPropertyValue("method")).toBe("PUBLISH");
    expect(warning).toHaveBeenCalledOnce();
  });

  it("rejects missing full rows, invalid nested field types, duplicate events, and foreign ownership", () => {
    const good = snapshot();
    const rows = good.events[0];
    const invalid: unknown[] = [
      { ...good, events: [{ ...rows, event: null }] },
      { ...good, events: [{ ...rows, event: { ...rows.event, sequence: "2" } }] },
      { ...good, events: [rows, rows] },
      { ...good, events: [{ ...rows, event: { ...rows.event, calendar_id: "other" } }] },
      { ...good, calendar: { ...good.calendar, id: "other" } },
      { ...good, timezones: ["invalid"] },
      { ...good, metadata: { method: null, mixed_methods: "yes" } },
    ];
    for (const value of invalid) expect(() => parseCalendarExportSnapshot(value, "calendar")).toThrow(/Invalid Calendar export/);
  });

  it("propagates native snapshot failure without issuing partial fallback reads", async () => {
    mocks.invoke.mockRejectedValue(new Error("Calendar export exceeds the byte limit"));
    await expect(exportCalendarAsIcs(calendar)).rejects.toThrow("byte limit");
    expect(mocks.invoke).toHaveBeenCalledTimes(1);
  });
});
