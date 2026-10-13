import { afterEach, describe, expect, it } from "vitest";
import {
  HOVER_RESUME_DISTANCE_PX,
  HOVER_SUPPRESSED_ATTRIBUTE,
  suppressHoverOnToggle,
  suppressHoverUntilPointerMoves,
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
