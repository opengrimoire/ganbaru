import {
  getSmoothScrollDelta,
  scrollMovedOutside,
  smoothScrollMaxScrollTop,
  type SmoothScrollFn,
} from "./timeline-scroll";

const SMOOTH_SCROLL_STOP_VELOCITY_PX_PER_SEC = 10;

/** Lifecycle of a momentum wheel animation. Only a running animation holds a frame handle and a velocity. */
type SmoothScrollState =
  | { phase: "idle" }
  | {
    phase: "animating";
    /** Handle of the scheduled animation frame. */
    frame: number;
    /** Timestamp of the previous frame in milliseconds, or null before the first frame. */
    prevTs: number | null;
    /** Current velocity, in pixels per second. */
    velocity: number;
    /** Unrounded offset the animation has reached. */
    position: number;
    /** Offset the animation last wrote, used to notice writes from other code. */
    lastWritten: number;
  };

/**
 * Create a momentum wheel handler for a small scroll list. Each wheel event adds velocity that decays with friction;
 * when other code moves the scroll offset (for example keeping a focused row visible), the animation stops instead of
 * writing its older position back.
 *
 * @param getEl Returns the scroll container, or undefined while it is not mounted.
 * @param gain Velocity added per wheel delta pixel, per second.
 * @param friction Exponential velocity decay rate, per second.
 * @returns The wheel handler with a `cancel` that stops any running animation.
 */
export function createSmoothScroll(
  getEl: () => HTMLElement | undefined,
  gain = 3,
  friction = 6,
): SmoothScrollFn {
  let state: SmoothScrollState = { phase: "idle" };

  function tick(ts: number) {
    if (state.phase !== "animating") return;
    const el = getEl();
    if (!el || scrollMovedOutside(el, state.lastWritten)) {
      state = { phase: "idle" };
      return;
    }
    if (state.prevTs === null) {
      state.prevTs = ts;
      state.frame = requestAnimationFrame(tick);
      return;
    }
    const dt = (ts - state.prevTs) / 1000;
    state.prevTs = ts;
    state.velocity *= Math.exp(-friction * dt);
    if (Math.abs(state.velocity) < SMOOTH_SCROLL_STOP_VELOCITY_PX_PER_SEC) {
      state = { phase: "idle" };
      return;
    }
    const max = smoothScrollMaxScrollTop(el);
    const newPos = state.position + state.velocity * dt;

    if (state.velocity > 0 && newPos >= max) {
      el.scrollTop = max;
      state = { phase: "idle" };
      return;
    }
    if (state.velocity < 0 && newPos <= 0) {
      el.scrollTop = 0;
      state = { phase: "idle" };
      return;
    }

    state.position = newPos;
    state.lastWritten = newPos;
    el.scrollTop = newPos;
    state.frame = requestAnimationFrame(tick);
  }

  function cancel() {
    if (state.phase === "animating") cancelAnimationFrame(state.frame);
    state = { phase: "idle" };
  }

  const fn = ((e: WheelEvent) => {
    const el = getEl();
    if (!el) return;
    e.preventDefault();
    if (state.phase === "animating" && scrollMovedOutside(el, state.lastWritten)) cancel();
    const impulse = getSmoothScrollDelta(e) * gain;
    if (state.phase === "animating") {
      state.velocity += impulse;
      return;
    }
    state = {
      phase: "animating",
      frame: requestAnimationFrame(tick),
      prevTs: null,
      velocity: impulse,
      position: el.scrollTop,
      lastWritten: el.scrollTop,
    };
  }) as SmoothScrollFn;

  fn.cancel = cancel;

  return fn;
}
