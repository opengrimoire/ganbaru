// @vitest-environment jsdom

import { afterEach, describe, expect, it, vi } from "vitest";
import { SMOOTH_SWAP_TIMING, smoothSwapTiming, swapIn, swapOut } from "./smooth-swap";

function element(offsetTop: number, offsetLeft: number, width: number): HTMLElement {
  const node = document.createElement("div");
  node.style.margin = "20px auto 0";
  Object.defineProperty(node, "offsetTop", { value: offsetTop });
  Object.defineProperty(node, "offsetLeft", { value: offsetLeft });
  node.getBoundingClientRect = () => new DOMRect(0, 0, width, 40);
  return node;
}

describe("smooth swap", () => {
  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it("pins leaving content where it stands, out of the flow, while it fades", () => {
    const node = element(36, 12, 320);
    const config = swapOut(node, SMOOTH_SWAP_TIMING);
    expect(node.style.position).toBe("absolute");
    expect(node.style.top).toBe("36px");
    expect(node.style.left).toBe("12px");
    expect(node.style.width).toBe("320px");
    expect(node.style.margin).toBe("0px");
    expect(config.duration).toBe(SMOOTH_SWAP_TIMING.fadeOutMs);
    expect(config.css?.(0.25, 0.75)).toBe("opacity: 0.25");
  });

  it("starts arriving content once the leaving content is gone and the height has nearly settled, and releases a pin left by an interrupted outro", () => {
    const node = element(36, 12, 320);
    swapOut(node, SMOOTH_SWAP_TIMING);
    const config = swapIn(node, SMOOTH_SWAP_TIMING);
    expect(node.style.position).toBe("");
    expect(node.style.top).toBe("");
    expect(node.style.width).toBe("");
    expect(node.style.margin).toBe("");
    expect(config.delay).toBe(SMOOTH_SWAP_TIMING.fadeInDelayMs);
    expect(SMOOTH_SWAP_TIMING.fadeInDelayMs).toBeGreaterThanOrEqual(SMOOTH_SWAP_TIMING.fadeOutMs);
    expect(config.duration).toBe(SMOOTH_SWAP_TIMING.fadeInMs);
  });

  it("leaves content in the flow and drops all motion when the user prefers less", () => {
    vi.stubGlobal("matchMedia", (query: string) => ({ matches: query.includes("reduce") }));
    const timing = smoothSwapTiming();
    expect(timing).toEqual({ heightMs: 0, fadeOutMs: 0, fadeInDelayMs: 0, fadeInMs: 0 });
    const node = element(36, 12, 320);
    expect(swapOut(node, timing).duration).toBe(0);
    expect(node.style.position).toBe("");
  });
});
