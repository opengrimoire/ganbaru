import { describe, expect, it } from "vitest";
import {
  clampViewerTransform,
  panViewer,
  pinchViewer,
  QUICK_NOTE_VIEWER_MAX_SCALE,
  stepViewerIndex,
  toggleViewerZoom,
  viewerFit,
  viewerFitPadding,
  viewerFitScale,
  viewerFlight,
  viewerFrameClip,
  viewerIndexAfterRemoval,
  viewerIsZoomed,
  viewerLayoutTransform,
  viewerMaxScale,
  viewerPointOnImage,
  viewerRelativeZoom,
  viewerSwipeAction,
  viewerTargetVisible,
  viewerWheelFactor,
  zoomViewerAt,
  zoomViewerBy,
} from "./image-viewer";

const WIDE = { width: 2000, height: 1000 };
const SMALL = { width: 200, height: 100 };
const VIEWPORT = { width: 1000, height: 800 };

describe("Quick note image viewer fit", () => {
  it("shrinks large images to fit and never enlarges small ones", () => {
    expect(viewerFitScale(WIDE, VIEWPORT)).toBe(0.5);
    expect(viewerFitScale(SMALL, VIEWPORT)).toBe(1);
    expect(viewerFitScale({ width: 0, height: 10 }, VIEWPORT)).toBe(1);
    expect(viewerFit(WIDE, VIEWPORT)).toEqual({ scale: 0.5, x: 0, y: 0 });
  });

  it("fits inside the padding and ignores padding that would leave no room", () => {
    const padded = { ...VIEWPORT, padding: { width: 200, height: 100 } };
    expect(viewerFitScale(WIDE, padded)).toBe(0.4);
    expect(viewerFitScale(WIDE, { ...VIEWPORT, padding: { width: 1200, height: 900 } })).toBe(0.5);
    expect(viewerRelativeZoom(viewerFit(WIDE, padded), WIDE, padded)).toBe(1);
    expect(viewerRelativeZoom({ scale: 0.8, x: 0, y: 0 }, WIDE, padded)).toBe(2);
  });

  it("keeps room for the arrows only on wide stages that browse several images", () => {
    expect(viewerFitPadding({ width: 1200, height: 800 }, true).width).toBeGreaterThan(
      viewerFitPadding({ width: 1200, height: 800 }, false).width,
    );
    expect(viewerFitPadding({ width: 400, height: 800 }, true).width).toBe(0);
    expect(viewerFitPadding({ width: 400, height: 800 }, true).height).toBeGreaterThan(0);
  });

  it("waits for a measured stage before fitting, then fits on the first measurement", () => {
    const unmeasured = { width: 0, height: 0 };
    expect(viewerLayoutTransform(null, WIDE, unmeasured)).toBeNull();
    expect(viewerLayoutTransform(null, WIDE, { width: 0, height: 600 })).toBeNull();
    const early = { transform: viewerFit(WIDE, unmeasured), viewport: unmeasured };
    expect(early.transform.scale).toBe(1);
    expect(viewerLayoutTransform(early, WIDE, VIEWPORT)).toEqual({ scale: 0.5, x: 0, y: 0 });
    expect(viewerLayoutTransform(null, WIDE, VIEWPORT)).toEqual({ scale: 0.5, x: 0, y: 0 });
  });

  it("refits a fitted image when the stage shrinks and keeps a zoomed one in bounds", () => {
    const fitted = { transform: viewerFit(WIDE, VIEWPORT), viewport: VIEWPORT };
    expect(viewerLayoutTransform(fitted, WIDE, { width: 500, height: 800 })).toEqual({ scale: 0.25, x: 0, y: 0 });
    const zoomed = { transform: { scale: 1, x: 400, y: 0 }, viewport: VIEWPORT };
    expect(viewerLayoutTransform(zoomed, WIDE, { width: 1600, height: 800 })).toEqual({ scale: 1, x: 200, y: 0 });
  });

  it("allows zooming past full size on every image", () => {
    expect(viewerMaxScale(WIDE, VIEWPORT)).toBe(QUICK_NOTE_VIEWER_MAX_SCALE);
    expect(viewerMaxScale(SMALL, VIEWPORT)).toBe(QUICK_NOTE_VIEWER_MAX_SCALE);
  });
});

describe("Quick note image viewer transforms", () => {
  it("keeps the scale in range and the image covering the viewport", () => {
    expect(clampViewerTransform({ scale: 0.1, x: 300, y: 300 }, WIDE, VIEWPORT)).toEqual({ scale: 0.5, x: 0, y: 0 });
    expect(clampViewerTransform({ scale: 1, x: 900, y: -900 }, WIDE, VIEWPORT)).toEqual({ scale: 1, x: 500, y: -100 });
    expect(clampViewerTransform({ scale: 20, x: 0, y: 0 }, WIDE, VIEWPORT).scale).toBe(QUICK_NOTE_VIEWER_MAX_SCALE);
  });

  it("keeps the image point under the cursor in place while zooming", () => {
    const point = { x: 750, y: 400 };
    const before = viewerFit(WIDE, VIEWPORT);
    const after = zoomViewerAt(before, 1, point, WIDE, VIEWPORT);
    expect(after).toEqual({ scale: 1, x: -250, y: 0 });
    const imageX = (transform: typeof after) => (point.x - VIEWPORT.width / 2 - transform.x) / transform.scale;
    expect(imageX(after)).toBeCloseTo(imageX(before));
  });

  it("zooms around the center by default and reports zoom state", () => {
    const zoomed = zoomViewerBy(viewerFit(WIDE, VIEWPORT), 2, WIDE, VIEWPORT);
    expect(zoomed).toEqual({ scale: 1, x: 0, y: 0 });
    expect(viewerIsZoomed(zoomed, WIDE, VIEWPORT)).toBe(true);
    expect(viewerIsZoomed(viewerFit(WIDE, VIEWPORT), WIDE, VIEWPORT)).toBe(false);
  });

  it("pans only as far as the image edges", () => {
    const zoomed = { scale: 1, x: 0, y: 0 };
    expect(panViewer(zoomed, { x: 200, y: 50 }, WIDE, VIEWPORT)).toEqual({ scale: 1, x: 200, y: 50 });
    expect(panViewer(zoomed, { x: -2000, y: 400 }, WIDE, VIEWPORT)).toEqual({ scale: 1, x: -500, y: 100 });
    expect(panViewer(viewerFit(WIDE, VIEWPORT), { x: 100, y: 100 }, WIDE, VIEWPORT)).toEqual({ scale: 0.5, x: 0, y: 0 });
  });

  it("toggles between the fitted size and a detail zoom", () => {
    const fit = viewerFit(WIDE, VIEWPORT);
    const detail = toggleViewerZoom(fit, { x: 500, y: 400 }, WIDE, VIEWPORT);
    expect(detail.scale).toBe(1.25);
    expect(toggleViewerZoom(detail, { x: 500, y: 400 }, WIDE, VIEWPORT)).toEqual(fit);
  });

  it("zooms small images to at least full size on a detail zoom", () => {
    const fit = viewerFit({ width: 900, height: 700 }, VIEWPORT);
    expect(toggleViewerZoom(fit, { x: 500, y: 400 }, { width: 900, height: 700 }, VIEWPORT).scale).toBe(2.5);
  });

  it("follows the finger distance and midpoint during a pinch", () => {
    const fit = viewerFit(WIDE, VIEWPORT);
    const spread = pinchViewer(
      fit,
      [{ x: 450, y: 400 }, { x: 550, y: 400 }],
      [{ x: 400, y: 400 }, { x: 600, y: 400 }],
      WIDE,
      VIEWPORT,
    );
    expect(spread).toEqual({ scale: 1, x: 0, y: 0 });
    const moved = pinchViewer(
      spread,
      [{ x: 400, y: 400 }, { x: 600, y: 400 }],
      [{ x: 460, y: 420 }, { x: 660, y: 420 }],
      WIDE,
      VIEWPORT,
    );
    expect(moved).toEqual({ scale: 1, x: 60, y: 20 });
  });
});

describe("Quick note image viewer navigation", () => {
  it("wraps at both ends", () => {
    expect(stepViewerIndex(2, 3, 1)).toBe(0);
    expect(stepViewerIndex(0, 3, -1)).toBe(2);
    expect(stepViewerIndex(1, 3, 1)).toBe(2);
    expect(stepViewerIndex(4, 0, 1)).toBe(0);
  });

  it("shows the following image after a removal, or the new last one", () => {
    expect(viewerIndexAfterRemoval(0, 1)).toBeNull();
    expect(viewerIndexAfterRemoval(0, 3)).toBe(0);
    expect(viewerIndexAfterRemoval(1, 3)).toBe(1);
    expect(viewerIndexAfterRemoval(2, 3)).toBe(1);
  });

  it("knows whether a point is on the shown image", () => {
    const fit = viewerFit(WIDE, VIEWPORT);
    expect(viewerPointOnImage({ x: 500, y: 400 }, fit, WIDE, VIEWPORT)).toBe(true);
    expect(viewerPointOnImage({ x: 500, y: 100 }, fit, WIDE, VIEWPORT)).toBe(false);
    expect(viewerPointOnImage({ x: 500, y: 100 }, { scale: 1, x: 0, y: 0 }, WIDE, VIEWPORT)).toBe(true);
  });
});

describe("Quick note image viewer gestures", () => {
  it("turns horizontal swipes into navigation and downward swipes into closing", () => {
    expect(viewerSwipeAction({ x: -80, y: 10 }, VIEWPORT)).toBe("next");
    expect(viewerSwipeAction({ x: 80, y: 0 }, VIEWPORT)).toBe("previous");
    expect(viewerSwipeAction({ x: 50, y: 0 }, VIEWPORT)).toBeNull();
    expect(viewerSwipeAction({ x: 10, y: 120 }, VIEWPORT)).toBe("close");
    expect(viewerSwipeAction({ x: 10, y: -200 }, VIEWPORT)).toBeNull();
  });

  it("needs shorter swipes on narrow viewports", () => {
    expect(viewerSwipeAction({ x: -60, y: 0 }, { width: 300, height: 500 })).toBe("next");
    expect(viewerSwipeAction({ x: 0, y: 100 }, { width: 300, height: 500 })).toBe("close");
  });

  it("zooms in when scrolling up and counts lines and pages", () => {
    expect(viewerWheelFactor(-100, 0, 800, false)).toBeGreaterThan(1);
    expect(viewerWheelFactor(100, 0, 800, false)).toBeLessThan(1);
    expect(viewerWheelFactor(3, 1, 800, false)).toBeCloseTo(viewerWheelFactor(48, 0, 800, false));
    expect(viewerWheelFactor(1, 2, 800, false)).toBeCloseTo(viewerWheelFactor(800, 0, 800, false));
    expect(viewerWheelFactor(-10, 0, 800, true)).toBeGreaterThan(viewerWheelFactor(-10, 0, 800, false));
    expect(viewerWheelFactor(Number.NaN, 0, 800, false)).toBe(1);
  });
});

describe("Quick note image viewer flights", () => {
  const STAGE = { width: 1000, height: 800 };

  it("moves the frame center onto the tile and covers the tile", () => {
    const frame = { x: 100, y: 200, width: 800, height: 400 };
    const tile = { x: 20, y: 30, width: 100, height: 100 };
    const flight = viewerFlight(frame, tile, STAGE);
    expect(flight?.scale).toBe(0.25);
    expect((flight?.scale ?? 0) * 500 + (flight?.x ?? 0)).toBeCloseTo(70);
    expect((flight?.scale ?? 0) * 400 + (flight?.y ?? 0)).toBeCloseTo(80);
  });

  it("clips the frame to the part a cropping tile shows", () => {
    const frame = { x: 100, y: 200, width: 800, height: 400 };
    const flight = viewerFlight(frame, { x: 20, y: 30, width: 100, height: 100 }, STAGE);
    expect(flight?.clip).toEqual({ top: 200, right: 300, bottom: 200, left: 300 });
  });

  it("has no flight for empty rectangles", () => {
    expect(viewerFlight({ x: 0, y: 0, width: 0, height: 10 }, { x: 0, y: 0, width: 10, height: 10 }, STAGE)).toBeNull();
    expect(viewerFlight({ x: 0, y: 0, width: 10, height: 10 }, { x: 0, y: 0, width: 10, height: 0 }, STAGE)).toBeNull();
  });

  it("clips a resting frame to the stage", () => {
    expect(viewerFrameClip({ x: 100, y: 200, width: 800, height: 400 }, STAGE)).toEqual({
      top: 200,
      right: 100,
      bottom: 200,
      left: 100,
    });
    expect(viewerFrameClip({ x: -500, y: -100, width: 2000, height: 1000 }, STAGE)).toEqual({
      top: 0,
      right: 0,
      bottom: 0,
      left: 0,
    });
  });

  it("flies only to tiles that are mostly on screen", () => {
    expect(viewerTargetVisible({ x: 10, y: 10, width: 100, height: 100 }, STAGE)).toBe(true);
    expect(viewerTargetVisible({ x: -60, y: 10, width: 100, height: 100 }, STAGE)).toBe(false);
    expect(viewerTargetVisible({ x: 10, y: 790, width: 100, height: 100 }, STAGE)).toBe(false);
    expect(viewerTargetVisible({ x: 10, y: 10, width: 0, height: 100 }, STAGE)).toBe(false);
  });
});
