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

export interface ViewerPoint {
  x: number;
  y: number;
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

/** Scale that fits the whole image in the viewport without enlarging images smaller than it. */
export function viewerFitScale(image: ViewerSize, viewport: ViewerSize): number {
  if (!(image.width > 0) || !(image.height > 0) || !(viewport.width > 0) || !(viewport.height > 0)) return 1;
  return Math.min(1, viewport.width / image.width, viewport.height / image.height);
}

/** Largest scale for an image, which still allows a few steps of zoom on images that fit at full size. */
export function viewerMaxScale(image: ViewerSize, viewport: ViewerSize): number {
  return Math.max(QUICK_NOTE_VIEWER_MAX_SCALE, viewerFitScale(image, viewport) * QUICK_NOTE_VIEWER_MAX_SCALE);
}

/** The fitted, centered transform. */
export function viewerFit(image: ViewerSize, viewport: ViewerSize): ViewerTransform {
  return { scale: viewerFitScale(image, viewport), x: 0, y: 0 };
}

/** Whether the image is shown larger than its fitted size. */
export function viewerIsZoomed(transform: ViewerTransform, image: ViewerSize, viewport: ViewerSize): boolean {
  return transform.scale > viewerFitScale(image, viewport) * 1.001;
}

/** Keeps the scale in range and the image covering the viewport on each axis where it is larger than it. */
export function clampViewerTransform(
  transform: ViewerTransform,
  image: ViewerSize,
  viewport: ViewerSize,
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
  viewport: ViewerSize,
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
  viewport: ViewerSize,
  point: ViewerPoint = { x: viewport.width / 2, y: viewport.height / 2 },
): ViewerTransform {
  return zoomViewerAt(transform, transform.scale * factor, point, image, viewport);
}

/** Moves the image by a pointer or wheel delta. */
export function panViewer(
  transform: ViewerTransform,
  delta: ViewerPoint,
  image: ViewerSize,
  viewport: ViewerSize,
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
  viewport: ViewerSize,
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
  viewport: ViewerSize,
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
  viewport: ViewerSize,
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
export function viewerSwipeAction(offset: ViewerPoint, viewport: ViewerSize): "previous" | "next" | "close" | null {
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
