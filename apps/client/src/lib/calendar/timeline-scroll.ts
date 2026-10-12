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

export const CALENDAR_FORWARDED_WHEEL_EVENT = "ganbaru-ai-calendar-forwarded-wheel";

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

function clampScrollTopForElement(el: Pick<HTMLElement, "scrollHeight" | "clientHeight">, value: number): number {
  return Math.max(0, Math.min(value, smoothScrollMaxScrollTop(el)));
}

export function createTimelineWheelScroll(getEl: () => HTMLElement | undefined): SmoothScrollFn {
  let raf = 0;
  let prevTs = 0;
  let animatedScrollTop = 0;
  let targetScrollTop = 0;
  let scrollSpeedPxPerMs = CALENDAR_TIMELINE_BASE_SCROLL_SPEED_PX_PER_MS;

  function stop() {
    if (raf) cancelAnimationFrame(raf);
    raf = 0;
    prevTs = 0;
    scrollSpeedPxPerMs = CALENDAR_TIMELINE_BASE_SCROLL_SPEED_PX_PER_MS;
  }

  function tick(ts: number) {
    const el = getEl();
    if (!el) {
      stop();
      return;
    }

    targetScrollTop = clampScrollTopForElement(el, targetScrollTop);
    const remaining = targetScrollTop - animatedScrollTop;
    const dt = Math.min(64, Math.max(0, ts - prevTs));
    prevTs = ts;
    const step = Math.sign(remaining) * Math.min(
      Math.abs(remaining),
      scrollSpeedPxPerMs * dt,
    );
    animatedScrollTop += step;

    if (animatedScrollTop === targetScrollTop) {
      el.scrollTop = targetScrollTop;
      stop();
      return;
    }

    el.scrollTop = animatedScrollTop;
    raf = requestAnimationFrame(tick);
  }

  const scroll: SmoothScrollFn = (e: WheelEvent) => {
    const el = getEl();
    if (!el) return;

    const deltaPixels = wheelDeltaToScrollPixels(getSmoothScrollDelta(e), e.deltaMode, el.clientHeight);
    const movement = deltaPixels * calendarTimelineWheelMultiplierForDeltaPixels(deltaPixels);
    if (movement === 0) return;

    animatedScrollTop = el.scrollTop;
    const remainingBeforeInput = targetScrollTop - animatedScrollTop;
    targetScrollTop = raf
      ? targetScrollTop + movement
      : animatedScrollTop + movement;
    targetScrollTop = clampScrollTopForElement(el, targetScrollTop);
    const nextSpeed = calendarTimelineScrollSpeedForDistance(
      targetScrollTop - animatedScrollTop,
    );
    const sameDirection = Math.sign(remainingBeforeInput) === Math.sign(targetScrollTop - animatedScrollTop);
    scrollSpeedPxPerMs = raf && sameDirection
      ? Math.max(scrollSpeedPxPerMs, nextSpeed)
      : nextSpeed;

    if (!raf) {
      prevTs = performance.now();
      raf = requestAnimationFrame(tick);
    }
  };

  scroll.cancel = () => {
    stop();
    const el = getEl();
    animatedScrollTop = el?.scrollTop ?? 0;
    targetScrollTop = animatedScrollTop;
  };

  return scroll;
}
