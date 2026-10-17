import {
  getEventPanelUsableHeight,
  type EventPanelLayout,
} from "$lib/utils/responsive";

export interface EventPanelGeometryInput {
  baseLeft: number;
  baseTop: number;
  defaultPanelHeight: number;
  dragOffset: { x: number; y: number };
  gap: number;
  heightConstraintTolerance: number;
  layout: EventPanelLayout;
  minTop: number;
  panelHeight: number;
  pinnedBottom: number;
  pinnedDragY: number;
  titleBarHeight: number;
  viewportHeight: number;
  viewportWidth: number;
  width: number;
}

export function clampFloatingLeft(
  left: number,
  viewportWidth: number,
  width: number,
  gap: number,
): number {
  const maxLeft = Math.max(gap, viewportWidth - width - gap);
  return Math.max(gap, Math.min(maxLeft, left));
}

export function clampFloatingTop(
  top: number,
  viewportHeight: number,
  visibleHeight: number,
  minTop: number,
  gap: number,
): number {
  const maxTop = Math.max(minTop, viewportHeight - visibleHeight - gap);
  return Math.max(minTop, Math.min(maxTop, top));
}

export function getAvailablePanelHeight(
  viewportHeight: number,
  titleBarHeight: number,
  gap: number,
): number {
  return Math.max(
    96,
    getEventPanelUsableHeight(viewportHeight, titleBarHeight, gap),
  );
}

export function getPinnedHeightLimit(input: Pick<
  EventPanelGeometryInput,
  "dragOffset" | "gap" | "minTop" | "pinnedBottom" | "pinnedDragY" | "viewportHeight"
>): number {
  const dragDelta = input.dragOffset.y - input.pinnedDragY;
  const viewportBottom = Math.max(input.minTop + 96, input.viewportHeight - input.gap);
  const effectiveBottom = Math.min(input.pinnedBottom + dragDelta, viewportBottom);
  return Math.max(96, Math.round(effectiveBottom - input.minTop));
}

export function getPanelHeightLimit(input: Pick<
  EventPanelGeometryInput,
  | "dragOffset"
  | "gap"
  | "layout"
  | "minTop"
  | "pinnedBottom"
  | "pinnedDragY"
  | "titleBarHeight"
  | "viewportHeight"
>): number {
  const availableHeight = getAvailablePanelHeight(input.viewportHeight, input.titleBarHeight, input.gap);
  if (input.layout === "bottom") return Math.min(560, availableHeight);
  if (input.pinnedBottom > 0) return getPinnedHeightLimit(input);
  return availableHeight;
}

export function shouldConstrainPanelHeight(
  panelHeight: number,
  limit: number,
  tolerance: number,
): boolean {
  return panelHeight > 0 && panelHeight > limit + tolerance;
}

export function buildEventPanelStyle(input: EventPanelGeometryInput): string {
  const availableHeight = getAvailablePanelHeight(
    input.viewportHeight,
    input.titleBarHeight,
    input.gap,
  );
  const panelHeight = input.panelHeight || input.defaultPanelHeight;
  const visibleHeight = Math.min(panelHeight, availableHeight);

  if (input.layout === "fullscreen") {
    return `position:fixed; left:${input.gap}px; right:${input.gap}px; top:${input.minTop}px; bottom:${input.gap}px; z-index:50;`;
  }

  if (input.layout === "bottom") {
    const maxHeight = getPanelHeightLimit(input);
    const heightCss = shouldConstrainPanelHeight(input.panelHeight, maxHeight, input.heightConstraintTolerance)
      ? `height:${Math.round(maxHeight)}px;`
      : "";
    return `position:fixed; left:${input.gap}px; right:${input.gap}px; bottom:${input.gap}px; max-height:${Math.round(maxHeight)}px; ${heightCss} z-index:50;`;
  }

  const left = clampFloatingLeft(
    input.baseLeft + input.dragOffset.x,
    input.viewportWidth,
    input.width,
    input.gap,
  );
  const rawTop = Math.max(input.minTop, input.baseTop + input.dragOffset.y);

  if (input.pinnedBottom > 0) {
    const dragDelta = input.dragOffset.y - input.pinnedDragY;
    const bottomCss = Math.max(input.gap, Math.round(input.viewportHeight - input.pinnedBottom - dragDelta));
    const maxHeight = getPinnedHeightLimit(input);
    const heightCss = shouldConstrainPanelHeight(input.panelHeight, maxHeight, input.heightConstraintTolerance)
      ? `height:${maxHeight}px;`
      : "";
    return `position:fixed; left:${Math.round(left)}px; bottom:${bottomCss}px; max-height:${maxHeight}px; ${heightCss} width:${Math.round(input.width)}px; z-index:50;`;
  }

  let top = rawTop;
  const overflow = top + visibleHeight + input.gap - input.viewportHeight;
  if (overflow > 0) {
    top = Math.max(input.minTop, top - overflow);
  }

  const heightCss = shouldConstrainPanelHeight(
    input.panelHeight,
    availableHeight,
    input.heightConstraintTolerance,
  )
    ? `height:${Math.round(availableHeight)}px;`
    : "";
  return `position:fixed; left:${Math.round(left)}px; top:${Math.round(top)}px; width:${Math.round(input.width)}px; max-height:${Math.round(availableHeight)}px; ${heightCss} z-index:50;`;
}
