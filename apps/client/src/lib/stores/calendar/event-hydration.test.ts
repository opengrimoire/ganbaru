import { describe, expect, it } from "vitest";
import { slimEvent } from "./event-hydration";
import type { CalendarEvent } from "$lib/calendar/types";

function makeEvent(overrides: Partial<CalendarEvent> = {}): CalendarEvent {
  return {
    id: "event-a",
    title: "Focus",
    start: "2026-06-12 09:00",
    end: "2026-06-12 10:00",
    timezone: "America/Monterrey",
    calendarId: "local",
    ...overrides,
  };
}

describe("slimEvent", () => {
  it("preserves project automation identifiers", () => {
    const result = slimEvent(makeEvent({
      projectId: "project-a",
      environmentId: "environment-a",
      playlistId: "playlist-a",
    }));

    expect(result.projectId).toBe("project-a");
    expect(result.environmentId).toBe("environment-a");
    expect(result.playlistId).toBe("playlist-a");
  });
});
