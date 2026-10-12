export type ViewportSizeClass = "micro" | "narrow" | "compact" | "regular" | "wide";
export type EventPanelLayout = "anchored" | "centered" | "bottom" | "fullscreen";
export type ThemeEditorLayout = "floating" | "sheet" | "fullscreen";
export type ThemeEditorDensity = "comfortable" | "compact" | "micro";
export type ColorPickerLayout = "popover" | "sheet" | "fullscreen";

export const EVENT_PANEL_MAX_WIDTH = 320;
export const EVENT_PANEL_EDGE_MARGIN = 8;
export const EVENT_PANEL_TITLE_BAR_HEIGHT = 42;
export const EVENT_PANEL_MIN_USABLE_HEIGHT = 280;
export const THEME_EDITOR_WIDTH = 700;
export const THEME_EDITOR_MAX_HEIGHT = 640;
export const THEME_EDITOR_EDGE_MARGIN = 16;
export const THEME_EDITOR_SHEET_HEIGHT_RATIO = 0.72;
export const THEME_EDITOR_FLOATING_HEIGHT_RATIO = 0.8;
export const THEME_EDITOR_MIN_SHEET_HEIGHT = 360;
export const COLOR_PICKER_WIDTH = 228;
export const COLOR_PICKER_HEIGHT = 280;
export const COLOR_PICKER_EDGE_MARGIN = 8;

const SIZE_ORDER: readonly ViewportSizeClass[] = [
  "micro",
  "narrow",
  "compact",
  "regular",
  "wide",
];

export interface ViewportSize {
  width: number;
  height: number;
}

export interface PanelRect {
  x: number;
  y: number;
  width: number;
  height: number;
}

export interface EventPanelAnchor {
  /** Right edge of the trigger rect; the left edge is `x - width`. */
  x: number;
  y: number;
  width: number;
  height: number;
}

export interface EventPanelLayoutInput {
  viewport: ViewportSize;
  anchor: EventPanelAnchor;
  panelWidth?: number;
  edgeMargin?: number;
  titleBarHeight?: number;
  minUsableHeight?: number;
}

export interface ThemeEditorLayoutInput {
  viewport: ViewportSize;
  preferredWidth?: number;
  maxHeight?: number;
  edgeMargin?: number;
  titleBarHeight?: number;
  floatingHeightRatio?: number;
  sheetHeightRatio?: number;
  minSheetHeight?: number;
}

export interface ThemeEditorGeometry {
  layout: ThemeEditorLayout;
  density: ThemeEditorDensity;
  rect: PanelRect;
  canDrag: boolean;
}

export interface ColorPickerLayoutInput {
  viewport: ViewportSize;
  trigger: PanelRect;
  popoverWidth?: number;
  popoverHeight?: number;
  edgeMargin?: number;
  titleBarHeight?: number;
}

export interface ColorPickerGeometry {
  layout: ColorPickerLayout;
  rect: PanelRect;
}

export interface RectMargins {
  top: number;
  right: number;
  bottom: number;
  left: number;
}

function finiteOrZero(value: number): number {
  return Number.isFinite(value) ? Math.max(0, value) : 0;
}

export function classifyViewport(width: number, height: number): ViewportSizeClass {
  const w = finiteOrZero(width);
  const h = finiteOrZero(height);

  if (w < 390 || h < 360) return "micro";
  if (w < 560) return "narrow";
  if (w < 800 || h < 600) return "compact";
  if (w < 1100 || h < 700) return "regular";
  return "wide";
}

export function compareSizeClasses(a: ViewportSizeClass, b: ViewportSizeClass): number {
  return SIZE_ORDER.indexOf(a) - SIZE_ORDER.indexOf(b);
}

export function isSizeClassAtLeast(
  current: ViewportSizeClass,
  minimum: ViewportSizeClass,
): boolean {
  return compareSizeClasses(current, minimum) >= 0;
}

export function getResponsivePanelWidth(
  viewportWidth: number,
  maxWidth = EVENT_PANEL_MAX_WIDTH,
  margin = EVENT_PANEL_EDGE_MARGIN,
): number {
  const width = finiteOrZero(viewportWidth);
  const cap = finiteOrZero(maxWidth);
  const edge = finiteOrZero(margin);
  return Math.max(0, Math.min(cap, width - edge * 2));
}

export function getEventPanelUsableHeight(
  viewportHeight: number,
  titleBarHeight = EVENT_PANEL_TITLE_BAR_HEIGHT,
  edgeMargin = EVENT_PANEL_EDGE_MARGIN,
): number {
  return Math.max(
    0,
    finiteOrZero(viewportHeight) - finiteOrZero(titleBarHeight) - finiteOrZero(edgeMargin) * 2,
  );
}

export function pickEventPanelLayout(input: EventPanelLayoutInput): EventPanelLayout {
  const width = finiteOrZero(input.viewport.width);
  const edge = finiteOrZero(input.edgeMargin ?? EVENT_PANEL_EDGE_MARGIN);
  const panelWidth = finiteOrZero(input.panelWidth ?? EVENT_PANEL_MAX_WIDTH);
  const minUsableHeight = finiteOrZero(input.minUsableHeight ?? EVENT_PANEL_MIN_USABLE_HEIGHT);
  const usableHeight = getEventPanelUsableHeight(
    input.viewport.height,
    input.titleBarHeight ?? EVENT_PANEL_TITLE_BAR_HEIGHT,
    edge,
  );

  if (usableHeight < minUsableHeight) return "fullscreen";
  if (width < panelWidth + edge * 2) return "bottom";

  const anchorLeft = finiteOrZero(input.anchor.x) - finiteOrZero(input.anchor.width);
  const anchorRight = finiteOrZero(input.anchor.x);
  const requiredSideSpace = panelWidth + edge * 2;
  const fitsLeft = anchorLeft >= requiredSideSpace;
  const fitsRight = width - anchorRight >= requiredSideSpace;

  return fitsLeft || fitsRight ? "anchored" : "centered";
}

export function pickThemeEditorGeometry(
  input: ThemeEditorLayoutInput,
): ThemeEditorGeometry {
  const viewportWidth = finiteOrZero(input.viewport.width);
  const viewportHeight = finiteOrZero(input.viewport.height);
  const edge = finiteOrZero(input.edgeMargin ?? THEME_EDITOR_EDGE_MARGIN);
  const titleBarHeight = finiteOrZero(
    input.titleBarHeight ?? EVENT_PANEL_TITLE_BAR_HEIGHT,
  );
  const preferredWidth = finiteOrZero(input.preferredWidth ?? THEME_EDITOR_WIDTH);
  const maxHeight = finiteOrZero(input.maxHeight ?? THEME_EDITOR_MAX_HEIGHT);
  const floatingHeightRatio = finiteOrZero(
    input.floatingHeightRatio ?? THEME_EDITOR_FLOATING_HEIGHT_RATIO,
  );
  const sheetHeightRatio = finiteOrZero(
    input.sheetHeightRatio ?? THEME_EDITOR_SHEET_HEIGHT_RATIO,
  );
  const minSheetHeight = finiteOrZero(
    input.minSheetHeight ?? THEME_EDITOR_MIN_SHEET_HEIGHT,
  );
  const sizeClass = classifyViewport(viewportWidth, viewportHeight);

  if (sizeClass === "micro" || viewportHeight < titleBarHeight + minSheetHeight) {
    const height = Math.max(0, viewportHeight - titleBarHeight);
    return {
      layout: "fullscreen",
      density: "micro",
      rect: { x: 0, y: titleBarHeight, width: viewportWidth, height },
      canDrag: false,
    };
  }

  if (viewportWidth < preferredWidth + edge * 2) {
    const width = Math.max(0, viewportWidth - edge * 2);
    const height = Math.min(
      maxHeight,
      Math.round(viewportHeight * sheetHeightRatio),
      Math.max(0, viewportHeight - titleBarHeight - edge * 2),
    );
    return {
      layout: "sheet",
      density: width < 340 ? "micro" : width < 620 ? "compact" : "comfortable",
      rect: {
        x: edge,
        y: Math.max(titleBarHeight, viewportHeight - height - edge),
        width,
        height,
      },
      canDrag: false,
    };
  }

  const width = preferredWidth;
  const height = Math.min(
    maxHeight,
    Math.round(viewportHeight * floatingHeightRatio),
    Math.max(0, viewportHeight - edge * 2),
  );
  return {
    layout: "floating",
    density: "comfortable",
    rect: {
      x: Math.round((viewportWidth - width) / 2),
      y: Math.max(edge, Math.round((viewportHeight - height) / 2)),
      width,
      height,
    },
    canDrag: true,
  };
}

export function pickColorPickerGeometry(
  input: ColorPickerLayoutInput,
): ColorPickerGeometry {
  const viewportWidth = finiteOrZero(input.viewport.width);
  const viewportHeight = finiteOrZero(input.viewport.height);
  const edge = finiteOrZero(input.edgeMargin ?? COLOR_PICKER_EDGE_MARGIN);
  const titleBarHeight = finiteOrZero(
    input.titleBarHeight ?? EVENT_PANEL_TITLE_BAR_HEIGHT,
  );
  const popoverWidth = finiteOrZero(input.popoverWidth ?? COLOR_PICKER_WIDTH);
  const popoverHeight = finiteOrZero(input.popoverHeight ?? COLOR_PICKER_HEIGHT);

  if (viewportWidth < 480 || viewportHeight < 430) {
    const width = Math.max(0, viewportWidth - edge * 2);
    const height = Math.min(
      popoverHeight,
      Math.max(0, viewportHeight - titleBarHeight - edge * 2),
    );
    return {
      layout: "sheet",
      rect: {
        x: edge,
        y: Math.max(titleBarHeight, viewportHeight - height - edge),
        width,
        height,
      },
    };
  }

  let left = finiteOrZero(input.trigger.x);
  if (left + popoverWidth + edge > viewportWidth) {
    left = Math.max(edge, viewportWidth - popoverWidth - edge);
  }
  let top = finiteOrZero(input.trigger.y) + finiteOrZero(input.trigger.height) + 6;
  if (top + popoverHeight + edge > viewportHeight) {
    top = Math.max(edge, finiteOrZero(input.trigger.y) - popoverHeight - 6);
  }
  return {
    layout: "popover",
    rect: { x: left, y: top, width: popoverWidth, height: popoverHeight },
  };
}

export function normalizeMargins(margins: number | Partial<RectMargins>): RectMargins {
  if (typeof margins === "number") {
    const value = finiteOrZero(margins);
    return { top: value, right: value, bottom: value, left: value };
  }
  return {
    top: finiteOrZero(margins.top ?? 0),
    right: finiteOrZero(margins.right ?? 0),
    bottom: finiteOrZero(margins.bottom ?? 0),
    left: finiteOrZero(margins.left ?? 0),
  };
}

export function clampPanelRect(
  rect: PanelRect,
  viewport: ViewportSize,
  margins: number | Partial<RectMargins>,
): PanelRect {
  const m = normalizeMargins(margins);
  const viewportWidth = finiteOrZero(viewport.width);
  const viewportHeight = finiteOrZero(viewport.height);
  const maxWidth = Math.max(0, viewportWidth - m.left - m.right);
  const maxHeight = Math.max(0, viewportHeight - m.top - m.bottom);
  const width = Math.min(finiteOrZero(rect.width), maxWidth);
  const height = Math.min(finiteOrZero(rect.height), maxHeight);
  const minX = m.left;
  const maxX = Math.max(minX, viewportWidth - m.right - width);
  const minY = m.top;
  const maxY = Math.max(minY, viewportHeight - m.bottom - height);
  const x = Math.min(Math.max(finiteOrZero(rect.x), minX), maxX);
  const y = Math.min(Math.max(finiteOrZero(rect.y), minY), maxY);
  return { x, y, width, height };
}
