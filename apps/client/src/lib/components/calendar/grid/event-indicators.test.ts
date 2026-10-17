import { describe, expect, it } from "vitest";
import type { CalendarEvent } from "$lib/calendar/types";
import { getEventIndicatorState, getMeetingIndicatorState } from "./event-indicators";

function makeEvent(overrides: Partial<CalendarEvent> = {}): CalendarEvent {
  return {
    id: "event-1",
    title: "Meeting",
    start: "2026-05-15 10:00",
    end: "2026-05-15 11:00",
    timezone: "America/Monterrey",
    calendarId: "local",
    ...overrides,
  };
}

describe("getEventIndicatorState", () => {
  it("uses the generic meeting icon when meeting has no call link or location", () => {
    const state = getEventIndicatorState(makeEvent({ meetingEnabled: true }));

    expect(state.hasGenericMeeting).toBe(true);
    expect(state.hasCallLink).toBe(false);
    expect(state.hasLocation).toBe(false);
    expect(state.iconCount).toBe(1);
  });

  it("replaces the generic meeting icon with call and location icons", () => {
    const state = getEventIndicatorState(makeEvent({
      meetingEnabled: true,
      hasCallLink: true,
      location: "Room A",
    }));

    expect(state.hasGenericMeeting).toBe(false);
    expect(state.hasCallLink).toBe(true);
    expect(state.hasLocation).toBe(true);
    expect(state.iconCount).toBe(2);
  });

  it("counts repeat beside meeting detail icons", () => {
    const state = getEventIndicatorState(makeEvent({
      recurrence: { frequency: "daily", interval: 1, end: { type: "never" } },
      url: "https://meet.example.test",
      location: "Room A",
    }));

    expect(state.hasRepeat).toBe(true);
    expect(state.hasCallLink).toBe(true);
    expect(state.hasLocation).toBe(true);
    expect(state.iconCount).toBe(3);
  });

  it("exposes meeting indicators without counting repeat", () => {
    const state = getMeetingIndicatorState(makeEvent({
      recurrence: { frequency: "daily", interval: 1, end: { type: "never" } },
      url: "https://meet.example.test",
    }));

    expect(state.hasCallLink).toBe(true);
    expect(state.hasLocation).toBe(false);
    expect(state.hasGenericMeeting).toBe(false);
    expect(state.iconCount).toBe(1);
  });
});
