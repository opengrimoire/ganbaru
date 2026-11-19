import { describe, expect, it, vi } from "vitest";
import { CalendarMobileSwipeController, type CalendarMobileSwipeControllerOptions } from "./mobile-swipe-controller";

type Listener = (event: unknown) => void;

function setup(overrides: Partial<CalendarMobileSwipeControllerOptions> = {}) {
  const listeners = new Map<string, Listener>();
  const target = {
    addEventListener: vi.fn((type: string, listener: Listener) => listeners.set(type, listener)),
    removeEventListener: vi.fn((type: string, listener: Listener) => {
      if (listeners.get(type) === listener) listeners.delete(type);
    }),
  };
  let time = 0;
  const navigate = vi.fn();
  const controller = new CalendarMobileSwipeController({
    canStart: () => true,
    navigate,
    getViewportWidth: () => 400,
    target: target as unknown as CalendarMobileSwipeControllerOptions["target"],
    now: () => time,
    ...overrides,
  });
  return {
    controller,
    listeners,
    navigate,
    advance(ms: number) {
      time += ms;
    },
  };
}

function pointer(clientX: number, clientY = 0, overrides: Partial<PointerEvent> = {}) {
  return {
    pointerId: 1,
    pointerType: "touch",
    isPrimary: true,
    button: 0,
    clientX,
    clientY,
    preventDefault: vi.fn(),
    ...overrides,
  };
}

function click() {
  return { preventDefault: vi.fn(), stopImmediatePropagation: vi.fn() };
}

describe("CalendarMobileSwipeController", () => {
  it("tracks only primary touch pointers the view allows", () => {
    const blocked = setup({ canStart: () => false });
    blocked.controller.handleStart(pointer(100));
    expect(blocked.listeners.size).toBe(0);

    const mouse = setup();
    mouse.controller.handleStart(pointer(100, 0, { pointerType: "mouse" }));
    mouse.controller.handleStart(pointer(100, 0, { isPrimary: false }));
    expect(mouse.listeners.size).toBe(0);

    const touch = setup();
    touch.controller.handleStart(pointer(100));
    expect([...touch.listeners.keys()].sort()).toEqual(["pointercancel", "pointermove", "pointerup"]);
  });

  it("gives up on a vertical drag without navigating or swallowing the click", () => {
    const { controller, listeners, navigate } = setup();
    controller.handleStart(pointer(100, 100));
    const move = pointer(102, 160);
    controller.handleMove(move);
    expect(move.preventDefault).not.toHaveBeenCalled();
    expect(listeners.size).toBe(0);
    controller.handleEnd(pointer(102, 160));
    expect(navigate).not.toHaveBeenCalled();
    const event = click();
    controller.handleClick(event);
    expect(event.preventDefault).not.toHaveBeenCalled();
  });

  it("commits a long horizontal swipe and swallows the click that ends it", () => {
    const { controller, listeners, navigate, advance } = setup();
    controller.handleStart(pointer(300));
    advance(400);
    const move = pointer(200);
    controller.handleMove(move);
    expect(move.preventDefault).toHaveBeenCalled();
    controller.handleEnd(pointer(200));
    expect(navigate).toHaveBeenCalledWith("forward");
    expect(listeners.size).toBe(0);
    const first = click();
    controller.handleClick(first);
    expect(first.preventDefault).toHaveBeenCalled();
    expect(first.stopImmediatePropagation).toHaveBeenCalled();
    const second = click();
    controller.handleClick(second);
    expect(second.preventDefault).not.toHaveBeenCalled();
  });

  it("keeps a short slow horizontal swipe on the same period but still swallows its click", () => {
    const { controller, navigate, advance } = setup();
    controller.handleStart(pointer(100));
    advance(1000);
    controller.handleMove(pointer(120));
    controller.handleEnd(pointer(120));
    expect(navigate).not.toHaveBeenCalled();
    const event = click();
    controller.handleClick(event);
    expect(event.preventDefault).toHaveBeenCalled();
  });

  it("ignores another pointer while one is tracked", () => {
    const { controller, navigate, advance } = setup();
    controller.handleStart(pointer(300));
    advance(400);
    controller.handleMove(pointer(100, 0, { pointerId: 2 }));
    controller.handleEnd(pointer(100, 0, { pointerId: 2 }));
    expect(navigate).not.toHaveBeenCalled();
  });

  it("navigates by direction alone when a horizontal swipe is cancelled", () => {
    const { controller, navigate, advance } = setup();
    controller.handleStart(pointer(100));
    advance(1000);
    controller.handleMove(pointer(120));
    controller.handleCancel(pointer(120));
    expect(navigate).toHaveBeenCalledWith("back");
  });

  it("drops a cancelled swipe that never picked an axis", () => {
    const { controller, listeners, navigate } = setup();
    controller.handleStart(pointer(100));
    controller.handleCancel(pointer(100));
    expect(navigate).not.toHaveBeenCalled();
    expect(listeners.size).toBe(0);
  });

  it("drops a tracked swipe on reset", () => {
    const { controller, listeners, navigate, advance } = setup();
    controller.handleStart(pointer(300));
    advance(400);
    controller.handleMove(pointer(200));
    controller.reset();
    expect(listeners.size).toBe(0);
    controller.handleEnd(pointer(200));
    expect(navigate).not.toHaveBeenCalled();
  });

  it("swallows one click on request", () => {
    const { controller } = setup();
    controller.suppressNextClick();
    const event = click();
    controller.handleClick(event);
    expect(event.stopImmediatePropagation).toHaveBeenCalled();
  });
});
