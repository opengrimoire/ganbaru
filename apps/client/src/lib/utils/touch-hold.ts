/** Stationary touch duration that turns a press into a drag or edit gesture. */
export const TOUCH_LONG_PRESS_MS = 450;
/** Finger travel that marks a touch as a scroll or swipe instead of a hold. */
export const TOUCH_SLOP_PX = 10;

export interface TouchHoldOptions {
  immediate?: boolean;
  onTap?: (event: PointerEvent) => void;
}

/** Finger travel along one axis that makes a touch a swipe. */
export const TOUCH_SWIPE_MIN_PX = 40;
/** How much a swipe must favor one axis over the other; a stroke near 45 degrees has no reliable direction. */
const TOUCH_SWIPE_AXIS_RATIO = 1.5;

/** The way a finger moved across the screen. */
export type TouchSwipeDirection = "left" | "right" | "up" | "down";

export function touchMovedBeyondSlop(deltaX: number, deltaY: number): boolean {
  return Math.hypot(deltaX, deltaY) > TOUCH_SLOP_PX;
}

/**
 * Reads a finished touch as a swipe along its dominant axis.
 *
 * @param deltaX Horizontal travel from the press, positive to the right.
 * @param deltaY Vertical travel from the press, positive downward.
 * @returns The swipe direction, or null when the travel is too short or too diagonal to tell.
 */
export function touchSwipeDirection(deltaX: number, deltaY: number): TouchSwipeDirection | null {
  const horizontal = Math.abs(deltaX);
  const vertical = Math.abs(deltaY);
  if (Math.max(horizontal, vertical) < TOUCH_SWIPE_MIN_PX) return null;
  if (horizontal > vertical * TOUCH_SWIPE_AXIS_RATIO) return deltaX < 0 ? "left" : "right";
  if (vertical > horizontal * TOUCH_SWIPE_AXIS_RATIO) return deltaY < 0 ? "up" : "down";
  return null;
}

/**
 * Arbitrates one touch between native scrolling and a hold gesture. Movement past the slop before
 * the hold cancels it, and an activated hold blocks later touch movement from scrolling. While the
 * touch lasts, the long-press context menu stays closed so it cannot open over the gesture.
 */
export class TouchHoldArbiter {
  private pointerId: number | null = null;
  private startX = 0;
  private startY = 0;
  private timer = 0;
  private active = false;
  private activate: (() => void) | null = null;
  private onTap: ((event: PointerEvent) => void) | null = null;

  get editingActive(): boolean {
    return this.active;
  }

  begin(
    event: PointerEvent,
    activate: () => void,
    options: TouchHoldOptions = {},
  ): void {
    this.finish();
    this.pointerId = event.pointerId;
    this.startX = event.clientX;
    this.startY = event.clientY;
    this.activate = activate;
    this.onTap = options.onTap ?? null;
    window.addEventListener("pointermove", this.handlePendingPointerMove);
    window.addEventListener("pointerup", this.handlePendingPointerEnd);
    window.addEventListener("pointercancel", this.handlePendingPointerEnd);
    window.addEventListener("touchmove", this.blockActiveTouchMove, { capture: true, passive: false });
    window.addEventListener("contextmenu", this.blockContextMenu, { capture: true });
    if (options.immediate) {
      this.activateNow();
      return;
    }
    this.timer = window.setTimeout(this.activateNow, TOUCH_LONG_PRESS_MS);
  }

  finish(): void {
    this.clearTimer();
    this.removePendingPointerListeners();
    window.removeEventListener("touchmove", this.blockActiveTouchMove, { capture: true });
    window.removeEventListener("contextmenu", this.blockContextMenu, { capture: true });
    this.pointerId = null;
    this.activate = null;
    this.onTap = null;
    this.active = false;
  }

  private readonly activateNow = (): void => {
    const activate = this.activate;
    if (this.pointerId === null || !activate) return;
    this.clearTimer();
    this.removePendingPointerListeners();
    this.active = true;
    activate();
  };

  private readonly handlePendingPointerMove = (event: PointerEvent): void => {
    if (event.pointerId !== this.pointerId) return;
    if (!touchMovedBeyondSlop(event.clientX - this.startX, event.clientY - this.startY)) return;
    this.finish();
  };

  private readonly handlePendingPointerEnd = (event: PointerEvent): void => {
    if (event.pointerId !== this.pointerId) return;
    const onTap = event.type === "pointerup" ? this.onTap : null;
    this.finish();
    onTap?.(event);
  };

  private readonly blockActiveTouchMove = (event: TouchEvent): void => {
    if (this.active && event.cancelable) event.preventDefault();
  };

  private readonly blockContextMenu = (event: Event): void => {
    event.preventDefault();
    event.stopPropagation();
  };

  private clearTimer(): void {
    if (!this.timer) return;
    window.clearTimeout(this.timer);
    this.timer = 0;
  }

  private removePendingPointerListeners(): void {
    window.removeEventListener("pointermove", this.handlePendingPointerMove);
    window.removeEventListener("pointerup", this.handlePendingPointerEnd);
    window.removeEventListener("pointercancel", this.handlePendingPointerEnd);
  }
}
