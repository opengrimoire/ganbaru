export interface SelectPopoverRect {
  top: number;
  right: number;
  bottom: number;
  left: number;
  width: number;
  height: number;
}

export interface SelectPopoverGeometry {
  top: number;
  left: number;
  width: number | null;
  minWidth: number;
  maxWidth: number;
  maxHeight: number;
  placement: "above" | "below";
}

export type SelectPopoverHorizontalAlign = "start" | "end";

interface SelectPopoverGeometryInput {
  triggerRect: SelectPopoverRect;
  boundaryRect: SelectPopoverRect;
  contentHeight: number;
  contentWidth?: number;
  horizontalAlign?: SelectPopoverHorizontalAlign;
  gap?: number;
  inset?: number;
}

function clamp(value: number, min: number, max: number): number {
  if (max < min) return min;
  return Math.min(Math.max(value, min), max);
}

function finiteOrZero(value: number): number {
  return Number.isFinite(value) ? Math.max(0, value) : 0;
}

export function pickSelectPopoverGeometry({
  triggerRect,
  boundaryRect,
  contentHeight,
  contentWidth,
  horizontalAlign = "start",
  gap = 6,
  inset = 8,
}: SelectPopoverGeometryInput): SelectPopoverGeometry {
  const safeGap = finiteOrZero(gap);
  const safeInset = finiteOrZero(inset);
  const topBound = boundaryRect.top + safeInset;
  const bottomBound = boundaryRect.bottom - safeInset;
  const leftBound = boundaryRect.left + safeInset;
  const rightBound = boundaryRect.right - safeInset;

  const belowTop = triggerRect.bottom + safeGap;
  const aboveBottom = triggerRect.top - safeGap;
  const belowSpace = finiteOrZero(bottomBound - belowTop);
  const aboveSpace = finiteOrZero(aboveBottom - topBound);
  const desiredHeight = finiteOrZero(contentHeight);
  const placement = belowSpace >= desiredHeight || belowSpace >= aboveSpace
    ? "below"
    : "above";
  const availableHeight = placement === "below" ? belowSpace : aboveSpace;
  const maxHeight = finiteOrZero(availableHeight);
  const renderedHeight = Math.min(desiredHeight, maxHeight);
  const top = placement === "below"
    ? belowTop
    : Math.max(topBound, aboveBottom - renderedHeight);

  const maxAvailableWidth = finiteOrZero(rightBound - leftBound);
  const triggerWidth = finiteOrZero(triggerRect.width);
  const desiredWidth = Math.max(triggerWidth, finiteOrZero(contentWidth ?? triggerWidth));
  const renderedWidth = Math.min(desiredWidth, maxAvailableWidth);
  const left = horizontalAlign === "end"
    ? clamp(
        clamp(triggerRect.right, leftBound, rightBound) - renderedWidth,
        leftBound,
        rightBound - renderedWidth,
      )
    : clamp(
        triggerRect.left,
        leftBound,
        rightBound - Math.min(triggerWidth, maxAvailableWidth),
      );
  const maxWidth = finiteOrZero(horizontalAlign === "end" ? renderedWidth : rightBound - left);

  return {
    top,
    left,
    width: horizontalAlign === "end" ? renderedWidth : null,
    minWidth: Math.min(triggerWidth, maxWidth),
    maxWidth,
    maxHeight,
    placement,
  };
}
