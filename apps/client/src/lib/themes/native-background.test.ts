// @vitest-environment jsdom

import { afterEach, describe, expect, it } from "vitest";
import { parseOpaqueComputedColor, readMountedRootBackground } from "./native-background";

afterEach(() => {
  document.body.innerHTML = "";
  document.documentElement.removeAttribute("style");
});

describe("parseOpaqueComputedColor", () => {
  it.each([
    ["rgb(38, 38, 38)", { r: 38, g: 38, b: 38 }],
    ["rgba(1,2,3,1)", { r: 1, g: 2, b: 3 }],
    [" rgba(255, 0, 128, 1.0) ", { r: 255, g: 0, b: 128 }],
  ])("reads opaque sRGB %s", (value, expected) => {
    expect(parseOpaqueComputedColor(value)).toEqual(expected);
  });

  it.each(["rgba(0, 0, 0, 0)", "rgba(10, 10, 10, 0.5)", "rgb(256, 0, 0)", "oklch(0.2 0 0)", "transparent", ""])(
    "rejects %s, which the native layers cannot paint as the page does",
    (value) => {
      expect(parseOpaqueComputedColor(value)).toBeNull();
    },
  );
});

describe("readMountedRootBackground", () => {
  it("uses the background the mounted root paints", () => {
    document.body.innerHTML = '<div id="app"><main style="background-color: rgb(20, 30, 40)"></main></div>';
    document.documentElement.style.setProperty("--sidebar", "#ffffff");
    expect(readMountedRootBackground(document.getElementById("app")!, "--sidebar")).toEqual({ r: 20, g: 30, b: 40 });
  });

  it("falls back to the theme token when the root paints no opaque background", () => {
    document.body.innerHTML = '<div id="app"><main style="background-color: rgba(0, 0, 0, 0)"></main></div>';
    document.documentElement.style.setProperty("--sidebar", "#102030");
    expect(readMountedRootBackground(document.getElementById("app")!, "--sidebar")).toEqual({ r: 16, g: 32, b: 48 });
  });

  it("falls back to the theme token when nothing is mounted", () => {
    document.body.innerHTML = '<div id="app"></div>';
    document.documentElement.style.setProperty("--sidebar", "#000000");
    expect(readMountedRootBackground(document.getElementById("app")!, "--sidebar")).toEqual({ r: 0, g: 0, b: 0 });
  });

  it("returns null when neither source yields an opaque color", () => {
    document.body.innerHTML = '<div id="app"></div>';
    expect(readMountedRootBackground(document.getElementById("app")!, "--sidebar")).toBeNull();
  });
});
