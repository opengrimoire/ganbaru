import { describe, expect, it, vi } from "vitest";
import type { CalendarEvent } from "$lib/calendar/types";
import type { EditSessionState } from "$lib/components/calendar/edit-session.svelte";
import { CalendarDragCommitController } from "./drag-commit-controller";

function event(id: string, start = "2026-07-12 09:00", end = "2026-07-12 10:00"): CalendarEvent {
  return { id, title: id, start, end, timezone: "UTC", calendarId: "calendar-a" };
}

function setup(state: EditSessionState = { mode: "closed" }, dirty = false) {
  const updateChanges = vi.fn();
  const openEdit = vi.fn();
  const updateEvent = vi.fn(async () => undefined);
  const confirmDiscard = vi.fn();
  const original = event("event-a");
  const controller = new CalendarDragCommitController({
    session: { state, dirty, updateChanges, openEdit },
    isCommitHidden: () => false,
    editingId: () => undefined,
    visibleEvents: () => [original],
    isRecurring: (candidate) => !!candidate.recurrence,
    isActivePomodoroEvent: () => false,
    panelAnchor: () => ({ x: 1, y: 2, width: 3, height: 4 }),
    loadPanel: async () => undefined,
    confirmDiscard,
    getTemplate: () => original,
    updateEvent,
    now: () => 42,
  });
  return { controller, updateChanges, openEdit, updateEvent, confirmDiscard, original };
}

describe("CalendarDragCommitController", () => {
  it("applies a following draft occurrence drag to the authored anchor without persisting a pending ID", async () => {
    const state: EditSessionState = { mode: "create", sessionKey: 1,
      start: "2026-07-12 09:00", end: "2026-07-12 10:00", anchor: { x: 0, y: 0, width: 0, height: 0 } };
    const anchor = event("__pending_create__");
    const following = event("__pending_create__::native::2026-07-13", "2026-07-13 09:00", "2026-07-13 10:00");
    const updateChanges = vi.fn();
    const updateEvent = vi.fn();
    const controller = new CalendarDragCommitController({
      session: { state, dirty: true, changes: { start: "2026-07-12 09:15", end: "2026-07-12 10:15" },
        openEdit: vi.fn(), updateChanges },
      isCommitHidden: () => false, editingId: () => anchor.id, visibleEvents: () => [anchor, following],
      isRecurring: () => true, isActivePomodoroEvent: () => false,
      panelAnchor: () => state.anchor, loadPanel: async () => undefined, confirmDiscard: vi.fn(),
      getTemplate: () => undefined, updateEvent,
    });
    await controller.handle({ ...following, start: "2026-07-13 23:30", end: "2026-07-14 00:30" });
    expect(updateChanges).toHaveBeenCalledWith({ start: "2026-07-12 23:45", end: "2026-07-13 00:45" });
    expect(updateEvent).not.toHaveBeenCalled();
  });

  it("routes an open edit drag through the session without persistence", async () => {
    const original = event("event-a");
    const state: EditSessionState = {
      mode: "edit",
      sessionKey: 1,
      originalEvent: original,
      instanceEvent: original,
      templateId: original.id,
      detailsLoaded: true,
      anchor: { x: 0, y: 0, width: 0, height: 0 },
    };
    const { controller, updateChanges, updateEvent } = setup(state);
    await controller.handle(event("event-a", "2026-07-12 11:00", "2026-07-12 12:00"));
    expect(updateChanges).toHaveBeenCalledWith({
      start: "2026-07-12 11:00",
      end: "2026-07-12 12:00",
    });
    expect(updateEvent).not.toHaveBeenCalled();
  });

  it("opens recurring drags in a session and preserves the original baseline", async () => {
    const recurring = event("event-a");
    recurring.recurrence = { frequency: "daily", interval: 1, end: { type: "never" } };
    const { openEdit, updateChanges } = setup();
    const optionsRecurring = new CalendarDragCommitController({
      session: { state: { mode: "closed" }, dirty: false, updateChanges, openEdit },
      isCommitHidden: () => false,
      editingId: () => undefined,
      visibleEvents: () => [recurring],
      isRecurring: () => true,
      isActivePomodoroEvent: () => false,
      panelAnchor: () => ({ x: 1, y: 2, width: 3, height: 4 }),
      loadPanel: async () => undefined,
      confirmDiscard: vi.fn(),
      getTemplate: () => recurring,
      updateEvent: vi.fn(async () => undefined),
    });
    await optionsRecurring.handle(event("event-a", "2026-07-13 09:00", "2026-07-13 10:00"));
    expect(openEdit).toHaveBeenCalledWith(recurring, expect.any(Object), recurring);
    expect(updateChanges).toHaveBeenCalledWith({
      start: "2026-07-13 09:00",
      end: "2026-07-13 10:00",
    });
  });

  it("does not persist a drag that ends at its original position", async () => {
    const { controller, original, updateEvent } = setup();
    await controller.handle({ ...original });
    expect(updateEvent).not.toHaveBeenCalled();
    expect(controller.lastDragEndTime).toBe(42);
  });
});
