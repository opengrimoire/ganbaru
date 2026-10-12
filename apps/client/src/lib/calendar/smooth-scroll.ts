import {
  getSmoothScrollDelta,
  smoothScrollMaxScrollTop,
  type SmoothScrollFn,
} from "./timeline-scroll";

const SMOOTH_SCROLL_STOP_VELOCITY_PX_PER_SEC = 10;

export function createSmoothScroll(
  getEl: () => HTMLElement | undefined,
  gain = 3,
  friction = 6,
): SmoothScrollFn {
  let velocity = 0;
  let running = false;
  let prev = 0;
  let trackedPos: number | null = null;

  function tick(ts: number) {
    if (!running) return;
    const el = getEl();
    if (!el) {
      running = false;
      trackedPos = null;
      return;
    }
    if (!prev) {
      prev = ts;
      requestAnimationFrame(tick);
      return;
    }
    const dt = (ts - prev) / 1000;
    prev = ts;
    velocity *= Math.exp(-friction * dt);
    if (Math.abs(velocity) < SMOOTH_SCROLL_STOP_VELOCITY_PX_PER_SEC) {
      velocity = 0;
      running = false;
      trackedPos = null;
      return;
    }
    const max = smoothScrollMaxScrollTop(el);

    if (trackedPos === null) trackedPos = el.scrollTop;
    const newPos = trackedPos + velocity * dt;

    if (velocity > 0 && newPos >= max) {
      el.scrollTop = max;
      velocity = 0;
      running = false;
      trackedPos = null;
      return;
    }
    if (velocity < 0 && newPos <= 0) {
      el.scrollTop = 0;
      velocity = 0;
      running = false;
      trackedPos = null;
      return;
    }

    trackedPos = newPos;
    el.scrollTop = newPos;
    requestAnimationFrame(tick);
  }

  const fn = ((e: WheelEvent) => {
    const el = getEl();
    if (!el) return;
    e.preventDefault();
    velocity += getSmoothScrollDelta(e) * gain;
    if (!running) {
      running = true;
      prev = 0;
      trackedPos = el.scrollTop;
      requestAnimationFrame(tick);
    }
  }) as SmoothScrollFn;

  fn.cancel = () => {
    running = false;
    velocity = 0;
    prev = 0;
    trackedPos = null;
  };

  return fn;
}
