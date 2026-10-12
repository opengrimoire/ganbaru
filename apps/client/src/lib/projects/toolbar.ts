import type {
  ProjectCustomFieldFilter,
  ProjectTaskListColumn,
} from "./types";

export type ProjectToolbarPanel = "group" | "filters" | "sort" | "customize" | "settings";
export type ProjectNavigatorPanelMode = "groups" | "projects";
export type ProjectTaskModalLayout = "modal" | "sheet" | "fullscreen";

export interface ProjectFilterChipInput {
  search: string;
  statusLabel?: string;
  sectionLabel?: string;
  priorityLabel?: string;
  dueLabel?: string;
  scheduleLabel?: string;
  dependencyLabel?: string;
  tagFilterLabel?: string;
  customFieldFilters: readonly ProjectCustomFieldFilter[];
  customFieldFilterLabel: (filter: ProjectCustomFieldFilter) => string;
}

export interface ProjectFilterChip {
  id: string;
  label: string;
  clearTarget:
    | "search"
    | "status"
    | "section"
    | "priority"
    | "due"
    | "schedule"
    | "dependency"
    | "tag"
    | `custom:${string}`;
}

export interface ProjectListColumnControl {
  column: ProjectTaskListColumn;
  label: string;
  visible: boolean;
}

export interface ProjectTaskModalLayoutInput {
  viewportWidth: number;
  viewportHeight: number;
  titleBarHeight?: number;
  edgeMargin?: number;
}

export interface ProjectNavigatorPanelGeometryInput {
  anchorLeft: number;
  anchorBottom: number;
  viewportWidth: number;
  viewportHeight: number;
  boundsLeft?: number;
  boundsRight?: number;
  boundsTop?: number;
  boundsBottom?: number;
  edgeMargin?: number;
  gap?: number;
  preferredWidth?: number;
  compactBreakpoint?: number;
  compactTop?: number;
}

export interface ProjectNavigatorPanelGeometry {
  left: number;
  top: number;
  width: number;
  height: number;
}

export interface ProjectToolbarPanelGeometryInput {
  anchorLeft: number;
  anchorRight: number;
  anchorTop: number;
  anchorBottom: number;
  viewportWidth: number;
  viewportHeight: number;
  edgeMargin?: number;
  gap?: number;
  preferredWidth?: number;
  preferredHeight?: number;
  compactBreakpoint?: number;
}

export interface ProjectToolbarPanelGeometry {
  left: number;
  top: number;
  width: number;
  maxHeight: number;
}

export const PROJECT_TASK_MODAL_TITLE_BAR_HEIGHT = 42;
export const PROJECT_TASK_MODAL_EDGE_MARGIN = 12;
export const PROJECT_TASK_MODAL_MIN_FULLSCREEN_WIDTH = 390;
export const PROJECT_TASK_MODAL_MIN_FULLSCREEN_HEIGHT = 420;
export const PROJECT_TASK_MODAL_SHEET_WIDTH = 720;
export const PROJECT_NAVIGATOR_PANEL_EDGE_MARGIN = 8;
export const PROJECT_NAVIGATOR_PANEL_GAP = 4;
export const PROJECT_NAVIGATOR_PANEL_WIDTH = 259;
export const PROJECT_NAVIGATOR_PANEL_COMPACT_BREAKPOINT = 520;
export const PROJECT_NAVIGATOR_PANEL_COMPACT_TOP = 48;
export const PROJECT_TOOLBAR_PANEL_EDGE_MARGIN = 8;
export const PROJECT_TOOLBAR_PANEL_GAP = 6;
export const PROJECT_TOOLBAR_PANEL_WIDTH = 380;
export const PROJECT_TOOLBAR_PANEL_HEIGHT = 560;
export const PROJECT_SETTINGS_PANEL_MAX_HEIGHT = 680;
export const PROJECT_TOOLBAR_PANEL_COMPACT_BREAKPOINT = 460;

function boundedNumber(value: number): number {
  return Number.isFinite(value) ? Math.max(0, value) : 0;
}

function clamp(value: number, min: number, max: number): number {
  return Math.min(Math.max(value, min), max);
}

function chip(id: ProjectFilterChip["id"], label: string, clearTarget: ProjectFilterChip["clearTarget"]): ProjectFilterChip {
  return { id, label, clearTarget };
}

export function deriveProjectFilterChips(input: ProjectFilterChipInput): ProjectFilterChip[] {
  const chips: ProjectFilterChip[] = [];
  const search = input.search.trim();
  if (search) chips.push(chip("search", search, "search"));
  if (input.statusLabel) chips.push(chip("status", input.statusLabel, "status"));
  if (input.sectionLabel) chips.push(chip("section", input.sectionLabel, "section"));
  if (input.priorityLabel) chips.push(chip("priority", input.priorityLabel, "priority"));
  if (input.dueLabel) chips.push(chip("due", input.dueLabel, "due"));
  if (input.scheduleLabel) chips.push(chip("schedule", input.scheduleLabel, "schedule"));
  if (input.dependencyLabel) chips.push(chip("dependency", input.dependencyLabel, "dependency"));
  if (input.tagFilterLabel) chips.push(chip("tag", input.tagFilterLabel, "tag"));
  for (const filter of input.customFieldFilters) {
    chips.push(chip(`custom:${filter.fieldId}`, input.customFieldFilterLabel(filter), `custom:${filter.fieldId}`));
  }
  return chips;
}

export function deriveProjectListColumnControls(
  availableColumns: readonly ProjectTaskListColumn[],
  visibleColumns: readonly ProjectTaskListColumn[],
  labelForColumn: (column: ProjectTaskListColumn) => string,
): ProjectListColumnControl[] {
  return availableColumns.map((column) => ({
    column,
    label: labelForColumn(column),
    visible: visibleColumns.includes(column),
  }));
}

export function toggleProjectListColumn(
  visibleColumns: readonly ProjectTaskListColumn[],
  column: ProjectTaskListColumn,
): ProjectTaskListColumn[] {
  return visibleColumns.includes(column)
    ? visibleColumns.filter((entry) => entry !== column)
    : [...visibleColumns, column];
}

export function pickProjectTaskModalLayout(input: ProjectTaskModalLayoutInput): ProjectTaskModalLayout {
  const width = boundedNumber(input.viewportWidth);
  const height = boundedNumber(input.viewportHeight);
  const titleBarHeight = boundedNumber(input.titleBarHeight ?? PROJECT_TASK_MODAL_TITLE_BAR_HEIGHT);
  const edge = boundedNumber(input.edgeMargin ?? PROJECT_TASK_MODAL_EDGE_MARGIN);
  const usableHeight = Math.max(0, height - titleBarHeight - edge * 2);

  if (
    width < PROJECT_TASK_MODAL_MIN_FULLSCREEN_WIDTH
    || usableHeight < PROJECT_TASK_MODAL_MIN_FULLSCREEN_HEIGHT
  ) {
    return "fullscreen";
  }

  if (width < PROJECT_TASK_MODAL_SHEET_WIDTH + edge * 2) {
    return "sheet";
  }

  return "modal";
}

export function projectNavigatorPanelGeometry(
  input: ProjectNavigatorPanelGeometryInput,
): ProjectNavigatorPanelGeometry {
  const viewportWidth = boundedNumber(input.viewportWidth);
  const viewportHeight = boundedNumber(input.viewportHeight);
  const edge = boundedNumber(input.edgeMargin ?? PROJECT_NAVIGATOR_PANEL_EDGE_MARGIN);
  const gap = boundedNumber(input.gap ?? PROJECT_NAVIGATOR_PANEL_GAP);
  const preferredWidth = boundedNumber(input.preferredWidth ?? PROJECT_NAVIGATOR_PANEL_WIDTH);
  const compactBreakpoint = boundedNumber(
    input.compactBreakpoint ?? PROJECT_NAVIGATOR_PANEL_COMPACT_BREAKPOINT,
  );
  const compactTop = boundedNumber(input.compactTop ?? PROJECT_NAVIGATOR_PANEL_COMPACT_TOP);
  const boundsLeft = boundedNumber(input.boundsLeft ?? 0);
  const boundsTop = boundedNumber(input.boundsTop ?? 0);
  const boundsRight = boundedNumber(input.boundsRight ?? viewportWidth);
  const boundsBottom = boundedNumber(input.boundsBottom ?? viewportHeight);
  const minLeft = boundsLeft + edge;
  const maxRight = Math.max(minLeft, boundsRight - edge);
  const minTop = boundsTop + edge;
  const maxBottom = Math.max(minTop, boundsBottom - edge);
  const usableWidth = Math.max(0, maxRight - minLeft);
  const compact = usableWidth <= compactBreakpoint;
  const width = compact ? usableWidth : Math.min(preferredWidth, usableWidth);
  const left = compact
    ? minLeft
    : clamp(
      boundedNumber(input.anchorLeft),
      minLeft,
      Math.max(minLeft, maxRight - width),
    );
  const preferredTop = compact
    ? Math.max(minTop, boundsTop + compactTop)
    : boundedNumber(input.anchorBottom) + gap;
  const top = Math.min(
    Math.max(minTop, preferredTop),
    maxBottom,
  );
  const height = Math.max(0, maxBottom - top);

  return { left, top, width, height };
}

export function projectToolbarPanelGeometry(
  input: ProjectToolbarPanelGeometryInput,
): ProjectToolbarPanelGeometry {
  const viewportWidth = boundedNumber(input.viewportWidth);
  const viewportHeight = boundedNumber(input.viewportHeight);
  const edge = boundedNumber(input.edgeMargin ?? PROJECT_TOOLBAR_PANEL_EDGE_MARGIN);
  const gap = boundedNumber(input.gap ?? PROJECT_TOOLBAR_PANEL_GAP);
  const preferredWidth = boundedNumber(input.preferredWidth ?? PROJECT_TOOLBAR_PANEL_WIDTH);
  const preferredHeight = boundedNumber(input.preferredHeight ?? PROJECT_TOOLBAR_PANEL_HEIGHT);
  const compactBreakpoint = boundedNumber(
    input.compactBreakpoint ?? PROJECT_TOOLBAR_PANEL_COMPACT_BREAKPOINT,
  );
  const usableWidth = Math.max(0, viewportWidth - edge * 2);
  const compact = viewportWidth <= compactBreakpoint;
  const width = compact ? usableWidth : Math.min(preferredWidth, usableWidth);
  const left = compact
    ? edge
    : clamp(
      boundedNumber(input.anchorRight) - width,
      edge,
      Math.max(edge, viewportWidth - edge - width),
    );

  const anchorTop = boundedNumber(input.anchorTop);
  const anchorBottom = boundedNumber(input.anchorBottom);
  const belowTop = anchorBottom + gap;
  const aboveBottom = anchorTop - gap;
  const availableBelow = Math.max(0, viewportHeight - belowTop - edge);
  const availableAbove = Math.max(0, aboveBottom - edge);
  const useAbove = availableBelow < Math.min(preferredHeight, availableAbove);
  const maxHeight = Math.min(preferredHeight, useAbove ? availableAbove : availableBelow);
  const top = useAbove
    ? Math.max(edge, aboveBottom - maxHeight)
    : Math.min(belowTop, Math.max(edge, viewportHeight - edge - maxHeight));

  return {
    left,
    top,
    width,
    maxHeight,
  };
}
