import { afterEach, describe, expect, it, vi } from "vitest";
import { createSmoothScroll } from "./smooth-scroll";
import { createFakeScrollElement, installFakeFrameClock, verticalWheel } from "./scroll-animation.fixtures";

describe("createSmoothScroll", () => {
  afterEach(() => {
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
  });

  function setup() {
    const clock = installFakeFrameClock();
    const el = createFakeScrollElement(5000, 200);
    const scroll = createSmoothScroll(() => el as HTMLElement, 2, 8);
    return { clock, el, scroll };
  }

  it("glides forward and settles once velocity decays", () => {
    const { clock, el, scroll } = setup();

    scroll(verticalWheel(100));
    clock.runUntilIdle();

    expect(el.scrollTop).toBeGreaterThan(0);
    expect(el.scrollTop).toBeLessThan(5000 - 200);
    expect(clock.pendingFrames()).toBe(0);
  });

  it("stops at the end of the scroll range", () => {
    const { clock, el, scroll } = setup();
    el.scrollTop = 4790;

    scroll(verticalWheel(100));
    clock.runUntilIdle();

    expect(el.scrollTop).toBe(4800);
  });

  it("stops when other code moves the offset during the glide", () => {
    const { clock, el, scroll } = setup();

    scroll(verticalWheel(100));
    clock.runFrame();
    clock.runFrame();
    expect(el.scrollTop).toBeGreaterThan(0);
    el.scrollTop = 1000;
    clock.runUntilIdle();

    expect(el.scrollTop).toBe(1000);
    expect(clock.pendingFrames()).toBe(0);
  });

  it("starts the next glide from an offset that other code wrote", () => {
    const { clock, el, scroll } = setup();

    scroll(verticalWheel(100));
    clock.runFrame();
    clock.runFrame();
    el.scrollTop = 1000;
    scroll(verticalWheel(-100));
    clock.runUntilIdle();

    expect(el.scrollTop).toBeLessThan(1000);
    expect(el.scrollTop).toBeGreaterThan(800);
  });

  it("cancels the scheduled frame", () => {
    const { clock, el, scroll } = setup();

    scroll(verticalWheel(100));
    clock.runFrame();
    clock.runFrame();
    const reached = el.scrollTop;
    scroll.cancel();

    expect(clock.pendingFrames()).toBe(0);
    clock.runUntilIdle();
    expect(el.scrollTop).toBe(reached);
  });
});
