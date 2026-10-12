import { describe, expect, it } from "vitest";
import {
  MOBILE_NAVIGATION_RAIL_MIN_WIDTH,
  mobileCenteredPanelGeometry,
  mobileNavigationPresentation,
  mobileTopBarPanelGeometry,
} from "./layout";

describe("mobileNavigationPresentation", () => {
  it("keeps compact Android navigation in the top bar", () => {
    expect(mobileNavigationPresentation(MOBILE_NAVIGATION_RAIL_MIN_WIDTH - 1)).toBe("top");
  });

  it("uses a rail from the Android medium-width boundary", () => {
    expect(mobileNavigationPresentation(MOBILE_NAVIGATION_RAIL_MIN_WIDTH)).toBe("rail");
    expect(mobileNavigationPresentation(840)).toBe("rail");
  });

  it("falls back safely for invalid measurements", () => {
    expect(mobileNavigationPresentation(Number.NaN)).toBe("top");
    expect(mobileNavigationPresentation(Number.POSITIVE_INFINITY)).toBe("top");
  });
});

describe("mobileCenteredPanelGeometry", () => {
  it("centers the panel on its trigger when the viewport has room", () => {
    expect(mobileCenteredPanelGeometry({
      anchorLeft: 180,
      anchorWidth: 40,
      desiredWidth: 256,
      viewportLeft: 0,
      viewportWidth: 400,
    })).toEqual({ left: 72, width: 256 });
  });

  it("shifts the panel only enough to remain inside either viewport edge", () => {
    expect(mobileCenteredPanelGeometry({
      anchorLeft: 8,
      anchorWidth: 40,
      desiredWidth: 256,
      viewportLeft: 0,
      viewportWidth: 400,
    }).left).toBe(8);
    expect(mobileCenteredPanelGeometry({
      anchorLeft: 352,
      anchorWidth: 40,
      desiredWidth: 256,
      viewportLeft: 0,
      viewportWidth: 400,
    }).left).toBe(136);
  });

  it("shrinks the panel within narrow safe viewport bounds", () => {
    expect(mobileCenteredPanelGeometry({
      anchorLeft: 90,
      anchorWidth: 40,
      desiredWidth: 256,
      viewportLeft: 12,
      viewportWidth: 196,
    })).toEqual({ left: 20, width: 180 });
  });
});

describe("mobileTopBarPanelGeometry", () => {
  it("places a full-size panel below its trigger when the viewport has room", () => {
    expect(mobileTopBarPanelGeometry({
      anchorLeft: 180,
      anchorWidth: 40,
      anchorBottom: 56,
      desiredWidth: 760,
      desiredHeight: 680,
      viewportLeft: 0,
      viewportWidth: 800,
      viewportTop: 0,
      viewportHeight: 760,
    })).toEqual({ left: 8, width: 760, top: 60, height: 680 });
  });

  it("shrinks a panel to the safe space below its trigger", () => {
    expect(mobileTopBarPanelGeometry({
      anchorLeft: 340,
      anchorWidth: 40,
      anchorBottom: 80,
      desiredWidth: 760,
      desiredHeight: 680,
      viewportLeft: 12,
      viewportWidth: 388,
      viewportTop: 24,
      viewportHeight: 700,
    })).toEqual({ left: 20, width: 372, top: 84, height: 632 });
  });
});
