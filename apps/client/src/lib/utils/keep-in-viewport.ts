import { getViewport } from "$lib/stores/viewport.svelte";
import type { SystemBarInsets } from "$lib/viewport";

/** Distance kept between a floating panel and the edge of the visible viewport. */
export const VIEWPORT_EDGE_MARGIN_PX = 8;

/** A box in viewport coordinates. */
export interface ViewportBox {
  left: number;
  top: number;
  right: number;
  bottom: number;
}

/**
 * Offset that moves a box back inside the viewport, keeping `margin` from each edge. A box larger
 * than the viewport on an axis aligns to the start edge, so its leading content stays reachable.
 */
export function viewportShift(box: ViewportBox, viewport: ViewportBox, margin = VIEWPORT_EDGE_MARGIN_PX): { x: number; y: number } {
  return {
    x: axisShift(box.left, box.right, viewport.left + margin, viewport.right - margin),
    y: axisShift(box.top, box.bottom, viewport.top + margin, viewport.bottom - margin),
  };
}

function axisShift(start: number, end: number, min: number, max: number): number {
  if (end - start > max - min) return min - start;
  if (start < min) return min - start;
  if (end > max) return max - end;
  return 0;
}

/** A box in viewport coordinates with its size. */
export interface ViewportRect extends ViewportBox {
  width: number;
  height: number;
}

/** What {@link safeViewportRectFrom} needs to know about the viewport, in CSS pixels. */
export interface SafeViewportInput {
  /** Visual viewport position and size inside the layout viewport. */
  offsetLeft: number;
  offsetTop: number;
  width: number;
  height: number;
  /** Layout viewport size, which the system bar insets are measured from. */
  layoutWidth: number;
  layoutHeight: number;
  insets: SystemBarInsets;
  /** Top edge of an open soft keyboard, or the visual viewport bottom while it is hidden. */
  visibleBottom: number;
}

/** Return the part of the visual viewport that no system bar, display cutout, or soft keyboard covers. */
export function safeViewportRectFrom(input: SafeViewportInput): ViewportRect {
  const left = Math.max(input.offsetLeft, input.insets.left);
  const top = Math.max(input.offsetTop, input.insets.top);
  const right = Math.max(left, Math.min(input.offsetLeft + input.width, input.layoutWidth - input.insets.right));
  const bottom = Math.max(top, Math.min(
    input.offsetTop + input.height,
    input.layoutHeight - input.insets.bottom,
    input.visibleBottom,
  ));
  return { left, top, right, bottom, width: right - left, height: bottom - top };
}

/**
 * Return the part of the visible viewport that floating panels, menus, and popovers stay inside: clear of the status
 * bar, the system navigation, display cutouts, and an open soft keyboard.
 */
export function safeViewportRect(): ViewportRect {
  const viewport = getViewport();
  return safeViewportRectFrom({
    offsetLeft: viewport.offsetLeft,
    offsetTop: viewport.offsetTop,
    width: viewport.width,
    height: viewport.height,
    layoutWidth: viewport.layoutWidth,
    layoutHeight: viewport.layoutHeight,
    insets: viewport.systemBarInsets,
    visibleBottom: viewport.visibleBottom,
  });
}

/**
 * Nudge a panel that CSS places beside its anchor (for example `position: absolute; left: 0`) back
 * inside the visible viewport. The panel keeps its CSS placement wherever it fits, so desktop
 * layouts are unchanged; on a narrow screen it slides only as far as needed. The shift uses the
 * `translate` property, which composes with any `transform` the panel already has. Pair it with a
 * width capped by the viewport so the panel can always fit.
 */
export function keepInViewport(node: HTMLElement) {
  let frame = 0;

  function place(): void {
    frame = 0;
    node.style.translate = "";
    const shift = viewportShift(node.getBoundingClientRect(), safeViewportRect());
    if (shift.x !== 0 || shift.y !== 0) node.style.translate = `${Math.round(shift.x)}px ${Math.round(shift.y)}px`;
  }

  function schedule(): void {
    if (frame) return;
    frame = requestAnimationFrame(place);
  }

  const observer = typeof ResizeObserver === "undefined" ? null : new ResizeObserver(schedule);
  observer?.observe(node);
  place();
  window.addEventListener("resize", schedule);
  window.addEventListener("scroll", schedule, true);
  window.visualViewport?.addEventListener("resize", schedule);

  return {
    destroy() {
      if (frame) cancelAnimationFrame(frame);
      observer?.disconnect();
      window.removeEventListener("resize", schedule);
      window.removeEventListener("scroll", schedule, true);
      window.visualViewport?.removeEventListener("resize", schedule);
    },
  };
}
