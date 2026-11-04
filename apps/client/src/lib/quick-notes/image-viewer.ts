/** Screen pixels per image pixel; a viewer never shows an image larger than this. */
export const QUICK_NOTE_VIEWER_MAX_SCALE = 4;
/** Factor one zoom step or key press applies. */
export const QUICK_NOTE_VIEWER_ZOOM_STEP = 1.25;
/** Magnification relative to the fitted size that a click or double tap zooms to. */
export const QUICK_NOTE_VIEWER_DETAIL_ZOOM = 2.5;

export interface ViewerSize {
  width: number;
  height: number;
}

/** The viewer stage, with the room a fitted image leaves free for controls, summed over both sides of each axis. */
export interface ViewerViewport extends ViewerSize {
  padding?: ViewerSize;
}

export interface ViewerPoint {
  x: number;
  y: number;
}

/** A rectangle relative to the stage's top left corner. */
export interface ViewerRect {
  x: number;
  y: number;
  width: number;
  height: number;
}

/** Distances from each stage edge, in pixels. */
export interface ViewerInsets {
  top: number;
  right: number;
  bottom: number;
  left: number;
}

/**
 * How the viewer shows an image: `scale` is screen pixels per image pixel, and `x` and `y` move the image center
 * away from the viewport center, in screen pixels.
 */
export interface ViewerTransform {
  scale: number;
  x: number;
  y: number;
}

/** Stages narrower than this give images their full width and keep room only for the toolbar. */
const VIEWER_COMPACT_WIDTH = 640;
/** Room kept above and below a fitted image for the toolbar, per side; compact stages are touch screens with taller controls. */
const VIEWER_TOOLBAR_ROOM = 64;
const VIEWER_COMPACT_TOOLBAR_ROOM = 72;
/** Room kept beside a fitted image, per side; browsing several images leaves space for the arrows. */
const VIEWER_SIDE_ROOM = 32;
const VIEWER_BROWSING_SIDE_ROOM = 72;

/** Room a fitted image leaves free on a stage, so the image sits clear of the toolbar and arrows. */
export function viewerFitPadding(stage: ViewerSize, browsing: boolean): ViewerSize {
  if (stage.width < VIEWER_COMPACT_WIDTH) return { width: 0, height: 2 * VIEWER_COMPACT_TOOLBAR_ROOM };
  return {
    width: 2 * (browsing ? VIEWER_BROWSING_SIDE_ROOM : VIEWER_SIDE_ROOM),
    height: 2 * VIEWER_TOOLBAR_ROOM,
  };
}

/** Whether the stage has a size yet; an unmeasured stage cannot fit an image. */
export function viewerViewportMeasured(viewport: ViewerSize): boolean {
  return viewport.width > 0 && viewport.height > 0;
}

/**
 * Scale that fits the whole image in the viewport, inside its padding, without enlarging images smaller than it.
 * Padding that would leave no room is ignored.
 */
export function viewerFitScale(image: ViewerSize, viewport: ViewerViewport): number {
  if (!(image.width > 0) || !(image.height > 0) || !viewerViewportMeasured(viewport)) return 1;
  const paddedWidth = viewport.width - (viewport.padding?.width ?? 0);
  const paddedHeight = viewport.height - (viewport.padding?.height ?? 0);
  const width = paddedWidth > 0 ? paddedWidth : viewport.width;
  const height = paddedHeight > 0 ? paddedHeight : viewport.height;
  return Math.min(1, width / image.width, height / image.height);
}

/** Zoom relative to the fitted size, so the whole image reads as 100%. */
export function viewerRelativeZoom(transform: ViewerTransform, image: ViewerSize, viewport: ViewerViewport): number {
  return transform.scale / viewerFitScale(image, viewport);
}

/** Largest scale for an image, which still allows a few steps of zoom on images that fit at full size. */
export function viewerMaxScale(image: ViewerSize, viewport: ViewerViewport): number {
  return Math.max(QUICK_NOTE_VIEWER_MAX_SCALE, viewerFitScale(image, viewport) * QUICK_NOTE_VIEWER_MAX_SCALE);
}

/** The fitted, centered transform. */
export function viewerFit(image: ViewerSize, viewport: ViewerViewport): ViewerTransform {
  return { scale: viewerFitScale(image, viewport), x: 0, y: 0 };
}

/** A transform with the stage it was computed for. */
export interface ViewerPlacement {
  transform: ViewerTransform;
  viewport: ViewerViewport;
}

/**
 * Transform after the stage or image changes. Without a current placement, or when the image was not zoomed on the
 * stage it was placed for, the image fits; a zoomed image stays where it is, within bounds. Returns null until the
 * stage is measured, since a fit computed then would show the image at full size.
 */
export function viewerLayoutTransform(
  current: ViewerPlacement | null,
  image: ViewerSize,
  viewport: ViewerViewport,
): ViewerTransform | null {
  if (!viewerViewportMeasured(viewport)) return null;
  if (!current || !viewerViewportMeasured(current.viewport) || !viewerIsZoomed(current.transform, image, current.viewport)) {
    return viewerFit(image, viewport);
  }
  return clampViewerTransform(current.transform, image, viewport);
}

/** Whether the image is shown larger than its fitted size. */
export function viewerIsZoomed(transform: ViewerTransform, image: ViewerSize, viewport: ViewerViewport): boolean {
  return transform.scale > viewerFitScale(image, viewport) * 1.001;
}

/** Keeps the scale in range and the image covering the viewport on each axis where it is larger than it. */
export function clampViewerTransform(
  transform: ViewerTransform,
  image: ViewerSize,
  viewport: ViewerViewport,
): ViewerTransform {
  const scale = Math.min(
    viewerMaxScale(image, viewport),
    Math.max(viewerFitScale(image, viewport), transform.scale),
  );
  const limitX = Math.max(0, (image.width * scale - viewport.width) / 2);
  const limitY = Math.max(0, (image.height * scale - viewport.height) / 2);
  return {
    scale,
    x: Math.min(limitX, Math.max(-limitX, transform.x)),
    y: Math.min(limitY, Math.max(-limitY, transform.y)),
  };
}

/**
 * Zooms to `scale` while keeping the image point under `point` in place. `point` is relative to the viewport's
 * top left corner.
 */
export function zoomViewerAt(
  transform: ViewerTransform,
  scale: number,
  point: ViewerPoint,
  image: ViewerSize,
  viewport: ViewerViewport,
): ViewerTransform {
  const bounded = clampViewerTransform({ ...transform, scale }, image, viewport).scale;
  const ratio = bounded / transform.scale;
  const offsetX = point.x - viewport.width / 2;
  const offsetY = point.y - viewport.height / 2;
  return clampViewerTransform(
    {
      scale: bounded,
      x: offsetX - (offsetX - transform.x) * ratio,
      y: offsetY - (offsetY - transform.y) * ratio,
    },
    image,
    viewport,
  );
}

/** Multiplies the scale by `factor` around `point`, or around the viewport center when no point is given. */
export function zoomViewerBy(
  transform: ViewerTransform,
  factor: number,
  image: ViewerSize,
  viewport: ViewerViewport,
  point: ViewerPoint = { x: viewport.width / 2, y: viewport.height / 2 },
): ViewerTransform {
  return zoomViewerAt(transform, transform.scale * factor, point, image, viewport);
}

/** Moves the image by a pointer or wheel delta. */
export function panViewer(
  transform: ViewerTransform,
  delta: ViewerPoint,
  image: ViewerSize,
  viewport: ViewerViewport,
): ViewerTransform {
  return clampViewerTransform(
    { scale: transform.scale, x: transform.x + delta.x, y: transform.y + delta.y },
    image,
    viewport,
  );
}

/** A click or double tap: fits a zoomed image, or zooms in on the clicked point. */
export function toggleViewerZoom(
  transform: ViewerTransform,
  point: ViewerPoint,
  image: ViewerSize,
  viewport: ViewerViewport,
): ViewerTransform {
  if (viewerIsZoomed(transform, image, viewport)) return viewerFit(image, viewport);
  const fit = viewerFitScale(image, viewport);
  const detail = Math.max(fit * QUICK_NOTE_VIEWER_DETAIL_ZOOM, Math.min(1, viewerMaxScale(image, viewport)));
  return zoomViewerAt(transform, detail, point, image, viewport);
}

/**
 * Follows a two finger pinch from one pair of points to the next: the scale changes with the finger distance and
 * the image moves with their midpoint.
 */
export function pinchViewer(
  transform: ViewerTransform,
  previous: readonly [ViewerPoint, ViewerPoint],
  next: readonly [ViewerPoint, ViewerPoint],
  image: ViewerSize,
  viewport: ViewerViewport,
): ViewerTransform {
  const previousDistance = Math.hypot(previous[1].x - previous[0].x, previous[1].y - previous[0].y);
  const nextDistance = Math.hypot(next[1].x - next[0].x, next[1].y - next[0].y);
  const previousMid = { x: (previous[0].x + previous[1].x) / 2, y: (previous[0].y + previous[1].y) / 2 };
  const nextMid = { x: (next[0].x + next[1].x) / 2, y: (next[0].y + next[1].y) / 2 };
  const factor = previousDistance > 0 && nextDistance > 0 ? nextDistance / previousDistance : 1;
  const zoomed = zoomViewerAt(transform, transform.scale * factor, previousMid, image, viewport);
  return panViewer(zoomed, { x: nextMid.x - previousMid.x, y: nextMid.y - previousMid.y }, image, viewport);
}

/** Index of the next image in `direction`, wrapping at the ends. */
export function stepViewerIndex(index: number, count: number, direction: -1 | 1): number {
  if (count <= 0) return 0;
  return (((index + direction) % count) + count) % count;
}

/** Image index to show after removing the image at `removed`, or null when none remain. */
export function viewerIndexAfterRemoval(removed: number, count: number): number | null {
  if (count <= 1) return null;
  return Math.min(removed, count - 2);
}

/** Whether a viewport point falls on the shown image. */
export function viewerPointOnImage(
  point: ViewerPoint,
  transform: ViewerTransform,
  image: ViewerSize,
  viewport: ViewerViewport,
): boolean {
  const centerX = viewport.width / 2 + transform.x;
  const centerY = viewport.height / 2 + transform.y;
  return Math.abs(point.x - centerX) <= (image.width * transform.scale) / 2
    && Math.abs(point.y - centerY) <= (image.height * transform.scale) / 2;
}

/** Shortest horizontal swipe that changes image, in pixels; narrow viewports need less. */
const SWIPE_STEP_DISTANCE = 72;
/** Shortest downward swipe that closes the viewer, in pixels. */
const SWIPE_CLOSE_DISTANCE = 110;

/** What a drag on a fitted image does when it ends: a mostly horizontal swipe changes image, a downward one closes. */
export function viewerSwipeAction(offset: ViewerPoint, viewport: ViewerViewport): "previous" | "next" | "close" | null {
  const stepDistance = Math.min(SWIPE_STEP_DISTANCE, viewport.width * 0.18);
  const closeDistance = Math.min(SWIPE_CLOSE_DISTANCE, viewport.height * 0.2);
  if (Math.abs(offset.x) > Math.abs(offset.y)) {
    if (offset.x <= -stepDistance) return "next";
    if (offset.x >= stepDistance) return "previous";
    return null;
  }
  return offset.y >= closeDistance ? "close" : null;
}

/** Zoom change per wheel pixel; trackpad pinches report smaller deltas, so they zoom faster per pixel. */
const WHEEL_ZOOM_RATE = 0.0015;
const PINCH_ZOOM_RATE = 0.01;
/** Pixels one wheel line scrolls when the browser reports lines. */
const WHEEL_LINE_PIXELS = 16;

/**
 * Scale factor for one wheel event: scrolling up zooms in. `deltaMode` follows `WheelEvent`, where 1 counts lines and
 * 2 counts pages.
 */
export function viewerWheelFactor(deltaY: number, deltaMode: number, pageHeight: number, pinch: boolean): number {
  const unit = deltaMode === 1 ? WHEEL_LINE_PIXELS : deltaMode === 2 ? Math.max(1, pageHeight) : 1;
  const delta = Number.isFinite(deltaY) ? deltaY * unit : 0;
  return Math.exp(-delta * (pinch ? PINCH_ZOOM_RATE : WHEEL_ZOOM_RATE));
}

/**
 * How the image layer moves for an open or close flight between the shown frame and a tile. The layer scales by
 * `scale` around the stage's top left corner and then moves by `x` and `y`, which puts the frame's center on the
 * tile's center. The scale covers the tile, and `clip` keeps only the part of the frame the tile shows, so a tile
 * that crops its image is matched exactly.
 */
export interface ViewerFlight {
  scale: number;
  x: number;
  y: number;
  /** Insets of the visible part from the layer's edges, in layer pixels before scaling. */
  clip: ViewerInsets;
}

/** The flight that shows `frame` where `target` is, or null when either has no area. */
export function viewerFlight(frame: ViewerRect, target: ViewerRect, stage: ViewerSize): ViewerFlight | null {
  if (!(frame.width > 0) || !(frame.height > 0) || !(target.width > 0) || !(target.height > 0)) return null;
  const scale = Math.max(target.width / frame.width, target.height / frame.height);
  const frameX = frame.x + frame.width / 2;
  const frameY = frame.y + frame.height / 2;
  const halfWidth = target.width / (2 * scale);
  const halfHeight = target.height / (2 * scale);
  return {
    scale,
    x: target.x + target.width / 2 - scale * frameX,
    y: target.y + target.height / 2 - scale * frameY,
    clip: {
      top: frameY - halfHeight,
      right: stage.width - (frameX + halfWidth),
      bottom: stage.height - (frameY + halfHeight),
      left: frameX - halfWidth,
    },
  };
}

/** Clip that leaves the visible part of a resting frame showing, as insets from the stage edges. */
export function viewerFrameClip(frame: ViewerRect, stage: ViewerSize): ViewerInsets {
  return {
    top: Math.max(0, frame.y),
    right: Math.max(0, stage.width - frame.x - frame.width),
    bottom: Math.max(0, stage.height - frame.y - frame.height),
    left: Math.max(0, frame.x),
  };
}

/** Whether a tile is far enough on screen for a flight to read as coming from or going back to it. */
export function viewerTargetVisible(target: ViewerRect, stage: ViewerSize): boolean {
  if (!(target.width > 0) || !(target.height > 0)) return false;
  const visibleWidth = Math.min(stage.width, target.x + target.width) - Math.max(0, target.x);
  const visibleHeight = Math.min(stage.height, target.y + target.height) - Math.max(0, target.y);
  return visibleWidth >= target.width / 2 && visibleHeight >= target.height / 2;
}
