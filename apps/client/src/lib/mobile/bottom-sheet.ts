/**
 * Resting positions of a bottom sheet, from lowest to highest. `peek` shows the sheet's summary, `half` shows about
 * half the screen, and `full` shows the whole sheet.
 */
export type BottomSheetDetent = "peek" | "half" | "full";

/** Visible heights of a bottom sheet in CSS pixels, measured from the bottom of its area. */
export interface BottomSheetHeights {
  /** Header only, used while the user edits what the sheet covers. */
  dip: number;
  peek: number;
  half: number;
  full: number;
}

/** A pointer position at a moment, used to measure release velocity. */
export interface BottomSheetSample {
  y: number;
  time: number;
}

/** Share of the full sheet height shown at the `half` detent. */
export const BOTTOM_SHEET_HALF_RATIO = 0.55;
/** How far ahead the release velocity carries the sheet before it picks a detent, in milliseconds. */
export const BOTTOM_SHEET_PROJECTION_MS = 200;
/** Share of the peek height below which a released sheet closes instead of settling. */
export const BOTTOM_SHEET_CLOSE_RATIO = 0.55;
/** Only samples this recent count toward release velocity, in milliseconds. */
export const BOTTOM_SHEET_VELOCITY_WINDOW_MS = 100;

const DETENT_ORDER: readonly BottomSheetDetent[] = ["peek", "half", "full"];

function clamp(value: number, min: number, max: number): number {
  return Math.min(Math.max(value, min), max);
}

/**
 * Return the detent heights of a sheet whose full visible height is `fullHeight`, whose header is `headerHeight`
 * tall, and whose summary ends `peekHeight` below its top. Every height stays within the sheet, and the detents never
 * cross: dip, peek, half, and full only grow.
 */
export function bottomSheetHeights(fullHeight: number, headerHeight: number, peekHeight: number): BottomSheetHeights {
  const full = Math.max(0, Math.round(fullHeight));
  const dip = clamp(Math.round(headerHeight), 0, full);
  const peek = clamp(Math.round(peekHeight), dip, full);
  const half = clamp(Math.round(full * BOTTOM_SHEET_HALF_RATIO), peek, full);
  return { dip, peek, half, full };
}

/** Return the visible height of `detent`. */
export function bottomSheetDetentHeight(detent: BottomSheetDetent, heights: BottomSheetHeights): number {
  return heights[detent];
}

/** Return the visible height while a finger that started at `startVisible` has moved `deltaY` (down is positive). */
export function bottomSheetDragHeight(startVisible: number, deltaY: number, heights: BottomSheetHeights): number {
  return clamp(startVisible - deltaY, 0, heights.full);
}

/**
 * Return the release velocity in CSS pixels per millisecond from recent samples, positive when the finger moves up
 * and the sheet opens. Samples older than {@link BOTTOM_SHEET_VELOCITY_WINDOW_MS} before the last one are ignored.
 */
export function bottomSheetReleaseVelocity(samples: readonly BottomSheetSample[]): number {
  const last = samples.at(-1);
  if (!last) return 0;
  const first = samples.find((sample) => last.time - sample.time <= BOTTOM_SHEET_VELOCITY_WINDOW_MS) ?? last;
  const elapsed = last.time - first.time;
  if (elapsed <= 0) return 0;
  return (first.y - last.y) / elapsed;
}

/**
 * Return where a sheet released at `visible` with `velocity` (pixels per millisecond, positive opens) comes to rest:
 * the detent nearest to where the velocity carries it, or `close` when `closable` and it ends well below the peek.
 */
export function settleBottomSheet(
  visible: number,
  velocity: number,
  heights: BottomSheetHeights,
  closable: boolean,
): BottomSheetDetent | "close" {
  const projected = visible + velocity * BOTTOM_SHEET_PROJECTION_MS;
  if (closable && projected < heights.peek * BOTTOM_SHEET_CLOSE_RATIO) return "close";
  let nearest: BottomSheetDetent = "peek";
  for (const detent of DETENT_ORDER) {
    if (Math.abs(heights[detent] - projected) < Math.abs(heights[nearest] - projected)) nearest = detent;
  }
  return nearest;
}

/** Return the higher of two detents. */
export function higherBottomSheetDetent(a: BottomSheetDetent, b: BottomSheetDetent): BottomSheetDetent {
  return DETENT_ORDER.indexOf(a) >= DETENT_ORDER.indexOf(b) ? a : b;
}

/** Vertical span on screen, in CSS pixels. */
export interface VerticalSpan {
  top: number;
  bottom: number;
}

/**
 * Return how far to scroll content so `target` sits inside `area` with `margin` around it, in CSS pixels; positive
 * scrolls the content up. A target taller than the area keeps its top visible.
 */
export function revealScrollDelta(target: VerticalSpan, area: VerticalSpan, margin: number): number {
  const top = area.top + margin;
  const bottom = area.bottom - margin;
  if (bottom <= top) return 0;
  if (target.top < top) return target.top - top;
  if (target.bottom > bottom) return Math.min(target.bottom - bottom, target.top - top);
  return 0;
}
