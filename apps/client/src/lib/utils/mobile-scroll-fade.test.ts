// @vitest-environment jsdom

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { installMobileScrollFades, MOBILE_SCROLL_FADE_ATTRIBUTE } from "./mobile-scroll-fade";
import { scrollEdgeFadeAction } from "./scroll-edge-fade";

class StubResizeObserver {
  observe(): void {}
  unobserve(): void {}
  disconnect(): void {}
}

/** Builds a scroll container with fixed metrics, since jsdom has no layout. */
function scroller(className: string, metrics: { scrollHeight: number; clientHeight: number }): HTMLDivElement {
  const element = document.createElement("div");
  element.className = className;
  Object.defineProperty(element, "scrollHeight", { configurable: true, value: metrics.scrollHeight });
  Object.defineProperty(element, "clientHeight", { configurable: true, value: metrics.clientHeight });
  return element;
}

const OVERFLOWING = { scrollHeight: 400, clientHeight: 200 };
const FITTING = { scrollHeight: 200, clientHeight: 200 };

/** Lets the mutation observer run and the frame it schedules flush. */
async function settle(): Promise<void> {
  await Promise.resolve();
  await Promise.resolve();
}

describe("mobile scroll fades", () => {
  let root: HTMLDivElement;
  let stop: (() => void) | undefined;

  beforeEach(() => {
    vi.stubGlobal("ResizeObserver", StubResizeObserver);
    vi.stubGlobal("requestAnimationFrame", (callback: FrameRequestCallback) => {
      queueMicrotask(() => callback(0));
      return 1;
    });
    vi.stubGlobal("cancelAnimationFrame", () => {});
    root = document.createElement("div");
    document.body.append(root);
  });

  afterEach(() => {
    stop?.();
    stop = undefined;
    root.remove();
    vi.unstubAllGlobals();
  });

  it("fades overflowing scroll containers present at install and leaves fitting ones plain", () => {
    const overflowing = scroller("overflow-y-auto", OVERFLOWING);
    const fitting = scroller("overflow-auto", FITTING);
    root.append(overflowing, fitting);

    stop = installMobileScrollFades(root);

    expect(overflowing.getAttribute("data-scroll-fade")).toBe("bottom");
    expect(fitting.hasAttribute("data-scroll-fade")).toBe(false);
  });

  it("follows containers that appear later, gain the marker, and leave", async () => {
    stop = installMobileScrollFades(root);
    const added = scroller("overflow-y-scroll", OVERFLOWING);
    const wrapper = document.createElement("section");
    wrapper.append(added);
    root.append(wrapper);
    const scoped = scroller("thread-scroll", OVERFLOWING);
    root.append(scoped);
    await settle();
    expect(added.getAttribute("data-scroll-fade")).toBe("bottom");
    expect(scoped.hasAttribute("data-scroll-fade")).toBe(false);

    scoped.setAttribute(MOBILE_SCROLL_FADE_ATTRIBUTE, "");
    await settle();
    expect(scoped.getAttribute("data-scroll-fade")).toBe("bottom");

    wrapper.remove();
    await settle();
    expect(added.hasAttribute("data-scroll-fade")).toBe(false);
  });

  it("leaves the Calendar, text entry, floating surfaces, and containers with their own fade alone", () => {
    const calendar = document.createElement("div");
    calendar.setAttribute("data-mobile-scrollbars", "keep");
    const calendarScroller = scroller("overflow-y-auto", OVERFLOWING);
    calendar.append(calendarScroller);
    const textarea = document.createElement("textarea");
    textarea.className = "overflow-y-auto";
    Object.defineProperty(textarea, "scrollHeight", { value: 400 });
    Object.defineProperty(textarea, "clientHeight", { value: 200 });
    const floating = scroller("surface-floating overflow-y-auto", OVERFLOWING);
    const owned = scroller("overflow-y-auto", OVERFLOWING);
    owned.scrollTop = 100;
    const ownedFade = scrollEdgeFadeAction(owned, { edges: "bottom" });
    root.append(calendar, textarea, floating, owned);

    stop = installMobileScrollFades(root);

    expect(calendarScroller.hasAttribute("data-scroll-fade")).toBe(false);
    expect(textarea.hasAttribute("data-scroll-fade")).toBe(false);
    expect(floating.hasAttribute("data-scroll-fade")).toBe(false);
    owned.dispatchEvent(new Event("scroll"));
    expect(owned.getAttribute("data-scroll-fade")).toBe("bottom");
    ownedFade.destroy();
  });

  it("adds a vertically scrolling container styled elsewhere when it first scrolls", () => {
    const scoped = scroller("channel-feed", OVERFLOWING);
    scoped.style.overflowY = "auto";
    root.append(scoped);
    stop = installMobileScrollFades(root);
    expect(scoped.hasAttribute("data-scroll-fade")).toBe(false);

    scoped.scrollTop = 100;
    scoped.dispatchEvent(new Event("scroll"));

    expect(scoped.getAttribute("data-scroll-fade")).toBe("both");
  });

  it("removes every fade it applied when stopped", () => {
    const overflowing = scroller("overflow-y-auto", OVERFLOWING);
    root.append(overflowing);
    const stopFades = installMobileScrollFades(root);
    expect(overflowing.hasAttribute("data-scroll-fade")).toBe(true);

    stopFades();

    expect(overflowing.hasAttribute("data-scroll-fade")).toBe(false);
  });
});
