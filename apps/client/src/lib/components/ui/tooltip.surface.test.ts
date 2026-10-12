// @vitest-environment jsdom

import { afterEach, describe, expect, it } from "vitest";
import { deriveTooltipPalette, tooltipSurfaceColorFor } from "./tooltip";

afterEach(() => {
  document.body.replaceChildren();
});

describe("tooltipSurfaceColorFor", () => {
  it("keeps the same palette when a button changes between normal and hover fills", () => {
    const surface = document.createElement("div");
    surface.style.backgroundColor = "rgb(245, 245, 245)";
    const button = document.createElement("button");
    surface.append(button);
    document.body.append(surface);

    button.style.backgroundColor = "rgb(245, 245, 245)";
    const normalSurface = tooltipSurfaceColorFor(button, "rgb(20, 20, 20)", 12);
    button.style.backgroundColor = "rgb(20, 20, 20)";
    const hoverSurface = tooltipSurfaceColorFor(button, "rgb(20, 20, 20)", 12);

    expect(normalSurface).toBe("rgb(245, 245, 245)");
    expect(hoverSurface).toBe(normalSurface);
    expect(deriveTooltipPalette(hoverSurface)).toEqual(deriveTooltipPalette(normalSurface));
  });

  it("ignores interactive wrappers and translucent highlights over a dark surface", () => {
    const surface = document.createElement("div");
    surface.style.backgroundColor = "rgb(30, 30, 30)";
    const highlight = document.createElement("div");
    highlight.style.backgroundColor = "rgba(255, 255, 255, 0.15)";
    const wrapper = document.createElement("div");
    wrapper.setAttribute("role", "button");
    wrapper.style.backgroundColor = "rgb(250, 250, 250)";
    const icon = document.createElement("span");
    wrapper.append(icon);
    highlight.append(wrapper);
    surface.append(highlight);
    document.body.append(surface);

    expect(tooltipSurfaceColorFor(icon, "rgb(255, 255, 255)", 12)).toBe("rgb(30, 30, 30)");
  });

  it("ignores a noninteractive target's changing fill too", () => {
    const surface = document.createElement("div");
    surface.style.backgroundColor = "rgb(30, 30, 30)";
    const target = document.createElement("div");
    target.dataset.appTooltip = "Details";
    surface.append(target);
    document.body.append(surface);

    target.style.backgroundColor = "rgb(30, 30, 30)";
    const normalSurface = tooltipSurfaceColorFor(target, "rgb(255, 255, 255)", 12);
    target.style.backgroundColor = "rgb(245, 245, 245)";
    expect(tooltipSurfaceColorFor(target, "rgb(255, 255, 255)", 12)).toBe(normalSurface);
  });

  it("uses the theme fallback when no stable local surface exists", () => {
    const button = document.createElement("button");
    document.body.append(button);

    expect(tooltipSurfaceColorFor(button, "rgb(22, 22, 22)", 12)).toBe("rgb(22, 22, 22)");
  });
});
