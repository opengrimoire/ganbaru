export interface SmoothScrollFn {
  (e: WheelEvent): void;
  cancel(): void;
}

export function getSmoothScrollDelta(e: WheelEvent): number {
  if (e.shiftKey && Math.abs(e.deltaX) > Math.abs(e.deltaY)) {
    return e.deltaX;
  }
  return e.deltaY;
}

export const CALENDAR_FORWARDED_WHEEL_EVENT = "ganbaru-calendar-forwarded-wheel";

export interface CalendarForwardedWheelDetail {
  deltaY: number;
  deltaMode: number;
  scrollTop: number;
  scrollHeight: number;
  clientHeight: number;
}

const WHEEL_LINE_DELTA_PX = 40;
export const CALENDAR_TIMELINE_WHEEL_MULTIPLIER = 1.24;
export const CALENDAR_TIMELINE_LOW_DELTA_WHEEL_MULTIPLIER = 1.5;
const CALENDAR_TIMELINE_REFERENCE_WHEEL_DELTA_PX = 102;
const CALENDAR_TIMELINE_BASE_SCROLL_DURATION_MS = 145;
const CALENDAR_TIMELINE_MAX_SCROLL_SPEED_MULTIPLIER = 2;
const CALENDAR_TIMELINE_REFERENCE_SCROLL_DISTANCE_PX =
  CALENDAR_TIMELINE_REFERENCE_WHEEL_DELTA_PX * CALENDAR_TIMELINE_WHEEL_MULTIPLIER;
const CALENDAR_TIMELINE_BASE_SCROLL_SPEED_PX_PER_MS =
  CALENDAR_TIMELINE_REFERENCE_SCROLL_DISTANCE_PX / CALENDAR_TIMELINE_BASE_SCROLL_DURATION_MS;

/**
 * Timeline wheel model:
 * - A normal wheel tick defines the base distance and base speed.
 * - Lighter deltas get slightly more sensitivity.
 * - Longer accumulated distances increase speed.
 * - Motion is linear to avoid an end-settle feel.
 */

/** Return the usable vertical scroll range, clamped for non-scrollable content. */
export function smoothScrollMaxScrollTop(el: Pick<HTMLElement, "scrollHeight" | "clientHeight">): number {
  return Math.max(0, el.scrollHeight - el.clientHeight);
}

/** Scroll range below which the Calendar hides its scrollbar thumb, in pixels. */
const CALENDAR_SCROLLBAR_VISIBILITY_THRESHOLD_PX = 2;
/** Width of the Calendar's scrollbar track, which overlays the right edge of the grid, in pixels. */
export const CALENDAR_SCROLLBAR_TRACK_PX = 8;
/** Shortest the Calendar's scrollbar thumb gets, in pixels. */
const CALENDAR_SCROLLBAR_MIN_THUMB_PX = 24;

/** The Calendar scrollbar thumb's offset and height in its track, both in pixels. */
export interface CalendarScrollbarThumb {
  top: number;
  height: number;
}

/**
 * Place the Calendar's scrollbar thumb for a scroll position. The thumb takes the visible share of the content, never
 * shorter than a grabbable minimum, and has no height when there is nothing to scroll.
 *
 * @param scroll The scroll container's offset, content height, and visible height.
 * @param trackHeight The height of the track the thumb moves in.
 * @returns The thumb's offset and height in the track.
 */
export function calendarScrollbarThumb(
  scroll: Pick<HTMLElement, "scrollTop" | "scrollHeight" | "clientHeight">,
  trackHeight: number,
): CalendarScrollbarThumb {
  const scrollRange = smoothScrollMaxScrollTop(scroll);
  if (scrollRange <= CALENDAR_SCROLLBAR_VISIBILITY_THRESHOLD_PX) return { top: 0, height: 0 };
  const height = Math.max((scroll.clientHeight / scroll.scrollHeight) * trackHeight, CALENDAR_SCROLLBAR_MIN_THUMB_PX);
  const progress = Math.min(1, Math.max(0, scroll.scrollTop / scrollRange));
  return { top: progress * (trackHeight - height), height };
}

/** Convert wheel delta units into direct scroll pixels. */
export function wheelDeltaToScrollPixels(
  delta: number,
  deltaMode: number,
  pageSizePx = 0,
): number {
  if (deltaMode === 0) return delta;
  if (deltaMode === 1) return delta * WHEEL_LINE_DELTA_PX;
  return delta * pageSizePx;
}

export function calendarTimelineWheelMultiplierForDeltaPixels(deltaPixels: number): number {
  const absDelta = Math.abs(deltaPixels);
  if (!Number.isFinite(absDelta) || absDelta === 0) return CALENDAR_TIMELINE_WHEEL_MULTIPLIER;
  const t = Math.min(1, absDelta / CALENDAR_TIMELINE_REFERENCE_WHEEL_DELTA_PX);
  return CALENDAR_TIMELINE_LOW_DELTA_WHEEL_MULTIPLIER +
    (CALENDAR_TIMELINE_WHEEL_MULTIPLIER - CALENDAR_TIMELINE_LOW_DELTA_WHEEL_MULTIPLIER) * t;
}

export function calendarTimelineScrollSpeedForDistance(distancePx: number): number {
  const distance = Math.abs(distancePx);
  if (!Number.isFinite(distance) || distance <= CALENDAR_TIMELINE_REFERENCE_SCROLL_DISTANCE_PX) {
    return CALENDAR_TIMELINE_BASE_SCROLL_SPEED_PX_PER_MS;
  }
  const distanceScale = Math.sqrt(distance / CALENDAR_TIMELINE_REFERENCE_SCROLL_DISTANCE_PX);
  return CALENDAR_TIMELINE_BASE_SCROLL_SPEED_PX_PER_MS *
    Math.min(CALENDAR_TIMELINE_MAX_SCROLL_SPEED_MULTIPLIER, distanceScale);
}

/**
 * Distance between the scroll offset an animation last wrote and the element's current offset beyond which the
 * offset counts as moved by other code. Browsers round written offsets to device pixels, so an exact comparison
 * would misread that rounding as an outside write.
 */
export const SCROLL_OUTSIDE_WRITE_TOLERANCE_PX = 1;

/**
 * Report whether code other than an animation moved the element since the animation last wrote its offset.
 *
 * @param el The scroll container.
 * @param lastWritten The offset the animation last wrote.
 * @returns True when the current offset differs from the written one by more than rounding.
 */
export function scrollMovedOutside(el: Pick<HTMLElement, "scrollTop">, lastWritten: number): boolean {
  return Math.abs(el.scrollTop - lastWritten) > SCROLL_OUTSIDE_WRITE_TOLERANCE_PX;
}

function clampScrollTopForElement(el: Pick<HTMLElement, "scrollHeight" | "clientHeight">, value: number): number {
  return Math.max(0, Math.min(value, smoothScrollMaxScrollTop(el)));
}

/** Lifecycle of the timeline wheel animation. Only a running animation holds a frame handle and a target. */
type TimelineWheelState =
  | { phase: "idle" }
  | {
    phase: "animating";
    /** Handle of the scheduled animation frame. */
    frame: number;
    /** Timestamp of the previous frame, in milliseconds. */
    prevTs: number;
    /** Unrounded offset the animation has reached. */
    position: number;
    /** Offset the animation moves toward. */
    target: number;
    /** Current linear speed, in pixels per millisecond. */
    speedPxPerMs: number;
    /** Offset the animation last wrote, used to notice writes from other code. */
    lastWritten: number;
  };

/**
 * Create the Calendar timeline wheel handler. Wheel input accumulates into a target that a linear animation reaches;
 * when other code moves the scroll offset (zoom, the all-day adjustment, the scrollbar), the animation stops instead
 * of writing its older position back.
 *
 * @param getEl Returns the scroll container, or undefined while it is not mounted.
 * @returns The wheel handler with a `cancel` that stops any running animation.
 */
export function createTimelineWheelScroll(getEl: () => HTMLElement | undefined): SmoothScrollFn {
  let state: TimelineWheelState = { phase: "idle" };

  function stop() {
    if (state.phase === "animating") cancelAnimationFrame(state.frame);
    state = { phase: "idle" };
  }

  function tick(ts: number) {
    if (state.phase !== "animating") return;
    const el = getEl();
    if (!el || scrollMovedOutside(el, state.lastWritten)) {
      state = { phase: "idle" };
      return;
    }

    state.target = clampScrollTopForElement(el, state.target);
    const remaining = state.target - state.position;
    const dt = Math.min(64, Math.max(0, ts - state.prevTs));
    state.prevTs = ts;
    const step = Math.sign(remaining) * Math.min(
      Math.abs(remaining),
      state.speedPxPerMs * dt,
    );
    state.position += step;

    if (state.position === state.target) {
      el.scrollTop = state.target;
      state = { phase: "idle" };
      return;
    }

    el.scrollTop = state.position;
    state.lastWritten = state.position;
    state.frame = requestAnimationFrame(tick);
  }

  const scroll: SmoothScrollFn = (e: WheelEvent) => {
    const el = getEl();
    if (!el) return;

    const deltaPixels = wheelDeltaToScrollPixels(getSmoothScrollDelta(e), e.deltaMode, el.clientHeight);
    const movement = deltaPixels * calendarTimelineWheelMultiplierForDeltaPixels(deltaPixels);
    if (movement === 0) return;

    if (state.phase === "animating" && scrollMovedOutside(el, state.lastWritten)) stop();

    const position = el.scrollTop;
    const running = state.phase === "animating" ? state : null;
    const target = clampScrollTopForElement(el, (running ? running.target : position) + movement);
    const nextSpeed = calendarTimelineScrollSpeedForDistance(target - position);

    if (running) {
      const sameDirection = Math.sign(running.target - position) === Math.sign(target - position);
      running.speedPxPerMs = sameDirection ? Math.max(running.speedPxPerMs, nextSpeed) : nextSpeed;
      running.position = position;
      running.target = target;
      running.lastWritten = position;
      return;
    }

    state = {
      phase: "animating",
      frame: requestAnimationFrame(tick),
      prevTs: performance.now(),
      position,
      target,
      speedPxPerMs: nextSpeed,
      lastWritten: position,
    };
  };

  scroll.cancel = stop;

  return scroll;
}
