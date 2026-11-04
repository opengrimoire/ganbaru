import { describe, expect, it } from "vitest";
import { WINDOW_TEXT_CHARS } from "$lib/calendar/db-rows";
import type { CalendarEvent } from "$lib/calendar/types";
import { createEditSession, type EditSessionState } from "$lib/components/calendar/edit-session.svelte";
import { EventPanelSessionController } from "$lib/components/calendar/event-panel/session-controller.svelte";
import { buildNativeCalendarEdit } from "./native-edit-intent";
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

  it("restores a shortened window title from the full event", () => {
    const shortened = "T".repeat(WINDOW_TEXT_CHARS);
    const full = { ...event("event-a"), title: `${shortened} and the rest` };
    const canonical = { ...event("event-a"), title: shortened, windowTitleShortened: true };
    const resolved = resolvePersistedCalendarPanelEvent(canonical.id, [canonical], full);
    expect(resolved?.title).toBe(full.title);
    expect(resolved).not.toHaveProperty("windowTitleShortened");
  });

  it("restores a shortened window location from the full event", () => {
    const shortened = "L".repeat(WINDOW_TEXT_CHARS);
    const full = { ...event("event-a"), location: `${shortened} and the place` };
    const canonical = { ...event("event-a"), location: shortened, windowLocationShortened: true };
    const resolved = resolvePersistedCalendarPanelEvent(canonical.id, [canonical], full);
    expect(resolved?.location).toBe(full.location);
    expect(resolved).not.toHaveProperty("windowLocationShortened");
  });

  it("restores shortened text on an occurrence from its parent", () => {
    const shortened = "T".repeat(WINDOW_TEXT_CHARS);
    const full = { ...event("series"), title: `${shortened} and the rest` };
    const canonical = { ...event("series::2026-07-12"), title: shortened, recurringParentId: "series",
      windowTitleShortened: true };
    const resolved = resolvePersistedCalendarPanelEvent(canonical.id, [canonical], full);
    expect(resolved?.title).toBe(full.title);
    expect(resolved?.id).toBe(canonical.id);
  });

  it("opens the current full text when a remote edit changed the start of a shortened value", () => {
    const stale = "A".repeat(WINDOW_TEXT_CHARS);
    const full = { ...event("event-a"), title: "B".repeat(WINDOW_TEXT_CHARS * 2),
      location: "C".repeat(WINDOW_TEXT_CHARS * 2) };
    const canonical = { ...event("event-a"), title: stale, location: stale,
      windowTitleShortened: true, windowLocationShortened: true };
    const resolved = resolvePersistedCalendarPanelEvent(canonical.id, [canonical], full);
    expect(resolved?.title).toBe(full.title);
    expect(resolved?.location).toBe(full.location);
  });

  it("keeps a whole override title that is a prefix of the longer parent title", () => {
    const override = "T".repeat(WINDOW_TEXT_CHARS);
    const full = { ...event("series"), title: `${override} and the rest` };
    const canonical = { ...event("series::2026-07-12"), title: override, recurringParentId: "series",
      windowTitleShortened: false };
    expect(resolvePersistedCalendarPanelEvent(canonical.id, [canonical], full)?.title).toBe(override);
  });

  it("keeps whole window text of exactly the window length that the full event extends", () => {
    const whole = "T".repeat(WINDOW_TEXT_CHARS);
    const full = { ...event("series"), title: `${whole} and the rest`, location: `${whole} and the place` };
    const canonical = { ...event("series::2026-07-12"), title: whole, location: whole, recurringParentId: "series" };
    const resolved = resolvePersistedCalendarPanelEvent(canonical.id, [canonical], full);
    expect(resolved?.title).toBe(whole);
    expect(resolved?.location).toBe(whole);
  });

  it("keeps a differing occurrence override of exactly the window length", () => {
    const override = "O".repeat(WINDOW_TEXT_CHARS);
    const full = { ...event("series"), title: "T".repeat(WINDOW_TEXT_CHARS + 5), location: "L".repeat(WINDOW_TEXT_CHARS + 5) };
    const canonical = { ...event("series::2026-07-12"), title: override, location: override, recurringParentId: "series" };
    const resolved = resolvePersistedCalendarPanelEvent(canonical.id, [canonical], full);
    expect(resolved?.title).toBe(override);
    expect(resolved?.location).toBe(override);
  });

  it("keeps window text below the window length even when the full event extends it", () => {
    const whole = "T".repeat(WINDOW_TEXT_CHARS - 1);
    const full = { ...event("series"), title: `${whole} and the rest` };
    const canonical = { ...event("series::2026-07-12"), title: whole, recurringParentId: "series" };
    expect(resolvePersistedCalendarPanelEvent(canonical.id, [canonical], full)?.title).toBe(whole);
  });

  it("saves a typo fix over the full text on the open path", () => {
    const shortened = "T".repeat(WINDOW_TEXT_CHARS);
    const full = { ...event("event-a"), title: `${shortened} and the rest`, location: `${shortened} and the place`,
      meetingEnabled: true, recurrenceDate: "2026-07-12" };
    const canonical = { ...event("event-a"), title: shortened, location: shortened,
      windowTitleShortened: true, windowLocationShortened: true,
      meetingEnabled: true, recurrenceDate: "2026-07-12" };
    const hydrated = resolvePersistedCalendarPanelEvent(canonical.id, [canonical], full);
    if (!hydrated) throw new Error("the event did not hydrate");

    const session = createEditSession();
    session.openEdit(hydrated, anchor, undefined, true);
    expect(session.changes.title).toBe(full.title);
    expect(session.changes.location).toBe(full.location);
    expect(session.dirty).toBe(false);

    const panel = new EventPanelSessionController({
      projects: { projectById: () => undefined },
      controlsDisabled: () => false,
      lockStartControls: () => false,
      timeFormat: () => "24h",
      mode: () => "edit",
      preferences: () => ({ idlePauseEnabled: true, idleThresholdMinutes: 3 }),
      onChange: () => () => undefined,
    });
    if (session.state.mode !== "edit") throw new Error("the session did not open");
    panel.initializeEdit(session.state.originalEvent);
    expect(panel.title).toBe(full.title);
    expect(panel.location).toBe(full.location);

    const fixed = `${full.title}!`;
    session.updateChanges({ title: fixed });
    const edit = buildNativeCalendarEdit({ state: session.state, baseline: session.baseline,
      changes: session.changes, scope: "this", renderZone: "UTC" });
    expect(edit.draft.fields).toEqual([{ field: "title", value: fixed }]);
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
