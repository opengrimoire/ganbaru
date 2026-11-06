// @vitest-environment jsdom

import { afterEach, describe, expect, it, vi } from "vitest";
import { limitScrollEdgeFade, scrollEdgeFade, scrollEdgeFadeAction } from "./scroll-edge-fade";

describe("scroll edge fade", () => {
  it("does not fade content that fits", () => {
    expect(scrollEdgeFade({ scrollTop: 0, scrollHeight: 200, clientHeight: 200 })).toBe("none");
    expect(scrollEdgeFade({ scrollTop: 0, scrollHeight: 200.5, clientHeight: 200 })).toBe("none");
  });

  it("fades only the bottom at the start of scrollable content", () => {
    expect(scrollEdgeFade({ scrollTop: 0, scrollHeight: 400, clientHeight: 200 })).toBe("bottom");
  });

  it("fades both edges in the middle of scrollable content", () => {
    expect(scrollEdgeFade({ scrollTop: 100, scrollHeight: 400, clientHeight: 200 })).toBe("both");
  });

  it("fades only the top at the end, tolerating fractional offsets", () => {
    expect(scrollEdgeFade({ scrollTop: 200, scrollHeight: 400, clientHeight: 200 })).toBe("top");
    expect(scrollEdgeFade({ scrollTop: 199.5, scrollHeight: 400, clientHeight: 200 })).toBe("top");
  });

  it("drops the top fade for bottom-only containers and keeps every fade otherwise", () => {
    expect(limitScrollEdgeFade("top", "bottom")).toBe("none");
    expect(limitScrollEdgeFade("both", "bottom")).toBe("bottom");
    expect(limitScrollEdgeFade("bottom", "bottom")).toBe("bottom");
    expect(limitScrollEdgeFade("none", "bottom")).toBe("none");
    expect(limitScrollEdgeFade("both", "both")).toBe("both");
    expect(limitScrollEdgeFade("top", "both")).toBe("top");
  });
});

describe("scroll edge fade action", () => {
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

  it("applies the fade within the resize observation instead of a later frame", () => {
    const resize: { notify: (() => void) | null } = { notify: null };
    vi.stubGlobal("ResizeObserver", class {
      constructor(callback: () => void) { resize.notify = callback; }
      observe(): void {}
      disconnect(): void {}
    });
    const requestFrame = vi.fn();
    vi.stubGlobal("requestAnimationFrame", requestFrame);
    const metrics = { scrollHeight: 200, clientHeight: 400 };
    const node = scroller(metrics);
    const action = scrollEdgeFadeAction(node);
    expect(node.hasAttribute("data-scroll-fade")).toBe(false);

    metrics.clientHeight = 100;
    resize.notify?.();
    expect(node.getAttribute("data-scroll-fade")).toBe("bottom");
    expect(requestFrame).not.toHaveBeenCalled();
    action.destroy();
    expect(node.hasAttribute("data-scroll-fade")).toBe(false);
  });

  it("refreshes when rows are added to a capped container", async () => {
    const metrics = { scrollHeight: 100, clientHeight: 100 };
    const node = scroller(metrics);
    const action = scrollEdgeFadeAction(node);
    expect(node.hasAttribute("data-scroll-fade")).toBe(false);

    metrics.scrollHeight = 300;
    node.append(document.createElement("div"));
    await Promise.resolve();
    expect(node.getAttribute("data-scroll-fade")).toBe("bottom");
    action.destroy();
  });

  it("fades only the bottom edge of a bottom-only container while scrolled", () => {
    const node = scroller({ scrollHeight: 400, clientHeight: 200 });
    node.scrollTop = 100;
    const action = scrollEdgeFadeAction(node, { edges: "bottom" });
    expect(node.getAttribute("data-scroll-fade")).toBe("bottom");

    node.scrollTop = 200;
    node.dispatchEvent(new Event("scroll"));
    expect(node.hasAttribute("data-scroll-fade")).toBe(false);

    action.update({});
    expect(node.getAttribute("data-scroll-fade")).toBe("top");
    action.destroy();
  });
});
