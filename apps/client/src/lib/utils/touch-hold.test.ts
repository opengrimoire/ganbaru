import { afterEach, describe, expect, it, vi } from "vitest";
import {
  TOUCH_LONG_PRESS_MS,
  TouchHoldArbiter,
  touchMovedBeyondSlop,
} from "./touch-hold";

function pointerEvent(
  type: string,
  pointerId: number,
  clientX: number,
  clientY: number,
): PointerEvent {
  const event = new Event(type);
  Object.defineProperties(event, {
    pointerId: { value: pointerId },
    clientX: { value: clientX },
    clientY: { value: clientY },
  });
  return event as PointerEvent;
}

function installWindowEventTarget(): EventTarget {
  const eventTarget = new EventTarget();
  vi.stubGlobal("window", {
    addEventListener: eventTarget.addEventListener.bind(eventTarget),
    removeEventListener: eventTarget.removeEventListener.bind(eventTarget),
    setTimeout: globalThis.setTimeout.bind(globalThis),
    clearTimeout: globalThis.clearTimeout.bind(globalThis),
  });
  return eventTarget;
}

describe("touch hold arbitration", () => {
  afterEach(() => {
    vi.useRealTimers();
    vi.unstubAllGlobals();
  });

  it("keeps small pointer drift below touch slop", () => {
    expect(touchMovedBeyondSlop(6, 6)).toBe(false);
    expect(touchMovedBeyondSlop(11, 0)).toBe(true);
  });

  it("activates a stationary hold and owns later touch movement", () => {
    vi.useFakeTimers();
    const eventTarget = installWindowEventTarget();
    const activate = vi.fn();
    const onTap = vi.fn();
    const arbiter = new TouchHoldArbiter();

    arbiter.begin(pointerEvent("pointerdown", 3, 20, 30), activate, { onTap });
    vi.advanceTimersByTime(TOUCH_LONG_PRESS_MS - 1);
    expect(activate).not.toHaveBeenCalled();
    vi.advanceTimersByTime(1);
    expect(activate).toHaveBeenCalledOnce();
    expect(arbiter.editingActive).toBe(true);

    const touchMove = new Event("touchmove", { cancelable: true });
    eventTarget.dispatchEvent(touchMove);
    expect(touchMove.defaultPrevented).toBe(true);
    eventTarget.dispatchEvent(pointerEvent("pointerup", 3, 20, 30));
    expect(onTap).not.toHaveBeenCalled();
    arbiter.finish();
  });

  it("recognizes a stationary release before the hold as a tap", () => {
    vi.useFakeTimers();
    const eventTarget = installWindowEventTarget();
    const activate = vi.fn();
    const onTap = vi.fn();
    const arbiter = new TouchHoldArbiter();

    arbiter.begin(pointerEvent("pointerdown", 5, 20, 30), activate, { onTap });
    eventTarget.dispatchEvent(pointerEvent("pointerup", 5, 22, 34));
    vi.advanceTimersByTime(TOUCH_LONG_PRESS_MS);

    expect(onTap).toHaveBeenCalledOnce();
    expect(activate).not.toHaveBeenCalled();
    expect(arbiter.editingActive).toBe(false);
  });

  it("cancels a pending hold once movement crosses touch slop", () => {
    vi.useFakeTimers();
    const eventTarget = installWindowEventTarget();
    const activate = vi.fn();
    const onTap = vi.fn();
    const arbiter = new TouchHoldArbiter();

    arbiter.begin(pointerEvent("pointerdown", 4, 20, 30), activate, { onTap });
    eventTarget.dispatchEvent(pointerEvent("pointermove", 4, 31, 30));
    eventTarget.dispatchEvent(pointerEvent("pointerup", 4, 31, 30));
    vi.advanceTimersByTime(TOUCH_LONG_PRESS_MS);

    expect(activate).not.toHaveBeenCalled();
    expect(onTap).not.toHaveBeenCalled();
    expect(arbiter.editingActive).toBe(false);
  });
});
