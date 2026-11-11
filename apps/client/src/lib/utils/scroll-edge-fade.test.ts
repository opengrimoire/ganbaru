// @vitest-environment jsdom

import { afterEach, describe, expect, it, vi } from "vitest";
import { limitScrollEdgeFade, scrollEdgeFade, scrollEdgeFadeAction, scrollEdgeFadeDistances } from "./scroll-edge-fade";

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

  it("sizes each fade by the distance left to its edge, up to that edge's size", () => {
    const sizes = { top: 24, bottom: 48 };
    const metrics = { scrollHeight: 400, clientHeight: 200 };
    expect(scrollEdgeFadeDistances({ ...metrics, scrollTop: 0 }, "bottom", sizes)).toEqual({ top: 0, bottom: 48 });
    expect(scrollEdgeFadeDistances({ ...metrics, scrollTop: 9.6 }, "both", sizes)).toEqual({ top: 10, bottom: 48 });
    expect(scrollEdgeFadeDistances({ ...metrics, scrollTop: 100 }, "both", sizes)).toEqual({ top: 24, bottom: 48 });
    expect(scrollEdgeFadeDistances({ ...metrics, scrollTop: 180 }, "both", sizes)).toEqual({ top: 24, bottom: 20 });
  });

  it("gives no distance to an edge the fade leaves out", () => {
    const sizes = { top: 24, bottom: 48 };
    expect(scrollEdgeFadeDistances({ scrollTop: 100, scrollHeight: 400, clientHeight: 200 }, "bottom", sizes))
      .toEqual({ top: 0, bottom: 48 });
    expect(scrollEdgeFadeDistances({ scrollTop: 100, scrollHeight: 400, clientHeight: 200 }, "none", sizes))
      .toEqual({ top: 0, bottom: 0 });
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

  it("reads the fade sizes only once an edge fades", () => {
    const computedStyle = vi.spyOn(window, "getComputedStyle");
    const metrics = { scrollHeight: 100, clientHeight: 100 };
    const node = scroller(metrics);
    node.style.setProperty("--scroll-fade-bottom-size", "48px");
    const action = scrollEdgeFadeAction(node);
    expect(computedStyle).not.toHaveBeenCalled();

    metrics.scrollHeight = 300;
    node.dispatchEvent(new Event("scroll"));
    expect(computedStyle).toHaveBeenCalledTimes(1);
    expect(node.style.getPropertyValue("--scroll-fade-bottom-distance")).toBe("48px");
    node.dispatchEvent(new Event("scroll"));
    expect(computedStyle).toHaveBeenCalledTimes(1);
    action.destroy();
    computedStyle.mockRestore();
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

  it("writes the distance left to each fading edge, capped at its fade size, and clears it on destroy", () => {
    const node = scroller({ scrollHeight: 400, clientHeight: 200 });
    node.style.setProperty("--scroll-fade-size", "24px");
    node.style.setProperty("--scroll-fade-bottom-size", "48px");
    node.scrollTop = 10;
    const action = scrollEdgeFadeAction(node);
    expect(node.style.getPropertyValue("--scroll-fade-top-distance")).toBe("10px");
    expect(node.style.getPropertyValue("--scroll-fade-bottom-distance")).toBe("48px");

    node.scrollTop = 170;
    node.dispatchEvent(new Event("scroll"));
    expect(node.style.getPropertyValue("--scroll-fade-top-distance")).toBe("24px");
    expect(node.style.getPropertyValue("--scroll-fade-bottom-distance")).toBe("30px");

    action.destroy();
    expect(node.style.getPropertyValue("--scroll-fade-top-distance")).toBe("");
    expect(node.style.getPropertyValue("--scroll-fade-bottom-distance")).toBe("");
  });
});
