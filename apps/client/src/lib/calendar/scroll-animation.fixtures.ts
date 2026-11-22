import { vi } from "vitest";

/** Milliseconds between fake animation frames, matching a 60 Hz display. */
const FAKE_FRAME_INTERVAL_MS = 16;

/** A manually driven `requestAnimationFrame` and `performance.now` pair for scroll animation tests. */
export interface FakeFrameClock {
  /** Number of frames scheduled and not yet run or cancelled. */
  pendingFrames(): number;
  /** Advance the clock by one frame interval and run every frame scheduled before it. */
  runFrame(): void;
  /** Run frames until none remain, failing after `limit` frames. */
  runUntilIdle(limit?: number): void;
}

/**
 * Replace `requestAnimationFrame`, `cancelAnimationFrame`, and `performance.now` with a deterministic clock.
 * Call `vi.unstubAllGlobals()` and `vi.restoreAllMocks()` after each test.
 *
 * @returns The clock that runs scheduled frames.
 */
export function installFakeFrameClock(): FakeFrameClock {
  let now = 1000;
  let nextHandle = 1;
  const frames = new Map<number, FrameRequestCallback>();

  vi.spyOn(performance, "now").mockImplementation(() => now);
  vi.stubGlobal("requestAnimationFrame", (callback: FrameRequestCallback) => {
    const handle = nextHandle++;
    frames.set(handle, callback);
    return handle;
  });
  vi.stubGlobal("cancelAnimationFrame", (handle: number) => {
    frames.delete(handle);
  });

  function runFrame() {
    now += FAKE_FRAME_INTERVAL_MS;
    const due = [...frames.entries()];
    frames.clear();
    for (const [, callback] of due) callback(now);
  }

  return {
    pendingFrames: () => frames.size,
    runFrame,
    runUntilIdle(limit = 500) {
      for (let count = 0; frames.size > 0; count++) {
        if (count >= limit) throw new Error(`Scroll animation still running after ${limit} frames`);
        runFrame();
      }
    },
  };
}

/** Scroll container stand-in whose `scrollTop` rounds and clamps like a browser element. */
export interface FakeScrollElement {
  scrollTop: number;
  readonly scrollHeight: number;
  readonly clientHeight: number;
}

/**
 * Create a scroll container stand-in. Written offsets are clamped to the scroll range and rounded to whole pixels.
 *
 * @param scrollHeight Total content height.
 * @param clientHeight Visible height.
 * @returns The stand-in element.
 */
export function createFakeScrollElement(scrollHeight: number, clientHeight: number): FakeScrollElement {
  let offset = 0;
  return {
    get scrollTop() {
      return offset;
    },
    set scrollTop(value: number) {
      offset = Math.round(Math.max(0, Math.min(value, scrollHeight - clientHeight)));
    },
    scrollHeight,
    clientHeight,
  };
}

/**
 * Build a vertical pixel-mode wheel event.
 *
 * @param deltaY Vertical wheel delta in pixels.
 * @returns A wheel event carrying only the fields the scroll handlers read.
 */
export function verticalWheel(deltaY: number): WheelEvent {
  const event: Pick<WheelEvent, "deltaX" | "deltaY" | "deltaMode" | "shiftKey" | "preventDefault"> = {
    deltaX: 0,
    deltaY,
    deltaMode: 0,
    shiftKey: false,
    preventDefault: () => {},
  };
  return event as WheelEvent;
}
