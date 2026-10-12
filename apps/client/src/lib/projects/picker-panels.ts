import type { MenuAimPoint, MenuAimRect, MenuAimSide } from "$lib/utils/menu-aim";
import type { ProjectNavigatorPanelMode } from "./toolbar";

export type ProjectPickerMobilePane = "groups" | "projects" | "search";
export type ProjectPickerMobileBackAction =
  | "close-group-creator"
  | "close-project-creator"
  | "clear-search"
  | "show-groups"
  | "close-picker";

export interface ProjectPickerMobileState {
  mode: ProjectNavigatorPanelMode;
  activeGroupId: string | null;
  searchActive: boolean;
  createGroupOpen: boolean;
  createProjectGroupId: string | null;
}

export interface ProjectPickerPanelBounds {
  left: number;
  right: number;
  top: number;
  bottom: number;
}

export interface ProjectPickerPanelRect extends ProjectPickerPanelBounds {
  width: number;
  height: number;
}

export interface ProjectPickerPanelFrame {
  left: number;
  top: number;
  width: number;
  height: number;
  maxHeight: number;
}

export interface ProjectPickerBridgeFrame {
  left: number;
  top: number;
  width: number;
  height: number;
}

export interface ProjectPickerSubpanelGeometry {
  panel: ProjectPickerPanelFrame;
  bridge: ProjectPickerBridgeFrame;
  openRight: boolean;
}

export interface ProjectPickerScrollState {
  scrollable: boolean;
  canScrollUp: boolean;
  canScrollDown: boolean;
}

function boundedProjectPickerNumber(value: number | null): number | null {
  if (value === null || !Number.isFinite(value)) return null;
  return Math.max(0, value);
}

function projectPickerVisibleRows(itemCount: number, visibleRows: number | null): number {
  if (visibleRows === null) return itemCount;
  return Math.min(itemCount, visibleRows);
}

function projectPickerSubpanelFallbackVisibleRows(projectCount: number, visibleRows: number | null): number {
  if (visibleRows === null) return projectCount;
  return Math.max(1, Math.min(visibleRows, projectCount));
}

function projectPickerRoundedStyleValue(value: number): number {
  return Math.round(value);
}

/** Resolve the single visible pane used by the compact project selector. */
export function projectPickerMobilePane(
  state: Pick<ProjectPickerMobileState, "mode" | "activeGroupId" | "searchActive">,
): ProjectPickerMobilePane {
  if (state.searchActive) return "search";
  if (state.mode === "groups" && state.activeGroupId === null) return "groups";
  return "projects";
}

/** Resolve the next nested layer consumed by Android Back inside the selector. */
export function projectPickerMobileBackAction(
  state: ProjectPickerMobileState,
): ProjectPickerMobileBackAction {
  if (state.createProjectGroupId !== null) return "close-project-creator";
  if (state.createGroupOpen) return "close-group-creator";
  if (state.searchActive) return "clear-search";
  if (state.mode === "groups" && state.activeGroupId !== null) return "show-groups";
  return "close-picker";
}

export function projectPickerPanelEstimatedListHeight(input: {
  itemCount: number;
  visibleRows: number | null;
  listPadding: number;
  rowHeight: number;
}): number {
  return input.listPadding + input.rowHeight * projectPickerVisibleRows(input.itemCount, input.visibleRows);
}

export function projectPickerPanelHeight(input: {
  headerHeight: number;
  footerHeight: number;
  listHeight: number;
  maxHeight: number | null;
  visibleRows: number | null;
  listPadding: number;
  rowHeight: number;
}): number {
  const naturalHeight = Math.ceil(input.headerHeight + input.listHeight + input.footerHeight);
  const availableMaxHeight = boundedProjectPickerNumber(input.maxHeight);
  const rowCapHeight = input.visibleRows === null
    ? null
    : input.headerHeight + input.footerHeight + input.listPadding + input.rowHeight * input.visibleRows;

  let cappedHeight: number;
  if (availableMaxHeight === null && rowCapHeight === null) {
    cappedHeight = naturalHeight;
  } else if (availableMaxHeight === null) {
    cappedHeight = Math.min(naturalHeight, rowCapHeight ?? naturalHeight);
  } else if (rowCapHeight === null) {
    cappedHeight = Math.min(naturalHeight, availableMaxHeight);
  } else {
    cappedHeight = Math.min(naturalHeight, availableMaxHeight, rowCapHeight);
  }

  return Number.isFinite(cappedHeight) ? Math.round(cappedHeight) : naturalHeight;
}

export function projectPickerMenuAimRect(rect: ProjectPickerPanelBounds): MenuAimRect {
  return {
    left: rect.left,
    right: rect.right,
    top: rect.top,
    bottom: rect.bottom,
  };
}

export function projectPickerPointerPoint(point: { clientX: number; clientY: number }): MenuAimPoint {
  return { x: point.clientX, y: point.clientY };
}

export function projectPickerSubpanelSide(
  anchorRect: ProjectPickerPanelBounds,
  subpanelRect: ProjectPickerPanelBounds,
): MenuAimSide {
  if (subpanelRect.right <= anchorRect.left) return "left";
  if (subpanelRect.left >= anchorRect.right) return "right";
  return subpanelRect.left < anchorRect.left ? "left" : "right";
}

export function projectPickerSubpanelAimOrigin(anchorRect: ProjectPickerPanelRect): MenuAimPoint {
  return {
    x: anchorRect.left + anchorRect.width / 2,
    y: anchorRect.top + anchorRect.height / 2,
  };
}

/** Place a submenu within its bounds, using its rendered height when available. */
export function projectPickerSubpanelGeometry(input: {
  anchorRect: ProjectPickerPanelRect;
  panelRect: ProjectPickerPanelRect;
  bounds: ProjectPickerPanelBounds;
  gap: number;
  footerHeight: number;
  projectCount: number;
  visibleRows: number | null;
  panelHeight?: number;
  listHeight?: number;
  listPadding: number;
  rowHeight: number;
}): ProjectPickerSubpanelGeometry {
  const minLeft = input.bounds.left;
  const maxRight = Math.max(minLeft, input.bounds.right);
  const minTop = input.bounds.top;
  const maxBottom = Math.max(minTop, input.bounds.bottom);
  const usableWidth = Math.max(0, maxRight - minLeft);
  const width = Math.min(Math.max(input.panelRect.width, 0), usableWidth);
  const rightOrigin = Math.min(Math.max(input.anchorRect.right, input.panelRect.left), input.panelRect.right);
  const leftOrigin = Math.min(Math.max(input.anchorRect.left, input.panelRect.left), input.panelRect.right);
  const spaceRight = maxRight - rightOrigin - input.gap;
  const spaceLeft = leftOrigin - minLeft - input.gap;
  const openRight = spaceRight >= width || spaceRight >= spaceLeft;
  const left = openRight
    ? Math.min(rightOrigin + input.gap, maxRight - width)
    : Math.max(minLeft, leftOrigin - input.gap - width);
  const fallbackVisibleRows = projectPickerSubpanelFallbackVisibleRows(input.projectCount, input.visibleRows);
  const fallbackListHeight = input.listPadding + input.rowHeight * fallbackVisibleRows;
  const listHeight = input.listHeight ?? fallbackListHeight;
  const naturalHeight = Math.ceil(
    boundedProjectPickerNumber(input.panelHeight ?? null) ?? listHeight + input.footerHeight,
  );
  const rowCapHeight = input.visibleRows === null
    ? null
    : input.listPadding + input.rowHeight * input.visibleRows + input.footerHeight;
  const boundsMaxHeight = Math.max(0, maxBottom - minTop);
  const maxHeight = rowCapHeight === null
    ? boundsMaxHeight
    : Math.min(rowCapHeight, boundsMaxHeight);
  const height = Math.min(maxHeight, naturalHeight);
  const top = Math.min(
    Math.max(minTop, input.anchorRect.top),
    Math.max(minTop, maxBottom - height),
  );
  const panelRight = left + width;
  const bridgeLeft = openRight ? rightOrigin : panelRight;
  const bridgeRight = openRight ? left : leftOrigin;
  const bridgeTop = Math.max(minTop, Math.min(input.anchorRect.top, top));
  const bridgeBottom = Math.min(maxBottom, Math.max(input.anchorRect.bottom, top + height));

  return {
    panel: {
      left,
      top,
      width,
      height,
      maxHeight,
    },
    bridge: {
      left: bridgeLeft,
      top: bridgeTop,
      width: Math.max(0, bridgeRight - bridgeLeft),
      height: Math.max(0, bridgeBottom - bridgeTop),
    },
    openRight,
  };
}

export function projectPickerPanelFrameStyle(frame: ProjectPickerPanelFrame): string {
  return [
    `left: ${projectPickerRoundedStyleValue(frame.left)}px`,
    `top: ${projectPickerRoundedStyleValue(frame.top)}px`,
    `width: ${projectPickerRoundedStyleValue(frame.width)}px`,
    `height: ${projectPickerRoundedStyleValue(frame.height)}px`,
    `max-height: ${projectPickerRoundedStyleValue(frame.maxHeight)}px`,
  ].join("; ");
}

export function projectPickerBridgeFrameStyle(frame: ProjectPickerBridgeFrame): string {
  return [
    `left: ${projectPickerRoundedStyleValue(frame.left)}px`,
    `top: ${projectPickerRoundedStyleValue(frame.top)}px`,
    `width: ${Math.max(0, projectPickerRoundedStyleValue(frame.width))}px`,
    `height: ${Math.max(0, projectPickerRoundedStyleValue(frame.height))}px`,
  ].join("; ");
}

export function projectPickerScrollState(input: {
  scrollHeight: number;
  clientHeight: number;
  scrollTop: number;
}): ProjectPickerScrollState {
  const maxScrollTop = input.scrollHeight - input.clientHeight;
  return {
    scrollable: maxScrollTop > 1,
    canScrollUp: input.scrollTop > 1,
    canScrollDown: input.scrollTop < maxScrollTop - 1,
  };
}
