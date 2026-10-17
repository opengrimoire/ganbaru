import { describe, it, expect, vi } from "vitest";
import {
  mapOverride,
  mapRow as mapRowImpl,
  type DbCalendarEvent,
  type DbOverride,
} from "./db-rows";

// Tests fix the render zone so wall-clock-shaped inputs round-trip with the
// stored string. UTC conversion coverage lives in utils.test.ts.
const TEST_ZONE = "America/New_York";
function mapRow(r: DbCalendarEvent) {
  return mapRowImpl(r, TEST_ZONE);
}

function makeDbRow(overrides: Partial<DbCalendarEvent> = {}): DbCalendarEvent {
  return {
    id: "evt-1",
    title: "Test event",
    start_time: "2026-03-15 09:00:00",
    end_time: "2026-03-15 10:00:00",
    timezone: "America/New_York",
    calendar_id: "local",
    project_id: null,
    environment_id: null,
    playlist_id: null,
    color: null,
    rrule: null,
    notifications: null,
    exceptions: null,
    repeat_until: null,
    all_day: 0,
    location: "",
    has_call_link: 0,
    meeting_enabled: 0,
    transparency: "opaque",
    status: "confirmed",
    local_rsvp_status: null,
    created_at: "2026-03-15T08:00:00Z",
    rdate: null,
    rhythm_kind: null,
    rhythm_source: null,
    preset_key: null,
    count_focus_duration_minutes: null,
    count_short_break_minutes: null,
    count_long_break_minutes: null,
    count_long_break_after_focus_count: null,
    sequence_steps: null,
    idle_timeout_minutes: null,
    ...overrides,
  };
}

function makeDbOverride(overrides: Partial<DbOverride> = {}): DbOverride {
  return {
    id: "override-1",
    parent_event_id: "evt-1",
    recurrence_id: "2026-05-01T00:00:00Z",
    recurrence_range: null,
    title: null,
    start_time: "2026-05-01T00:00:00Z",
    end_time: "2026-05-01T00:00:00Z",
    color: null,
    status: null,
    transparency: null,
    ...overrides,
  };
}

describe("mapRow", () => {
  it("maps basic fields correctly", () => {
    const result = mapRow(makeDbRow());
    expect(result.id).toBe("evt-1");
    expect(result.title).toBe("Test event");
    expect(result.start).toBe("2026-03-15 09:00");
    expect(result.end).toBe("2026-03-15 10:00");
    expect(result.timezone).toBe("America/New_York");
    expect(result.calendarId).toBe("local");
    expect(result.createdAt).toBe("2026-03-15T08:00:00Z");
  });

  it("maps project automation identifiers", () => {
    const result = mapRow(makeDbRow({
      project_id: "project-a",
      environment_id: "environment-a",
      playlist_id: "playlist-a",
    }));
    expect(result.projectId).toBe("project-a");
    expect(result.environmentId).toBe("environment-a");
    expect(result.playlistId).toBe("playlist-a");
  });

  it("preserves non-zero seconds from canonical UTC event times", () => {
    const result = mapRow(makeDbRow({
      start_time: "2026-03-15T13:30:00Z",
      end_time: "2026-03-15T14:30:45Z",
    }));

    expect(result.start).toBe("2026-03-15 09:30");
    expect(result.end).toBe("2026-03-15 10:30:45");
  });

  it("returns undefined for default values", () => {
    const result = mapRow(makeDbRow());
    expect(result.color).toBeUndefined();
    expect(result.recurrence).toBeUndefined();
    expect(result.notifications).toBeUndefined();
    expect(result.exceptions).toBeUndefined();
    expect(result.allDay).toBeUndefined();
    expect(result.location).toBeUndefined();
    expect(result.transparency).toBeUndefined();
    expect(result.status).toBeUndefined();
    expect(result.pomodoroConfig).toBeUndefined();
    expect(result.rdate).toBeUndefined();
    expect(result.meetingEnabled).toBeUndefined();
  });

  it("maps enabled empty meeting state", () => {
    const result = mapRow(makeDbRow({ meeting_enabled: 1 }));
    expect(result.meetingEnabled).toBe(true);
  });

  it("maps local RSVP status", () => {
    const result = mapRow(makeDbRow({ local_rsvp_status: "tentative" }));
    expect(result.localParticipationStatus).toBe("tentative");
  });

  it("omits heavy fields entirely (loaded on demand by loadFullEvent)", () => {
    const result = mapRow(makeDbRow());
    // These keys must be absent (not just undefined) so spreading the slim
    // event into a patch payload does not accidentally clear DB columns.
    expect("description" in result).toBe(false);
    expect("url" in result).toBe(false);
    expect("sourceUid" in result).toBe(false);
    expect("visibility" in result).toBe(false);
    expect("priority" in result).toBe(false);
    expect("categories" in result).toBe(false);
    expect("geo" in result).toBe(false);
    expect("sequence" in result).toBe(false);
    expect("extendedProperties" in result).toBe(false);
    expect("organizer" in result).toBe(false);
    expect("attendees" in result).toBe(false);
    expect("alarms" in result).toBe(false);
    expect("guestPermissions" in result).toBe(false);
  });

  it("deserializes rdate JSON", () => {
    const result = mapRow(makeDbRow({ rdate: '["2026-04-01","2026-05-01"]' }));
    expect(result.rdate).toEqual(["2026-04-01", "2026-05-01"]);
  });

  it("handles malformed JSON gracefully", () => {
    const result = mapRow(makeDbRow({
      rdate: "{{",
      notifications: "[invalid",
      exceptions: "x",
    }));
    expect(result.rdate).toBeUndefined();
    expect(result.notifications).toBeUndefined();
    expect(result.exceptions).toBeUndefined();
  });

  it("rejects valid JSON with incorrect calendar field shapes", () => {
    const result = mapRow(makeDbRow({
      rdate: '"2026-04-01"',
      notifications: '[15,"60"]',
      exceptions: '{"date":"2026-04-01"}',
    }));
    expect(result.rdate).toBeUndefined();
    expect(result.notifications).toBeUndefined();
    expect(result.exceptions).toBeUndefined();
  });

  it("maps all-day events", () => {
    const result = mapRow(makeDbRow({ all_day: 1 }));
    expect(result.allDay).toBe(true);
  });

  it("maps transparency", () => {
    expect(mapRow(makeDbRow({ transparency: "opaque" })).transparency).toBeUndefined();
    expect(mapRow(makeDbRow({ transparency: "transparent" })).transparency).toBe("transparent");
  });

  it("maps non-default status", () => {
    expect(mapRow(makeDbRow({ status: "confirmed" })).status).toBeUndefined();
    expect(mapRow(makeDbRow({ status: "tentative" })).status).toBe("tentative");
    expect(mapRow(makeDbRow({ status: "cancelled" })).status).toBe("cancelled");
  });

  it("maps pomodoro config when present", () => {
    const result = mapRow(makeDbRow({
      rhythm_kind: "count",
      rhythm_source: "preset",
      preset_key: "deep",
      count_focus_duration_minutes: 25,
      count_short_break_minutes: 5,
      count_long_break_minutes: 15,
      count_long_break_after_focus_count: 4,
      idle_timeout_minutes: 1,
    }));
    expect(result.pomodoroConfig).toEqual({
      rhythm: {
        kind: "count",
        focusDurationMinutes: 25,
        shortBreakMinutes: 5,
        longBreakMinutes: 15,
        longBreakAfterFocusCount: 4,
      },
      rhythmSource: "preset",
      presetKey: "deep",
      idleTimeoutMinutes: 1,
    });
  });

  it("maps color", () => {
    expect(mapRow(makeDbRow({ color: 2 })).color).toBe(2);
    expect(mapRow(makeDbRow({ color: null })).color).toBeUndefined();
  });

  it("drops out-of-range color values so render falls back to the default", () => {
    const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
    try {
      expect(mapRow(makeDbRow({ color: 99 })).color).toBeUndefined();
    } finally {
      warn.mockRestore();
    }
  });

  it("maps location when non-empty", () => {
    const result = mapRow(makeDbRow({ location: "Room A" }));
    expect(result.location).toBe("Room A");
  });

  it("maps call link presence without loading the URL", () => {
    const result = mapRow(makeDbRow({ has_call_link: 1 }));
    expect(result.hasCallLink).toBe(true);
    expect(result.url).toBeUndefined();
  });
});

describe("mapOverride", () => {
  it("maps all-day override dates without zone shifting", () => {
    const result = mapOverride(makeDbOverride(), TEST_ZONE, true);
    expect(result.start).toBe("2026-05-01 00:00");
    expect(result.end).toBe("2026-05-01 00:00");
  });

  it("maps recurrence range", () => {
    const result = mapOverride(makeDbOverride({ recurrence_range: "this-and-future" }), TEST_ZONE, true);
    expect(result.recurrenceRange).toBe("this-and-future");
  });
});
