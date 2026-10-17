// @vitest-environment jsdom

import { afterEach, describe, expect, it, vi } from "vitest";

const STORAGE_KEY = "ganbaru-zoom";

async function loadMobileZoom() {
  vi.resetModules();
  return import("./zoom.mobile.svelte");
}

describe("mobile interface scale", () => {
  afterEach(() => {
    localStorage.clear();
    document.documentElement.style.zoom = "";
    document.documentElement.style.removeProperty("--mobile-interface-scale-inverse");
  });

  it("applies the saved scale as soon as the mobile store loads", async () => {
    localStorage.setItem(STORAGE_KEY, "1.25");

    const { getZoom } = await loadMobileZoom();

    expect(getZoom().percent).toBe(125);
    expect(document.documentElement.style.zoom).toBe("1.25");
    expect(document.documentElement.style.getPropertyValue("--mobile-interface-scale-inverse"))
      .toBe("0.8");
  });

  it("persists and applies the nearest supported scale", async () => {
    const { getZoom } = await loadMobileZoom();
    const zoom = getZoom();

    zoom.setLevel(0.87);

    expect(zoom.percent).toBe(90);
    expect(localStorage.getItem(STORAGE_KEY)).toBe("0.9");
    expect(document.documentElement.style.zoom).toBe("0.9");
  });

  it("supports meaningful enlargement and reset", async () => {
    const { getZoom } = await loadMobileZoom();
    const zoom = getZoom();

    zoom.setLevel(1.5);
    expect(zoom.percent).toBe(150);

    zoom.reset();
    expect(zoom.percent).toBe(100);
    expect(document.documentElement.style.zoom).toBe("1");
  });
});
