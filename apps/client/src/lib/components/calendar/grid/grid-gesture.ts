import type { DragState, PositionedEvent } from "$lib/calendar/types";

/** Where a create-by-drag began in a day column. */
export interface CreateGestureOrigin {
  dateStr: string;
  /** Minute the selection grows from. */
  anchorMinute: number;
  /** Minute a plain click creates its event at. */
  clickStartMinute: number;
  columnEl: HTMLElement;
  pointerStartX: number;
  pointerStartY: number;
}

/**
 * One pointer gesture on the timed Calendar grid. An event press becomes a drag once the pointer passes the
 * threshold, and a drag that drew a preview holds it on screen while the update commits. A create press waits
 * for the hold delay or the threshold before it selects a range. Only one gesture runs at a time, so a create
 * and an event drag can never coexist.
 */
export type GridGesture =
  | { phase: "idle" }
  /** An event is pressed but the pointer has not passed the drag threshold. */
  | { phase: "eventPressed"; drag: DragState }
  /** An event follows the pointer; `preview` is null until the first preview is computed. */
  | { phase: "eventDragging"; drag: DragState; preview: PositionedEvent | null }
  /** The drag ended and its preview stays on screen until the update settles. */
  | { phase: "eventCommitting"; drag: DragState; preview: PositionedEvent }
  /** An empty slot is pressed but no range is selected yet. */
  | { phase: "createPending"; create: CreateGestureOrigin }
  /** A range is selected and drawn as `preview`. */
  | { phase: "createSelecting"; create: CreateGestureOrigin; preview: PositionedEvent };

export type GridGestureEvent =
  /** An event drag began. */
  | { type: "eventPressed"; drag: DragState }
  /** The pointer passed the drag threshold. */
  | { type: "dragThresholdPassed" }
  /** The drag preview was recomputed. */
  | { type: "dragPreviewed"; preview: PositionedEvent }
  /** The pointer was released over an event drag. */
  | { type: "dragReleased" }
  /** The update of a released drag finished, successfully or not. */
  | { type: "dragSettled" }
  /** A create-by-drag began. */
  | { type: "createPressed"; create: CreateGestureOrigin }
  /** The create range was entered or moved. */
  | { type: "createPreviewed"; preview: PositionedEvent }
  /** The create gesture finished or the gesture was cancelled. */
  | { type: "ended" };

export const IDLE_GRID_GESTURE: GridGesture = { phase: "idle" };

/**
 * Apply one gesture event. A move that does not apply to the current phase returns the same state object, so a
 * press while another gesture runs or commits is ignored.
 */
export function transitionGridGesture(state: GridGesture, event: GridGestureEvent): GridGesture {
  switch (event.type) {
    case "eventPressed":
      return state.phase === "idle" ? { phase: "eventPressed", drag: event.drag } : state;
    case "dragThresholdPassed":
      return state.phase === "eventPressed" ? { phase: "eventDragging", drag: state.drag, preview: null } : state;
    case "dragPreviewed":
      return state.phase === "eventDragging" ? { ...state, preview: event.preview } : state;
    case "dragReleased":
      if (state.phase === "eventPressed") return IDLE_GRID_GESTURE;
      if (state.phase !== "eventDragging") return state;
      return state.preview
        ? { phase: "eventCommitting", drag: state.drag, preview: state.preview }
        : IDLE_GRID_GESTURE;
    case "dragSettled":
      return state.phase === "eventCommitting" ? IDLE_GRID_GESTURE : state;
    case "createPressed":
      return state.phase === "idle" ? { phase: "createPending", create: event.create } : state;
    case "createPreviewed":
      return state.phase === "createPending" || state.phase === "createSelecting"
        ? { phase: "createSelecting", create: state.create, preview: event.preview }
        : state;
    case "ended":
      return state.phase === "idle" ? state : IDLE_GRID_GESTURE;
  }
}

/** The event drag of the gesture, while one is pressed, dragging, or committing. */
export function gestureDrag(state: GridGesture): DragState | null {
  return state.phase === "eventPressed" || state.phase === "eventDragging" || state.phase === "eventCommitting"
    ? state.drag
    : null;
}

/** The drag preview on screen, while an event is dragging or committing. */
export function gestureDragPreview(state: GridGesture): PositionedEvent | null {
  return state.phase === "eventDragging" || state.phase === "eventCommitting" ? state.preview : null;
}

/** The create range on screen, while one is selected. */
export function gestureCreatePreview(state: GridGesture): PositionedEvent | null {
  return state.phase === "createSelecting" ? state.preview : null;
}
