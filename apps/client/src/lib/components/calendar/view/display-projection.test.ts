import { describe, expect, it } from "vitest";
import type { CalendarEvent } from "$lib/calendar/types";
import type { CalendarEditPreview } from "$lib/api/calendar-edit";
import type { EditSessionState } from "$lib/components/calendar/edit-session.svelte";
import { PENDING_CREATE_ID } from "$lib/components/calendar/display-events";
import { computeViewWindow } from "$lib/calendar/utils";
import {
  projectCalendarSurfaceStatuses,
  projectCalendarDisplay,
  buildCalendarSaveFreeze,
  visibleCalendarEvents,
} from "./display-projection";

function event(id: string, calendarId = "calendar-a"): CalendarEvent {
  return {
    id,
    title: id,
    start: "2026-07-12 09:00",
    end: "2026-07-12 10:00",
    timezone: "UTC",
    calendarId,
  };
}

describe("visibleCalendarEvents", () => {
  it("applies calendar visibility before the route filter", () => {
    const events = [event("keep"), event("filtered"), event("hidden", "calendar-b")];
    expect(visibleCalendarEvents({
      events,
      visibleCalendarIds: new Set(["calendar-a"]),
      filter: (candidate) => candidate.id !== "filtered",
    }).map((candidate) => candidate.id)).toEqual(["keep"]);
  });
});

describe("native Calendar edit display", () => {
  function input() {
    const selected = { ...event("source::2026-07-12"), recurringParentId: "source", recurrenceDate: "2026-07-12" };
    const state: EditSessionState = { mode: "edit", sessionKey: 1,
      originalEvent: selected, instanceEvent: selected, templateId: "source", detailsLoaded: true,
      anchor: { x: 0, y: 0, width: 0, height: 0 } };
    return { storeEvents: [selected, { ...selected, id: "source::2026-07-13" }, event("unrelated")],
      frozenEvents: null, state, createPreview: null, changes: { title: "Typing", start: "2026-07-12 09:15" },
      dirty: true, nativePreview: null as CalendarEditPreview | null,
      window: computeViewWindow(new Date(2026, 6, 12), "week"), suppressEditPreview: false };
  }

  it("replaces only the source family with the native window and keeps unrelated events", () => {
    const value = input();
    const nativeEvent = event("native-detach");
    value.nativePreview = { vaultId: "vault", vaultGeneration: 1, commandId: "command",
      sourceId: "source", editedId: nativeEvent.id, reviewRevision: "a".repeat(64), changed: true,
      scope: { effectiveScope: "this", selectedStarted: false, selectedHasHistory: false, selectedActive: false },
      window: { sourceEvents: [nativeEvent], windowEvents: [nativeEvent], totalEventCount: null, diagnostics: [] },
      previewedIds: new Set([nativeEvent.id]), editingId: nativeEvent.id };
    const result = projectCalendarDisplay(value);
    expect(result.events.map((candidate) => candidate.id)).toEqual(["unrelated", "native-detach"]);
    expect(result.editingId).toBe("native-detach");
    expect(result.previewedIds).toEqual(new Set(["native-detach"]));
  });

  it("overlays the selected card immediately without changing sibling occurrences", () => {
    const value = input();
    const result = projectCalendarDisplay(value);
    expect(result.events[0]).toMatchObject({ title: "Typing", start: "2026-07-12 09:15", id: "source::2026-07-12" });
    expect(result.events[1]).toBe(value.storeEvents[1]);
    expect(value.storeEvents[0].title).toBe("source::2026-07-12");
  });

  it("freezes the displayed projection without recomputing a recurrence plan", () => {
    const value = input();
    const projected = projectCalendarDisplay(value);
    const frozen = buildCalendarSaveFreeze(projected.events);
    expect(frozen).toEqual(projected.events);
    expect(frozen[0]).not.toBe(projected.events[0]);
    expect(projectCalendarDisplay({ ...value, frozenEvents: frozen }).events).toBe(frozen);
  });

  it("renders the native creation family beside a coincident event and deduplicates only its accepted identity", () => {
    const value = input();
    const created = event("native-created");
    const following = { ...created, id: "native-created::2026-07-13", recurringParentId: created.id,
      start: "2026-07-13 09:00", end: "2026-07-13 10:00" };
    const nativePreview: CalendarEditPreview = { vaultId: "vault", vaultGeneration: 1, commandId: "create",
      sourceId: created.id, editedId: created.id, reviewRevision: "a".repeat(64), changed: true,
      scope: { effectiveScope: "this", selectedStarted: false, selectedHasHistory: false, selectedActive: false },
      window: { sourceEvents: [created], windowEvents: [created, following], totalEventCount: null, diagnostics: [] },
      previewedIds: new Set([created.id, following.id]), editingId: created.id };
    const state: EditSessionState = { mode: "create", sessionKey: 2, start: created.start, end: created.end,
      anchor: { x: 0, y: 0, width: 0, height: 0 } };
    const coincident = event("coincident");
    const projected = projectCalendarDisplay({ ...value, state, nativePreview, storeEvents: [coincident] });
    expect(projected.events.map((event) => event.id)).toEqual([coincident.id, PENDING_CREATE_ID,
      `${PENDING_CREATE_ID}::${following.id}`]);
    expect(projected.previewedIds.size).toBe(2);
    expect(projected.editingId).toBe(PENDING_CREATE_ID);
    expect(nativePreview.window.windowEvents[0].id).toBe(created.id);
    const accepted = [coincident, created, following];
    expect(projectCalendarDisplay({ ...value, state, nativePreview, storeEvents: accepted }).events).toBe(accepted);
  });
});

describe("projectCalendarSurfaceStatuses", () => {
  it("gives an in-panel RSVP preview priority over stored attendee status", () => {
    const selected = event("event-a");
    selected.attendees = [{
      id: "attendee-a",
      email: "me@example.com",
      role: "req-participant",
      status: "accepted",
      rsvp: true,
    }];
    const [projected] = projectCalendarSurfaceStatuses({
      events: [selected],
      identityByCalendarId: new Map([["calendar-a", "me@example.com"]]),
      pendingStatus: "declined",
      pendingEventId: selected.id,
    });
    expect(projected.surfaceStatus).toBe("declined");
  });
});
