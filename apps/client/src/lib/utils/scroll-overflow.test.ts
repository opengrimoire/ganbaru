// @vitest-environment jsdom

import { afterEach, describe, expect, it, vi } from "vitest";
import { hasVerticalOverflow, scrollOverflowAction } from "./scroll-overflow";

describe("vertical overflow", () => {
  it("treats content that fits, including fractional excess, as not overflowing", () => {
    expect(hasVerticalOverflow({ scrollHeight: 200, clientHeight: 200 })).toBe(false);
    expect(hasVerticalOverflow({ scrollHeight: 200.5, clientHeight: 200 })).toBe(false);
    expect(hasVerticalOverflow({ scrollHeight: 150, clientHeight: 200 })).toBe(false);
  });

  it("detects content taller than the visible area", () => {
    expect(hasVerticalOverflow({ scrollHeight: 202, clientHeight: 200 })).toBe(true);
  });
});

describe("scroll overflow action", () => {
  afterEach(() => {
    vi.unstubAllGlobals();
    document.body.replaceChildren();
  });

  /** Scroll container whose metrics the test controls, since jsdom has no layout. */
  function scroller(metrics: { scrollHeight: number; clientHeight: number }): HTMLDivElement {
    const node = document.createElement("div");
    Object.defineProperty(node, "scrollHeight", { get: () => metrics.scrollHeight });
    Object.defineProperty(node, "clientHeight", { get: () => metrics.clientHeight });
    document.body.append(node);
    return node;
  }

  it("marks the container while it overflows and clears the mark when it fits again", () => {
    const resize: { notify: (() => void) | null } = { notify: null };
    vi.stubGlobal("ResizeObserver", class {
      constructor(callback: () => void) { resize.notify = callback; }
      observe(): void {}
      disconnect(): void {}
    });
    const metrics = { scrollHeight: 300, clientHeight: 100 };
    const node = scroller(metrics);
    const action = scrollOverflowAction(node);
    expect(node.hasAttribute("data-scroll-overflow")).toBe(true);

    metrics.clientHeight = 400;
    resize.notify?.();
    expect(node.hasAttribute("data-scroll-overflow")).toBe(false);
    action.destroy();
  });

  it("refreshes when rows are added and removes the mark on destroy", async () => {
    const metrics = { scrollHeight: 100, clientHeight: 100 };
    const node = scroller(metrics);
    const action = scrollOverflowAction(node);
    expect(node.hasAttribute("data-scroll-overflow")).toBe(false);

    metrics.scrollHeight = 300;
    node.append(document.createElement("div"));
    await Promise.resolve();
    expect(node.hasAttribute("data-scroll-overflow")).toBe(true);
    action.destroy();
    expect(node.hasAttribute("data-scroll-overflow")).toBe(false);
  });
});
