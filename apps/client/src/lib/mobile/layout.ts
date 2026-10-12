export const MOBILE_NAVIGATION_RAIL_MIN_WIDTH = 600;

export type MobileNavigationPresentation = "top" | "rail";

export interface MobileCenteredPanelGeometryInput {
  anchorLeft: number;
  anchorWidth: number;
  desiredWidth: number;
  viewportLeft: number;
  viewportWidth: number;
  inset?: number;
}

export interface MobileCenteredPanelGeometry {
  left: number;
  width: number;
}

export interface MobileTopBarPanelGeometryInput extends MobileCenteredPanelGeometryInput {
  anchorBottom: number;
  desiredHeight: number;
  viewportTop: number;
  viewportHeight: number;
  gap?: number;
}

export interface MobileTopBarPanelGeometry extends MobileCenteredPanelGeometry {
  top: number;
  height: number;
}

function finiteNonNegative(value: number): number {
  return Number.isFinite(value) ? Math.max(0, value) : 0;
}

function clamp(value: number, minimum: number, maximum: number): number {
  if (maximum < minimum) return minimum;
  return Math.min(Math.max(value, minimum), maximum);
}

/** Select primary navigation from stable layout width, independent of the input method. */
export function mobileNavigationPresentation(
  layoutWidth: number,
): MobileNavigationPresentation {
  return Number.isFinite(layoutWidth) && layoutWidth >= MOBILE_NAVIGATION_RAIL_MIN_WIDTH
    ? "rail"
    : "top";
}

/** Center a mobile floating panel on its trigger, then keep it inside the visible viewport. */
export function mobileCenteredPanelGeometry({
  anchorLeft,
  anchorWidth,
  desiredWidth,
  viewportLeft,
  viewportWidth,
  inset = 8,
}: MobileCenteredPanelGeometryInput): MobileCenteredPanelGeometry {
  const safeViewportLeft = Number.isFinite(viewportLeft) ? viewportLeft : 0;
  const safeViewportWidth = finiteNonNegative(viewportWidth);
  const safeInset = Math.min(finiteNonNegative(inset), safeViewportWidth / 2);
  const leftBound = safeViewportLeft + safeInset;
  const availableWidth = Math.max(0, safeViewportWidth - safeInset * 2);
  const width = Math.min(finiteNonNegative(desiredWidth), availableWidth);
  const safeAnchorLeft = Number.isFinite(anchorLeft) ? anchorLeft : leftBound;
  const anchorCenter = safeAnchorLeft + finiteNonNegative(anchorWidth) / 2;
  const maximumLeft = leftBound + availableWidth - width;

  return {
    left: clamp(anchorCenter - width / 2, leftBound, maximumLeft),
    width,
  };
}

/** Place a mobile utility panel below its top-bar trigger within the visible viewport. */
export function mobileTopBarPanelGeometry({
  anchorBottom,
  desiredHeight,
  viewportTop,
  viewportHeight,
  gap = 4,
  ...horizontalInput
}: MobileTopBarPanelGeometryInput): MobileTopBarPanelGeometry {
  const horizontal = mobileCenteredPanelGeometry(horizontalInput);
  const safeViewportTop = Number.isFinite(viewportTop) ? viewportTop : 0;
  const safeViewportHeight = finiteNonNegative(viewportHeight);
  const safeInset = Math.min(
    finiteNonNegative(horizontalInput.inset ?? 8),
    safeViewportHeight / 2,
  );
  const topBound = safeViewportTop + safeInset;
  const bottomBound = safeViewportTop + safeViewportHeight - safeInset;
  const safeAnchorBottom = Number.isFinite(anchorBottom) ? anchorBottom : topBound;
  const top = clamp(safeAnchorBottom + finiteNonNegative(gap), topBound, bottomBound);

  return {
    ...horizontal,
    top,
    height: Math.min(finiteNonNegative(desiredHeight), Math.max(0, bottomBound - top)),
  };
}
