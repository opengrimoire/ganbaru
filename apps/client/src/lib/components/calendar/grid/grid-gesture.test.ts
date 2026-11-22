import { describe, expect, it } from "vitest";
import type { DragState, PositionedEvent } from "$lib/calendar/types";
import {
  IDLE_GRID_GESTURE,
  gestureCreatePreview,
  gestureDrag,
  gestureDragPreview,
  transitionGridGesture,
  type CreateGestureOrigin,
  type GridGesture,
  type GridGestureEvent,
} from "./grid-gesture";

const drag: DragState = {
  eventId: "event-1",
  type: "move",
  originDate: "2026-11-20",
  startColumnDate: "2026-11-20",
  originStartMinute: 540,
  originEndMinute: 600,
  pointerStartX: 10,
  pointerStartY: 20,
  scrollTopAtStart: 0,
};

const create: CreateGestureOrigin = {
  dateStr: "2026-11-21",
  anchorMinute: 600,
  clickStartMinute: 600,
  columnEl: {} as HTMLElement,
  pointerStartX: 5,
  pointerStartY: 5,
};

function preview(id: string, startMinute: number): PositionedEvent {
  return {
    event: { id, title: "", start: "2026-11-20 09:00", end: "2026-11-20 10:00", timezone: "UTC", calendarId: "default" },
    startMinute,
    durationMinutes: 60,
    left: 0,
    width: 100,
    column: 0,
    totalColumns: 1,
  };
}

function run(events: GridGestureEvent[], from: GridGesture = IDLE_GRID_GESTURE): GridGesture {
  return events.reduce(transitionGridGesture, from);
}

const pressed = run([{ type: "eventPressed", drag }]);
const dragging = run([{ type: "dragThresholdPassed" }], pressed);
const previewed = run([{ type: "dragPreviewed", preview: preview("event-1", 600) }], dragging);
const committing = run([{ type: "dragReleased" }], previewed);
const createPending = run([{ type: "createPressed", create }]);
const createSelecting = run([{ type: "createPreviewed", preview: preview("__create__", 600) }], createPending);

describe("transitionGridGesture event drags", () => {
  it("presses, drags past the threshold, previews, and commits the preview", () => {
    expect(pressed).toEqual({ phase: "eventPressed", drag });
    expect(dragging).toEqual({ phase: "eventDragging", drag, preview: null });
    expect(previewed).toEqual({ phase: "eventDragging", drag, preview: preview("event-1", 600) });
    expect(committing).toEqual({ phase: "eventCommitting", drag, preview: preview("event-1", 600) });
    expect(transitionGridGesture(committing, { type: "dragSettled" })).toEqual(IDLE_GRID_GESTURE);
  });

  it("replaces the preview as the pointer moves", () => {
    const moved = transitionGridGesture(previewed, { type: "dragPreviewed", preview: preview("event-1", 630) });
    expect(gestureDragPreview(moved)?.startMinute).toBe(630);
  });

  it("ends a press released before the threshold without committing", () => {
    expect(transitionGridGesture(pressed, { type: "dragReleased" })).toEqual(IDLE_GRID_GESTURE);
  });

  it("ends a drag released before any preview was drawn without committing", () => {
    expect(transitionGridGesture(dragging, { type: "dragReleased" })).toEqual(IDLE_GRID_GESTURE);
  });

  it("ignores a preview before the threshold and while committing", () => {
    expect(transitionGridGesture(pressed, { type: "dragPreviewed", preview: preview("event-1", 0) })).toBe(pressed);
    expect(transitionGridGesture(committing, { type: "dragPreviewed", preview: preview("event-1", 0) })).toBe(committing);
  });

  it("ignores a new press while a drag runs or its update commits", () => {
    const other = { ...drag, eventId: "event-2" };
    for (const state of [pressed, dragging, previewed, committing]) {
      expect(transitionGridGesture(state, { type: "eventPressed", drag: other })).toBe(state);
    }
  });

  it("cancels a drag in any phase, including while it commits", () => {
    for (const state of [pressed, dragging, previewed, committing]) {
      expect(transitionGridGesture(state, { type: "ended" })).toEqual(IDLE_GRID_GESTURE);
    }
  });

  it("ignores a settle that does not follow a released drag", () => {
    for (const state of [IDLE_GRID_GESTURE, pressed, previewed, createSelecting]) {
      expect(transitionGridGesture(state, { type: "dragSettled" })).toBe(state);
    }
  });

  it("ignores drag moves when no drag runs", () => {
    for (const state of [IDLE_GRID_GESTURE, createPending, createSelecting]) {
      expect(transitionGridGesture(state, { type: "dragThresholdPassed" })).toBe(state);
      expect(transitionGridGesture(state, { type: "dragReleased" })).toBe(state);
    }
  });
});

describe("transitionGridGesture create drags", () => {
  it("selects a range once previewed and follows the pointer", () => {
    expect(createPending).toEqual({ phase: "createPending", create });
    expect(gestureCreatePreview(createSelecting)?.startMinute).toBe(600);
    const moved = transitionGridGesture(createSelecting, { type: "createPreviewed", preview: preview("__create__", 660) });
    expect(gestureCreatePreview(moved)?.startMinute).toBe(660);
  });

  it("ends a create from either phase", () => {
    expect(transitionGridGesture(createPending, { type: "ended" })).toEqual(IDLE_GRID_GESTURE);
    expect(transitionGridGesture(createSelecting, { type: "ended" })).toEqual(IDLE_GRID_GESTURE);
  });

  it("never lets a create and an event drag run together", () => {
    for (const state of [pressed, dragging, committing]) {
      expect(transitionGridGesture(state, { type: "createPressed", create })).toBe(state);
      expect(transitionGridGesture(state, { type: "createPreviewed", preview: preview("__create__", 0) })).toBe(state);
    }
    for (const state of [createPending, createSelecting]) {
      expect(transitionGridGesture(state, { type: "eventPressed", drag })).toBe(state);
      expect(transitionGridGesture(state, { type: "createPressed", create })).toBe(state);
    }
  });

  it("ignores a create preview while idle and an end while idle", () => {
    expect(transitionGridGesture(IDLE_GRID_GESTURE, { type: "createPreviewed", preview: preview("__create__", 0) }))
      .toBe(IDLE_GRID_GESTURE);
    expect(transitionGridGesture(IDLE_GRID_GESTURE, { type: "ended" })).toBe(IDLE_GRID_GESTURE);
  });
});

describe("gesture projections", () => {
  it("reports the grabbed drag from press until the update settles", () => {
    expect(gestureDrag(IDLE_GRID_GESTURE)).toBeNull();
    for (const state of [pressed, dragging, committing]) expect(gestureDrag(state)).toEqual(drag);
    expect(gestureDrag(createSelecting)).toBeNull();
  });

  it("shows the drag preview only while dragging or committing", () => {
    expect(gestureDragPreview(pressed)).toBeNull();
    expect(gestureDragPreview(dragging)).toBeNull();
    expect(gestureDragPreview(committing)).toEqual(preview("event-1", 600));
    expect(gestureCreatePreview(createPending)).toBeNull();
    expect(gestureDragPreview(createSelecting)).toBeNull();
  });
});
