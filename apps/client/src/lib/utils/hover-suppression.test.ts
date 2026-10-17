import { afterEach, describe, expect, it } from "vitest";
import {
  HOVER_RESUME_DISTANCE_PX,
  HOVER_SUPPRESSED_ATTRIBUTE,
  POINTER_STEP_MAX_PX,
  createSteadyPointerFilter,
  suppressHoverOnToggle,
  suppressHoverUntilPointerMoves,
  waitForPointerTravel,
  type HoverSuppressionHost,
} from "./hover-suppression";

interface TestHost extends HoverSuppressionHost {
  attributes: Set<string>;
  target: EventTarget;
}

function createHost(canHover = true): TestHost {
  const attributes = new Set<string>();
  const target = new EventTarget();
  return {
    attributes,
    target,
    root: {
      setAttribute: (name: string) => void attributes.add(name),
      removeAttribute: (name: string) => void attributes.delete(name),
    },
    events: target,
    canHover: () => canHover,
  };
}

function pointer(target: EventTarget, type: string, clientX = 0, clientY = 0): void {
  const event = new Event(type);
  Object.defineProperties(event, { clientX: { value: clientX }, clientY: { value: clientY } });
  target.dispatchEvent(event);
}

function suppressed(host: TestHost): boolean {
  return host.attributes.has(HOVER_SUPPRESSED_ATTRIBUTE);
}

describe("hover suppression after layout changes", () => {
  let hosts: TestHost[] = [];

  function start(canHover = true): TestHost {
    const host = createHost(canHover);
    hosts.push(host);
    suppressHoverUntilPointerMoves(host);
    return host;
  }

  afterEach(() => {
    for (const host of hosts) pointer(host.target, "pointerdown");
    hosts = [];
  });

  it("stays suppressed while the pointer only reports its resting position", () => {
    const host = start();
    pointer(host.target, "pointermove", 120, 40);
    pointer(host.target, "pointermove", 120, 40);
    pointer(host.target, "pointermove", 121, 41);

    expect(suppressed(host)).toBe(true);
  });

  it("resumes hover once the pointer travels the resume distance", () => {
    const host = start();
    pointer(host.target, "pointermove", 120, 40);
    pointer(host.target, "pointermove", 120 + HOVER_RESUME_DISTANCE_PX, 40);

    expect(suppressed(host)).toBe(false);
  });

  it("resumes hover when a button is pressed without moving", () => {
    const host = start();
    pointer(host.target, "pointerdown", 120, 40);

    expect(suppressed(host)).toBe(false);
  });

  it("resumes hover when the wheel turns without moving", () => {
    const host = start();
    pointer(host.target, "pointermove", 120, 40);
    pointer(host.target, "wheel", 120, 40);

    expect(suppressed(host)).toBe(false);
  });

  it("leaves devices without hover untouched", () => {
    const host = start(false);

    expect(suppressed(host)).toBe(false);
  });

  it("suppresses on toggle changes but not on the first reading", () => {
    const host = createHost();
    hosts.push(host);
    const observe = suppressHoverOnToggle(host);

    observe(false);
    expect(suppressed(host)).toBe(false);
    observe(false);
    expect(suppressed(host)).toBe(false);
    observe(true);
    expect(suppressed(host)).toBe(true);
  });

  it("restarts from the next resting position when triggered again", () => {
    const first = start();
    pointer(first.target, "pointermove", 0, 0);
    const second = start();

    expect(suppressed(first)).toBe(false);
    pointer(second.target, "pointermove", 200, 200);
    expect(suppressed(second)).toBe(true);
    pointer(second.target, "pointermove", 200, 200 + HOVER_RESUME_DISTANCE_PX);
    expect(suppressed(second)).toBe(false);
  });
});

describe("following steady pointer positions", () => {
  it("accepts a position only when it continues the one before", () => {
    const steady = createSteadyPointerFilter();
    expect(steady(400, 14)).toBe(false);
    expect(steady(401, 14)).toBe(true);
    expect(steady(401 + POINTER_STEP_MAX_PX, 14)).toBe(true);
  });

  it("rejects replays of where a drag started, alternating with the real pointer position", () => {
    const steady = createSteadyPointerFilter();
    const accepted = [[400, 14], [180, 14], [400, 14], [180, 14], [402, 14]].map(([x, y]) => steady(x, y));
    expect(accepted).toEqual([false, false, false, false, false]);
  });

  it("follows the pointer again once it moves on from a jump", () => {
    const steady = createSteadyPointerFilter();
    steady(400, 14);
    expect(steady(180, 14)).toBe(false);
    expect(steady(400, 14)).toBe(false);
    expect(steady(403, 15)).toBe(true);
  });
});

describe("waiting for deliberate pointer travel", () => {
  function wait(): { target: EventTarget; travelled: () => number; stop: () => void } {
    const target = new EventTarget();
    let count = 0;
    const stop = waitForPointerTravel(target, () => { count += 1; });
    return { target, travelled: () => count, stop };
  }

  it("takes the first move as the resting position, even far from where the wait began", () => {
    const { target, travelled } = wait();
    pointer(target, "pointermove", 400, 300);
    pointer(target, "pointermove", 401, 300);
    expect(travelled()).toBe(0);
    pointer(target, "pointermove", 400, 300 + HOVER_RESUME_DISTANCE_PX);
    expect(travelled()).toBe(1);
  });

  it("ignores replays of where a drag started, alternating with the real pointer position", () => {
    const { target, travelled } = wait();
    pointer(target, "pointermove", 400, 14);
    pointer(target, "pointermove", 180, 14);
    pointer(target, "pointermove", 400, 14);
    pointer(target, "pointermove", 180, 14);
    pointer(target, "pointermove", 180, 14);
    expect(travelled()).toBe(0);
  });

  it("counts movement again from the first move after a jump", () => {
    const { target, travelled } = wait();
    pointer(target, "pointermove", 180, 14);
    pointer(target, "pointermove", 400, 14);
    pointer(target, "pointermove", 401, 14);
    pointer(target, "pointermove", 402, 15);
    expect(travelled()).toBe(0);
    pointer(target, "pointermove", 403, 16);
    expect(travelled()).toBe(1);
  });

  it("waits for a fast sweep to slow down to steps it can follow", () => {
    const { target, travelled } = wait();
    pointer(target, "pointermove", 0, 0);
    pointer(target, "pointermove", POINTER_STEP_MAX_PX + 1, 0);
    pointer(target, "pointermove", 2 * (POINTER_STEP_MAX_PX + 1), 0);
    expect(travelled()).toBe(0);
    pointer(target, "pointermove", 2 * (POINTER_STEP_MAX_PX + 1) + HOVER_RESUME_DISTANCE_PX, 0);
    expect(travelled()).toBe(1);
  });

  it("ends once on a press or a wheel turn and then stops listening", () => {
    const { target, travelled } = wait();
    pointer(target, "pointerdown");
    pointer(target, "wheel");
    pointer(target, "pointermove", 0, 0);
    pointer(target, "pointermove", 50, 50);
    expect(travelled()).toBe(1);
  });

  it("never reports travel after it is stopped", () => {
    const { target, travelled, stop } = wait();
    stop();
    pointer(target, "pointermove", 0, 0);
    pointer(target, "pointermove", 50, 50);
    pointer(target, "pointerdown");
    expect(travelled()).toBe(0);
  });
});
