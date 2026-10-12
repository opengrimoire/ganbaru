import { beforeEach, describe, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import type { DbCalendarEvent } from "$lib/calendar/db-rows";
import { loadNativeCalendarWindow, mapNativeCalendarWindow, parseNativeCalendarWindow } from "./native-window";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("$lib/api/db", () => ({ ensureDbUrl: vi.fn(async () => "sqlite:native-window") }));

function event(): DbCalendarEvent {
  return {
    id: "series", title: "Home-zone series", start_time: "2024-03-09T14:00:00Z", end_time: "2024-03-09T15:00:00Z",
    timezone: "America/New_York", calendar_id: "calendar", project_id: null, environment_id: null, playlist_id: null,
    color: null, rrule: "FREQ=DAILY;COUNT=3", notifications: null, exceptions: null, repeat_until: null,
    all_day: 0, location: "", has_call_link: 0, meeting_enabled: 0, transparency: "opaque", status: "confirmed",
    local_rsvp_status: null, created_at: "2024-03-01T09:00:00Z", rdate: null, rhythm_kind: null,
    rhythm_source: null, preset_key: null, count_focus_duration_minutes: null, count_short_break_minutes: null,
    count_long_break_minutes: null, count_long_break_after_focus_count: null, sequence_steps: null, idle_timeout_minutes: null,
  };
}

function snapshot() {
  return {
    events: [event()], overrides: [], attendees: [], total_event_count: 1,
    occurrences: [{
      template_id: "series", id: "series::2024-03-11", recurring_parent_id: "series", recurrence_date: "2024-03-11",
      start_time: "2024-03-11T13:00:00.000Z", end_time: "2024-03-11T14:00:00.000Z", override_id: null,
    }],
    diagnostics: [],
  };
}

describe("native Calendar window boundary", () => {
  beforeEach(() => vi.clearAllMocks());

  it("requests one canonical snapshot without resending mapped frontend events", async () => {
    vi.mocked(invoke).mockResolvedValue(snapshot());
    const request = { windowStartDate: "2024-03-09", windowEndDate: "2024-03-15", renderZone: "America/New_York", includeTotalEventCount: true };
    const mapped = await loadNativeCalendarWindow(request);
    expect(invoke).toHaveBeenCalledExactlyOnceWith("calendar_load_native_window", { dbUrl: "sqlite:native-window", request });
    expect(mapped.windowEvents[0].start).toBe("2024-03-11 09:00");
    expect(mapped.windowEvents[0].recurrenceDate).toBe("2024-03-11");
  });

  it("requests the native Focus subset through the same one-request contract", async () => {
    vi.mocked(invoke).mockResolvedValue(snapshot());
    const request = { windowStartDate: "2024-03-09", windowEndDate: "2024-03-15", renderZone: "UTC", includeTotalEventCount: false };
    await loadNativeCalendarWindow(request, "focus");
    expect(invoke).toHaveBeenCalledExactlyOnceWith("calendar_load_native_focus_window", { dbUrl: "sqlite:native-window", request });
  });

  it("requests only canonical reminder sources without a frontend expansion round trip", async () => {
    vi.mocked(invoke).mockResolvedValue(snapshot());
    const request = { windowStartDate: "2024-03-09", windowEndDate: "2025-03-09", renderZone: "UTC", includeTotalEventCount: false };
    await loadNativeCalendarWindow(request, "notifications");
    expect(invoke).toHaveBeenCalledExactlyOnceWith("calendar_load_native_notification_window", { dbUrl: "sqlite:native-window", request });
  });

  it("changes rendered labels without changing native home-zone occurrence identity", () => {
    const input = snapshot();
    const newYork = mapNativeCalendarWindow(input, "America/New_York").windowEvents[0];
    const tokyo = mapNativeCalendarWindow(input, "Asia/Tokyo").windowEvents[0];
    expect([newYork.start, tokyo.start]).toEqual(["2024-03-11 09:00", "2024-03-11 22:00"]);
    expect(newYork.id).toBe(tokyo.id);
    expect(newYork.recurringParentId).toBe("series");
    expect(tokyo.recurrenceDate).toBe("2024-03-11");
    expect(tokyo.startInstant).toBe("2024-03-11T13:00:00.000Z");
    expect(tokyo.endInstant).toBe("2024-03-11T14:00:00.000Z");
  });

  it("uses only the native-selected override while retaining moved occurrence provenance", () => {
    const input = snapshot();
    const override = {
      id: "moved", parent_event_id: "series", recurrence_id: "2024-03-11", recurrence_range: null,
      title: "", start_time: "2024-03-20T13:00:00Z", end_time: "2024-03-20T14:00:00Z", color: null,
      status: "tentative", transparency: "transparent",
    };
    const mapped = mapNativeCalendarWindow({ ...input, overrides: [override], occurrences: [{ ...input.occurrences[0],
      override_id: "moved", start_time: "2024-03-20T13:00:00.000Z", end_time: "2024-03-20T14:00:00.000Z",
    }] }, "America/New_York").windowEvents[0];
    expect(mapped.id).toBe("series::2024-03-11");
    expect(mapped.recurrenceDate).toBe("2024-03-11");
    expect(mapped.start).toBe("2024-03-20 09:00");
    expect(mapped.title).toBe("");
    expect(mapped.status).toBe("tentative");
  });

  it.each([
    (input: ReturnType<typeof snapshot>) => ({ ...input, events: [{ ...input.events[0], id: "foreign" }] }),
    (input: ReturnType<typeof snapshot>) => ({ ...input, occurrences: [input.occurrences[0], input.occurrences[0]] }),
    (input: ReturnType<typeof snapshot>) => ({ ...input, occurrences: [{ ...input.occurrences[0], recurring_parent_id: "foreign" }] }),
    (input: ReturnType<typeof snapshot>) => ({ ...input, occurrences: [{ ...input.occurrences[0], override_id: "missing" }] }),
    (input: ReturnType<typeof snapshot>) => ({ ...input, occurrences: [{ ...input.occurrences[0], start_time: "2024-02-30T13:00:00.000Z" }] }),
    (input: ReturnType<typeof snapshot>) => ({ ...input, occurrences: [{ ...input.occurrences[0], end_time: "2024-03-11T12:00:00.000Z" }] }),
    (input: ReturnType<typeof snapshot>) => ({ ...input, occurrences: [{ ...input.occurrences[0], recurrence_date: "2024-02-30" }] }),
    (input: ReturnType<typeof snapshot>) => ({ ...input, attendees: [{ event_id: "foreign", email: "guest@example.test", status: "accepted" }] }),
    (input: ReturnType<typeof snapshot>) => ({ ...input, total_event_count: 0 }),
  ])("rejects invalid child ownership, identities, dates, or ranges before rendering", (corrupt) => {
    expect(() => parseNativeCalendarWindow(corrupt(snapshot()))).toThrow();
  });

  it("retains explicit unsupported-projection diagnostics with canonical rows", () => {
    const input = { ...snapshot(), occurrences: [], diagnostics: [{ event_id: "series", message: "BYHOUR is preservation-only" }] };
    const mapped = mapNativeCalendarWindow(input, "UTC");
    expect(mapped.windowEvents).toEqual([]);
    expect(mapped.sourceEvents).toHaveLength(1);
    expect(mapped.diagnostics).toEqual(input.diagnostics);
  });
});
