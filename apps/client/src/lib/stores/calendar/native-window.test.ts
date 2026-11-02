import { beforeEach, describe, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { WINDOW_TEXT_CHARS, type DbCalendarEvent } from "$lib/calendar/db-rows";
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
    omitted_template_count: 0,
    shortened_text: [] as { event_id: string; title: boolean; location: boolean }[],
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
    (input: ReturnType<typeof snapshot>) => ({ ...input, shortened_text: [{ event_id: "foreign", title: true, location: false }] }),
    (input: ReturnType<typeof snapshot>) => ({ ...input, shortened_text: [{ event_id: "series", title: 1, location: false }] }),
    (input: ReturnType<typeof snapshot>) => ({ ...input, shortened_text: undefined }),
  ])("rejects invalid child ownership, identities, dates, or ranges before rendering", (corrupt) => {
    expect(() => parseNativeCalendarWindow(corrupt(snapshot()))).toThrow();
  });

  it("marks only the values the native window reports as shortened", () => {
    const long = "T".repeat(WINDOW_TEXT_CHARS);
    const input = { ...snapshot(), events: [{ ...event(), title: long, location: long }],
      shortened_text: [{ event_id: "series", title: true, location: true }] };
    const mapped = mapNativeCalendarWindow(input, "UTC");
    expect(mapped.sourceEvents[0]).toMatchObject({ windowTitleShortened: true, windowLocationShortened: true });
    expect(mapped.windowEvents[0]).toMatchObject({ windowTitleShortened: true, windowLocationShortened: true });

    const whole = mapNativeCalendarWindow({ ...input, shortened_text: [] }, "UTC");
    expect(whole.sourceEvents[0]).not.toHaveProperty("windowTitleShortened");
    expect(whole.windowEvents[0]).not.toHaveProperty("windowLocationShortened");
  });

  it("clears the title marker of an occurrence whose override supplies the title", () => {
    const input = snapshot();
    const override = {
      id: "renamed", parent_event_id: "series", recurrence_id: "2024-03-11", recurrence_range: null,
      title: "T".repeat(WINDOW_TEXT_CHARS), start_time: null, end_time: null, color: null, status: null, transparency: null,
    };
    const mapped = mapNativeCalendarWindow({ ...input, overrides: [override],
      occurrences: [{ ...input.occurrences[0], override_id: "renamed" }],
      shortened_text: [{ event_id: "series", title: true, location: true }] }, "UTC").windowEvents[0];
    expect(mapped.title).toBe(override.title);
    expect(mapped).not.toHaveProperty("windowTitleShortened");
    expect(mapped.windowLocationShortened).toBe(true);
  });

  it("retains explicit unsupported-projection diagnostics with canonical rows", () => {
    const input = { ...snapshot(), occurrences: [], diagnostics: [{ event_id: "series", message: "BYHOUR is preservation-only" }] };
    const mapped = mapNativeCalendarWindow(input, "UTC");
    expect(mapped.windowEvents).toEqual([]);
    expect(mapped.sourceEvents).toHaveLength(1);
    expect(mapped.diagnostics).toEqual(input.diagnostics);
  });

  it.each([-1, 1.5, "3", null, undefined])("rejects an omitted template count of %s", (count) => {
    expect(() => parseNativeCalendarWindow(JSON.stringify({ ...snapshot(), omitted_template_count: count })))
      .toThrow();
  });

  it("parses a window at the native text budget with its omitted template count", () => {
    const events = Array.from({ length: 10_000 }, (_, index) => ({
      ...event(), id: `event-${index}`, rrule: null, title: "T".repeat(WINDOW_TEXT_CHARS), location: "L".repeat(653),
    }));
    const occurrences = events.map((row) => ({
      template_id: row.id, id: row.id, recurring_parent_id: null, recurrence_date: "2024-03-09",
      start_time: "2024-03-09T14:00:00.000Z", end_time: "2024-03-09T15:00:00.000Z", override_id: null,
    }));
    const input = { ...snapshot(), events, occurrences, total_event_count: 10_003, omitted_template_count: 3 };
    const mapped = mapNativeCalendarWindow(input, "UTC");
    expect(mapped.windowEvents).toHaveLength(10_000);
    expect(mapped.omittedTemplateCount).toBe(3);
  });
});
