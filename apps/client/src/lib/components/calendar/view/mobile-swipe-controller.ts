/**
 * Horizontal touch swipe navigation for the mobile Calendar: tracks one primary touch pointer, gives
 * up on vertical movement, commits a period change by distance or speed, and swallows the click that
 * ends a horizontal swipe.
 */
import {
  calendarSwipeAxis,
  calendarSwipeDirection,
  calendarSwipeNavigation,
  type CalendarSwipeAxis,
} from "../mobile-gestures";

type SwipePointerEvent = Pick<
  PointerEvent,
  "pointerId" | "pointerType" | "isPrimary" | "button" | "clientX" | "clientY" | "preventDefault"
>;

interface CalendarMobileSwipeState {
  pointerId: number;
  startX: number;
  startY: number;
  startedAt: number;
  lastX: number;
  lastY: number;
  lastAt: number;
  axis: CalendarSwipeAxis;
}

export interface CalendarMobileSwipeControllerOptions {
  /** Whether a touch may start a swipe: the mobile layout with no panel or confirmation open. */
  canStart: () => boolean;
  navigate: (direction: "back" | "forward") => void;
  /** Width that sets the swipe distance threshold, read when a horizontal swipe ends. */
  getViewportWidth: () => number;
  /** Receives the move, end, and cancel listeners while a swipe is tracked; defaults to `window`. */
  target?: Pick<Window, "addEventListener" | "removeEventListener">;
  now?: () => number;
}

/** Owns the swipe state and the window listeners that follow one touch. */
export class CalendarMobileSwipeController {
  private suppressClick = false;
  private state: CalendarMobileSwipeState | null = null;
  private readonly target: Pick<Window, "addEventListener" | "removeEventListener">;
  private readonly now: () => number;

  constructor(private readonly options: CalendarMobileSwipeControllerOptions) {
    this.target = options.target ?? window;
    this.now = options.now ?? (() => performance.now());
  }

  /** Drops any tracked swipe and its listeners. */
  reset = (): void => {
    this.removeListeners();
    this.state = null;
  };

  /** Swallows the next click, as after a touch edit on the grid ends. */
  suppressNextClick = (): void => {
    this.suppressClick = true;
  };

  handleStart = (event: SwipePointerEvent): void => {
    if (event.pointerType !== "touch" || !event.isPrimary || event.button !== 0
      || !this.options.canStart()) return;
    this.state = {
      pointerId: event.pointerId,
      startX: event.clientX,
      startY: event.clientY,
      startedAt: this.now(),
      lastX: event.clientX,
      lastY: event.clientY,
      lastAt: this.now(),
      axis: null,
    };
    this.target.addEventListener("pointermove", this.handleMove);
    this.target.addEventListener("pointerup", this.handleEnd);
    this.target.addEventListener("pointercancel", this.handleCancel);
  };

  handleMove = (event: SwipePointerEvent): void => {
    const state = this.state;
    if (!state || event.pointerId !== state.pointerId) return;
    state.lastX = event.clientX;
    state.lastY = event.clientY;
    state.lastAt = this.now();
    const deltaX = event.clientX - state.startX;
    const deltaY = event.clientY - state.startY;
    if (state.axis === null) {
      const axis = calendarSwipeAxis(deltaX, deltaY);
      if (axis === null) return;
      if (axis === "vertical") {
        this.reset();
        return;
      }
      state.axis = axis;
    }
    event.preventDefault();
  };

  handleEnd = (event: SwipePointerEvent): void => {
    const state = this.state;
    if (!state || event.pointerId !== state.pointerId) return;
    this.finish(state);
  };

  handleCancel = (event: SwipePointerEvent): void => {
    const state = this.state;
    if (!state || state.pointerId !== event.pointerId) return;
    if (state.axis !== "horizontal") {
      this.reset();
      return;
    }
    const direction = calendarSwipeDirection(state.lastX - state.startX);
    this.suppressClick = true;
    if (direction) this.completeNavigation(direction);
    else this.reset();
  };

  handleClick = (event: Pick<MouseEvent, "preventDefault" | "stopImmediatePropagation">): void => {
    if (!this.suppressClick) return;
    this.suppressClick = false;
    event.preventDefault();
    event.stopImmediatePropagation();
  };

  private removeListeners(): void {
    this.target.removeEventListener("pointermove", this.handleMove);
    this.target.removeEventListener("pointerup", this.handleEnd);
    this.target.removeEventListener("pointercancel", this.handleCancel);
  }

  private completeNavigation(direction: "back" | "forward"): void {
    this.removeListeners();
    this.state = null;
    this.options.navigate(direction);
  }

  private finish(state: CalendarMobileSwipeState): void {
    const direction = state.axis === "horizontal"
      ? calendarSwipeNavigation({
        deltaX: state.lastX - state.startX,
        elapsedMs: state.lastAt - state.startedAt,
        viewportWidth: this.options.getViewportWidth(),
      })
      : null;
    if (state.axis === "horizontal") this.suppressClick = true;
    if (direction) this.completeNavigation(direction);
    else this.reset();
  }
}
