import { describe, expect, it } from "vitest";
import type { CalendarEvent } from "$lib/calendar/types";
import type { EditSessionState } from "$lib/components/calendar/edit-session.svelte";
import {
  projectCalendarPanel,
  resolvePersistedCalendarPanelEvent,
  snapshotCalendarPanel,
  type PanelEditProjection,
} from "./panel-projection";

const anchor = { x: 1, y: 2, width: 3, height: 4 };

function event(id: string): CalendarEvent {
  return {
    id,
    title: id,
    start: "2026-07-12 09:00",
    end: "2026-07-12 10:00",
    timezone: "America/Monterrey",
    calendarId: "calendar-a",
  };
}

function editState(): Extract<EditSessionState, { mode: "edit" }> {
  const selected = event("event-a");
  return {
    mode: "edit",
    sessionKey: 4,
    originalEvent: selected,
    instanceEvent: selected,
    templateId: selected.id,
    detailsLoaded: true,
    anchor,
  };
}

function editProjection(overrides: Partial<PanelEditProjection> = {}): PanelEditProjection {
  return {
    selectedActive: false,
    recurring: false,
    detailsLoaded: true,
    locked: false,
    allowArchive: false,
    allowPomodoroWhenReadOnly: false,
    deleteWouldStopSession: false,
    endWouldStopProductivity: false,
    ...overrides,
  };
}

describe("calendar panel projection", () => {
  it("does not open a proposed-only preview identity as a persisted event", () => {
    expect(resolvePersistedCalendarPanelEvent("proposed-source::2026-07-13", [event("source")]))
      .toBeUndefined();
  });

  it("opens persisted metadata and geometry after discarding a preview", () => {
    const canonical = event("source");
    const proposed = { ...canonical, title: "Unsaved title", start: "2026-07-12 11:00" };
    expect(resolvePersistedCalendarPanelEvent(proposed.id, [canonical]))
      .toEqual(canonical);
  });

  it("hydrates source details while retaining native occurrence geometry and identity", () => {
    const canonical = {
      ...event("source::2026-07-12"),
      recurringParentId: "source",
      recurrenceDate: "2026-07-12",
      start: "2026-07-13 09:00",
      end: "2026-07-13 10:00",
      title: "Override title",
    };
    const full = { ...event("source"), description: "Persisted description" };
    expect(resolvePersistedCalendarPanelEvent(canonical.id, [canonical], full))
      .toEqual({ ...full, ...canonical });
  });

  it("rejects a stale full source and an occurrence removed during hydration", () => {
    const canonical = event("source");
    expect(resolvePersistedCalendarPanelEvent(canonical.id, [canonical], event("other")))
      .toBeUndefined();
    expect(resolvePersistedCalendarPanelEvent(canonical.id, [], canonical)).toBeUndefined();
  });

  it("parks a create session without losing its draft or anchor", () => {
    const state: EditSessionState = {
      mode: "create",
      sessionKey: 2,
      start: "2026-07-12 09:00",
      end: "2026-07-12 10:00",
      anchor,
    };
    const parked = snapshotCalendarPanel({
      state,
      changes: { title: "Draft", allDay: true },
      panelEvent: undefined,
    });

    expect(parked).toMatchObject({
      mode: "create",
      sessionKey: 2,
      initialCreateData: { title: "Draft", allDay: true },
      initialAllDay: true,
      anchor,
    });
  });

  it("locks an active edit to this occurrence and exposes end confirmation", () => {
    const state = editState();
    const render = projectCalendarPanel({
      hidden: false,
      state,
      changes: {},
      dirty: true,
      panelEvent: state.instanceEvent,
      parked: null,
      endingActiveEvent: false,
      edit: editProjection({
        selectedActive: true,
        recurring: true,
        locked: true,
        allowArchive: true,
      }),
    });

    expect(render).toMatchObject({
      mode: "edit",
      recurringScopeEnabled: false,
      externalDirty: true,
      readOnly: true,
      allowDeleteWhenReadOnly: true,
      endActiveEventAvailable: true,
      inlineEndEventConfirm: true,
    });
  });

  it("keeps a parked edit renderable while the live session is closed", () => {
    const state = editState();
    const parked = snapshotCalendarPanel({
      state,
      changes: {},
      panelEvent: state.instanceEvent,
      edit: editProjection({ locked: true, allowArchive: true }),
    });
    const render = projectCalendarPanel({
      hidden: false,
      state: { mode: "closed" },
      changes: {},
      dirty: false,
      panelEvent: undefined,
      parked,
      endingActiveEvent: false,
    });

    expect(render).toMatchObject({
      parked: true,
      mode: "edit",
      event: { id: "event-a" },
      readOnly: true,
    });
  });
});
