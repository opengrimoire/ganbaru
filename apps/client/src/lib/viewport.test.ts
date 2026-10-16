import { describe, expect, it } from "vitest";
import {
  calculateKeyboardInset,
  parseAndroidKeyboardHeight,
  parseAndroidSystemBarInsets,
  resolvePointerCapabilities,
  viewportOrientation,
} from "./viewport";

describe("parseAndroidSystemBarInsets", () => {
  it("parses native system-bar values in CSS pixels", () => {
    expect(parseAndroidSystemBarInsets("32,0,48,0")).toEqual({
      top: 32,
      right: 0,
      bottom: 48,
      left: 0,
    });
  });

  it("rejects malformed and unbounded bridge values", () => {
    expect(parseAndroidSystemBarInsets("32,0,48")).toBeNull();
    expect(parseAndroidSystemBarInsets("32,0,-1,0")).toBeNull();
    expect(parseAndroidSystemBarInsets("32,0,Infinity,0")).toBeNull();
    expect(parseAndroidSystemBarInsets("32,0,4097,0")).toBeNull();
    expect(parseAndroidSystemBarInsets({ bottom: 48 })).toBeNull();
  });
});

describe("parseAndroidKeyboardHeight", () => {
  it("parses the native keyboard height in CSS pixels", () => {
    expect(parseAndroidKeyboardHeight("0")).toBe(0);
    expect(parseAndroidKeyboardHeight("312")).toBe(312);
  });

  it("rejects missing, malformed, negative, and unbounded values", () => {
    expect(parseAndroidKeyboardHeight(undefined)).toBeNull();
    expect(parseAndroidKeyboardHeight("")).toBeNull();
    expect(parseAndroidKeyboardHeight("12px")).toBeNull();
    expect(parseAndroidKeyboardHeight("-1")).toBeNull();
    expect(parseAndroidKeyboardHeight("4097")).toBeNull();
    expect(parseAndroidKeyboardHeight(312)).toBeNull();
  });
});

describe("calculateKeyboardInset", () => {
  it("measures keyboard occlusion below an offset visual viewport", () => {
    expect(calculateKeyboardInset(800, 500, 20)).toBe(280);
  });

  it("clamps invalid and expanded viewport measurements", () => {
    expect(calculateKeyboardInset(800, 900, 0)).toBe(0);
    expect(calculateKeyboardInset(Number.NaN, 500, 0)).toBe(0);
  });
});

describe("viewportOrientation", () => {
  it("treats square and taller viewports as portrait", () => {
    expect(viewportOrientation(400, 400)).toBe("portrait");
    expect(viewportOrientation(400, 800)).toBe("portrait");
    expect(viewportOrientation(800, 400)).toBe("landscape");
  });
});

describe("resolvePointerCapabilities", () => {
  it("uses media query results when the WebView provides them", () => {
    expect(resolvePointerCapabilities({ matches: true }, { matches: false }, 0)).toEqual({
      coarsePointer: true,
      hoverAvailable: false,
    });
  });

  it("falls back to touch input when media queries are unavailable", () => {
    expect(resolvePointerCapabilities(null, null, 2)).toEqual({
      coarsePointer: true,
      hoverAvailable: false,
    });
    expect(resolvePointerCapabilities(null, null, 0)).toEqual({
      coarsePointer: false,
      hoverAvailable: true,
    });
  });
});
